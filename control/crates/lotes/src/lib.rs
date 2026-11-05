//! Lotes desde CSV (contrato: `docs/protocolo.md`, sección "Lotes (CSV) — v1").
//!
//! - [`csv`] + [`normalizar`]: lectura tolerante del archivo y limpieza de valores.
//! - [`descomponer`]: 12 Bs → [10, 2] con la menor cantidad de envíos.
//! - [`validacion`]: chequeos por fila y plan de reparto entre chips (no toca teléfonos).
//! - [`contexto`]: dispositivos, recetas, saldo conocido y cupo de cada chip; ejecución de recetas.
//! - [`ejecutor`]: corre un lote con las reglas de seguridad (incierto no se reintenta solo, solo se reintenta lo abortado).
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
use nucleo::Receta;
pub use persistencia::{FiltroFilas, FiltroLotes};
use protocolo::ahora_ms;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, MutexGuard};

/// Edad máxima de un saldo para no volver a consultarlo al validar.
pub const SALDO_FRESCO_MS: i64 = 5 * 60 * 1000;

/// Ventana en la que un lote real igual a otro se considera duplicado.
pub const VENTANA_DUPLICADO_MS: i64 = 24 * 60 * 60 * 1000;

/// Hash (SHA-256, hex) del CSV normalizado (sin espacios al final de las líneas ni líneas vacías) y del origen
/// pedido. Dos lotes con el mismo hash mandarían lo mismo desde lo mismo.
pub fn hash_lote(csv: &str, origen: Option<&OrigenLote>) -> String {
    let normal: Vec<&str> =
        csv.trim_start_matches('\u{feff}').lines().map(str::trim_end).filter(|l| !l.trim().is_empty()).collect();
    let mut h = Sha256::new();
    h.update(normal.join("\n").as_bytes());
    h.update(b"\n--origen--\n");
    if let Some(o) = origen {
        if o.chips.is_empty() {
            // Forma vieja: el mismo texto de siempre (los lotes de antes se siguen reconociendo).
            let d = o.dispositivo.as_deref().unwrap_or("");
            h.update(format!("{d}|{}|{}", o.slot.unwrap_or(0), o.receta.as_deref().unwrap_or("")).as_bytes());
        } else {
            let mut chips: Vec<String> = o.chips.iter().map(|c| format!("{}#{}", c.dispositivo, c.slot)).collect();
            chips.sort();
            chips.dedup();
            let accion = normalizar::accion(o.accion.as_deref().unwrap_or_default());
            h.update(format!("chips:{}|{accion}", chips.join(",")).as_bytes());
        }
    }
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

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

/// ¿Los envíos del lote descuentan del saldo principal? Solo las transferencias; otra acción (p. ej. recarga dealer)
/// gasta el saldo de dealer.
pub fn cuadra_saldo(lote: &Lote) -> bool {
    lote.filas.as_ref().is_none_or(|fs| fs.iter().all(|f| f.estado == EstadoFila::Omitida || f.accion == "transferir"))
}

/// Saldo que gasta un envío de esa acción: `principal` (transferir) o `dealer` (recargar desde un chip dealer).
pub fn tipo_saldo(accion: &str) -> &'static str {
    if accion == "transferir" { "principal" } else { "dealer" }
}

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

    fn activos(&self) -> MutexGuard<'_, HashMap<i64, Arc<Control>>> {
        self.activos.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Al arrancar: todo envío `en_curso`, en cualquier lote y estado, queda `incierto` (no sabemos si se
    /// confirmó); en un lote simulado vuelve a `pendiente` (no mueve dinero). Un lote `en_curso` pasa a `pausado`.
    fn recuperar(&self) -> rusqlite::Result<()> {
        let en_curso = self.ctx.pers.ids_en_estado(EstadoLote::EnCurso)?;
        let mut ids = self.ctx.pers.ids_con_envios_en_curso()?;
        ids.extend(en_curso.iter().copied());
        ids.sort();
        ids.dedup();
        for id in ids {
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
            if lote.estado == EstadoLote::EnCurso {
                self.ctx.pers.estado_lote(id, EstadoLote::Pausado, Some(&motivo))?;
            }
            self.ctx.pers.refrescar_progreso(id)?;
        }
        Ok(())
    }

    // ───────────── validar / crear ─────────────

    /// Revisa los chips elegidos (si hay `origen`).
    fn resolver(&self, recetas: &BTreeMap<String, Receta>, origen: Option<&OrigenLote>) -> ResLote<Option<contexto::OrigenResuelto>> {
        origen.map(|o| self.ctx.resolver_origen(recetas, o).map_err(ErrorLote::parametro)).transpose()
    }

    fn validar_con(&self, csv: &str, origen: Option<&OrigenLote>) -> ResLote<validacion::Validado> {
        let recetas = self.ctx.recetas.read().unwrap().clone();
        let resuelto = self.resolver(&recetas, origen)?;
        let mut v = match &resuelto {
            None => {
                let chips = self.ctx.chips_con(&recetas);
                let reparto = |pares: &[validacion::Par]| (self.ctx.reparto(&recetas, &chips, pares), chips.clone());
                validacion::validar(csv, &recetas, reparto, None).map_err(ErrorLote::parametro)?
            }
            Some(r) => {
                let fijo = validacion::Fijo {
                    accion: r.accion.clone(),
                    recetas: r.recetas.clone(),
                    sin_chip: r.sin_chip.clone(),
                    un_chip: r.un_chip,
                };
                // Solo los chips elegidos que sirven; `chips` los muestra con el cupo y el saldo de esa acción.
                let candidatos: Vec<ChipInfo> = r.chips.iter().map(|c| c.info.clone()).collect();
                let en_linea: Vec<ChipInfo> =
                    self.ctx.chips_con(&recetas).into_iter().filter(|c| candidatos.iter().any(|x| x.dispositivo == c.dispositivo && x.slot == c.slot)).collect();
                let reparto = |pares: &[validacion::Par]| (self.ctx.reparto(&recetas, &en_linea, pares), candidatos.clone());
                validacion::validar(csv, &recetas, reparto, Some(&fijo)).map_err(ErrorLote::parametro)?
            }
        };
        if let Some(r) = resuelto {
            v.validacion.origen = Some(r.origen);
            v.validacion.avisos_origen = r.avisos;
        }
        Ok(v)
    }

    pub async fn validar(&self, csv: &str, consultar_saldos: bool, origen: Option<&OrigenLote>) -> ResLote<Validacion> {
        if origen.is_some() {
            // Chips elegidos: el saldo que importa (si hay transferencias) se consulta solo de sus operadores.
            let v = self.validar_con(csv, origen)?;
            if consultar_saldos && v.validacion.filas.iter().any(|f| f.accion == "transferir") {
                let mut ops: Vec<String> = v
                    .validacion
                    .origen
                    .iter()
                    .flat_map(|o| o.chips.iter().filter_map(|c| c.operador.clone()))
                    .collect();
                ops.sort();
                ops.dedup();
                self.ctx.consultar_saldos(Some(&ops), Some(SALDO_FRESCO_MS), false).await;
                return Ok(self.con_avisos(self.validar_con(csv, origen)?.validacion));
            }
            return Ok(self.con_avisos(v.validacion));
        }
        // Primera pasada para saber qué operadores usa el lote (y validar el formato antes de marcar nada).
        let recetas = self.ctx.recetas.read().unwrap().clone();
        let previa = validacion::validar(csv, &recetas, |_| (validacion::Reparto::default(), vec![]), None)
            .map_err(ErrorLote::parametro)?;
        if consultar_saldos {
            // Solo los operadores con transferencias: las recargas no usan el saldo principal.
            let mut ops: Vec<String> = previa
                .validacion
                .filas
                .iter()
                .filter(|f| f.errores.is_empty() && f.accion == "transferir")
                .filter_map(|f| f.operador.clone())
                .collect();
            ops.sort();
            ops.dedup();
            if !ops.is_empty() {
                self.ctx.consultar_saldos(Some(&ops), Some(SALDO_FRESCO_MS), false).await;
            }
        }
        let v = self.validar_con(csv, None)?;
        Ok(self.con_avisos(v.validacion))
    }

    /// Agrega los avisos del lote entero: recetas cuya versión actual no se ha probado.
    fn con_avisos(&self, mut v: Validacion) -> Validacion {
        let recetas = self.ctx.recetas.read().unwrap().clone();
        let almacen = &self.ctx.almacen;
        let probada = |r: &Receta| {
            r.version.as_deref().is_none_or(|ver| almacen.version_probada(&r.todos_los_nombres(), ver).unwrap_or(true))
        };
        v.resumen.avisos = validacion::avisos_recetas(&v, &recetas, probada);
        v
    }

    /// Crea el lote en `listo`. Un lote real igual a otro real de las últimas 24 h (no cancelado) es
    /// `409 duplicado`, salvo con `forzar`.
    pub fn crear(&self, nombre: &str, csv: &str, simular: bool, origen: Option<&OrigenLote>, forzar: bool) -> ResLote<Lote> {
        let v = self.validar_con(csv, origen)?;
        if v.filas.iter().all(|f| f.estado == EstadoFila::Omitida) {
            return Err(ErrorLote::parametro("ninguna fila es válida: corrige el CSV"));
        }
        let hash = hash_lote(csv, origen);
        if !simular && !forzar {
            if let Some(d) = self.ctx.pers.duplicado(&hash, ahora_ms() - VENTANA_DUPLICADO_MS).map_err(ErrorLote::interno)? {
                let hora = contexto::hora_local(d.creado_ms);
                let hora = hora.get(11..16).unwrap_or(&hora);
                return Err(ErrorLote::new(
                    409,
                    protocolo::codigos::DUPLICADO,
                    format!("Igual al lote #{} «{}», creado {hora}", d.id, d.nombre),
                ));
            }
        }
        let nombre = if nombre.trim().is_empty() {
            format!("Lote {}", contexto::hora_local(ahora_ms()))
        } else {
            nombre.trim().to_string()
        };
        let id = self
            .ctx
            .pers
            .crear_lote(&nombre, simular, ahora_ms(), v.validacion.origen.as_ref(), &v.filas, Some(&hash))
            .map_err(ErrorLote::interno)?;
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

    /// El lote sin filas: progreso guardado, no lee las filas.
    pub fn obtener_sin_filas(&self, id: i64) -> ResLote<Lote> {
        self.ctx.pers.cabecera(id).map_err(ErrorLote::interno)?.ok_or_else(|| ErrorLote::sin_lote(id))
    }

    /// Página de lotes sin filas, más recientes primero (no lee las filas de ningún lote).
    pub fn listar(&self, filtro: &FiltroLotes) -> ResLote<ListaLotes> {
        let (lotes, siguiente, total) = self.ctx.pers.listar(filtro).map_err(ErrorLote::interno)?;
        let conteos = self.ctx.pers.conteos().map_err(ErrorLote::interno)?;
        Ok(ListaLotes { lotes, siguiente, total, conteos })
    }

    /// Página de filas de un lote, por número de fila.
    pub fn filas(&self, id: i64, filtro: &FiltroFilas) -> ResLote<PaginaFilas> {
        self.obtener_sin_filas(id)?;
        let (filas, siguiente, total) = self.ctx.pers.filas(id, filtro).map_err(ErrorLote::interno)?;
        Ok(PaginaFilas { filas, siguiente, total })
    }

    /// Archiva (o saca del archivo) un lote `terminado` o `cancelado`. No cambia nada más.
    pub fn archivar(&self, id: i64, archivado: bool) -> ResLote<Lote> {
        let lote = self.obtener_sin_filas(id)?;
        if !matches!(lote.estado, EstadoLote::Terminado | EstadoLote::Cancelado) || self.activos().contains_key(&id) {
            return Err(ErrorLote::new(409, "no_aplica", "solo se archivan lotes terminados o cancelados"));
        }
        self.ctx.pers.archivar(id, archivado).map_err(ErrorLote::interno)?;
        self.obtener_sin_filas(id)
    }

    // ───────────── control ─────────────

    pub fn iniciar(self: &Arc<Self>, id: i64) -> ResLote<Lote> {
        let lote = self.obtener(id)?;
        let mut activos = self.activos();
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
            yo.activos().remove(&id);
        });
        self.obtener(id)
    }

    pub fn pausar(&self, id: i64) -> ResLote<Lote> {
        let lote = self.obtener(id)?;
        match self.activos().get(&id) {
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
        match self.activos().get(&id) {
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
    /// Con `inciertas` también re-encola los `incierto`: eso NO es seguro por sí solo (pudo haberse hecho), el
    /// operador lo pide tras revisar el saldo. No inicia nada: el operador decide cuándo.
    pub fn reintentar_fallidas(&self, id: i64, inciertas: bool) -> ResLote<Lote> {
        let lote = self.obtener(id)?;
        // Con el candado tomado, `iniciar` no puede arrancar el lote mientras se re-encolan los envíos.
        let activos = self.activos();
        if activos.contains_key(&id) || lote.estado == EstadoLote::EnCurso {
            return Err(ErrorLote::new(409, "en_curso", "el lote está en curso: páusalo antes de reintentar"));
        }
        if lote.estado == EstadoLote::Cancelado {
            return Err(ErrorLote::new(409, "cancelado", "el lote está cancelado"));
        }
        let mut filas = lote.filas.unwrap_or_default();
        let reintentable = |e: &EnvioLote| e.estado == EstadoEnvio::Fallido || (inciertas && e.estado == EstadoEnvio::Incierto);
        let mut reencoladas = 0;
        let mut inciertas_reencoladas = 0;
        for f in &mut filas {
            let mut cambio = false;
            for e in f.envios.iter_mut().filter(|e| reintentable(e)) {
                if e.estado == EstadoEnvio::Incierto {
                    inciertas_reencoladas += 1;
                    tracing::warn!(
                        lote = id,
                        fila = f.fila,
                        monto = e.monto,
                        origen = e.numero_origen.as_deref().unwrap_or("?"),
                        "envío incierto re-encolado por el operador"
                    );
                }
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
            let que = if inciertas { "fallidos ni inciertos" } else { "fallidos" };
            return Err(ErrorLote::new(409, "nada_que_reintentar", format!("el lote no tiene envíos {que}")));
        }
        if lote.estado == EstadoLote::Terminado {
            self.ctx.pers.estado_lote(id, EstadoLote::Listo, None).map_err(ErrorLote::interno)?;
        }
        let _ = self.ctx.pers.borrar_verificacion(id);
        self.ctx.pers.refrescar_progreso(id).map_err(ErrorLote::interno)?;
        drop(activos);
        tracing::info!(lote = id, reencoladas, inciertas_reencoladas, "envíos re-encolados");
        self.ctx.bus.emitir(
            ejecutor::TIPO_EVENTO,
            serde_json::json!({ "lote_id": id, "fase": "reintento", "reencoladas": reencoladas,
                                "inciertas": inciertas_reencoladas }),
        );
        self.obtener(id)
    }

    /// Cuadra el dinero, por chip y por tipo de saldo: consulta el saldo actual de los chips que enviaron algo
    /// (el principal para lo transferido; el dealer para lo recargado desde un chip dealer) y lo compara con el saldo
    /// inicial guardado al último inicio (solo lectura; tarda ~3.5 s por chip).
    pub async fn verificar(&self, id: i64) -> ResLote<Verificacion> {
        let (lote, comisiones) =
            self.ctx.pers.lote(id).map_err(ErrorLote::interno)?.ok_or_else(|| ErrorLote::sin_lote(id))?;
        if self.activos().contains_key(&id) || lote.estado == EstadoLote::EnCurso {
            return Err(ErrorLote::new(409, "en_curso", "el lote está en curso: espera a que pare para verificarlo"));
        }
        if lote.simular {
            return Err(ErrorLote::new(409, "simulado", "es una simulación: no movió dinero, no hay nada que cuadrar"));
        }
        let base = self.ctx.pers.saldos_iniciales(id).map_err(ErrorLote::interno)?;
        let filas = lote.filas.clone().unwrap_or_default();
        let hay_ok = filas.iter().flat_map(|f| f.envios.iter()).any(|e| e.estado == EstadoEnvio::Ok);
        let inicio_ms = base.iter().map(|b| b.ts_ms).min().unwrap_or(0);
        let inicial = |d: &str, sl: u32, tipo: &str| base.iter().find(|b| b.dispositivo == d && b.slot == sl && b.tipo == tipo).map(|b| b.saldo);

        // Lo que se envió desde el último inicio, por chip y tipo de saldo: (cantidad, Bs con comisión, número de
        // origen, operador).
        type Clave = (String, u32, &'static str);
        let mut por_chip: BTreeMap<Clave, (u32, f64, Option<String>, Option<String>)> = Default::default();
        for f in &filas {
            let com = comisiones.get(&f.id).copied().unwrap_or(0.0);
            for e in f.envios.iter().filter(|e| e.estado == EstadoEnvio::Ok && e.ts_ms.unwrap_or(0) >= inicio_ms) {
                let (Some(d), Some(sl)) = (e.dispositivo.clone(), e.slot) else { continue };
                let x = por_chip.entry((d, sl, tipo_saldo(&f.accion))).or_insert((0, 0.0, None, f.operador.clone()));
                x.0 += 1;
                x.1 += e.monto + com;
                if x.2.is_none() {
                    x.2 = e.numero_origen.clone();
                }
            }
        }
        // Un chip con recargas solo se puede cuadrar si tiene saldo dealer inicial o se puede consultar ahora.
        let cuadrable = |(d, sl, tipo): &Clave| *tipo == "principal" || inicial(d, *sl, "dealer").is_some() || self.ctx.receta_saldo_dealer(d, *sl).is_some();
        if por_chip.is_empty() {
            let solo_recargas = !cuadra_saldo(&lote);
            let consultable = || {
                let candidatos: Vec<(String, u32)> = match &lote.origen {
                    Some(o) => o.chips.iter().map(|c| (c.dispositivo.clone(), c.slot)).collect(),
                    None => self.ctx.chips().into_iter().map(|c| (c.dispositivo, c.slot)).collect(),
                };
                candidatos.iter().any(|(d, sl)| self.ctx.receta_saldo_dealer(d, *sl).is_some())
            };
            if solo_recargas && !base.iter().any(|b| b.tipo == "dealer") && !consultable() {
                return Err(no_aplica());
            }
            if base.is_empty() && !hay_ok {
                return Err(ErrorLote::new(409, "sin_base", "el lote nunca se inició: no hay saldo inicial con qué comparar"));
            }
        } else if !por_chip.keys().any(cuadrable) {
            return Err(no_aplica());
        }

        let mut avisos = vec![];
        let mut chips = vec![];
        if por_chip.is_empty() {
            avisos.push("no hubo envíos ok desde el último inicio: no hay nada que cuadrar".to_string());
        } else {
            let desde = ahora_ms();
            // Saldo principal: una consulta por operador (en paralelo entre teléfonos).
            let mut ops: Vec<String> = por_chip
                .iter()
                .filter(|((_, _, t), _)| *t == "principal")
                .filter_map(|(_, x)| x.3.clone())
                .collect();
            ops.sort();
            ops.dedup();
            let en_linea = if ops.is_empty() { self.ctx.chips() } else { self.ctx.consultar_saldos(Some(&ops), None, false).await };
            // Saldo dealer: uno por chip (envía el PIN guardado).
            let mut actuales_dealer = HashMap::new();
            let mut sin_receta_dealer: HashMap<(String, u32), String> = HashMap::new();
            for (d, sl, _) in por_chip.keys().filter(|(_, _, t)| *t == "dealer") {
                if self.ctx.receta_saldo_dealer(d, *sl).is_none() {
                    sin_receta_dealer.insert((d.clone(), *sl), self.ctx.motivo_sin_saldo_dealer(d, *sl));
                    continue;
                }
                if let Some(x) = self.ctx.consultar_saldo_dealer(d, *sl).await {
                    actuales_dealer.insert((d.clone(), *sl), x);
                }
            }
            for ((d, sl, tipo), (n, esperado, numero, _)) in por_chip {
                let ini = inicial(&d, sl, tipo);
                // Solo vale una lectura hecha ahora; una vieja puede ser el saldo "ajustado" por el propio lote.
                let actual = if tipo == "dealer" {
                    actuales_dealer.get(&(d.clone(), sl)).copied()
                } else {
                    self.ctx.saldo(&d, sl).filter(|(_, ts)| *ts >= desde).map(|(s, _)| s)
                };
                let numero = numero.or_else(|| en_linea.iter().find(|c| c.dispositivo == d && c.slot == sl).and_then(|c| c.numero.clone()));
                let esperado = (esperado * 100.0).round() / 100.0;
                let real = ini.zip(actual).map(|(i, a)| ((i - a) * 100.0).round() / 100.0);
                let dif = real.map(|r| ((r - esperado) * 100.0).round() / 100.0);
                let estado = match dif {
                    None => EstadoCuadre::SinDato,
                    Some(x) if x.abs() <= TOLERANCIA_BS + 1e-9 => EstadoCuadre::Cuadra,
                    Some(_) => EstadoCuadre::Diferencia,
                };
                if estado == EstadoCuadre::SinDato {
                    let quien = format!("{} chip {}", numero.as_deref().unwrap_or(&d), sl + 1);
                    avisos.push(match (tipo, ini, sin_receta_dealer.get(&(d.clone(), sl))) {
                        ("dealer", _, Some(m)) => format!("{quien}: {m}"),
                        ("dealer", None, _) => format!("{quien}: no hay saldo dealer inicial (¿el chip no tenía PIN guardado al iniciar?)"),
                        ("dealer", _, _) => format!("{quien}: no se pudo consultar el saldo dealer ahora (¿desconectado, ocupado o sin PIN?)"),
                        (_, None, _) => format!("{quien}: no hay saldo inicial guardado"),
                        _ => format!("{quien}: no se pudo consultar el saldo ahora (¿desconectado u ocupado?)"),
                    });
                }
                chips.push(CuadreChip {
                    dispositivo: d,
                    slot: sl,
                    numero,
                    saldo: tipo.to_string(),
                    estado,
                    saldo_inicial: ini,
                    saldo_actual: actual,
                    descontado_esperado: esperado,
                    descontado_real: real,
                    diferencia_bs: dif,
                    envios_ok: n,
                });
            }
        }
        let cuadra = !chips.is_empty() && chips.iter().all(|c| c.estado == EstadoCuadre::Cuadra);
        let tiene = |t: &str| chips.iter().any(|c| c.saldo == t);
        let saldo = match (tiene("principal"), tiene("dealer")) {
            (true, true) => "mixto",
            (false, true) => "dealer",
            (true, false) => "principal",
            (false, false) if cuadra_saldo(&lote) => "principal",
            _ => "dealer",
        };
        let v = Verificacion {
            lote_id: id,
            verificado_ms: ahora_ms(),
            saldo: saldo.into(),
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

    /// Un carril por chip que participa (o puede participar) en el lote: qué hace, cuánto lleva, su cupo y saldo.
    /// Sirve en vivo (lote en curso) y después (con lo guardado).
    pub fn carriles(&self, id: i64) -> ResLote<Carriles> {
        let lote = self.obtener(id)?;
        let filas = lote.filas.clone().unwrap_or_default();
        let comisiones = self.ctx.pers.lote(id).map_err(ErrorLote::interno)?.map(|(_, c)| c).unwrap_or_default();
        let recetas = self.ctx.recetas.read().unwrap().clone();
        let en_linea = self.ctx.chips_con(&recetas);
        let candidatos = ejecutor::candidatos(&self.ctx, &recetas, lote.origen.as_ref());
        let pares = ejecutor::pares_del_lote(&filas, &recetas);
        let reparto = self.ctx.reparto(&recetas, &candidatos, &pares);
        let duraciones = self.ctx.pers.duraciones(id).map_err(ErrorLote::interno)?;
        let pendientes_de = |op: &str, accion: &str| -> Vec<(f64, f64)> {
            filas
                .iter()
                .filter(|f| f.operador.as_deref() == Some(op) && f.accion == accion)
                .flat_map(|f| {
                    let com = comisiones.get(&f.id).copied().unwrap_or(0.0);
                    f.envios.iter().filter(|e| matches!(e.estado, EstadoEnvio::Pendiente | EstadoEnvio::SinCapacidad)).map(move |e| (e.monto, com))
                })
                .collect()
        };

        // Qué chips van: los que pueden participar (o quedaron fuera con motivo), los elegidos y los que ya enviaron.
        let mut claves: Vec<(String, u32)> = vec![];
        let mut sumar = |d: &str, sl: u32| {
            if !claves.iter().any(|(x, y)| x == d && *y == sl) {
                claves.push((d.to_string(), sl));
            }
        };
        for c in &reparto.chips {
            sumar(&c.info.dispositivo, c.info.slot);
        }
        for e in &reparto.excluidos {
            sumar(&e.info.dispositivo, e.info.slot);
        }
        if let Some(o) = &lote.origen {
            for c in &o.chips {
                sumar(&c.dispositivo, c.slot);
            }
        }
        for e in filas.iter().flat_map(|f| f.envios.iter()) {
            if let (Some(d), Some(sl)) = (&e.dispositivo, e.slot) {
                sumar(d, sl);
            }
        }
        claves.sort();

        let mut carriles = vec![];
        for (d, sl) in claves {
            let info = en_linea.iter().find(|c| c.dispositivo == d && c.slot == sl);
            // La acción con más envíos pendientes manda (un chip puede transferir y recargar en el mismo lote).
            let mut entradas: Vec<&validacion::ChipReparto> =
                reparto.chips.iter().filter(|c| c.info.dispositivo == d && c.info.slot == sl).collect();
            entradas.sort_by_key(|c| std::cmp::Reverse(pendientes_de(&c.info.operador, &c.accion).len()));
            let entrada = entradas.first().copied();
            let excluido = reparto.excluidos.iter().find(|e| e.info.dispositivo == d && e.info.slot == sl);
            let origen_chip = lote.origen.as_ref().and_then(|o| o.chips.iter().find(|c| c.dispositivo == d && c.slot == sl));
            let envios: Vec<(&FilaLote, &EnvioLote)> = filas
                .iter()
                .flat_map(|f| f.envios.iter().map(move |e| (f, e)))
                .filter(|(_, e)| e.dispositivo.as_deref() == Some(d.as_str()) && e.slot == Some(sl))
                .collect();
            let cuenta = |x: EstadoEnvio| envios.iter().filter(|(_, e)| e.estado == x).count() as u32;
            let actual = envios.iter().find(|(_, e)| e.estado == EstadoEnvio::EnCurso).map(|(f, e)| ActualCarril {
                fila: f.fila,
                numero: f.numero.clone(),
                monto: e.monto,
                desde_ms: e.ts_ms,
            });
            let enviado_bs =
                normalizar::centavos(envios.iter().filter(|(_, e)| e.estado == EstadoEnvio::Ok).map(|(_, e)| e.monto).sum());
            let numero = info
                .and_then(|c| c.numero.clone())
                .or_else(|| origen_chip.and_then(|c| c.numero.clone()))
                .or_else(|| envios.iter().find_map(|(_, e)| e.numero_origen.clone()));
            let mut motivo = None;
            let estado = if actual.is_some() {
                "enviando"
            } else if info.is_none() {
                "desconectado"
            } else if let (None, Some(x)) = (entrada, excluido) {
                motivo = Some(x.motivo.clone());
                "excluido"
            } else if let Some(c) = entrada {
                let pend = pendientes_de(&c.info.operador, &c.accion);
                let sin_saldo = c.saldo.is_some_and(|s| !pend.is_empty() && pend.iter().all(|(m, com)| s + 1e-9 < m + com));
                if c.info.sin_verificar {
                    motivo = Some("chip nuevo sin saldo verificado".into());
                    "sin_capacidad"
                } else if c.cupo == 0 {
                    motivo = Some(if c.info.agotado_hoy { "llegó al máximo diario del operador" } else { "sin cupo hoy" }.into());
                    "sin_capacidad"
                } else if sin_saldo {
                    motivo = Some("saldo insuficiente".into());
                    "sin_capacidad"
                } else if self.ctx.ocupado(&d) {
                    "esperando"
                } else {
                    "libre"
                }
            } else {
                // En línea, pero el lote ya no le pide nada (p. ej. de otro operador o ya terminado).
                "libre"
            };
            let saldo = match (entrada, info) {
                (Some(c), Some(i)) if c.accion == "transferir" => {
                    i.saldo.map(|bs| SaldoCarril { tipo: "principal".into(), bs, ms: i.saldo_ms })
                }
                (Some(_), Some(i)) if i.tipo == TIPO_DEALER => i.saldo_dealer.map(|bs| SaldoCarril { tipo: TIPO_DEALER.into(), bs, ms: i.saldo_dealer_ms }),
                _ => None,
            };
            carriles.push(Carril {
                telefono: self.ctx.registro.obtener(&d).map(|x| x.modelo),
                operador: info
                    .map(|c| c.operador.clone())
                    .or_else(|| origen_chip.and_then(|c| c.operador.clone()))
                    .or_else(|| envios.first().and_then(|(f, _)| f.operador.clone())),
                numero,
                tipo: info.map(|c| c.tipo.clone()).or_else(|| origen_chip.and_then(|c| c.tipo.clone())),
                // Con el nombre actual: un lote guardado antes de renombrar la receta trae el nombre anterior.
                receta: entrada
                    .and_then(|c| c.receta.clone())
                    .or_else(|| origen_chip.and_then(|c| c.receta.clone()))
                    .map(|n| nucleo::receta::buscar(&recetas, &n).map(|r| r.nombre.clone()).unwrap_or(n)),
                estado: estado.into(),
                actual,
                ok: cuenta(EstadoEnvio::Ok) + cuenta(EstadoEnvio::Simulado),
                fallidos: cuenta(EstadoEnvio::Fallido),
                inciertos: cuenta(EstadoEnvio::Incierto),
                enviado_bs,
                cupo_restante: entrada.filter(|c| !c.sin_limite).map(|c| c.cupo),
                cupo_sin_limite: entrada.is_some_and(|c| c.sin_limite),
                saldo,
                ms_promedio: duraciones.get(&(d.clone(), sl)).map(|(ms, _)| *ms),
                motivo,
                dispositivo: d,
                slot: sl,
            });
        }
        let pendientes = filas
            .iter()
            .flat_map(|f| f.envios.iter())
            .filter(|e| matches!(e.estado, EstadoEnvio::Pendiente | EstadoEnvio::EnCurso))
            .count() as u32;
        let eta_ms = match self.activos().get(&id) {
            Some(c) if lote.estado == EstadoLote::EnCurso => c.ritmo.lock().unwrap_or_else(|e| e.into_inner()).eta_ms(pendientes),
            _ => None,
        };
        Ok(Carriles { carriles, pendientes, eta_ms })
    }

    /// ¿Hay un lote corriendo? (para tests y la interfaz)
    pub fn en_curso(&self) -> Vec<i64> {
        self.activos().keys().copied().collect()
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
            // Sin repetir y en el orden en que se usaron (dedup solo quita repetidos seguidos).
            let mut chips: Vec<String> = vec![];
            for c in f
                .envios
                .iter()
                .filter(|e| matches!(e.estado, EstadoEnvio::Ok | EstadoEnvio::Simulado | EstadoEnvio::Incierto))
                .map(|e| {
                    e.numero_origen.clone().unwrap_or_else(|| {
                        format!("{}#{}", e.dispositivo.clone().unwrap_or_default(), e.slot.map(|s| s + 1).unwrap_or(0))
                    })
                })
            {
                if !chips.contains(&c) {
                    chips.push(c);
                }
            }
            let hora = f.envios.iter().filter_map(|e| e.ts_ms).max().map(contexto::hora_local).unwrap_or_default();
            let campos = [
                f.fila.to_string(),
                f.numero.clone().unwrap_or_default(),
                f.operador.clone().unwrap_or_default(),
                f.accion.clone(),
                f.monto.map(normalizar::bs).unwrap_or_default(),
                f.referencia.clone().unwrap_or_default(),
                f.estado.texto().to_string(),
                normalizar::bs(f.enviado_bs),
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

fn no_aplica() -> ErrorLote {
    ErrorLote::new(
        409,
        "no_aplica",
        "ningún chip del lote se puede cuadrar: no gastan el saldo principal y no son dealer con PIN guardado y receta de saldo dealer",
    )
}

/// Un texto que Excel ejecutaría como fórmula (`=`, `+`, `-`, `@`, tabulador o retorno al inicio) va precedido
/// de `'`. Un número (`-5`, `+59178815232`, `12.50`) no se toca.
fn sin_formula(c: &str) -> String {
    let peligroso = c.starts_with(['=', '+', '-', '@', '\t', '\r']);
    if peligroso && c.trim().parse::<f64>().is_err() {
        format!("'{c}")
    } else {
        c.to_string()
    }
}

/// Entrecomilla si hace falta (separador, comillas o saltos de línea) y neutraliza fórmulas.
fn campo_csv(c: &str, sep: char) -> String {
    let c = &sin_formula(c);
    if c.contains(sep) || c.contains('"') || c.contains('\n') || c.contains('\r') {
        format!("\"{}\"", c.replace('"', "\"\""))
    } else {
        c.to_string()
    }
}

/// CSV de ejemplo (transferencias) para `GET /api/lotes/plantilla.csv`.
pub const PLANTILLA: &str = "\u{feff}numero,monto,operador,accion,referencia\r\n\
69869869,12,TIGO,transferir,Pedido 1001\r\n\
+591 7881-5232,5,TIGO,,Pedido 1002\r\n\
70000000,20,TIGO,transferir,Pedido 1003\r\n";

/// CSV de ejemplo de `GET /api/lotes/plantilla.csv?tipo=…&separador=…`: `transferencias` (por defecto), `recargas`
/// (varios operadores, `recargar`, montos con centavos) o `chips` (para "desde chips elegidos"). `None` si el tipo
/// no existe. Los montos usan punto decimal, así que con `;` no hace falta entrecomillar nada.
pub fn plantilla(tipo: Option<&str>, separador: char) -> Option<String> {
    let filas: &[&[&str]] = match tipo.map(str::trim).filter(|t| !t.is_empty()).unwrap_or("transferencias") {
        "transferencias" => &[
            &["numero", "monto", "operador", "accion", "referencia"],
            &["69869869", "12", "TIGO", "transferir", "Pedido 1001"],
            &["+591 7881-5232", "5", "TIGO", "", "Pedido 1002"],
            &["70000000", "20", "TIGO", "transferir", "Pedido 1003"],
        ],
        "recargas" => &[
            &["numero", "monto", "operador", "accion", "referencia"],
            &["71234567", "0.50", "ENTEL", "recargar", "Cliente 1"],
            &["69869869", "1.50", "TIGO", "recargar", "Cliente 2"],
            &["70000000", "5", "TIGO", "recargar", "Cliente 3"],
        ],
        "chips" => &[
            &["numero", "monto", "operador", "referencia"],
            &["69869869", "5", "TIGO", "Pedido 1001"],
            &["71234567", "0.50", "ENTEL", "Pedido 1002"],
            &["70000000", "1.50", "TIGO", "Pedido 1003"],
        ],
        _ => return None,
    };
    let mut s = String::from("\u{feff}");
    for f in filas {
        s.push_str(&f.join(&separador.to_string()));
        s.push_str("\r\n");
    }
    Some(s)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod tests_csv_salida {
    use super::*;

    #[test]
    fn neutraliza_formulas_pero_no_numeros() {
        assert_eq!(campo_csv("=HYPERLINK(\"x\")", ','), "\"'=HYPERLINK(\"\"x\"\")\"");
        assert_eq!(campo_csv("@SUM(A1)", ';'), "'@SUM(A1)");
        assert_eq!(campo_csv("+cmd|' /C calc'!A0", ';'), "'+cmd|' /C calc'!A0");
        assert_eq!(campo_csv("-2+3", ','), "'-2+3");
        assert_eq!(campo_csv("-5", ','), "-5");
        assert_eq!(campo_csv("+59178815232", ','), "+59178815232");
        assert_eq!(campo_csv("12.50", ','), "12.50");
        assert_eq!(campo_csv("Pedido 1001", ','), "Pedido 1001");
    }

    #[test]
    fn plantillas() {
        assert_eq!(plantilla(None, ',').unwrap(), PLANTILLA);
        let r = plantilla(Some("recargas"), ';').unwrap();
        assert!(r.starts_with("\u{feff}numero;monto;operador;accion;referencia\r\n"), "{r}");
        assert!(r.contains("71234567;0.50;ENTEL;recargar;"));
        let c = plantilla(Some("chips"), ',').unwrap();
        assert!(c.contains("numero,monto,operador,referencia\r\n"));
        assert!(plantilla(Some("otra"), ',').is_none());
        // Cada plantilla se valida sin errores de formato.
        for t in ["transferencias", "recargas", "chips"] {
            let csv = plantilla(Some(t), ';').unwrap();
            let leido = csv::leer(&csv, true).unwrap();
            assert_eq!(leido.filas.len(), 3, "{t}");
            assert!(leido.filas.iter().all(|f| f.error.is_none()), "{t}");
        }
    }

    #[test]
    fn hash_ignora_espacios_finales_y_lineas_vacias() {
        let a = hash_lote("numero,monto,operador\n69869869,5,TIGO\n", None);
        assert_eq!(a, hash_lote("\u{feff}numero,monto,operador  \r\n\r\n69869869,5,TIGO\r\n\n", None));
        assert_ne!(a, hash_lote("numero,monto,operador\n69869869,6,TIGO\n", None));
        let o = OrigenLote::un_chip("t", 0, "r");
        assert_ne!(a, hash_lote("numero,monto,operador\n69869869,5,TIGO\n", Some(&o)));
        assert_eq!(a.len(), 64);
    }
}
