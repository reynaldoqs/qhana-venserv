//! Contexto compartido por la API, la validación y el ejecutor: dispositivos, recetas, almacén y el
//! estado de los chips (saldo conocido, cupo del día, agotados).

use crate::modelo::{ChipInfo, TIPO_NORMAL, TIPO_PDA};
use crate::normalizar::{operador_de_red, operador_de_sim, sin_acentos};
use crate::persistencia::{CambioChip, Ocupante, Persistencia, SaldoChip};
use almacen::Almacen;
use nucleo::{motor, Bus, ErrorCanal, Receta, Registro};
use protocolo::{ahora_ms, codigos, tipos_evento as ev, EstadoResultado, Resultado};
use serde_json::json;
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

pub type Recetas = Arc<RwLock<BTreeMap<String, Receta>>>;

/// Cada cuánto se reintenta consultar el saldo de un chip nuevo, y cuántas veces (≈ 2 minutos).
const REINTENTO_SALDO: Duration = Duration::from_secs(10);
const INTENTOS_SALDO: u32 = 12;

/// Cupo cuando la receta no declara `limites.por_dia`.
const CUPO_SIN_LIMITE: u32 = 1_000;

// ───────────────────────────── hora local ─────────────────────────────

/// "2026-10-29" en hora local del PC.
pub fn hoy() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// Medianoche local de hoy, en epoch ms.
pub fn inicio_de_hoy_ms() -> i64 {
    use chrono::TimeZone;
    let hoy = chrono::Local::now().date_naive();
    let medianoche = hoy.and_hms_opt(0, 0, 0).expect("medianoche válida");
    chrono::Local
        .from_local_datetime(&medianoche)
        .earliest()
        .map(|d| d.timestamp_millis())
        .unwrap_or_else(|| ahora_ms() - 86_400_000)
}

/// "2026-10-29 18:45:03" en hora local.
pub fn hora_local(ms: i64) -> String {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_default()
}

fn norm(s: &str) -> String {
    sin_acentos(s.trim()).to_lowercase()
}

// ───────────────────────────── contexto ─────────────────────────────

pub struct Contexto {
    pub registro: Arc<Registro>,
    pub bus: Bus,
    pub almacen: Arc<Almacen>,
    pub recetas: Recetas,
    pub pers: Persistencia,
    /// Saldo conocido por (dispositivo, slot).
    saldos: RwLock<HashMap<(String, u32), SaldoChip>>,
    /// Quién ocupa cada ranura (identidad del chip). Se actualiza al cambiar los `sims` de un teléfono.
    ocupantes: RwLock<HashMap<(String, u32), Ocupante>>,
    /// Última lectura de chips ya reconciliada por teléfono (para no tocar la base si nada cambió).
    firmas: Mutex<HashMap<String, String>>,
    /// Chips a los que falta consultarles el saldo → intentos hechos.
    por_consultar: Mutex<HashMap<(String, u32), u32>>,
}

impl Contexto {
    pub fn new(registro: Arc<Registro>, bus: Bus, almacen: Arc<Almacen>, recetas: Recetas) -> rusqlite::Result<Arc<Self>> {
        let pers = Persistencia::new(almacen.clone())?;
        let saldos = pers.saldos()?.into_iter().map(|s| ((s.dispositivo.clone(), s.slot), s)).collect();
        let ocupantes = pers.ocupantes(None)?.into_iter().map(|(d, s, o)| ((d, s), o)).collect();
        Ok(Arc::new(Self {
            registro,
            bus,
            almacen,
            recetas,
            pers,
            saldos: RwLock::new(saldos),
            ocupantes: RwLock::new(ocupantes),
            firmas: Mutex::new(HashMap::new()),
            por_consultar: Mutex::new(HashMap::new()),
        }))
    }

    /// Receta de la compañía para (operador, acción), p. ej. ("TIGO", "transferir"). Ignora las recetas PDA.
    pub fn receta_para(&self, operador: &str, accion: &str) -> Option<Receta> {
        self.receta_para_tipo(operador, accion, TIPO_NORMAL)
    }

    /// Receta para un chip de `tipo`: si es `pda` y existe la receta PDA de esa (operador, acción), esa;
    /// si no, la de la compañía.
    pub fn receta_para_tipo(&self, operador: &str, accion: &str, tipo: &str) -> Option<Receta> {
        let mut v = self.recetas_con_accion(operador, &norm(accion), tipo);
        if v.is_empty() {
            None
        } else {
            Some(v.remove(0))
        }
    }

    /// Recetas que un chip de `tipo` usaría para (operador, acción): las PDA si es `pda` y hay alguna; si no,
    /// las de la compañía.
    fn recetas_con_accion(&self, operador: &str, accion: &str, tipo: &str) -> Vec<Receta> {
        let op = operador_de_red(operador);
        let recetas = self.recetas.read().unwrap();
        let de_la_accion = |pda: bool| -> Vec<Receta> {
            recetas
                .values()
                .filter(|r| {
                    r.accion.as_deref().map(norm).as_deref() == Some(accion)
                        && r.operador.as_deref().map(operador_de_red).as_deref() == Some(op.as_str())
                        && r.chip_tipo.is_some() == pda
                })
                .cloned()
                .collect()
        };
        if tipo == TIPO_PDA {
            let pda = de_la_accion(true);
            if !pda.is_empty() {
                return pda;
            }
        }
        de_la_accion(false)
    }

    /// Nombres de todas las recetas (de la compañía y PDA) de (operador, acción): sirven para contar los
    /// envíos de hoy de un chip aunque haya cambiado de tipo.
    fn nombres_con_accion(&self, operador: &str, accion: &str) -> Vec<String> {
        let op = operador_de_red(operador);
        self.recetas
            .read()
            .unwrap()
            .values()
            .filter(|r| {
                r.accion.as_deref().map(norm).as_deref() == Some(accion)
                    && r.operador.as_deref().map(operador_de_red).as_deref() == Some(op.as_str())
            })
            .map(|r| r.nombre.clone())
            .collect()
    }

    /// Tipo del chip que ocupa la ranura (`normal` si no se sabe).
    pub fn tipo_chip(&self, dispositivo: &str, slot: u32) -> String {
        self.reconciliar_todos();
        let Some(o) = self.ocupante(dispositivo, slot) else { return TIPO_NORMAL.into() };
        self.pers.tipos().unwrap_or_default().remove(&o.chip_id).unwrap_or_else(|| TIPO_NORMAL.into())
    }

    /// La receta que corre en un chip de `tipo` cuando se pidió `receta`:
    /// - chip `pda` y receta de la compañía → su variante PDA (misma acción y operador), si existe;
    /// - receta PDA en un chip que no es `pda` → error.
    pub fn receta_efectiva(&self, receta: &Receta, tipo: &str) -> Result<Receta, String> {
        if receta.chip_tipo.is_some() {
            return if tipo == TIPO_PDA {
                Ok(receta.clone())
            } else {
                Err(format!("la receta '{}' es para chips PDA y este chip es {tipo}", receta.nombre))
            };
        }
        if tipo == TIPO_PDA {
            if let (Some(op), Some(acc)) = (&receta.operador, &receta.accion) {
                if let Some(r) = self.receta_para_tipo(op, acc, TIPO_PDA).filter(|r| r.chip_tipo.is_some()) {
                    return Ok(r);
                }
            }
        }
        Ok(receta.clone())
    }

    /// Marca el tipo del chip que ocupa la ranura. Devuelve el tipo anterior.
    pub fn fijar_tipo(&self, dispositivo: &str, slot: u32, tipo: &str) -> Result<String, String> {
        if tipo != TIPO_NORMAL && tipo != TIPO_PDA {
            return Err(format!("tipo '{tipo}' no válido (normal | pda)"));
        }
        self.reconciliar_todos();
        let o = self.ocupante(dispositivo, slot).ok_or_else(|| "no hay un chip en esa ranura".to_string())?;
        let anterior = self.pers.fijar_tipo(o.chip_id, tipo).map_err(|e| e.to_string())?;
        if anterior != tipo {
            self.bus.emitir(
                ev::CHIP_TIPO,
                json!({ "dispositivo_id": dispositivo, "slot": slot, "tipo": tipo, "anterior": anterior }),
            );
        }
        Ok(anterior)
    }

    pub fn marcar_ocupado(&self, id: &str, ocupado: bool) {
        if self.registro.obtener(id).map(|d| d.ocupado) == Some(ocupado) {
            return;
        }
        if let Some(d) = self.registro.marcar_ocupado(id, ocupado) {
            self.bus.emitir(ev::DISPOSITIVO_ACTUALIZADO, json!({ "dispositivo_id": id, "dispositivo": d }));
        }
    }

    pub fn ocupado(&self, id: &str) -> bool {
        self.registro.obtener(id).map(|d| d.ocupado).unwrap_or(false)
    }

    /// Ejecuta una receta en un chip: marca ocupado, emite eventos, guarda la ejecución y actualiza el
    /// saldo conocido. Es el único camino para correr recetas (API, lotes y consulta de saldos).
    /// Devuelve el resultado y el id de la ejecución guardada.
    pub async fn ejecutar_receta(
        &self,
        receta: &Receta,
        dispositivo: &str,
        slot: u32,
        parametros: &BTreeMap<String, String>,
        simular: bool,
    ) -> Result<(Resultado, Option<i64>), ErrorCanal> {
        let canal = self
            .registro
            .canal(dispositivo)
            .ok_or_else(|| ErrorCanal::nuevo(codigos::SIN_DISPOSITIVO, format!("el dispositivo '{dispositivo}' no está conectado")))?;
        self.marcar_ocupado(dispositivo, true);
        self.bus.emitir(
            ev::RECETA,
            json!({ "fase": "inicio", "receta": receta.nombre, "dispositivo_id": dispositivo, "slot": slot,
                    "simular": simular, "parametros": parametros }),
        );
        let mut r = motor::ejecutar(receta, canal.as_ref(), slot, parametros, simular).await;
        r.dispositivo = Some(dispositivo.to_string());
        self.marcar_ocupado(dispositivo, false);
        let id = match self.almacen.guardar_ejecucion(&r, parametros) {
            Ok(id) => Some(id),
            Err(e) => {
                tracing::warn!("no pude guardar la ejecución: {e}");
                None
            }
        };
        self.registrar_resultado(receta, dispositivo, slot, &r, parametros);
        tracing::info!(receta = %r.receta, %dispositivo, slot, estado = r.estado.como_texto(), ms = r.duracion_ms, "receta ejecutada");
        self.bus.emitir(ev::RECETA, json!({ "fase": "fin", "dispositivo_id": dispositivo, "resultado": r }));
        Ok((r, id))
    }

    /// Actualiza el saldo conocido según lo que hizo la receta.
    fn registrar_resultado(
        &self,
        receta: &Receta,
        dispositivo: &str,
        slot: u32,
        r: &Resultado,
        parametros: &BTreeMap<String, String>,
    ) {
        if r.estado != EstadoResultado::Ok {
            return;
        }
        let accion = receta.accion.as_deref().map(norm).unwrap_or_default();
        match accion.as_str() {
            "saldo" => {
                let leer = |k: &str| r.salida.get(k).and_then(|s| s.replace(',', ".").parse::<f64>().ok());
                // Tigo con saldo en cero responde solo "Saldo Total: 0 Bs vence el …", sin línea de principal.
                // Si el total es 0, el principal también; con cualquier otro total no se asume nada.
                let Some(saldo) = leer("saldo_principal").or_else(|| leer("saldo_total").filter(|t| *t == 0.0)) else {
                    return;
                };
                let numero = r.salida.get("linea").cloned().or_else(|| self.numero_de(dispositivo, slot));
                self.fijar_saldo(dispositivo, slot, saldo, numero, receta.operador.clone());
            }
            "transferir" if !r.simular => {
                let monto = parametros.get("monto").and_then(|m| m.parse::<f64>().ok()).unwrap_or(0.0);
                let comision = r
                    .salida
                    .get("costo")
                    .and_then(|c| c.parse::<f64>().ok())
                    .or_else(|| receta.limites.as_ref().and_then(|l| l.comision))
                    .unwrap_or(0.0);
                self.ajustar_saldo(dispositivo, slot, -(monto + comision));
            }
            _ => {}
        }
    }

    fn numero_de(&self, dispositivo: &str, slot: u32) -> Option<String> {
        self.registro
            .obtener(dispositivo)
            .and_then(|d| d.sims.into_iter().find(|s| s.slot == slot))
            .and_then(|s| s.numero)
    }

    pub fn fijar_saldo(&self, dispositivo: &str, slot: u32, saldo: f64, numero: Option<String>, operador: Option<String>) {
        // El saldo es de quien está en la ranura ahora: primero se registra quién es.
        self.reconciliar_todos();
        let s = SaldoChip {
            dispositivo: dispositivo.into(),
            slot,
            numero,
            operador,
            saldo: (saldo * 100.0).round() / 100.0,
            ts_ms: ahora_ms(),
        };
        if let Err(e) = self.pers.guardar_saldo(&s) {
            tracing::warn!("no pude guardar el saldo: {e}");
        }
        self.saldos.write().unwrap().insert((s.dispositivo.clone(), slot), s);
        if let Err(e) = self.pers.marcar_verificado(dispositivo, slot) {
            tracing::warn!("no pude marcar el chip verificado: {e}");
        }
        if let Some(o) = self.ocupantes.write().unwrap().get_mut(&(dispositivo.to_string(), slot)) {
            o.verificado = true;
        }
        self.por_consultar.lock().unwrap().remove(&(dispositivo.to_string(), slot));
    }

    /// Ajusta el saldo conocido (si se conoce) sin renovar la hora de la última consulta.
    pub fn ajustar_saldo(&self, dispositivo: &str, slot: u32, delta: f64) {
        let mut m = self.saldos.write().unwrap();
        if let Some(s) = m.get_mut(&(dispositivo.to_string(), slot)) {
            s.saldo = ((s.saldo + delta).max(0.0) * 100.0).round() / 100.0;
            if let Err(e) = self.pers.guardar_saldo(s) {
                tracing::warn!("no pude guardar el saldo: {e}");
            }
        }
    }

    pub fn saldo(&self, dispositivo: &str, slot: u32) -> Option<(f64, i64)> {
        self.saldos.read().unwrap().get(&(dispositivo.to_string(), slot)).map(|s| (s.saldo, s.ts_ms))
    }

    /// El operador dijo "máximo de transferencias": el chip no se usa más hoy.
    pub fn agotar_hoy(&self, dispositivo: &str, slot: u32) {
        self.reconciliar_todos();
        let Some(o) = self.ocupante(dispositivo, slot) else {
            tracing::warn!(%dispositivo, slot, "no sé qué chip hay en la ranura: no pude marcarlo agotado");
            return;
        };
        if let Err(e) = self.pers.marcar_agotado(o.chip_id, &hoy()) {
            tracing::warn!("no pude marcar el chip agotado: {e}");
        }
    }

    fn ocupante(&self, dispositivo: &str, slot: u32) -> Option<Ocupante> {
        self.ocupantes.read().unwrap().get(&(dispositivo.to_string(), slot)).copied()
    }

    // ───────────── identidad de los chips ─────────────

    /// Revisa los chips de todos los teléfonos en línea contra quién ocupaba cada ranura. Es barato si nada
    /// cambió, así que se llama antes de usar los chips (reparto, cupo, saldos).
    pub fn reconciliar_todos(&self) {
        for d in self.registro.listar() {
            if d.conectado {
                self.reconciliar(&d.id, &d.sims);
            } else {
                // Al reconectar se vuelve a revisar: el chip pudo cambiar de teléfono mientras tanto.
                self.firmas.lock().unwrap().remove(&d.id);
            }
        }
    }

    fn reconciliar(&self, dispositivo: &str, sims: &[protocolo::Sim]) {
        let firma = sims
            .iter()
            .map(|s| format!("{}|{}|{}|{}", s.slot, s.estado, s.iccid.as_deref().unwrap_or(""), s.numero.as_deref().unwrap_or("")))
            .collect::<Vec<_>>()
            .join(";");
        if self.firmas.lock().unwrap().get(dispositivo) == Some(&firma) {
            return;
        }
        let cambios = match self.pers.reconciliar_sims(dispositivo, sims, ahora_ms()) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(%dispositivo, "no pude registrar los chips: {e}");
                return;
            }
        };
        {
            let mut oc = self.ocupantes.write().unwrap();
            oc.retain(|(d, _), _| d != dispositivo);
            for (d, sl, o) in self.pers.ocupantes(Some(dispositivo)).unwrap_or_default() {
                oc.insert((d, sl), o);
            }
        }
        self.firmas.lock().unwrap().insert(dispositivo.to_string(), firma);
        for c in cambios {
            self.chip_nuevo(c);
        }
    }

    /// Entró un chip a una ranura. Si reemplaza a otro, lo del anterior (saldo) no se hereda.
    fn chip_nuevo(&self, c: CambioChip) {
        if !c.primera_vez {
            self.saldos.write().unwrap().remove(&(c.dispositivo.clone(), c.slot));
            if let Err(e) = self.pers.borrar_saldo(&c.dispositivo, c.slot) {
                tracing::warn!("no pude borrar el saldo del chip anterior: {e}");
            }
            tracing::info!(dispositivo = %c.dispositivo, slot = c.slot, nuevo = ?c.nuevo, anterior = ?c.anterior, "chip cambiado");
            self.bus.emitir(
                ev::CHIP_CAMBIADO,
                json!({ "dispositivo_id": c.dispositivo, "slot": c.slot, "anterior": c.anterior, "nuevo": c.nuevo, "regreso": c.regreso }),
            );
        }
        if self.saldo(&c.dispositivo, c.slot).is_none() {
            self.por_consultar.lock().unwrap().insert((c.dispositivo, c.slot), 0);
        }
    }

    /// Vigila los teléfonos: cada vez que cambian sus chips los reconcilia, y consulta solo el saldo de los
    /// chips nuevos (o conectados sin saldo conocido). Reintenta mientras el chip no tenga servicio.
    pub fn vigilar_chips(self: &Arc<Self>) {
        let ctx = self.clone();
        let mut rx = self.bus.suscribir();
        tokio::spawn(async move {
            let mut reloj = tokio::time::interval(REINTENTO_SALDO);
            loop {
                tokio::select! {
                    e = rx.recv() => match e {
                        Ok(e) if e.tipo == ev::DISPOSITIVO_CONECTADO => ctx.encolar_sin_saldo(),
                        Ok(e) if e.tipo == ev::DISPOSITIVO_ACTUALIZADO => {}
                        Ok(_) => continue,
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                        Err(_) => break,
                    },
                    _ = reloj.tick() => {}
                }
                ctx.reconciliar_todos();
                ctx.consultar_pendientes().await;
            }
        });
    }

    /// Al conectar un teléfono, los chips sin saldo conocido quedan para consultar.
    fn encolar_sin_saldo(&self) {
        self.reconciliar_todos();
        for c in self.chips().into_iter().filter(|c| c.saldo.is_none()) {
            self.por_consultar.lock().unwrap().entry((c.dispositivo, c.slot)).or_insert(0);
        }
    }

    async fn consultar_pendientes(&self) {
        let pendientes: Vec<((String, u32), u32)> = self.por_consultar.lock().unwrap().iter().map(|(k, v)| (k.clone(), *v)).collect();
        if pendientes.is_empty() {
            return;
        }
        let en_linea = self.chips();
        for ((d, sl), intentos) in pendientes {
            let chip = en_linea.iter().find(|c| c.dispositivo == d && c.slot == sl);
            let hecho = match chip {
                // El chip ya no está o ya tiene saldo: nada que consultar.
                None => intentos + 1 >= INTENTOS_SALDO,
                Some(c) if c.saldo.is_some() => true,
                Some(c) => match self.receta_para_tipo(&c.operador, "saldo", &c.tipo) {
                    Some(r) if !self.ocupado(&d) => {
                        if let Err(e) = self.ejecutar_receta(&r, &d, sl, &BTreeMap::new(), false).await {
                            tracing::warn!(%d, sl, "saldo del chip nuevo: {e}");
                        }
                        self.saldo(&d, sl).is_some()
                    }
                    Some(_) => false,
                    None => true,
                },
            };
            let mut m = self.por_consultar.lock().unwrap();
            if hecho || intentos + 1 >= INTENTOS_SALDO {
                m.remove(&(d, sl));
            } else if let Some(n) = m.get_mut(&(d, sl)) {
                *n = intentos + 1;
            }
        }
    }

    /// Chips en línea y listos (conectado, chip puesto, con servicio de voz).
    pub fn chips(&self) -> Vec<ChipInfo> {
        self.reconciliar_todos();
        let desde = inicio_de_hoy_ms();
        let agotados = self.pers.agotados(&hoy()).unwrap_or_default();
        let tipos = self.pers.tipos().unwrap_or_default();
        let mut v = vec![];
        for d in self.registro.listar().into_iter().filter(|d| d.conectado) {
            for s in &d.sims {
                if s.estado != "listo" || !matches!(s.voz.as_deref(), None | Some("en_servicio")) {
                    continue;
                }
                let operador = operador_de_sim(&s.operador, &s.mcc_mnc);
                let ocupante = self.ocupante(&d.id, s.slot);
                let tipo = ocupante.and_then(|o| tipos.get(&o.chip_id).cloned()).unwrap_or_else(|| TIPO_NORMAL.into());
                let transferir = self.recetas_con_accion(&operador, "transferir", &tipo);
                let nombres = self.nombres_con_accion(&operador, "transferir");
                let enviados = ocupante.map(|o| self.pers.enviados_ok(&nombres, o.chip_id, desde).unwrap_or(0)).unwrap_or(0);
                let agotado = ocupante.map(|o| agotados.contains(&o.chip_id)).unwrap_or(false);
                let limite = transferir
                    .iter()
                    .filter_map(|r| r.limites.as_ref().and_then(|l| l.por_dia))
                    .min()
                    .unwrap_or(CUPO_SIN_LIMITE);
                let cupo = if transferir.is_empty() || agotado { 0 } else { limite.saturating_sub(enviados) };
                let (saldo, saldo_ms) = match self.saldo(&d.id, s.slot) {
                    Some((x, t)) => (Some(x), Some(t)),
                    None => (None, None),
                };
                let sin_verificar = ocupante.map(|o| !o.verificado).unwrap_or(false) && saldo.is_none();
                let numero = s.numero.clone().or_else(|| {
                    self.saldos.read().unwrap().get(&(d.id.clone(), s.slot)).and_then(|x| x.numero.clone())
                });
                v.push(ChipInfo {
                    dispositivo: d.id.clone(),
                    slot: s.slot,
                    operador,
                    numero,
                    saldo,
                    saldo_ms,
                    enviados_hoy: enviados,
                    cupo_hoy: cupo,
                    agotado_hoy: agotado,
                    sin_verificar,
                    tipo,
                });
            }
        }
        v
    }

    /// Consulta el saldo de los chips en línea (de `operadores`, o de todos). Solo los que no tienen un dato
    /// más nuevo que `max_edad_ms` (None = todos). En paralelo entre teléfonos, en serie dentro de cada uno.
    /// Los teléfonos ocupados se saltan.
    pub async fn consultar_saldos(self: &Arc<Self>, operadores: Option<&[String]>, max_edad_ms: Option<i64>) -> Vec<ChipInfo> {
        let ahora = ahora_ms();
        let ops: Option<Vec<String>> = operadores.map(|o| o.iter().map(|x| operador_de_red(x)).collect());
        let mut por_disp: BTreeMap<String, Vec<(u32, Receta)>> = BTreeMap::new();
        for c in self.chips() {
            if let Some(ops) = &ops {
                if !ops.contains(&c.operador) {
                    continue;
                }
            }
            if let (Some(max), Some(t)) = (max_edad_ms, c.saldo_ms) {
                if ahora - t < max {
                    continue;
                }
            }
            if let Some(r) = self.receta_para_tipo(&c.operador, "saldo", &c.tipo) {
                por_disp.entry(c.dispositivo.clone()).or_default().push((c.slot, r));
            }
        }
        let mut tareas = tokio::task::JoinSet::new();
        for (disp, chips) in por_disp {
            let ctx = self.clone();
            tareas.spawn(async move {
                for (slot, receta) in chips {
                    if ctx.ocupado(&disp) {
                        tracing::info!(%disp, "saldo: teléfono ocupado, lo salto");
                        break;
                    }
                    if let Err(e) = ctx.ejecutar_receta(&receta, &disp, slot, &BTreeMap::new(), false).await {
                        tracing::warn!(%disp, slot, "saldo: {e}");
                    }
                }
            });
        }
        while tareas.join_next().await.is_some() {}
        self.chips()
    }
}
