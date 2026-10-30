//! Ejecutor de un lote. Reglas (contrato, "Lotes — Ejecución"):
//! - Cada envío se asigna en el momento al mejor chip disponible; un envío por teléfono a la vez y
//!   teléfonos en paralelo.
//! - Solo se reintenta lo `abortado` (no se movió dinero): chip agotado ("maximo numero de transfer"),
//!   chip sin saldo (el monto no aparece en el menú), y `timeout`/`ocupado`/`red` antes de confirmar.
//! - `incierto` pausa el lote completo y nunca se reintenta.
//! - Al pausar o cancelar no se corta nada en curso: se espera a que terminen los envíos en vuelo.

use crate::contexto::Contexto;
use crate::modelo::*;
use crate::persistencia;
use crate::validacion::{elegir_chip, ChipReparto};
use nucleo::ErrorCanal;
use protocolo::{ahora_ms, codigos, EstadoResultado, Resultado};
use serde_json::json;
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Notify;

pub const TIPO_EVENTO: &str = "lote";
/// Reintentos de un envío por `timeout`/`red` antes de darlo por fallido.
pub const MAX_REINTENTOS: u32 = 2;

/// Esperas (configurables para tests).
#[derive(Debug, Clone, Copy)]
pub struct Esperas {
    pub reintento: Duration,
    pub ocupado: Duration,
    pub tic: Duration,
    /// Cuánto esperar a un teléfono ocupado por otra sesión antes de dejar lo pendiente como sin capacidad.
    pub max_espera_ocupado: Duration,
}

impl Default for Esperas {
    fn default() -> Self {
        Self {
            reintento: Duration::from_secs(5),
            ocupado: Duration::from_secs(2),
            tic: Duration::from_millis(500),
            max_espera_ocupado: Duration::from_secs(120),
        }
    }
}

/// Señales hacia un lote en curso.
#[derive(Default)]
pub struct Control {
    pub pausar: AtomicBool,
    pub cancelar: AtomicBool,
    pub avisar: Notify,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Parada {
    Ninguna,
    Pausa,
    Cancelar,
    Incierto,
}

/// Qué hacer con un envío abortado.
#[derive(Debug, PartialEq)]
enum Tras {
    /// El chip llegó al máximo diario: marcarlo agotado y reasignar.
    ChipAgotado,
    /// El chip no tiene saldo para ese monto: saldo 0 y reasignar.
    ChipSinSaldo,
    /// El teléfono estaba ocupado o se desconectó: reasignar sin contar intento.
    Esperar,
    /// Falla de red / timeout antes de confirmar: reintento contado.
    Reintentar,
    /// Otra cosa: no se reintenta.
    Fallar,
}

fn clasificar_abortado(detalle: &str) -> Tras {
    let d = detalle.to_lowercase();
    let tiene = |c: &str| d.contains(&format!("{c}: "));
    if d.contains("maximo numero de transfer") || d.contains("máximo número de transfer") {
        Tras::ChipAgotado
    } else if d.contains("saldo insuficiente") || (d.contains("no encontré la opción") && d.contains("bs")) {
        Tras::ChipSinSaldo
    } else if tiene(codigos::OCUPADO) || tiene(codigos::DESCONECTADO) || tiene(codigos::SIN_DISPOSITIVO) {
        Tras::Esperar
    } else if tiene(codigos::TIMEOUT) || tiene(codigos::RED) {
        Tras::Reintentar
    } else {
        Tras::Fallar
    }
}

struct Corrida {
    ctx: Arc<Contexto>,
    ctrl: Arc<Control>,
    esperas: Esperas,
    lote: Lote,
    filas: Vec<FilaLote>,
    comisiones: BTreeMap<i64, f64>,
    /// envío id → (índice de fila, índice de envío)
    indice: HashMap<i64, (usize, usize)>,
    no_antes: HashMap<i64, Instant>,
    reintentos: HashMap<i64, u32>,
    /// dispositivo → envío en vuelo
    en_vuelo: HashMap<String, i64>,
    parada: Parada,
    motivo: Option<String>,
}

type FinTarea = (i64, String, u32, Result<(Resultado, Option<i64>), ErrorCanal>);

impl Corrida {
    fn emitir(&self, fase: &str, mut extra: serde_json::Value) {
        let p = persistencia::progreso(&self.filas, &self.comisiones);
        if let Some(m) = extra.as_object_mut() {
            m.insert("lote_id".into(), json!(self.lote.id));
            m.insert("fase".into(), json!(fase));
            m.insert("progreso".into(), json!(p));
        }
        self.ctx.bus.emitir(TIPO_EVENTO, extra);
    }

    fn envio(&self, id: i64) -> &EnvioLote {
        let (f, e) = self.indice[&id];
        &self.filas[f].envios[e]
    }

    /// Guarda un envío y recalcula su fila; emite eventos.
    fn actualizar(&mut self, id: i64, cambio: impl FnOnce(&mut EnvioLote)) {
        let (fi, ei) = self.indice[&id];
        {
            let e = &mut self.filas[fi].envios[ei];
            cambio(e);
            if let Err(err) = self.ctx.pers.guardar_envio(e) {
                tracing::warn!("lote {}: no pude guardar el envío: {err}", self.lote.id);
            }
        }
        let antes = self.filas[fi].estado;
        let f = &mut self.filas[fi];
        f.estado = estado_fila(&f.envios);
        f.enviado_bs = f.envios.iter().filter(|e| e.estado == EstadoEnvio::Ok).map(|e| e.monto).sum();
        f.detalle = f
            .envios
            .iter()
            .rev()
            .find(|e| !matches!(e.estado, EstadoEnvio::Ok | EstadoEnvio::Simulado | EstadoEnvio::EnCurso))
            .and_then(|e| e.detalle.clone());
        if let Err(err) = self.ctx.pers.guardar_fila(f) {
            tracing::warn!("lote {}: no pude guardar la fila: {err}", self.lote.id);
        }
        let e = self.filas[fi].envios[ei].clone();
        let (fila, numero, estado_fila_nuevo) = (self.filas[fi].fila, self.filas[fi].numero.clone(), self.filas[fi].estado);
        self.emitir(
            "envio",
            json!({ "fila": fila, "numero": numero, "monto": e.monto, "dispositivo_id": e.dispositivo, "slot": e.slot,
                    "estado": e.estado, "detalle": e.detalle }),
        );
        if estado_fila_nuevo != antes {
            self.emitir("fila", json!({ "fila": fila, "estado": estado_fila_nuevo }));
        }
    }

    fn pendientes(&self) -> Vec<i64> {
        self.filas
            .iter()
            .flat_map(|f| f.envios.iter())
            .filter(|e| e.estado == EstadoEnvio::Pendiente)
            .map(|e| e.id)
            .collect()
    }

    fn info_envio(&self, id: i64) -> (String, String, String, f64) {
        let (fi, _) = self.indice[&id];
        let f = &self.filas[fi];
        (
            f.receta.clone().unwrap_or_default(),
            f.operador.clone().unwrap_or_default(),
            f.numero.clone().unwrap_or_default(),
            self.comisiones.get(&f.id).copied().unwrap_or(0.0),
        )
    }

    fn revisar_senales(&mut self) {
        if self.parada == Parada::Incierto {
            return;
        }
        if self.ctrl.cancelar.load(Ordering::SeqCst) {
            self.parada = Parada::Cancelar;
        } else if self.ctrl.pausar.load(Ordering::SeqCst) && self.parada == Parada::Ninguna {
            self.parada = Parada::Pausa;
        }
    }

    /// Asigna envíos pendientes a teléfonos libres. Devuelve las tareas lanzadas.
    fn repartir(&mut self, tareas: &mut tokio::task::JoinSet<FinTarea>) {
        let ahora = Instant::now();
        let mut chips: Vec<ChipReparto> = self.ctx.chips().iter().map(ChipReparto::de).collect();
        let mut ocupados: std::collections::HashSet<String> = self.en_vuelo.keys().cloned().collect();
        for id in self.pendientes() {
            if self.no_antes.get(&id).is_some_and(|t| *t > ahora) {
                continue;
            }
            let (receta_nombre, operador, numero, comision) = self.info_envio(id);
            let monto = self.envio(id).monto;
            let ctx = &self.ctx;
            let elegido = elegir_chip(&chips, &operador, numero.as_str(), monto, comision, |c| {
                !ocupados.contains(&c.info.dispositivo) && !ctx.ocupado(&c.info.dispositivo)
            });
            let Ok(i) = elegido else { continue };
            let Some(receta) = self.ctx.recetas.read().unwrap().get(&receta_nombre).cloned() else {
                self.actualizar(id, |e| {
                    e.estado = EstadoEnvio::Fallido;
                    e.detalle = Some(format!("la receta '{receta_nombre}' ya no existe"));
                });
                continue;
            };
            let chip = chips[i].info.clone();
            // El chip PDA corre su variante PDA de la receta si existe; si no, la de la compañía.
            let receta = match self.ctx.receta_efectiva(&receta, &chip.tipo) {
                Ok(r) => r,
                Err(detalle) => {
                    self.actualizar(id, |e| {
                        e.estado = EstadoEnvio::Fallido;
                        e.detalle = Some(detalle);
                    });
                    continue;
                }
            };
            if !self.lote.simular {
                chips[i].consumir(monto, comision);
            }
            ocupados.insert(chip.dispositivo.clone());
            self.en_vuelo.insert(chip.dispositivo.clone(), id);
            self.actualizar(id, |e| {
                e.estado = EstadoEnvio::EnCurso;
                e.dispositivo = Some(chip.dispositivo.clone());
                e.slot = Some(chip.slot);
                e.numero_origen = chip.numero.clone();
                e.intentos += 1;
                e.ts_ms = Some(ahora_ms());
            });
            let ctx = self.ctx.clone();
            let simular = self.lote.simular;
            let mut params = BTreeMap::new();
            params.insert("numero".to_string(), numero);
            params.insert("monto".to_string(), monto.to_string());
            let (disp, slot) = (chip.dispositivo.clone(), chip.slot);
            tareas.spawn(async move {
                let r = ctx.ejecutar_receta(&receta, &disp, slot, &params, simular).await;
                (id, disp, slot, r)
            });
        }
    }

    /// Procesa el fin de un envío.
    fn terminar_envio(&mut self, (id, disp, slot, r): FinTarea) {
        self.en_vuelo.remove(&disp);
        let volver = |e: &mut EnvioLote, detalle: String| {
            e.estado = EstadoEnvio::Pendiente;
            e.detalle = Some(detalle);
            e.dispositivo = None;
            e.slot = None;
            e.numero_origen = None;
        };
        let (r, ej) = match r {
            Ok(x) => x,
            Err(err) => {
                // No llegó a marcar: el teléfono se fue. Reasignar sin contar intento.
                self.no_antes.insert(id, Instant::now() + self.esperas.ocupado);
                self.actualizar(id, |e| volver(e, format!("reintento: {err}")));
                return;
            }
        };
        let detalle = r.detalle.clone().unwrap_or_default();
        match r.estado {
            EstadoResultado::Ok => self.actualizar(id, |e| {
                e.estado = EstadoEnvio::Ok;
                e.ejecucion_id = ej;
                e.detalle = None;
                e.ts_ms = Some(ahora_ms());
            }),
            EstadoResultado::Simulado => self.actualizar(id, |e| {
                e.estado = EstadoEnvio::Simulado;
                e.ejecucion_id = ej;
                e.detalle = r.detalle.clone();
                e.ts_ms = Some(ahora_ms());
            }),
            EstadoResultado::Incierto => {
                let (fi, _) = self.indice[&id];
                let (fila, numero) = (self.filas[fi].fila, self.filas[fi].numero.clone().unwrap_or_default());
                let monto = self.envio(id).monto;
                let origen = self.envio(id).numero_origen.clone().unwrap_or_else(|| format!("{disp} SIM {}", slot + 1));
                self.actualizar(id, |e| {
                    e.estado = EstadoEnvio::Incierto;
                    e.ejecucion_id = ej;
                    e.detalle = r.detalle.clone();
                    e.ts_ms = Some(ahora_ms());
                });
                self.parada = Parada::Incierto;
                self.motivo = Some(format!(
                    "envío incierto en la fila {fila} ({monto} Bs a {numero} desde {origen}): {detalle} Revisa el saldo antes de reanudar."
                ));
                self.emitir("alerta", json!({ "motivo": self.motivo, "fila": fila }));
            }
            EstadoResultado::Error => self.actualizar(id, |e| {
                e.estado = EstadoEnvio::Fallido;
                e.ejecucion_id = ej;
                e.detalle = r.detalle.clone();
                e.ts_ms = Some(ahora_ms());
            }),
            EstadoResultado::Abortado => match clasificar_abortado(&detalle) {
                Tras::ChipAgotado => {
                    // También en simulación: es un dato real del operador.
                    self.ctx.agotar_hoy(&disp, slot);
                    self.actualizar(id, |e| volver(e, format!("chip agotado por hoy, se reasigna: {detalle}")));
                }
                Tras::ChipSinSaldo => {
                    self.ctx.fijar_saldo(&disp, slot, 0.0, None, None);
                    self.actualizar(id, |e| volver(e, format!("chip sin saldo para el monto, se reasigna: {detalle}")));
                }
                Tras::Esperar => {
                    self.no_antes.insert(id, Instant::now() + self.esperas.ocupado);
                    self.actualizar(id, |e| volver(e, format!("teléfono ocupado, se reintenta: {detalle}")));
                }
                Tras::Reintentar => {
                    let n = self.reintentos.entry(id).or_insert(0);
                    *n += 1;
                    if *n <= MAX_REINTENTOS {
                        let n = *n;
                        self.no_antes.insert(id, Instant::now() + self.esperas.reintento);
                        self.actualizar(id, |e| volver(e, format!("reintento {n}/{MAX_REINTENTOS}: {detalle}")));
                    } else {
                        self.actualizar(id, |e| {
                            e.estado = EstadoEnvio::Fallido;
                            e.ejecucion_id = ej;
                            e.detalle = Some(format!("falló tras {MAX_REINTENTOS} reintentos: {detalle}"));
                            e.ts_ms = Some(ahora_ms());
                        });
                    }
                }
                Tras::Fallar => self.actualizar(id, |e| {
                    e.estado = EstadoEnvio::Fallido;
                    e.ejecucion_id = ej;
                    e.detalle = r.detalle.clone();
                    e.ts_ms = Some(ahora_ms());
                }),
            },
        }
    }

    /// ¿Algún pendiente podría asignarse más adelante (espera de reintento u ocupado)?
    /// Devuelve (hay_espera, motivo_sin_capacidad_si_no).
    fn algo_por_esperar(&self) -> (bool, Option<String>) {
        let ahora = Instant::now();
        let chips: Vec<ChipReparto> = self.ctx.chips().iter().map(ChipReparto::de).collect();
        let mut motivo = None;
        for id in self.pendientes() {
            if self.no_antes.get(&id).is_some_and(|t| *t > ahora) {
                return (true, None);
            }
            let (_, operador, numero, comision) = self.info_envio(id);
            match elegir_chip(&chips, &operador, numero.as_str(), self.envio(id).monto, comision, |_| true) {
                Ok(_) => return (true, None),
                Err(m) => {
                    motivo.get_or_insert(m);
                }
            }
        }
        (false, motivo)
    }

    fn marcar_sin_capacidad(&mut self, motivo: &str) {
        for id in self.pendientes() {
            self.actualizar(id, |e| {
                e.estado = EstadoEnvio::SinCapacidad;
                e.detalle = Some(format!("sin capacidad: {motivo}"));
            });
        }
    }
}

/// Guarda el saldo de los chips de los operadores del lote antes del primer envío de esta corrida
/// (consultando los que tengan un dato viejo). `verificar` lo compara con el saldo de después.
async fn guardar_saldos_iniciales(ctx: &Arc<Contexto>, lote_id: i64, filas: &[FilaLote]) {
    let mut ops: Vec<String> = filas
        .iter()
        .filter(|f| f.envios.iter().any(|e| e.estado == EstadoEnvio::Pendiente))
        .filter_map(|f| f.operador.clone())
        .collect();
    ops.sort();
    ops.dedup();
    if ops.is_empty() {
        return;
    }
    ctx.consultar_saldos(Some(&ops), Some(crate::SALDO_FRESCO_MS)).await;
    let base: Vec<_> = ctx
        .chips()
        .into_iter()
        .filter(|c| ops.contains(&c.operador))
        .filter_map(|c| {
            c.saldo.map(|saldo| persistencia::SaldoChip {
                dispositivo: c.dispositivo,
                slot: c.slot,
                numero: c.numero,
                operador: Some(c.operador),
                saldo,
                ts_ms: 0,
            })
        })
        .collect();
    if let Err(e) = ctx.pers.guardar_saldos_iniciales(lote_id, &base, ahora_ms()) {
        tracing::warn!("lote {lote_id}: no pude guardar los saldos iniciales: {e}");
    }
}

/// Corre el lote hasta terminar, pausar o cancelar. Deja el estado final guardado.
pub async fn correr(ctx: Arc<Contexto>, lote_id: i64, ctrl: Arc<Control>, esperas: Esperas) {
    let (lote, comisiones) = match ctx.pers.lote(lote_id) {
        Ok(Some(x)) => x,
        Ok(None) => return,
        Err(e) => {
            tracing::error!("lote {lote_id}: no pude leerlo: {e}");
            return;
        }
    };
    let mut filas = lote.filas.clone().unwrap_or_default();
    // Retomar: lo que quedó sin capacidad otro día vuelve a pendiente.
    let mut indice = HashMap::new();
    for (fi, f) in filas.iter_mut().enumerate() {
        for (ei, e) in f.envios.iter_mut().enumerate() {
            indice.insert(e.id, (fi, ei));
            if e.estado == EstadoEnvio::SinCapacidad {
                e.estado = EstadoEnvio::Pendiente;
                e.detalle = None;
                let _ = ctx.pers.guardar_envio(e);
            }
        }
        if f.estado != EstadoFila::Omitida {
            f.estado = estado_fila(&f.envios);
            let _ = ctx.pers.guardar_fila(f);
        }
    }
    let _ = ctx.pers.estado_lote(lote_id, EstadoLote::EnCurso, None);
    if !lote.simular {
        guardar_saldos_iniciales(&ctx, lote_id, &filas).await;
    }
    let mut c = Corrida {
        ctx: ctx.clone(),
        ctrl,
        esperas,
        lote: Lote { estado: EstadoLote::EnCurso, motivo_pausa: None, filas: None, ..lote },
        filas,
        comisiones,
        indice,
        no_antes: HashMap::new(),
        reintentos: HashMap::new(),
        en_vuelo: HashMap::new(),
        parada: Parada::Ninguna,
        motivo: None,
    };
    c.emitir("inicio", json!({ "simular": c.lote.simular }));
    tracing::info!(lote = lote_id, simular = c.lote.simular, "lote iniciado");

    let mut tareas: tokio::task::JoinSet<FinTarea> = tokio::task::JoinSet::new();
    let mut esperando_desde: Option<Instant> = None;
    loop {
        c.revisar_senales();
        if c.parada == Parada::Ninguna {
            c.repartir(&mut tareas);
        }
        if tareas.is_empty() {
            if c.parada != Parada::Ninguna || c.pendientes().is_empty() {
                break;
            }
            let (esperar, motivo) = c.algo_por_esperar();
            let vencido = esperando_desde.is_some_and(|t| t.elapsed() > c.esperas.max_espera_ocupado);
            if !esperar || vencido {
                let m = motivo.unwrap_or_else(|| "los teléfonos con capacidad siguieron ocupados".into());
                c.marcar_sin_capacidad(&m);
                break;
            }
            esperando_desde.get_or_insert_with(Instant::now);
        } else {
            esperando_desde = None;
        }
        let tic = c.esperas.tic;
        tokio::select! {
            Some(fin) = tareas.join_next(), if !tareas.is_empty() => {
                match fin {
                    Ok(fin) => c.terminar_envio(fin),
                    Err(e) => tracing::error!("lote {lote_id}: tarea de envío falló: {e}"),
                }
            }
            _ = c.ctrl.avisar.notified() => {}
            _ = tokio::time::sleep(tic) => {}
        }
    }

    let (estado, motivo) = match c.parada {
        Parada::Cancelar => (EstadoLote::Cancelado, Some("cancelado por el usuario".to_string())),
        Parada::Incierto => (EstadoLote::Pausado, c.motivo.clone()),
        Parada::Pausa => (EstadoLote::Pausado, Some("pausado por el usuario".to_string())),
        Parada::Ninguna => (EstadoLote::Terminado, None),
    };
    let _ = ctx.pers.estado_lote(lote_id, estado, motivo.as_deref());
    c.lote.estado = estado;
    let fase = if estado == EstadoLote::Pausado { "pausa" } else { "fin" };
    c.emitir(fase, json!({ "estado": estado, "motivo": motivo }));
    tracing::info!(lote = lote_id, estado = estado.texto(), "lote detenido");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clasifica_abortados() {
        assert_eq!(
            clasificar_abortado("el operador respondió: Bienvenido…\nUsted alcanzo el maximo numero de transferecias diarias"),
            Tras::ChipAgotado
        );
        assert_eq!(
            clasificar_abortado("paso 4: no encontré la opción /20\\s*Bs/ en el menú (opciones: 1 Bs | 2 Bs)"),
            Tras::ChipSinSaldo
        );
        assert_eq!(
            clasificar_abortado("paso 3: respuesta inesperada (esperaba /monto/): \"Saldo insuficiente para transferir\""),
            Tras::ChipSinSaldo
        );
        assert_eq!(clasificar_abortado("paso 1 (marcar \"*66#1#\"): ocupado: ya hay una sesión"), Tras::Esperar);
        assert_eq!(clasificar_abortado("paso 2 (elegir \"1\"): timeout: sin respuesta a tiempo"), Tras::Reintentar);
        assert_eq!(clasificar_abortado("paso 2 (elegir \"1\"): red: el operador cortó"), Tras::Reintentar);
        assert_eq!(clasificar_abortado("paso 3: respuesta inesperada: \"Numero de destino invalido\""), Tras::Fallar);
    }
}
