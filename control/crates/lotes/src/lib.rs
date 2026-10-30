//! Lotes desde CSV (contrato: `docs/protocolo.md`, sección "Lotes (CSV) — v1").
//!
//! - [`csv`] + [`normalizar`]: lectura tolerante del archivo y limpieza de valores.
//! - [`descomponer`]: 12 Bs → [10, 2] con la menor cantidad de envíos.
//! - [`validacion`]: chequeos por fila y plan de reparto entre chips (no toca teléfonos).
//! - [`contexto`]: dispositivos, recetas, saldo conocido y cupo de cada chip; ejecución de recetas.
//! - [`ejecutor`]: corre un lote con las reglas de seguridad (incierto pausa todo, solo se reintenta lo abortado).
//! - [`persistencia`]: tablas SQLite propias, en la misma base del historial.

pub mod contexto;
pub mod csv;
pub mod descomponer;
pub mod ejecutor;
pub mod modelo;
pub mod normalizar;
pub mod persistencia;
pub mod validacion;

pub use contexto::Contexto;
pub use ejecutor::Esperas;
pub use modelo::*;

use ejecutor::Control;
use protocolo::ahora_ms;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Edad máxima de un saldo para no volver a consultarlo al validar.
pub const SALDO_FRESCO_MS: i64 = 5 * 60 * 1000;

/// Error de la API de lotes (el servidor lo traduce a HTTP).
#[derive(Debug, Clone, PartialEq)]
pub struct ErrorLote {
    /// 400, 404, 409, 500
    pub http: u16,
    pub codigo: &'static str,
    pub detalle: String,
}

impl ErrorLote {
    fn new(http: u16, codigo: &'static str, detalle: impl Into<String>) -> Self {
        Self { http, codigo, detalle: detalle.into() }
    }
    fn parametro(d: impl Into<String>) -> Self {
        Self::new(400, protocolo::codigos::PARAMETRO, d)
    }
    fn interno(e: impl std::fmt::Display) -> Self {
        Self::new(500, "almacen", e.to_string())
    }
    fn sin_lote(id: i64) -> Self {
        Self::new(404, "sin_lote", format!("no existe el lote {id}"))
    }
}

pub type ResLote<T> = Result<T, ErrorLote>;

/// Servicio de lotes: una instancia por control.
pub struct Lotes {
    pub ctx: Arc<Contexto>,
    activos: Mutex<HashMap<i64, Arc<Control>>>,
    esperas: Esperas,
}

impl Lotes {
    /// Crea el servicio y recupera lotes que quedaron `en_curso` (el control se cortó).
    pub fn new(ctx: Arc<Contexto>, esperas: Esperas) -> rusqlite::Result<Arc<Self>> {
        let s = Arc::new(Self { ctx, activos: Mutex::new(HashMap::new()), esperas });
        s.recuperar()?;
        Ok(s)
    }

    /// Un lote `en_curso` al arrancar pasa a `pausado`. Sus envíos `en_curso` quedan `incierto`
    /// (no sabemos si se confirmó); en un lote simulado vuelven a `pendiente`.
    fn recuperar(&self) -> rusqlite::Result<()> {
        for id in self.ctx.pers.ids_en_estado(EstadoLote::EnCurso)? {
            let Some((lote, _)) = self.ctx.pers.lote(id)? else { continue };
            let mut inciertos = 0;
            for mut f in lote.filas.unwrap_or_default() {
                let mut cambio = false;
                for e in &mut f.envios {
                    if e.estado == EstadoEnvio::EnCurso {
                        cambio = true;
                        if lote.simular {
                            e.estado = EstadoEnvio::Pendiente;
                            e.detalle = Some("el control se reinició durante la simulación".into());
                        } else {
                            inciertos += 1;
                            e.estado = EstadoEnvio::Incierto;
                            e.detalle = Some("el control se reinició durante este envío: revisa el saldo".into());
                        }
                        self.ctx.pers.guardar_envio(e)?;
                    }
                }
                if cambio {
                    f.estado = estado_fila(&f.envios);
                    self.ctx.pers.guardar_fila(&f)?;
                }
            }
            let motivo = if inciertos > 0 {
                format!("el control se reinició con {inciertos} envío(s) en curso: quedaron inciertos, revisa el saldo")
            } else {
                "el control se reinició".to_string()
            };
            tracing::warn!(lote = id, "{motivo}");
            self.ctx.pers.estado_lote(id, EstadoLote::Pausado, Some(&motivo))?;
        }
        Ok(())
    }

    // ───────────── validar / crear ─────────────

    pub async fn validar(&self, csv: &str, consultar_saldos: bool) -> ResLote<Validacion> {
        // Primera pasada para saber qué operadores usa el lote (y validar el formato antes de marcar nada).
        let recetas = self.ctx.recetas.read().unwrap().clone();
        let previa = validacion::validar(csv, &recetas, vec![]).map_err(ErrorLote::parametro)?;
        if consultar_saldos {
            let mut ops: Vec<String> =
                previa.validacion.filas.iter().filter(|f| f.errores.is_empty()).filter_map(|f| f.operador.clone()).collect();
            ops.sort();
            ops.dedup();
            if !ops.is_empty() {
                self.ctx.consultar_saldos(Some(&ops), Some(SALDO_FRESCO_MS)).await;
            }
        }
        let v = validacion::validar(csv, &recetas, self.ctx.chips()).map_err(ErrorLote::parametro)?;
        Ok(v.validacion)
    }

    pub fn crear(&self, nombre: &str, csv: &str, simular: bool) -> ResLote<Lote> {
        let recetas = self.ctx.recetas.read().unwrap().clone();
        let v = validacion::validar(csv, &recetas, self.ctx.chips()).map_err(ErrorLote::parametro)?;
        if v.filas.iter().all(|f| f.estado == EstadoFila::Omitida) {
            return Err(ErrorLote::parametro("ninguna fila es válida: corrige el CSV"));
        }
        let nombre = if nombre.trim().is_empty() {
            format!("Lote {}", contexto::hora_local(ahora_ms()))
        } else {
            nombre.trim().to_string()
        };
        let id = self.ctx.pers.crear_lote(&nombre, simular, ahora_ms(), &v.filas).map_err(ErrorLote::interno)?;
        tracing::info!(lote = id, filas = v.filas.len(), simular, "lote creado");
        self.ctx.bus.emitir(
            ejecutor::TIPO_EVENTO,
            serde_json::json!({ "lote_id": id, "fase": "creado", "simular": simular }),
        );
        self.obtener(id)
    }

    // ───────────── consultar ─────────────

    pub fn obtener(&self, id: i64) -> ResLote<Lote> {
        self.ctx.pers.lote(id).map_err(ErrorLote::interno)?.map(|(l, _)| l).ok_or_else(|| ErrorLote::sin_lote(id))
    }

    /// Lista sin filas, más recientes primero.
    pub fn listar(&self) -> ResLote<Vec<Lote>> {
        let ids = self.ctx.pers.ids_lotes().map_err(ErrorLote::interno)?;
        let mut v = vec![];
        for id in ids {
            if let Ok(mut l) = self.obtener(id) {
                l.filas = None;
                v.push(l);
            }
        }
        Ok(v)
    }

    // ───────────── control ─────────────

    pub fn iniciar(self: &Arc<Self>, id: i64) -> ResLote<Lote> {
        let lote = self.obtener(id)?;
        let mut activos = self.activos.lock().unwrap();
        if activos.contains_key(&id) || lote.estado == EstadoLote::EnCurso {
            return Err(ErrorLote::new(409, "en_curso", "el lote ya está en curso"));
        }
        if let Some(otro) = activos.keys().next() {
            return Err(ErrorLote::new(409, "otro_lote", format!("el lote {otro} está en curso; espera o páusalo")));
        }
        if lote.estado == EstadoLote::Cancelado {
            return Err(ErrorLote::new(409, "cancelado", "el lote está cancelado"));
        }
        let hay = lote.filas.as_ref().is_some_and(|fs| {
            fs.iter().flat_map(|f| f.envios.iter()).any(|e| matches!(e.estado, EstadoEnvio::Pendiente | EstadoEnvio::SinCapacidad))
        });
        if !hay {
            return Err(ErrorLote::new(409, "nada_pendiente", "el lote no tiene envíos pendientes"));
        }
        let ctrl = Arc::new(Control::default());
        activos.insert(id, ctrl.clone());
        drop(activos);
        // Marcar en curso antes de responder, para que la interfaz vea el cambio de inmediato.
        self.ctx.pers.estado_lote(id, EstadoLote::EnCurso, None).map_err(ErrorLote::interno)?;
        let _ = self.ctx.pers.borrar_verificacion(id);
        let yo = self.clone();
        let ctx = self.ctx.clone();
        let esperas = self.esperas;
        tokio::spawn(async move {
            ejecutor::correr(ctx, id, ctrl, esperas).await;
            yo.activos.lock().unwrap().remove(&id);
        });
        self.obtener(id)
    }

    pub fn pausar(&self, id: i64) -> ResLote<Lote> {
        let lote = self.obtener(id)?;
        match self.activos.lock().unwrap().get(&id) {
            Some(c) => {
                c.pausar.store(true, std::sync::atomic::Ordering::SeqCst);
                c.avisar.notify_one();
            }
            None if lote.estado == EstadoLote::Listo => {
                self.ctx.pers.estado_lote(id, EstadoLote::Pausado, Some("pausado por el usuario")).map_err(ErrorLote::interno)?;
            }
            None => return Err(ErrorLote::new(409, "no_en_curso", "el lote no está en curso")),
        }
        self.obtener(id)
    }

    /// Cancela. Si está corriendo, termina los envíos en vuelo (nunca se corta una transferencia a medias).
    pub fn cancelar(&self, id: i64) -> ResLote<Lote> {
        let lote = self.obtener(id)?;
        match self.activos.lock().unwrap().get(&id) {
            Some(c) => {
                c.cancelar.store(true, std::sync::atomic::Ordering::SeqCst);
                c.avisar.notify_one();
            }
            None => match lote.estado {
                EstadoLote::Terminado | EstadoLote::Cancelado => {
                    return Err(ErrorLote::new(409, "terminado", "el lote ya terminó"))
                }
                _ => {
                    self.ctx
                        .pers
                        .estado_lote(id, EstadoLote::Cancelado, Some("cancelado por el usuario"))
                        .map_err(ErrorLote::interno)?;
                }
            },
        }
        self.obtener(id)
    }

    /// Vuelve a `pendiente` los envíos `fallido` para que `iniciar` los intente otra vez. Es seguro: un
    /// `fallido` nunca pasó por `confirmar` (después de confirmar todo es `incierto`), no movió dinero.
    /// No inicia nada: el operador decide cuándo.
    pub fn reintentar_fallidas(&self, id: i64) -> ResLote<Lote> {
        let lote = self.obtener(id)?;
        // Con el candado tomado, `iniciar` no puede arrancar el lote mientras se re-encolan los envíos.
        let activos = self.activos.lock().unwrap();
        if activos.contains_key(&id) || lote.estado == EstadoLote::EnCurso {
            return Err(ErrorLote::new(409, "en_curso", "el lote está en curso: páusalo antes de reintentar"));
        }
        if lote.estado == EstadoLote::Cancelado {
            return Err(ErrorLote::new(409, "cancelado", "el lote está cancelado"));
        }
        let mut filas = lote.filas.unwrap_or_default();
        if filas.iter().flat_map(|f| f.envios.iter()).any(|e| e.estado == EstadoEnvio::Incierto) {
            return Err(ErrorLote::new(
                409,
                "hay_inciertos",
                "hay un envío incierto: revisa el saldo del chip de origen antes de reintentar",
            ));
        }
        let mut reencoladas = 0;
        for f in &mut filas {
            let mut cambio = false;
            for e in f.envios.iter_mut().filter(|e| e.estado == EstadoEnvio::Fallido) {
                e.estado = EstadoEnvio::Pendiente;
                e.detalle = None;
                e.dispositivo = None;
                e.slot = None;
                e.numero_origen = None;
                self.ctx.pers.guardar_envio(e).map_err(ErrorLote::interno)?;
                cambio = true;
                reencoladas += 1;
            }
            if cambio {
                f.estado = estado_fila(&f.envios);
                f.detalle = None;
                self.ctx.pers.guardar_fila(f).map_err(ErrorLote::interno)?;
            }
        }
        if reencoladas == 0 {
            return Err(ErrorLote::new(409, "nada_que_reintentar", "el lote no tiene envíos fallidos"));
        }
        if lote.estado == EstadoLote::Terminado {
            self.ctx.pers.estado_lote(id, EstadoLote::Listo, None).map_err(ErrorLote::interno)?;
        }
        let _ = self.ctx.pers.borrar_verificacion(id);
        drop(activos);
        tracing::info!(lote = id, reencoladas, "envíos fallidos re-encolados");
        self.ctx.bus.emitir(
            ejecutor::TIPO_EVENTO,
            serde_json::json!({ "lote_id": id, "fase": "reintento", "reencoladas": reencoladas }),
        );
        self.obtener(id)
    }

    /// Cuadra el dinero: consulta el saldo actual de los chips que enviaron algo y lo compara con el saldo
    /// inicial guardado al último inicio (solo lectura; tarda ~3.5 s por chip).
    pub async fn verificar(&self, id: i64) -> ResLote<Verificacion> {
        let (lote, comisiones) =
            self.ctx.pers.lote(id).map_err(ErrorLote::interno)?.ok_or_else(|| ErrorLote::sin_lote(id))?;
        if self.activos.lock().unwrap().contains_key(&id) || lote.estado == EstadoLote::EnCurso {
            return Err(ErrorLote::new(409, "en_curso", "el lote está en curso: espera a que pare para verificarlo"));
        }
        if lote.simular {
            return Err(ErrorLote::new(409, "simulado", "es una simulación: no movió dinero, no hay nada que cuadrar"));
        }
        let base = self.ctx.pers.saldos_iniciales(id).map_err(ErrorLote::interno)?;
        let filas = lote.filas.clone().unwrap_or_default();
        let hay_ok = filas.iter().flat_map(|f| f.envios.iter()).any(|e| e.estado == EstadoEnvio::Ok);
        if base.is_empty() && !hay_ok {
            return Err(ErrorLote::new(409, "sin_base", "el lote nunca se inició: no hay saldo inicial con qué comparar"));
        }
        let inicio_ms = base.iter().map(|b| b.3).min().unwrap_or(0);

        // Lo que se envió desde el último inicio, por chip: (cantidad, Bs con comisión, número de origen).
        let mut por_chip: std::collections::BTreeMap<(String, u32), (u32, f64, Option<String>)> = Default::default();
        for f in &filas {
            let com = comisiones.get(&f.id).copied().unwrap_or(0.0);
            for e in f.envios.iter().filter(|e| e.estado == EstadoEnvio::Ok && e.ts_ms.unwrap_or(0) >= inicio_ms) {
                let (Some(d), Some(sl)) = (e.dispositivo.clone(), e.slot) else { continue };
                let x = por_chip.entry((d, sl)).or_insert((0, 0.0, None));
                x.0 += 1;
                x.1 += e.monto as f64 + com;
                if x.2.is_none() {
                    x.2 = e.numero_origen.clone();
                }
            }
        }

        let mut avisos = vec![];
        let mut chips = vec![];
        if por_chip.is_empty() {
            avisos.push("no hubo envíos ok desde el último inicio: no hay nada que cuadrar".to_string());
        } else {
            let mut ops: Vec<String> = self
                .ctx
                .chips()
                .into_iter()
                .filter(|c| por_chip.contains_key(&(c.dispositivo.clone(), c.slot)))
                .map(|c| c.operador)
                .collect();
            ops.sort();
            ops.dedup();
            let desde = ahora_ms();
            let en_linea = if ops.is_empty() { self.ctx.chips() } else { self.ctx.consultar_saldos(Some(&ops), None).await };
            for ((d, sl), (n, esperado, numero)) in por_chip {
                let inicial = base.iter().find(|b| b.0 == d && b.1 == sl).map(|b| b.2);
                // Solo vale una lectura hecha ahora; una vieja puede ser el saldo "ajustado" por el propio lote.
                let actual = self.ctx.saldo(&d, sl).filter(|(_, ts)| *ts >= desde).map(|(s, _)| s);
                let numero = numero.or_else(|| en_linea.iter().find(|c| c.dispositivo == d && c.slot == sl).and_then(|c| c.numero.clone()));
                let esperado = (esperado * 100.0).round() / 100.0;
                let real = inicial.zip(actual).map(|(i, a)| ((i - a) * 100.0).round() / 100.0);
                let dif = real.map(|r| ((r - esperado) * 100.0).round() / 100.0);
                let estado = match dif {
                    None => EstadoCuadre::SinDato,
                    Some(x) if x.abs() <= TOLERANCIA_BS + 1e-9 => EstadoCuadre::Cuadra,
                    Some(_) => EstadoCuadre::Diferencia,
                };
                if estado == EstadoCuadre::SinDato {
                    avisos.push(if inicial.is_none() {
                        format!("{d} chip {}: no hay saldo inicial guardado", sl + 1)
                    } else {
                        format!("{d} chip {}: no se pudo consultar el saldo ahora (¿desconectado u ocupado?)", sl + 1)
                    });
                }
                chips.push(CuadreChip {
                    dispositivo: d,
                    slot: sl,
                    numero,
                    estado,
                    saldo_inicial: inicial,
                    saldo_actual: actual,
                    descontado_esperado: esperado,
                    descontado_real: real,
                    diferencia_bs: dif,
                    envios_ok: n,
                });
            }
        }
        let cuadra = !chips.is_empty() && chips.iter().all(|c| c.estado == EstadoCuadre::Cuadra);
        let v = Verificacion {
            lote_id: id,
            verificado_ms: ahora_ms(),
            completo: lote.revision.completo,
            motivos: lote.revision.motivos.clone(),
            cuadra,
            confirmado: lote.revision.completo && cuadra,
            chips,
            avisos,
        };
        self.ctx.pers.guardar_verificacion(&v).map_err(ErrorLote::interno)?;
        Ok(v)
    }

    /// ¿Hay un lote corriendo? (para tests y la interfaz)
    pub fn en_curso(&self) -> Vec<i64> {
        self.activos.lock().unwrap().keys().copied().collect()
    }

    // ───────────── CSV de salida ─────────────

    pub fn resultado_csv(&self, id: i64, separador: char) -> ResLote<(String, String)> {
        let lote = self.obtener(id)?;
        let comisiones = self.ctx.pers.lote(id).map_err(ErrorLote::interno)?.map(|(_, c)| c).unwrap_or_default();
        let mut s = String::from("\u{feff}");
        let enc = [
            "fila", "numero", "operador", "accion", "monto", "referencia", "estado", "enviado_bs", "comision_bs",
            "envios_ok", "envios_total", "chips", "detalle", "hora",
        ];
        s.push_str(&enc.join(&separador.to_string()));
        s.push_str("\r\n");
        for f in lote.filas.unwrap_or_default() {
            let ok = f.envios.iter().filter(|e| e.estado == EstadoEnvio::Ok).count();
            let com = comisiones.get(&f.id).copied().unwrap_or(0.0) * ok as f64;
            let mut chips: Vec<String> = f
                .envios
                .iter()
                .filter(|e| matches!(e.estado, EstadoEnvio::Ok | EstadoEnvio::Simulado | EstadoEnvio::Incierto))
                .map(|e| {
                    e.numero_origen.clone().unwrap_or_else(|| {
                        format!("{}#{}", e.dispositivo.clone().unwrap_or_default(), e.slot.map(|s| s + 1).unwrap_or(0))
                    })
                })
                .collect();
            chips.dedup();
            let hora = f.envios.iter().filter_map(|e| e.ts_ms).max().map(contexto::hora_local).unwrap_or_default();
            let campos = [
                f.fila.to_string(),
                f.numero.clone().unwrap_or_default(),
                f.operador.clone().unwrap_or_default(),
                f.accion.clone(),
                f.monto.map(|m| m.to_string()).unwrap_or_default(),
                f.referencia.clone().unwrap_or_default(),
                f.estado.texto().to_string(),
                f.enviado_bs.to_string(),
                format!("{com:.2}"),
                ok.to_string(),
                f.envios.len().to_string(),
                chips.join("|"),
                f.detalle.clone().unwrap_or_default(),
                hora,
            ];
            let linea: Vec<String> = campos.iter().map(|c| campo_csv(c, separador)).collect();
            s.push_str(&linea.join(&separador.to_string()));
            s.push_str("\r\n");
        }
        let archivo = format!("lote-{}-resultado.csv", lote.id);
        Ok((archivo, s))
    }
}

/// Entrecomilla si hace falta (separador, comillas o saltos de línea).
fn campo_csv(c: &str, sep: char) -> String {
    if c.contains(sep) || c.contains('"') || c.contains('\n') || c.contains('\r') {
        format!("\"{}\"", c.replace('"', "\"\""))
    } else {
        c.to_string()
    }
}

/// CSV de ejemplo para `GET /api/lotes/plantilla.csv`.
pub const PLANTILLA: &str = "\u{feff}numero,monto,operador,accion,referencia\r\n\
69869869,12,TIGO,transferir,Pedido 1001\r\n\
+591 7881-5232,5,TIGO,,Pedido 1002\r\n\
70000000,20,TIGO,transferir,Pedido 1003\r\n";

#[cfg(test)]
mod tests;
