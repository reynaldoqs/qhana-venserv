//! Contexto compartido por la API, la validación y el ejecutor: dispositivos, recetas, almacén y el
//! estado de los chips (saldo conocido, cupo del día, agotados).

use crate::modelo::{ChipInfo, ChipOrigen, OrigenLote, ACCION_SALDO_DEALER, TIPO_NORMAL, TIPO_DEALER};
use crate::validacion::{ChipReparto, Excluido, Par, Reparto};
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

/// Los chips elegidos de un lote, revisados: los que sirven (con su capacidad), los motivos de los que no y la
/// receta con que se validan las filas de cada operador.
#[derive(Debug, Clone)]
pub struct OrigenResuelto {
    /// El origen completo (operador, número, tipo y receta de cada chip que sirve).
    pub origen: OrigenLote,
    pub accion: String,
    pub recetas: BTreeMap<String, Receta>,
    pub chips: Vec<ChipReparto>,
    /// Operadores con chips elegidos pero ninguno que sirva → motivo.
    pub sin_chip: BTreeMap<String, String>,
    /// Chips excluidos, con su motivo (`Validacion.avisos_origen`).
    pub avisos: Vec<String>,
    /// Forma vieja (un chip con su receta).
    pub un_chip: bool,
}

/// Cada cuánto se reintenta consultar el saldo de un chip nuevo, y cuántas veces (≈ 2 minutos).
const REINTENTO_SALDO: Duration = Duration::from_secs(10);
const INTENTOS_SALDO: u32 = 12;

/// Cupo cuando la receta no declara `limites.por_dia`.
pub const CUPO_SIN_LIMITE: u32 = 1_000;

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

fn sin_dispositivo(id: &str) -> ErrorCanal {
    ErrorCanal::nuevo(codigos::SIN_DISPOSITIVO, format!("el dispositivo '{id}' no está conectado"))
}

// ───────────────────────────── recetas por chip ─────────────────────────────

/// Recetas que un chip de `tipo` usaría para (operador, acción) dentro de `recetas`: las dealer si es `dealer` y hay
/// alguna; si no, las de la compañía.
pub fn recetas_con_accion_en(recetas: &BTreeMap<String, Receta>, operador: &str, accion: &str, tipo: &str) -> Vec<Receta> {
    let op = operador_de_red(operador);
    let accion = norm(accion);
    let de_la_accion = |dealer: bool| -> Vec<Receta> {
        recetas
            .values()
            .filter(|r| {
                r.accion.as_deref().map(norm).as_deref() == Some(accion.as_str())
                    && r.operador.as_deref().map(operador_de_red).as_deref() == Some(op.as_str())
                    && r.chip_tipo.is_some() == dealer
            })
            .cloned()
            .collect()
    };
    if tipo == TIPO_DEALER {
        let dealer = de_la_accion(true);
        if !dealer.is_empty() {
            return dealer;
        }
    }
    de_la_accion(false)
}

/// Nombres de todas las recetas (de la compañía y dealer) de (operador, acción): sirven para contar los envíos de
/// hoy de un chip aunque haya cambiado de tipo.
/// Incluye los `nombres_anteriores`: los envíos de hoy hechos antes de renombrar una receta también cuentan.
fn nombres_con_accion_en(recetas: &BTreeMap<String, Receta>, operador: &str, accion: &str) -> Vec<String> {
    let op = operador_de_red(operador);
    recetas
        .values()
        .filter(|r| {
            r.accion.as_deref().map(norm).as_deref() == Some(accion)
                && r.operador.as_deref().map(operador_de_red).as_deref() == Some(op.as_str())
        })
        .flat_map(|r| r.todos_los_nombres().into_iter().map(str::to_string))
        .collect()
}

/// La receta que corre en un chip de `tipo` cuando se pidió `receta`, buscando la variante en `recetas`:
/// - chip `dealer` y receta de la compañía → su variante dealer (misma acción y operador), si existe;
/// - receta dealer en un chip que no es `dealer` → error.
pub fn receta_efectiva_en(recetas: &BTreeMap<String, Receta>, receta: &Receta, tipo: &str) -> Result<Receta, String> {
    if receta.chip_tipo.is_some() {
        return if tipo == TIPO_DEALER {
            Ok(receta.clone())
        } else {
            Err(format!("la receta '{}' es para chips dealer y este chip es {tipo}", receta.nombre))
        };
    }
    if tipo == TIPO_DEALER {
        if let (Some(op), Some(acc)) = (&receta.operador, &receta.accion) {
            if let Some(r) = recetas_con_accion_en(recetas, op, acc, TIPO_DEALER).into_iter().find(|r| r.chip_tipo.is_some()) {
                return Ok(r);
            }
        }
    }
    Ok(receta.clone())
}

// ───────────────────────────── reserva de un teléfono ─────────────────────────────

/// Un teléfono reservado (figura `ocupado`). Al soltarse (fin normal, error o pánico de la tarea) se libera y
/// se avisa a la interfaz. Si el teléfono se reconectó y lo tomó otro, soltar una reserva vieja no lo toca.
pub struct Reserva {
    registro: Arc<Registro>,
    bus: Bus,
    id: String,
    ficha: u64,
}

impl Reserva {
    pub fn dispositivo(&self) -> &str {
        &self.id
    }
}

impl Drop for Reserva {
    fn drop(&mut self) {
        if let Some(d) = self.registro.liberar(&self.id, self.ficha) {
            self.bus.emitir(ev::DISPOSITIVO_ACTUALIZADO, json!({ "dispositivo_id": self.id, "dispositivo": d }));
        }
    }
}

/// Ejecución anotada en el historial mientras corre. Si se suelta sin terminar (pánico), queda `incierto`.
struct EjecucionAbierta<'a> {
    almacen: &'a Almacen,
    id: i64,
    terminada: bool,
}

impl Drop for EjecucionAbierta<'_> {
    fn drop(&mut self) {
        if !self.terminada {
            let _ = self.almacen.marcar_incierta(self.id, "la ejecución se cortó por un error interno: revisa el saldo");
        }
    }
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
    /// Último intento de consulta de cada chip pendiente: se reintenta cada [`REINTENTO_SALDO`], no en cada
    /// evento (cada reserva del teléfono emite uno).
    ultimo_intento: Mutex<HashMap<(String, u32), std::time::Instant>>,
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
            ultimo_intento: Mutex::new(HashMap::new()),
        }))
    }

    /// Receta de la compañía para (operador, acción), p. ej. ("TIGO", "transferir"). Ignora las recetas dealer.
    pub fn receta_para(&self, operador: &str, accion: &str) -> Option<Receta> {
        self.receta_para_tipo(operador, accion, TIPO_NORMAL)
    }

    /// Receta para un chip de `tipo`: si es `dealer` y existe la receta dealer de esa (operador, acción), esa;
    /// si no, la de la compañía.
    pub fn receta_para_tipo(&self, operador: &str, accion: &str, tipo: &str) -> Option<Receta> {
        let mut v = self.recetas_con_accion(operador, accion, tipo);
        if v.is_empty() {
            None
        } else {
            Some(v.remove(0))
        }
    }

    /// Recetas que un chip de `tipo` usaría para (operador, acción): las dealer si es `dealer` y hay alguna; si no,
    /// las de la compañía.
    fn recetas_con_accion(&self, operador: &str, accion: &str, tipo: &str) -> Vec<Receta> {
        recetas_con_accion_en(&self.recetas.read().unwrap(), operador, accion, tipo)
    }


    /// Tipo del chip que ocupa la ranura (`normal` si no se sabe).
    pub fn tipo_chip(&self, dispositivo: &str, slot: u32) -> String {
        self.reconciliar_todos();
        let Some(o) = self.ocupante(dispositivo, slot) else { return TIPO_NORMAL.into() };
        self.pers.tipos().unwrap_or_default().remove(&o.chip_id).unwrap_or_else(|| TIPO_NORMAL.into())
    }

    /// La receta que corre en un chip de `tipo` cuando se pidió `receta`:
    /// - chip `dealer` y receta de la compañía → su variante dealer (misma acción y operador), si existe;
    /// - receta dealer en un chip que no es `dealer` → error.
    pub fn receta_efectiva(&self, receta: &Receta, tipo: &str) -> Result<Receta, String> {
        receta_efectiva_en(&self.recetas.read().unwrap(), receta, tipo)
    }

    /// Qué puede hacer `chip` para las filas de `par` en un lote: la receta que correría (su variante dealer si
    /// corresponde) con el cupo y el saldo que cuentan para esa acción. Error = por qué no sirve.
    /// - transferir: el cupo y el saldo principal del chip, como siempre;
    /// - otra acción (recargar): el cupo es `limites.por_dia` de esa receta (sin límite si no tiene) y el saldo
    ///   principal no se usa (una recarga de dealer gasta otro saldo).
    fn capacidad(
        &self,
        recetas: &BTreeMap<String, Receta>,
        chip: &ChipInfo,
        par: &Par,
        enviados_por_chip: &HashMap<i64, HashMap<String, u32>>,
    ) -> Result<ChipReparto, String> {
        let receta = receta_efectiva_en(recetas, &par.receta, &chip.tipo).map_err(|_| {
            format!("es un chip {} y la receta '{}' es solo para chips dealer", chip.tipo, par.receta.nombre)
        })?;
        // El lote solo trae número y monto (y el PIN sale del chip).
        if let Some(p) = receta
            .parametros
            .iter()
            .find(|p| !(matches!(p.nombre.as_str(), "numero" | "monto") || p.secreto && p.nombre == "pin"))
        {
            return Err(format!("la receta '{}' pide '{}', que un lote no tiene", receta.nombre, p.nombre));
        }
        if pide_pin(&receta) && !chip.tiene_pin {
            return Err("sin PIN guardado: guárdalo en el Panel antes de usarlo en un lote".into());
        }
        let limite = receta.limites.as_ref().and_then(|l| l.por_dia);
        if par.accion == "transferir" {
            return Ok(ChipReparto {
                info: chip.clone(),
                accion: par.accion.clone(),
                receta: Some(receta.nombre.clone()),
                saldo: chip.saldo,
                cupo: chip.cupo_hoy,
                // El de `chips_con` (con todas las recetas de transferir y "agotado hoy"), como el cupo.
                sin_limite: chip.sin_limite,
                usados: chip.enviados_hoy,
            });
        }
        let nombres = nombres_con_accion_en(recetas, &chip.operador, &par.accion);
        let enviados: u32 = self
            .ocupante(&chip.dispositivo, chip.slot)
            .and_then(|o| enviados_por_chip.get(&o.chip_id))
            .map(|por_receta| nombres.iter().filter_map(|n| por_receta.get(n)).sum())
            .unwrap_or(0);
        let cupo = limite.map(|l| l.saturating_sub(enviados)).unwrap_or(CUPO_SIN_LIMITE);
        let mut info = chip.clone();
        info.enviados_hoy = enviados;
        info.cupo_hoy = cupo;
        info.agotado_hoy = false;
        info.sin_limite = limite.is_none();
        info.accion = Some(par.accion.clone());
        info.saldo = None;
        info.saldo_ms = None;
        info.sin_verificar = false;
        Ok(ChipReparto {
            info,
            accion: par.accion.clone(),
            receta: Some(receta.nombre.clone()),
            saldo: None,
            cupo,
            sin_limite: limite.is_none(),
            usados: enviados,
        })
    }

    /// Los chips para repartir las filas de `pares` entre `candidatos` (de cada par, solo los de su operador).
    pub fn reparto(&self, recetas: &BTreeMap<String, Receta>, candidatos: &[ChipInfo], pares: &[Par]) -> Reparto {
        let enviados = if pares.iter().any(|p| p.accion != "transferir") {
            self.pers.enviados_ok_por_chip(inicio_de_hoy_ms()).unwrap_or_default()
        } else {
            HashMap::new()
        };
        let mut r = Reparto::default();
        for par in pares {
            for c in candidatos.iter().filter(|c| c.operador == par.operador) {
                // Dos recetas para la misma acción (no debería): basta con un lugar por chip y acción.
                if r.chips.iter().any(|x| x.info.dispositivo == c.dispositivo && x.info.slot == c.slot && x.accion == par.accion) {
                    continue;
                }
                match self.capacidad(recetas, c, par, &enviados) {
                    Ok(x) => r.chips.push(x),
                    Err(motivo) => r.excluidos.push(Excluido { info: c.clone(), accion: par.accion.clone(), motivo }),
                }
            }
        }
        r
    }

    /// Revisa los chips elegidos de un lote (`origen`) y los completa. Error = ninguno sirve (con los motivos).
    pub fn resolver_origen(&self, recetas: &BTreeMap<String, Receta>, o: &OrigenLote) -> Result<OrigenResuelto, String> {
        let en_linea = self.chips_con(recetas);
        let enviados = self.pers.enviados_ok_por_chip(inicio_de_hoy_ms()).unwrap_or_default();
        let nombre_chip = |c: &ChipInfo| format!("{} chip {}", c.numero.as_deref().unwrap_or(&c.dispositivo), c.slot + 1);
        if o.chips.is_empty() {
            // Forma vieja: un chip con la receta elegida.
            let dispositivo = o.dispositivo.clone().ok_or("el origen no tiene chips")?;
            let slot = o.slot.unwrap_or(0);
            let nombre = o.receta.clone().ok_or("falta la receta del chip elegido")?;
            let receta = nucleo::receta::buscar(recetas, &nombre).cloned().ok_or_else(|| format!("no existe la receta '{nombre}'"))?;
            let chip = en_linea
                .iter()
                .find(|c| c.dispositivo == dispositivo && c.slot == slot)
                .cloned()
                .ok_or_else(|| "el chip elegido no está en línea o no tiene servicio".to_string())?;
            if let Some(op) = receta.operador.as_deref().map(operador_de_red) {
                if op != chip.operador {
                    return Err(format!("la receta '{}' es de {op} y el chip es de {}", receta.nombre, chip.operador));
                }
            }
            let efectiva = receta_efectiva_en(recetas, &receta, &chip.tipo)?;
            let accion = crate::normalizar::accion(efectiva.accion.as_deref().unwrap_or_default());
            let par = Par { operador: chip.operador.clone(), accion: accion.clone(), receta: efectiva.clone() };
            let x = self.capacidad(recetas, &chip, &par, &enviados).map_err(|m| format!("el chip elegido no sirve: {m}"))?;
            let completo = OrigenLote {
                chips: vec![ChipOrigen {
                    dispositivo: chip.dispositivo.clone(),
                    slot: chip.slot,
                    operador: Some(chip.operador.clone()),
                    numero: chip.numero.clone(),
                    tipo: Some(chip.tipo.clone()),
                    receta: Some(efectiva.nombre.clone()),
                }],
                accion: Some(accion.clone()),
                dispositivo: Some(chip.dispositivo.clone()),
                slot: Some(chip.slot),
                // Con el nombre actual: el pedido pudo traer un nombre anterior de la receta.
                receta: Some(receta.nombre.clone()),
                operador: Some(chip.operador.clone()),
                numero: chip.numero.clone(),
            };
            let mut recetas_op = BTreeMap::new();
            recetas_op.insert(chip.operador.clone(), efectiva);
            return Ok(OrigenResuelto { origen: completo, accion, recetas: recetas_op, chips: vec![x], sin_chip: BTreeMap::new(), avisos: vec![], un_chip: true });
        }

        let accion = crate::normalizar::accion(o.accion.as_deref().unwrap_or_default());
        if o.accion.as_deref().is_none_or(|a| a.trim().is_empty()) {
            return Err("falta 'accion' en el origen (p. ej. \"recargar\")".into());
        }
        let mut validos: Vec<ChipReparto> = vec![];
        let mut chips_origen: Vec<ChipOrigen> = vec![];
        let mut motivos: Vec<String> = vec![];
        let mut sin_chip: BTreeMap<String, String> = BTreeMap::new();
        let mut recetas_op: BTreeMap<String, Receta> = BTreeMap::new();
        let mut vistos: Vec<(String, u32)> = vec![];
        for pedido in &o.chips {
            if vistos.contains(&(pedido.dispositivo.clone(), pedido.slot)) {
                continue;
            }
            vistos.push((pedido.dispositivo.clone(), pedido.slot));
            let Some(chip) = en_linea.iter().find(|c| c.dispositivo == pedido.dispositivo && c.slot == pedido.slot) else {
                motivos.push(format!("{} chip {}: no está en línea o no tiene servicio", pedido.dispositivo, pedido.slot + 1));
                continue;
            };
            // La receta de las filas de ese operador: la de la compañía o, si no hay, la dealer.
            let Some(receta) = crate::validacion::receta_de_lote(recetas, &chip.operador, &accion).cloned() else {
                let m = format!("no hay receta de {accion} para {}", chip.operador);
                motivos.push(format!("{}: {m}", nombre_chip(chip)));
                sin_chip.entry(chip.operador.clone()).or_insert(m);
                continue;
            };
            let par = Par { operador: chip.operador.clone(), accion: accion.clone(), receta: receta.clone() };
            match self.capacidad(recetas, chip, &par, &enviados) {
                Ok(x) => {
                    chips_origen.push(ChipOrigen {
                        dispositivo: chip.dispositivo.clone(),
                        slot: chip.slot,
                        operador: Some(chip.operador.clone()),
                        numero: chip.numero.clone(),
                        tipo: Some(chip.tipo.clone()),
                        receta: x.receta.clone(),
                    });
                    recetas_op.entry(chip.operador.clone()).or_insert(receta);
                    validos.push(x);
                }
                Err(m) => {
                    motivos.push(format!("{}: {m}", nombre_chip(chip)));
                    sin_chip.entry(chip.operador.clone()).or_insert(m);
                }
            }
        }
        if validos.is_empty() {
            return Err(if motivos.is_empty() {
                "elige al menos un chip".to_string()
            } else {
                format!("ninguno de los chips elegidos sirve para {accion}: {}", motivos.join("; "))
            });
        }
        // Un operador con algún chip que sirve no cuenta como "sin chip".
        sin_chip.retain(|op, _| !recetas_op.contains_key(op));
        let completo = OrigenLote { chips: chips_origen, accion: Some(accion.clone()), ..Default::default() };
        Ok(OrigenResuelto { origen: completo, accion, recetas: recetas_op, chips: validos, sin_chip, avisos: motivos, un_chip: false })
    }

    /// Receta de saldo dealer del chip, si se puede consultar: en línea, `dealer`, con receta `accion: saldo_dealer`
    /// para su operador y, si esa receta pide PIN (Tigo; Entel no), con PIN guardado.
    pub fn receta_saldo_dealer(&self, dispositivo: &str, slot: u32) -> Option<Receta> {
        let c = self.chips().into_iter().find(|c| c.dispositivo == dispositivo && c.slot == slot)?;
        if c.tipo != TIPO_DEALER {
            return None;
        }
        self.receta_para_tipo(&c.operador, ACCION_SALDO_DEALER, &c.tipo).filter(|r| c.tiene_pin || !pide_pin(r))
    }

    /// Por qué no se puede consultar el saldo dealer del chip (para los avisos de `verificar`).
    pub fn motivo_sin_saldo_dealer(&self, dispositivo: &str, slot: u32) -> String {
        match self.chips().into_iter().find(|c| c.dispositivo == dispositivo && c.slot == slot) {
            None => "no está en línea: no se pudo consultar su saldo dealer".into(),
            Some(c) if c.tipo != TIPO_DEALER => "no es un chip dealer: no hay saldo dealer con qué cuadrar".into(),
            Some(c) if self.receta_para_tipo(&c.operador, ACCION_SALDO_DEALER, &c.tipo).is_none() => {
                format!("no hay receta de saldo dealer para {}", c.operador)
            }
            Some(_) => "sin PIN guardado: no se puede consultar su saldo dealer".into(),
        }
    }

    /// Consulta ahora el saldo dealer del chip (envía el PIN guardado si la receta lo pide). `None` si no se puede o no se leyó.
    pub async fn consultar_saldo_dealer(&self, dispositivo: &str, slot: u32) -> Option<f64> {
        let receta = self.receta_saldo_dealer(dispositivo, slot)?;
        let desde = ahora_ms();
        if let Err(e) = self.ejecutar_receta(&receta, dispositivo, slot, &BTreeMap::new(), false).await {
            tracing::warn!(%dispositivo, slot, "saldo dealer: {e}");
            return None;
        }
        self.chips()
            .into_iter()
            .find(|c| c.dispositivo == dispositivo && c.slot == slot)
            .filter(|c| c.saldo_dealer_ms.is_some_and(|t| t >= desde))
            .and_then(|c| c.saldo_dealer)
    }

    /// Marca el tipo del chip que ocupa la ranura. Devuelve el tipo anterior.
    /// Acepta el nombre viejo `pda` y lo guarda como `dealer`.
    pub fn fijar_tipo(&self, dispositivo: &str, slot: u32, tipo: &str) -> Result<String, String> {
        let tipo = nucleo::receta::tipo_chip_actual(tipo);
        let tipo = tipo.as_str();
        if tipo != TIPO_NORMAL && tipo != TIPO_DEALER {
            return Err(format!("tipo '{tipo}' no válido (normal | dealer)"));
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

    /// Guarda (o borra, con `None`) el PIN del chip que ocupa la ranura.
    pub fn fijar_pin(&self, dispositivo: &str, slot: u32, pin: Option<&str>) -> Result<(), String> {
        let pin = pin.map(str::trim).filter(|p| !p.is_empty());
        if let Some(p) = pin {
            if !(4..=8).contains(&p.len()) || !p.chars().all(|c| c.is_ascii_digit()) {
                return Err("el PIN debe tener de 4 a 8 dígitos".into());
            }
        }
        self.reconciliar_todos();
        let o = self.ocupante(dispositivo, slot).ok_or_else(|| "no hay un chip en esa ranura".to_string())?;
        self.pers.fijar_pin(o.chip_id, pin).map_err(|e| e.to_string())
    }

    /// Completa el parámetro `pin` secreto con el PIN guardado del chip, si no vino uno.
    fn completar_pin(
        &self,
        receta: &Receta,
        dispositivo: &str,
        slot: u32,
        parametros: &BTreeMap<String, String>,
    ) -> Result<BTreeMap<String, String>, ErrorCanal> {
        let mut p = parametros.clone();
        let pide_pin = receta.parametros.iter().any(|q| q.secreto && q.nombre == "pin");
        if pide_pin && p.get("pin").is_none_or(|v| v.trim().is_empty()) {
            self.reconciliar_todos();
            let guardado = self.ocupante(dispositivo, slot).and_then(|o| self.pers.pin(o.chip_id).ok().flatten());
            let pin = guardado.ok_or_else(|| {
                ErrorCanal::nuevo(codigos::PARAMETRO, "este chip no tiene PIN guardado: escríbelo o guárdalo en el Panel")
            })?;
            p.insert("pin".into(), pin);
        }
        Ok(p)
    }

    /// ¿El teléfono está reservado (corriendo una receta o con una sesión USSD manual)? Solo como pista para
    /// elegir: quien va a usar el teléfono debe [`Contexto::reservar`]lo.
    pub fn ocupado(&self, id: &str) -> bool {
        self.registro.obtener(id).map(|d| d.ocupado).unwrap_or(false)
    }

    /// Reserva el teléfono (conectado y libre) de forma atómica: nadie más puede tomarlo hasta que se suelte
    /// la [`Reserva`], también si la tarea que la tiene entra en pánico.
    pub fn reservar(&self, id: &str) -> Option<Reserva> {
        let (ficha, d) = self.registro.reservar(id)?;
        self.bus.emitir(ev::DISPOSITIVO_ACTUALIZADO, json!({ "dispositivo_id": id, "dispositivo": d }));
        Some(Reserva { registro: self.registro.clone(), bus: self.bus.clone(), id: id.to_string(), ficha })
    }

    /// Ejecuta una receta en un chip: reserva el teléfono, emite eventos, guarda la ejecución y actualiza el
    /// saldo conocido. Es el único camino para correr recetas (API, lotes y consulta de saldos).
    /// Devuelve el resultado y el id de la ejecución guardada. Teléfono ya reservado → error `ocupado`.
    pub async fn ejecutar_receta(
        &self,
        receta: &Receta,
        dispositivo: &str,
        slot: u32,
        parametros: &BTreeMap<String, String>,
        simular: bool,
    ) -> Result<(Resultado, Option<i64>), ErrorCanal> {
        if self.registro.canal(dispositivo).is_none() {
            return Err(sin_dispositivo(dispositivo));
        }
        let reserva = self
            .reservar(dispositivo)
            .ok_or_else(|| ErrorCanal::nuevo(codigos::OCUPADO, "el teléfono está ocupado con otra sesión"))?;
        self.ejecutar_reservado(reserva, receta, dispositivo, slot, parametros, simular).await
    }

    /// Como [`Contexto::ejecutar_receta`], con el teléfono ya reservado por quien llama (el ejecutor de lotes
    /// lo reserva antes de despachar). La reserva se suelta al terminar.
    pub async fn ejecutar_reservado(
        &self,
        reserva: Reserva,
        receta: &Receta,
        dispositivo: &str,
        slot: u32,
        parametros: &BTreeMap<String, String>,
        simular: bool,
    ) -> Result<(Resultado, Option<i64>), ErrorCanal> {
        debug_assert_eq!(reserva.id, dispositivo);
        let canal = self.registro.canal(dispositivo).ok_or_else(|| sin_dispositivo(dispositivo))?;
        let parametros = &self.completar_pin(receta, dispositivo, slot, parametros)?;
        // El monto queda en Bs canónico (`0,5` → `0.50`), también en el historial. Un `{monto_entero}` con centavos
        // no corre (`400 parametro`, contrato). Otros parámetros inválidos siguen como siempre: el motor no marca
        // nada y el resultado es `error` (queda en el historial).
        let parametros = &match motor::validar_parametros(receta, parametros) {
            Ok(p) => p,
            Err(e) if e.starts_with(nucleo::monto::SOLO_ENTEROS) => return Err(ErrorCanal::nuevo(codigos::PARAMETRO, e)),
            Err(_) => parametros.clone(),
        };
        // El PIN no va a los eventos ni al historial.
        let visibles = receta.sin_secretos(parametros);
        let _tapados = self.bus.tapar(dispositivo, slot, receta.secretos(parametros));
        // Queda en el historial antes de marcar: si el control se cae en medio, al arrancar pasa a incierto.
        let id = self
            .almacen
            .iniciar_ejecucion(ahora_ms(), &receta.nombre, dispositivo, slot, simular, &visibles, receta.version.as_deref())
            .map_err(|e| ErrorCanal::nuevo(codigos::DESCONOCIDO, format!("no se pudo guardar en la base: {e}")))?;
        self.bus.emitir(
            ev::RECETA,
            json!({ "fase": "inicio", "receta": receta.nombre, "dispositivo_id": dispositivo, "slot": slot,
                    "simular": simular, "parametros": visibles, "ejecucion_id": id }),
        );
        // Si la tarea se cae en medio (pánico), la ejecución no queda `en_curso`: pasa a incierto.
        let mut abierta = EjecucionAbierta { almacen: &self.almacen, id, terminada: false };
        let mut r = motor::ejecutar(receta, canal.as_ref(), slot, parametros, simular).await;
        r.dispositivo = Some(dispositivo.to_string());
        if let Err(e) = self.almacen.terminar_ejecucion(id, &r) {
            tracing::warn!("no pude completar la ejecución {id}: {e}");
        }
        abierta.terminada = true;
        drop(reserva);
        let id = Some(id);
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
        // Saldo dealer: lo trae la consulta y también la respuesta de una recarga ("Tu saldo total Bs …").
        if let Some(saldo) = r.salida.get("saldo_dealer").and_then(|s| s.replace(',', ".").parse::<f64>().ok()) {
            self.reconciliar_todos();
            if let Some(o) = self.ocupante(dispositivo, slot) {
                if let Err(e) = self.pers.fijar_saldo_dealer(o.chip_id, (saldo * 100.0).round() / 100.0, ahora_ms()) {
                    tracing::warn!("no pude guardar el saldo dealer: {e}");
                }
            }
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
            let reciente = self.ultimo_intento.lock().unwrap().get(&(d.clone(), sl)).is_some_and(|t| t.elapsed() < REINTENTO_SALDO);
            if reciente {
                continue;
            }
            let chip = en_linea.iter().find(|c| c.dispositivo == d && c.slot == sl);
            let hecho = match chip {
                // El chip ya no está o ya tiene saldo: nada que consultar.
                None => intentos + 1 >= INTENTOS_SALDO,
                Some(c) if c.saldo.is_some() => true,
                Some(c) => match self.receta_para_tipo(&c.operador, "saldo", &c.tipo) {
                    Some(r) if !self.ocupado(&d) => {
                        self.ultimo_intento.lock().unwrap().insert((d.clone(), sl), std::time::Instant::now());
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
                self.ultimo_intento.lock().unwrap().remove(&(d.clone(), sl));
                m.remove(&(d, sl));
            } else if let Some(n) = m.get_mut(&(d, sl)) {
                *n = intentos + 1;
            }
        }
    }

    /// Chips en línea y listos (conectado, chip puesto, con servicio de voz).
    /// Corre en cada vuelta del ejecutor (500 ms): tres consultas en total, no una por chip.
    pub fn chips(&self) -> Vec<ChipInfo> {
        let recetas = self.recetas.read().unwrap().clone();
        self.chips_con(&recetas)
    }

    /// Como [`Contexto::chips`], con el cupo según un juego de recetas dado (la copia de un lote en curso).
    pub fn chips_con(&self, recetas: &BTreeMap<String, Receta>) -> Vec<ChipInfo> {
        self.reconciliar_todos();
        let desde = inicio_de_hoy_ms();
        let agotados = self.pers.agotados(&hoy()).unwrap_or_default();
        let datos = self.pers.datos_chips().unwrap_or_default();
        let enviados_por_chip = self.pers.enviados_ok_por_chip(desde).unwrap_or_default();
        let mut v = vec![];
        for d in self.registro.listar().into_iter().filter(|d| d.conectado) {
            for s in &d.sims {
                if s.estado != "listo" || !matches!(s.voz.as_deref(), None | Some("en_servicio")) {
                    continue;
                }
                let operador = operador_de_sim(&s.operador, &s.mcc_mnc);
                let ocupante = self.ocupante(&d.id, s.slot);
                let dato = ocupante.and_then(|o| datos.get(&o.chip_id));
                let tipo = dato.map(|x| x.tipo.clone()).unwrap_or_else(|| TIPO_NORMAL.into());
                let transferir = recetas_con_accion_en(recetas, &operador, "transferir", &tipo);
                let nombres = nombres_con_accion_en(recetas, &operador, "transferir");
                let enviados: u32 = ocupante
                    .and_then(|o| enviados_por_chip.get(&o.chip_id))
                    .map(|por_receta| nombres.iter().filter_map(|n| por_receta.get(n)).sum())
                    .unwrap_or(0);
                let agotado = ocupante.map(|o| agotados.contains(&o.chip_id)).unwrap_or(false);
                let por_dia = transferir.iter().filter_map(|r| r.limites.as_ref().and_then(|l| l.por_dia)).min();
                let limite = por_dia.unwrap_or(CUPO_SIN_LIMITE);
                let cupo = if transferir.is_empty() || agotado { 0 } else { limite.saturating_sub(enviados) };
                // Con receta de transferir sin `por_dia` (y sin "máximo de transferencias" hoy): sin límite.
                let sin_limite = !transferir.is_empty() && !agotado && por_dia.is_none();
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
                    tiene_pin: dato.is_some_and(|x| x.tiene_pin),
                    saldo_dealer: dato.and_then(|x| x.saldo_dealer),
                    saldo_dealer_ms: dato.and_then(|x| x.saldo_dealer_ms),
                    sin_limite,
                    accion: None,
                });
            }
        }
        v
    }

    /// Consulta el saldo de los chips en línea (de `operadores`, o de todos). Solo los que no tienen un dato
    /// más nuevo que `max_edad_ms` (None = todos). En paralelo entre teléfonos, en serie dentro de cada uno.
    /// Los teléfonos ocupados se saltan.
    /// Con `con_dealer`, también el saldo dealer de los chips dealer (con PIN guardado si su receta lo pide; envía
    /// el PIN: solo a pedido).
    pub async fn consultar_saldos(
        self: &Arc<Self>,
        operadores: Option<&[String]>,
        max_edad_ms: Option<i64>,
        con_dealer: bool,
    ) -> Vec<ChipInfo> {
        let ahora = ahora_ms();
        let ops: Option<Vec<String>> = operadores.map(|o| o.iter().map(|x| operador_de_red(x)).collect());
        let mut por_disp: BTreeMap<String, Vec<(u32, Receta)>> = BTreeMap::new();
        for c in self.chips() {
            if let Some(ops) = &ops {
                if !ops.contains(&c.operador) {
                    continue;
                }
            }
            // Después del principal, el dealer (si se pidió y, si la receta pide PIN, hay PIN para enviar).
            if con_dealer && c.tipo == TIPO_DEALER {
                if let Some(r) = self.receta_para_tipo(&c.operador, ACCION_SALDO_DEALER, &c.tipo).filter(|r| c.tiene_pin || !pide_pin(r)) {
                    por_disp.entry(c.dispositivo.clone()).or_default().push((c.slot, r));
                }
            }
            if let (Some(max), Some(t)) = (max_edad_ms, c.saldo_ms) {
                if ahora - t < max {
                    continue;
                }
            }
            if let Some(r) = self.receta_para_tipo(&c.operador, "saldo", &c.tipo) {
                let cola = por_disp.entry(c.dispositivo.clone()).or_default();
                // El principal va antes que el dealer del mismo chip.
                let i = cola.iter().position(|(s, _)| *s == c.slot).unwrap_or(cola.len());
                cola.insert(i, (c.slot, r));
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

/// La receta pide el PIN del chip (parámetro secreto `pin`): Tigo dealer sí, Entel dealer no.
fn pide_pin(r: &Receta) -> bool {
    r.parametros.iter().any(|p| p.secreto && p.nombre == "pin")
}
