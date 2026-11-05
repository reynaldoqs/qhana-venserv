//! Ejecutor de un lote. Reglas (contrato, "Lotes — Ejecución"):
//! - Cada envío se asigna en el momento al mejor chip disponible; un envío por teléfono a la vez y
//!   teléfonos en paralelo.
//! - Solo se reintenta lo `abortado` (no se movió dinero): chip agotado ("maximo numero de transfer"),
//!   chip sin saldo (el monto no aparece en el menú), y `timeout`/`ocupado`/`red` antes de confirmar.
//! - `incierto` no detiene el lote: queda marcado, se emite una alerta y se sigue con lo demás. El ejecutor
//!   nunca lo reintenta solo (pudo haberse hecho); solo el operador, con `reintentar` + `inciertas`.
//! - Al pausar o cancelar no se corta nada en curso: se espera a que terminen los envíos en vuelo.
//! - Un envío solo sale si su paso a `en_curso` quedó guardado; si no, el lote se pausa.
//! - El teléfono se reserva antes de despachar (nadie más lo toma) y se libera al terminar, también si la
//!   tarea del envío se cae; en ese caso el envío queda `incierto`.
//! - Las recetas se toman al iniciar la corrida: recargarlas a mitad de un lote no cambia su flujo.

use crate::contexto::{receta_efectiva_en, Contexto, Reserva};
use crate::modelo::TIPO_DEALER;
use crate::modelo::*;
use crate::normalizar;
use crate::persistencia;
use crate::validacion::{pares_de, Par, Reparto};
use nucleo::{ErrorCanal, Receta};
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
/// Reintentos de un envío por teléfono `ocupado` o error del canal (antes de confirmar) antes de darlo por fallido.
pub const MAX_OCUPADO: u32 = 10;

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

/// Señales hacia un lote en curso (y su ritmo, para `carriles`).
#[derive(Default)]
pub struct Control {
    pub pausar: AtomicBool,
    pub cancelar: AtomicBool,
    pub avisar: Notify,
    pub ritmo: std::sync::Mutex<Ritmo>,
}

/// Cuántos envíos terminó la corrida desde su primer despacho (todos los teléfonos juntos).
#[derive(Debug, Default, Clone, Copy)]
pub struct Ritmo {
    pub desde: Option<Instant>,
    pub hechos: u32,
}

impl Ritmo {
    /// Lo que falta para `pendientes` envíos al ritmo actual; `None` si aún no hay ritmo.
    pub fn eta_ms(&self, pendientes: u32) -> Option<u64> {
        let desde = self.desde?;
        if self.hechos == 0 {
            return None;
        }
        let ms = desde.elapsed().as_millis() as f64;
        Some((pendientes as f64 * ms / self.hechos as f64).round() as u64)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Parada {
    Ninguna,
    Pausa,
    Cancelar,
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
    /// Veces que un envío volvió a la cola porque el teléfono estaba ocupado o el canal falló.
    ocupado: HashMap<i64, u32>,
    /// dispositivo → envío en vuelo
    en_vuelo: HashMap<String, i64>,
    /// tarea → (envío, dispositivo, slot): para saber qué envío era si la tarea se cae.
    tareas_envio: HashMap<tokio::task::Id, (i64, String, u32)>,
    /// Copia de las recetas tomada al iniciar esta corrida.
    recetas: BTreeMap<String, Receta>,
    parada: Parada,
    /// Por qué se pausó solo (p. ej. la base no guardó un envío); `None` = lo pidió el usuario.
    motivo_parada: Option<String>,
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

    /// Guarda un envío y recalcula su fila; emite eventos. Si la base falla, lo anota y sigue (el estado en
    /// memoria manda hasta el final de la corrida).
    fn actualizar(&mut self, id: i64, cambio: impl FnOnce(&mut EnvioLote)) {
        let _ = self.aplicar(id, cambio, false);
    }

    /// Como [`Corrida::actualizar`], pero si la base no guarda el envío lo deja como estaba y devuelve el
    /// error: es el paso a `en_curso`, y sin él guardado el envío no puede salir.
    fn despachar(&mut self, id: i64, cambio: impl FnOnce(&mut EnvioLote)) -> Result<(), String> {
        self.aplicar(id, cambio, true)
    }

    fn aplicar(&mut self, id: i64, cambio: impl FnOnce(&mut EnvioLote), estricto: bool) -> Result<(), String> {
        let (fi, ei) = self.indice[&id];
        {
            let e = &mut self.filas[fi].envios[ei];
            let antes = e.clone();
            cambio(e);
            if let Err(err) = self.ctx.pers.guardar_envio(e) {
                if estricto {
                    *e = antes;
                    return Err(err.to_string());
                }
                tracing::warn!("lote {}: no pude guardar el envío: {err}", self.lote.id);
            }
        }
        let antes = self.filas[fi].estado;
        let f = &mut self.filas[fi];
        f.estado = estado_fila(&f.envios);
        f.enviado_bs = normalizar::centavos(f.envios.iter().filter(|e| e.estado == EstadoEnvio::Ok).map(|e| e.monto).sum());
        f.detalle = f
            .envios
            .iter()
            .rev()
            .find(|e| !matches!(e.estado, EstadoEnvio::Ok | EstadoEnvio::Simulado | EstadoEnvio::EnCurso))
            .and_then(|e| e.detalle.clone());
        if let Err(err) = self.ctx.pers.guardar_fila(f) {
            tracing::warn!("lote {}: no pude guardar la fila: {err}", self.lote.id);
        }
        // La lista de lotes lee este progreso guardado (no las filas).
        let p = persistencia::progreso(&self.filas, &self.comisiones);
        if let Err(err) = self.ctx.pers.guardar_progreso(self.lote.id, &p) {
            tracing::warn!("lote {}: no pude guardar el progreso: {err}", self.lote.id);
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
        Ok(())
    }

    /// El envío quedó incierto (pudo haberse hecho): se marca, se avisa y el lote sigue.
    fn marcar_incierto(&mut self, id: i64, disp: &str, slot: u32, ej: Option<i64>, detalle: Option<String>) {
        let (fi, _) = self.indice[&id];
        let (fila, numero) = (self.filas[fi].fila, self.filas[fi].numero.clone().unwrap_or_default());
        let monto = self.envio(id).monto;
        let origen = self.envio(id).numero_origen.clone().unwrap_or_else(|| format!("{disp} SIM {}", slot + 1));
        let texto = detalle.clone().unwrap_or_default();
        self.actualizar(id, |e| {
            e.estado = EstadoEnvio::Incierto;
            if ej.is_some() {
                e.ejecucion_id = ej;
            }
            e.detalle = detalle;
            e.ts_ms = Some(ahora_ms());
        });
        let motivo = format!(
            "envío incierto en la fila {fila} ({} Bs a {numero} desde {origen}): {texto} Revisa el saldo antes de reintentarlo.",
            normalizar::bs(monto)
        );
        tracing::warn!(lote = self.lote.id, "{motivo}");
        self.emitir("alerta", json!({ "motivo": motivo, "fila": fila }));
    }

    /// El teléfono estaba ocupado o el canal falló antes de marcar: vuelve a la cola, hasta [`MAX_OCUPADO`] veces.
    fn reintentar_ocupado(&mut self, id: i64, ej: Option<i64>, detalle: String) {
        let n = self.ocupado.entry(id).or_insert(0);
        *n += 1;
        if *n > MAX_OCUPADO {
            self.actualizar(id, |e| {
                e.estado = EstadoEnvio::Fallido;
                if ej.is_some() {
                    e.ejecucion_id = ej;
                }
                e.detalle = Some(format!("el teléfono siguió ocupado ({MAX_OCUPADO} reintentos): {detalle}"));
                e.ts_ms = Some(ahora_ms());
            });
            return;
        }
        self.no_antes.insert(id, Instant::now() + self.esperas.ocupado);
        self.actualizar(id, |e| volver(e, detalle));
    }

    fn pendientes(&self) -> Vec<i64> {
        self.filas
            .iter()
            .flat_map(|f| f.envios.iter())
            .filter(|e| e.estado == EstadoEnvio::Pendiente)
            .map(|e| e.id)
            .collect()
    }

    /// (receta, operador, acción, número de destino, comisión) de la fila del envío.
    fn info_envio(&self, id: i64) -> (String, String, String, String, f64) {
        let (fi, _) = self.indice[&id];
        let f = &self.filas[fi];
        (
            f.receta.clone().unwrap_or_default(),
            f.operador.clone().unwrap_or_default(),
            f.accion.clone(),
            f.numero.clone().unwrap_or_default(),
            self.comisiones.get(&f.id).copied().unwrap_or(0.0),
        )
    }

    /// Chips para repartir lo pendiente: todos los del control o, si el lote tiene `origen`, solo los elegidos;
    /// cada uno con el cupo y el saldo de la acción que hace. Un chip elegido que dejó de servir (desconectado,
    /// sin PIN…) queda fuera con el motivo.
    fn chips_reparto(&self) -> Reparto {
        let pares = pares_pendientes(&self.filas, &self.recetas);
        let candidatos = candidatos(&self.ctx, &self.recetas, self.lote.origen.as_ref());
        self.ctx.reparto(&self.recetas, &candidatos, &pares)
    }

    fn revisar_senales(&mut self) {
        if self.ctrl.cancelar.load(Ordering::SeqCst) {
            self.parada = Parada::Cancelar;
        } else if self.ctrl.pausar.load(Ordering::SeqCst) && self.parada == Parada::Ninguna {
            self.parada = Parada::Pausa;
        }
    }

    /// Asigna envíos pendientes a teléfonos libres y lanza sus tareas.
    fn repartir(&mut self, tareas: &mut tokio::task::JoinSet<FinTarea>) {
        let ahora = Instant::now();
        let mut reparto = self.chips_reparto();
        let mut ocupados: std::collections::HashSet<String> = self.en_vuelo.keys().cloned().collect();
        for id in self.pendientes() {
            if self.no_antes.get(&id).is_some_and(|t| *t > ahora) {
                continue;
            }
            let (receta_nombre, operador, accion, numero, comision) = self.info_envio(id);
            let monto = self.envio(id).monto;
            let Some(receta) = nucleo::receta::buscar(&self.recetas, &receta_nombre).cloned() else {
                self.actualizar(id, |e| {
                    e.estado = EstadoEnvio::Fallido;
                    e.detalle = Some(format!("la receta '{receta_nombre}' no existe"));
                });
                continue;
            };
            // Elegir chip y reservar su teléfono. Si otro lo tomó justo ahora (una consulta, una sesión manual),
            // se prueba con el siguiente.
            let elegido: Option<(usize, Receta, Reserva)> = loop {
                let ctx = &self.ctx;
                let Ok(i) = reparto.elegir(&operador, &accion, numero.as_str(), monto, comision, |c| {
                    !ocupados.contains(&c.info.dispositivo) && !ctx.ocupado(&c.info.dispositivo)
                }) else {
                    break None;
                };
                let chip = &reparto.chips[i].info;
                // El chip dealer corre su variante dealer de la receta si existe; si no, la de la compañía.
                let receta = match receta_efectiva_en(&self.recetas, &receta, &chip.tipo) {
                    Ok(r) => r,
                    Err(detalle) => {
                        self.actualizar(id, |e| {
                            e.estado = EstadoEnvio::Fallido;
                            e.detalle = Some(detalle);
                        });
                        break None;
                    }
                };
                let disp = chip.dispositivo.clone();
                match self.ctx.reservar(&disp) {
                    Some(r) => break Some((i, receta, r)),
                    None => {
                        ocupados.insert(disp);
                    }
                }
            };
            let Some((i, receta, reserva)) = elegido else { continue };
            let chip = reparto.chips[i].info.clone();
            // Un envío solo sale si su paso a `en_curso` quedó guardado.
            let guardado = self.despachar(id, |e| {
                e.estado = EstadoEnvio::EnCurso;
                e.dispositivo = Some(chip.dispositivo.clone());
                e.slot = Some(chip.slot);
                e.numero_origen = chip.numero.clone();
                e.intentos += 1;
                e.ts_ms = Some(ahora_ms());
            });
            if let Err(err) = guardado {
                drop(reserva);
                let motivo = format!("no se pudo guardar en la base: {err}");
                tracing::error!(lote = self.lote.id, "{motivo}: pauso el lote sin despachar");
                self.parada = Parada::Pausa;
                self.motivo_parada = Some(motivo);
                return;
            }
            if !self.lote.simular {
                reparto.chips[i].consumir(monto, comision);
            }
            self.ctrl.ritmo.lock().unwrap_or_else(|e| e.into_inner()).desde.get_or_insert_with(Instant::now);
            ocupados.insert(chip.dispositivo.clone());
            self.en_vuelo.insert(chip.dispositivo.clone(), id);
            let ctx = self.ctx.clone();
            let simular = self.lote.simular;
            let mut params = BTreeMap::new();
            params.insert("numero".to_string(), numero);
            // En Bs canónico (`0.50`); el motor agrega `{monto_cent3}` y los demás valores calculados.
            params.insert("monto".to_string(), nucleo::monto::texto(nucleo::monto::de_bs(monto)));
            let (disp, slot) = (chip.dispositivo.clone(), chip.slot);
            let d = disp.clone();
            let tarea = tareas.spawn(async move {
                let r = ctx.ejecutar_reservado(reserva, &receta, &d, slot, &params, simular).await;
                (id, d, slot, r)
            });
            self.tareas_envio.insert(tarea.id(), (id, disp, slot));
        }
    }

    /// La tarea de un envío se cayó (pánico): el envío queda incierto y su teléfono, libre.
    fn tarea_caida(&mut self, tarea: tokio::task::Id, error: String) {
        let Some((id, disp, slot)) = self.tareas_envio.remove(&tarea) else {
            tracing::error!(lote = self.lote.id, "tarea desconocida se cayó: {error}");
            return;
        };
        tracing::error!(lote = self.lote.id, envio = id, %disp, "la tarea del envío se cayó: {error}");
        self.en_vuelo.remove(&disp);
        self.marcar_incierto(id, &disp, slot, None, Some(format!("la tarea del envío se cayó ({error}).")));
    }

    /// Procesa el fin de un envío.
    fn terminar_envio(&mut self, fin: FinTarea) {
        let id = fin.0;
        self.procesar_fin(fin);
        // El ritmo cuenta los envíos que terminaron (no los que vuelven a la cola).
        if !matches!(self.envio(id).estado, EstadoEnvio::Pendiente | EstadoEnvio::EnCurso) {
            self.ctrl.ritmo.lock().unwrap_or_else(|e| e.into_inner()).hechos += 1;
        }
    }

    fn procesar_fin(&mut self, (id, disp, slot, r): FinTarea) {
        self.en_vuelo.remove(&disp);
        let (r, ej) = match r {
            Ok(x) => x,
            Err(err) if err.codigo == codigos::PARAMETRO => {
                // No se marcó nada, pero reintentar no lo arregla (p. ej. el chip ya no tiene PIN guardado).
                self.actualizar(id, |e| {
                    e.estado = EstadoEnvio::Fallido;
                    e.detalle = Some(err.detalle.clone());
                    e.ts_ms = Some(ahora_ms());
                });
                return;
            }
            Err(err) => {
                // No llegó a marcar: el teléfono se fue o estaba ocupado. Reasignar, con tope.
                self.reintentar_ocupado(id, None, format!("reintento: {err}"));
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
            // No se detiene el lote: se avisa y se sigue con lo demás.
            EstadoResultado::Incierto => self.marcar_incierto(id, &disp, slot, ej, r.detalle.clone()),
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
                Tras::Esperar => self.reintentar_ocupado(id, ej, format!("teléfono ocupado, se reintenta: {detalle}")),
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
        let reparto = self.chips_reparto();
        let mut motivo = None;
        for id in self.pendientes() {
            if self.no_antes.get(&id).is_some_and(|t| *t > ahora) {
                return (true, None);
            }
            let (_, operador, accion, numero, comision) = self.info_envio(id);
            match reparto.elegir(&operador, &accion, numero.as_str(), self.envio(id).monto, comision, |_| true) {
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

/// Devuelve el envío a la cola (no se marcó nada).
fn volver(e: &mut EnvioLote, detalle: String) {
    e.estado = EstadoEnvio::Pendiente;
    e.detalle = Some(detalle);
    e.dispositivo = None;
    e.slot = None;
    e.numero_origen = None;
}

/// (operador, acción, receta) de las filas con algo por enviar (o de todas las filas a ejecutar, si `todas`).
fn pares_filas(filas: &[FilaLote], recetas: &BTreeMap<String, Receta>, todas: bool) -> Vec<Par> {
    pares_de(
        filas
            .iter()
            .filter(|f| f.estado != EstadoFila::Omitida)
            .filter(|f| todas || f.envios.iter().any(|e| matches!(e.estado, EstadoEnvio::Pendiente | EstadoEnvio::SinCapacidad)))
            .filter_map(|f| Some((f.operador.clone()?, f.accion.clone(), nucleo::receta::buscar(recetas, f.receta.as_deref()?)?.clone()))),
    )
}

pub fn pares_pendientes(filas: &[FilaLote], recetas: &BTreeMap<String, Receta>) -> Vec<Par> {
    pares_filas(filas, recetas, false)
}

pub fn pares_del_lote(filas: &[FilaLote], recetas: &BTreeMap<String, Receta>) -> Vec<Par> {
    pares_filas(filas, recetas, true)
}

/// Chips en línea que pueden recibir envíos del lote: todos o, con `origen`, solo los elegidos.
pub fn candidatos(ctx: &Contexto, recetas: &BTreeMap<String, Receta>, origen: Option<&OrigenLote>) -> Vec<ChipInfo> {
    let todos = ctx.chips_con(recetas);
    match origen {
        Some(o) => todos.into_iter().filter(|c| o.tiene(&c.dispositivo, c.slot)).collect(),
        None => todos,
    }
}

/// Guarda el saldo inicial de los chips que pueden participar, antes del primer envío de esta corrida (consultando
/// los que tengan un dato viejo). `verificar` lo compara con el saldo de después, por chip y por tipo:
/// - principal: los chips de los operadores con transferencias pendientes;
/// - dealer: los chips dealer (con PIN y receta `saldo_dealer`) que pueden hacer las recargas pendientes. Envía el PIN.
async fn guardar_saldos_iniciales(ctx: &Arc<Contexto>, lote: &Lote, filas: &[FilaLote], recetas: &BTreeMap<String, Receta>) {
    let lote_id = lote.id;
    let pares = pares_pendientes(filas, recetas);
    let mut base: Vec<persistencia::SaldoInicial> = vec![];
    let mut ops: Vec<String> = pares.iter().filter(|p| p.accion == "transferir").map(|p| p.operador.clone()).collect();
    ops.sort();
    ops.dedup();
    if !ops.is_empty() {
        ctx.consultar_saldos(Some(&ops), Some(crate::SALDO_FRESCO_MS), false).await;
        for c in candidatos(ctx, recetas, lote.origen.as_ref()).into_iter().filter(|c| ops.contains(&c.operador)) {
            if let Some(saldo) = c.saldo {
                base.push(persistencia::SaldoInicial { dispositivo: c.dispositivo, slot: c.slot, tipo: "principal".into(), saldo, ts_ms: 0 });
            }
        }
    }
    let recargas: Vec<Par> = pares.into_iter().filter(|p| p.accion != "transferir").collect();
    if !recargas.is_empty() {
        let cand = candidatos(ctx, recetas, lote.origen.as_ref());
        let reparto = ctx.reparto(recetas, &cand, &recargas);
        let mut vistos: Vec<(String, u32)> = vec![];
        for c in reparto.chips.iter().filter(|c| c.info.tipo == TIPO_DEALER) {
            let k = (c.info.dispositivo.clone(), c.info.slot);
            if vistos.contains(&k) {
                continue;
            }
            vistos.push(k);
            match ctx.consultar_saldo_dealer(&c.info.dispositivo, c.info.slot).await {
                Some(saldo) => base.push(persistencia::SaldoInicial {
                    dispositivo: c.info.dispositivo.clone(),
                    slot: c.info.slot,
                    tipo: "dealer".into(),
                    saldo,
                    ts_ms: 0,
                }),
                None => tracing::warn!(
                    "lote {lote_id}: no pude leer el saldo dealer inicial de {} chip {}; ese chip no se podrá verificar",
                    c.info.dispositivo,
                    c.info.slot + 1
                ),
            }
        }
    }
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
    // La receta se toma ahora y se usa esta copia hasta que el lote pare.
    let recetas = ctx.recetas.read().unwrap_or_else(|e| e.into_inner()).clone();
    if !lote.simular {
        guardar_saldos_iniciales(&ctx, &lote, &filas, &recetas).await;
    }
    if let Err(e) = ctx.pers.guardar_progreso(lote_id, &persistencia::progreso(&filas, &comisiones)) {
        tracing::warn!("lote {lote_id}: no pude guardar el progreso: {e}");
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
        ocupado: HashMap::new(),
        en_vuelo: HashMap::new(),
        tareas_envio: HashMap::new(),
        recetas,
        parada: Parada::Ninguna,
        motivo_parada: None,
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
            Some(fin) = tareas.join_next_with_id(), if !tareas.is_empty() => {
                match fin {
                    Ok((tarea, fin)) => {
                        c.tareas_envio.remove(&tarea);
                        c.terminar_envio(fin);
                    }
                    Err(e) => c.tarea_caida(e.id(), e.to_string()),
                }
            }
            _ = c.ctrl.avisar.notified() => {}
            _ = tokio::time::sleep(tic) => {}
        }
    }

    let (estado, motivo) = match c.parada {
        Parada::Cancelar => (EstadoLote::Cancelado, Some("cancelado por el usuario".to_string())),
        Parada::Pausa => (EstadoLote::Pausado, Some(c.motivo_parada.clone().unwrap_or_else(|| "pausado por el usuario".to_string()))),
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
