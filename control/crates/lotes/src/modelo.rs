//! Tipos de la API de lotes (contrato: `docs/protocolo.md`, sección "Lotes (CSV) — v1").

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ───────────────────────────── chips ─────────────────────────────

/// Un chip en línea con lo que se sabe de él para repartir envíos.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChipInfo {
    pub dispositivo: String,
    pub slot: u32,
    /// Operador normalizado (TIGO, ENTEL, VIVA) o el nombre de la red tal cual si no se reconoce.
    pub operador: String,
    pub numero: Option<String>,
    /// Saldo principal conocido (última consulta, ajustado por los envíos hechos desde entonces).
    pub saldo: Option<f64>,
    pub saldo_ms: Option<i64>,
    pub enviados_hoy: u32,
    /// Envíos que le quedan hoy (0 si no hay receta de transferir para su operador o si el operador lo bloqueó).
    pub cupo_hoy: u32,
    /// El operador respondió "máximo de transferencias" hoy.
    #[serde(default)]
    pub agotado_hoy: bool,
    /// Entró un chip distinto a esta ranura y todavía no se consultó su saldo: no entra al reparto.
    #[serde(default)]
    pub sin_verificar: bool,
    /// `normal` o `dealer`. Decide qué receta usa el chip (ver "Chips dealer" en el protocolo).
    #[serde(default = "tipo_normal", deserialize_with = "tipo_actual")]
    pub tipo: String,
    /// El chip tiene un PIN guardado (para recetas con un parámetro `pin` secreto). El PIN nunca sale por la API.
    #[serde(default)]
    pub tiene_pin: bool,
    /// Saldo dealer conocido: el que gastan las recargas de un chip dealer. No es el principal.
    #[serde(default, alias = "saldo_pda")]
    pub saldo_dealer: Option<f64>,
    #[serde(default, alias = "saldo_pda_ms")]
    pub saldo_dealer_ms: Option<i64>,
    /// Sin límite diario para la acción de estos números (la receta no tiene `por_dia`): `cupo_hoy` no cuenta.
    #[serde(default)]
    pub sin_limite: bool,
    /// En `Validacion.chips`: la acción del lote a la que se refieren `enviados_hoy`, `cupo_hoy` y `sin_limite`.
    /// `None` = los de transferir (fuera de una validación, o un chip que no hace nada en el lote).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accion: Option<String>,
}

impl ChipInfo {
    /// Para la API: un chip sin límite muestra `cupo_hoy: 0` (internamente lleva un cupo enorme para repartir).
    pub fn para_api(mut self) -> Self {
        if self.sin_limite {
            self.cupo_hoy = 0;
        }
        self
    }
}

/// Acción de las recetas que leen el saldo dealer. Su `salida` debe traer `saldo_dealer`.
pub const ACCION_SALDO_DEALER: &str = nucleo::receta::ACCION_SALDO_DEALER;

pub const TIPO_NORMAL: &str = "normal";
pub const TIPO_DEALER: &str = nucleo::receta::CHIP_DEALER;

/// Lee un tipo de chip o de saldo traduciendo el nombre viejo `pda` → `dealer` (lotes guardados antes del cambio
/// y pedidos de una interfaz vieja). Ver "Compatibilidad" en el protocolo.
fn tipo_actual<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    Ok(nucleo::receta::tipo_chip_actual(&String::deserialize(d)?))
}

/// Como [`tipo_actual`], para un campo opcional.
fn tipo_actual_opcional<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    Ok(Option::<String>::deserialize(d)?.map(|t| nucleo::receta::tipo_chip_actual(&t)))
}

fn saldo_principal() -> String {
    "principal".to_string()
}

fn tipo_normal() -> String {
    TIPO_NORMAL.to_string()
}

// ───────────────────────────── validación ─────────────────────────────

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EstadoValidacion {
    Valida,
    Aviso,
    Invalida,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EnvioPlan {
    pub monto: f64,
    pub dispositivo: String,
    pub slot: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FilaValidada {
    pub fila: u32,
    pub numero: Option<String>,
    pub monto: Option<f64>,
    pub operador: Option<String>,
    pub accion: String,
    pub referencia: Option<String>,
    pub receta: Option<String>,
    pub estado: EstadoValidacion,
    pub errores: Vec<String>,
    pub avisos: Vec<String>,
    /// Montos de cada envío (12 → [10, 2]).
    pub envios: Vec<f64>,
    pub plan: Vec<EnvioPlan>,
    /// Envíos de esta fila que hoy no entran en ningún chip.
    pub sin_capacidad: u32,
    /// Por qué no entran (cupo, saldo, sin chips). Campo agregado al contrato.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub motivo_sin_capacidad: Option<String>,
    /// Valores tal como venían en el CSV.
    pub original: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResumenOperador {
    pub operador: String,
    /// Una entrada por (operador, acción): la acción de esas filas.
    #[serde(default)]
    pub accion: String,
    pub chips: u32,
    /// Suma del cupo de los chips con límite diario para esa acción.
    pub cupo_hoy: u32,
    /// Algún chip no tiene límite diario para esa acción (el cupo no limita el plan).
    #[serde(default)]
    pub cupo_sin_limite: bool,
    /// Saldo principal: solo cuenta con `transferir` (con otra acción, 0).
    pub saldo_conocido: f64,
    pub saldo_desconocido_chips: u32,
    pub necesario_bs: f64,
    pub envios: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResumenValidacion {
    pub filas: u32,
    /// Filas que se pueden ejecutar (incluye las que tienen avisos).
    pub validas: u32,
    pub con_avisos: u32,
    pub invalidas: u32,
    pub total_bs: f64,
    pub envios: u32,
    pub comision_total: f64,
    pub planificados: u32,
    pub sin_capacidad: u32,
    pub operadores: Vec<ResumenOperador>,
    /// Avisos del lote entero (no de una fila), p. ej. "la receta X cambió y no se ha probado".
    #[serde(default)]
    pub avisos: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Validacion {
    pub filas: Vec<FilaValidada>,
    pub resumen: ResumenValidacion,
    pub chips: Vec<ChipInfo>,
    /// Los chips elegidos (con operador, número, tipo y la receta que correrá cada uno), si se pidió `origen`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origen: Option<OrigenLote>,
    /// Chips elegidos que no sirven y se excluyeron (con el motivo). Vacío si no hay.
    #[serde(default)]
    pub avisos_origen: Vec<String>,
}

// ───────────────────────────── lotes ─────────────────────────────

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EstadoLote {
    Listo,
    EnCurso,
    Pausado,
    Terminado,
    Cancelado,
}

impl EstadoLote {
    pub fn texto(self) -> &'static str {
        match self {
            EstadoLote::Listo => "listo",
            EstadoLote::EnCurso => "en_curso",
            EstadoLote::Pausado => "pausado",
            EstadoLote::Terminado => "terminado",
            EstadoLote::Cancelado => "cancelado",
        }
    }
    pub fn desde(s: &str) -> Self {
        match s {
            "en_curso" => EstadoLote::EnCurso,
            "pausado" => EstadoLote::Pausado,
            "terminado" => EstadoLote::Terminado,
            "cancelado" => EstadoLote::Cancelado,
            _ => EstadoLote::Listo,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EstadoFila {
    Omitida,
    Pendiente,
    EnCurso,
    Ok,
    Simulado,
    Parcial,
    Fallida,
    Incierta,
    SinCapacidad,
}

impl EstadoFila {
    pub fn texto(self) -> &'static str {
        match self {
            EstadoFila::Omitida => "omitida",
            EstadoFila::Pendiente => "pendiente",
            EstadoFila::EnCurso => "en_curso",
            EstadoFila::Ok => "ok",
            EstadoFila::Simulado => "simulado",
            EstadoFila::Parcial => "parcial",
            EstadoFila::Fallida => "fallida",
            EstadoFila::Incierta => "incierta",
            EstadoFila::SinCapacidad => "sin_capacidad",
        }
    }
    pub fn desde(s: &str) -> Self {
        match s {
            "omitida" => EstadoFila::Omitida,
            "en_curso" => EstadoFila::EnCurso,
            "ok" => EstadoFila::Ok,
            "simulado" => EstadoFila::Simulado,
            "parcial" => EstadoFila::Parcial,
            "fallida" => EstadoFila::Fallida,
            "incierta" => EstadoFila::Incierta,
            "sin_capacidad" => EstadoFila::SinCapacidad,
            _ => EstadoFila::Pendiente,
        }
    }
}

/// Estado de un envío (una pieza de una fila: 12 = 10 + 2 son dos envíos).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EstadoEnvio {
    Pendiente,
    EnCurso,
    Ok,
    Simulado,
    Fallido,
    Incierto,
    SinCapacidad,
}

impl EstadoEnvio {
    pub fn texto(self) -> &'static str {
        match self {
            EstadoEnvio::Pendiente => "pendiente",
            EstadoEnvio::EnCurso => "en_curso",
            EstadoEnvio::Ok => "ok",
            EstadoEnvio::Simulado => "simulado",
            EstadoEnvio::Fallido => "fallido",
            EstadoEnvio::Incierto => "incierto",
            EstadoEnvio::SinCapacidad => "sin_capacidad",
        }
    }
    pub fn desde(s: &str) -> Self {
        match s {
            "en_curso" => EstadoEnvio::EnCurso,
            "ok" => EstadoEnvio::Ok,
            "simulado" => EstadoEnvio::Simulado,
            "fallido" => EstadoEnvio::Fallido,
            "incierto" => EstadoEnvio::Incierto,
            "sin_capacidad" => EstadoEnvio::SinCapacidad,
            _ => EstadoEnvio::Pendiente,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EnvioLote {
    /// Id interno del envío.
    pub id: i64,
    pub n: u32,
    pub monto: f64,
    pub dispositivo: Option<String>,
    pub slot: Option<u32>,
    /// Número de la línea de origen, si se conoce.
    pub numero_origen: Option<String>,
    pub estado: EstadoEnvio,
    pub ejecucion_id: Option<i64>,
    pub detalle: Option<String>,
    pub intentos: u32,
    pub ts_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FilaLote {
    /// Id interno de la fila.
    pub id: i64,
    pub fila: u32,
    pub numero: Option<String>,
    pub monto: Option<f64>,
    pub operador: Option<String>,
    pub accion: String,
    pub referencia: Option<String>,
    pub receta: Option<String>,
    pub estado: EstadoFila,
    pub enviado_bs: f64,
    pub detalle: Option<String>,
    pub envios: Vec<EnvioLote>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Progreso {
    /// Filas ejecutables (sin las omitidas).
    pub filas: u32,
    pub ok: u32,
    pub simulado: u32,
    pub parcial: u32,
    pub fallidas: u32,
    pub inciertas: u32,
    /// Pendientes + en curso.
    pub pendientes: u32,
    pub sin_capacidad: u32,
    pub omitidas: u32,
    pub enviado_bs: f64,
    pub comision_bs: f64,
    pub total_bs: f64,
}

/// Un chip elegido como origen de un lote. `operador`, `numero`, `tipo` y `receta` los completa el control.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ChipOrigen {
    pub dispositivo: String,
    #[serde(default)]
    pub slot: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operador: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub numero: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "tipo_actual_opcional")]
    pub tipo: Option<String>,
    /// La receta que correrá este chip.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receta: Option<String>,
}

/// De dónde salen los envíos de un lote. Dos formas (ver "Lote de un chip" y "Lote desde chips elegidos"):
/// - `{ chips: [{dispositivo, slot}], accion }`: el lote se reparte solo entre esos chips;
/// - la vieja `{ dispositivo, slot, receta }`: un solo chip con esa receta (equivale a `chips` con uno).
///
/// El control la devuelve completa: `chips` con operador, número, tipo y receta, y `accion`; para la forma vieja,
/// además `dispositivo`, `slot`, `receta`, `operador` y `numero` como antes.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct OrigenLote {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub chips: Vec<ChipOrigen>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accion: Option<String>,
    // ── forma vieja (un chip y una receta) ──
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dispositivo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<u32>,
    /// Nombre de la receta que corre cada envío (p. ej. `tigo-dealer-recargar`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receta: Option<String>,
    /// Operador del chip (lo completa el control).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operador: Option<String>,
    /// Número del chip, si se conoce (lo completa el control).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub numero: Option<String>,
}

impl OrigenLote {
    /// Origen viejo: un chip con una receta.
    pub fn un_chip(dispositivo: &str, slot: u32, receta: &str) -> Self {
        Self { dispositivo: Some(dispositivo.into()), slot: Some(slot), receta: Some(receta.into()), ..Default::default() }
    }

    /// Chips elegidos con una acción.
    pub fn de_chips(chips: &[(&str, u32)], accion: &str) -> Self {
        Self {
            chips: chips.iter().map(|(d, s)| ChipOrigen { dispositivo: d.to_string(), slot: *s, ..Default::default() }).collect(),
            accion: Some(accion.into()),
            ..Default::default()
        }
    }

    /// ¿Es la forma vieja (un chip con su receta)?
    pub fn es_un_chip(&self) -> bool {
        self.chips.is_empty() && self.dispositivo.is_some()
    }

    /// Un origen guardado con la forma vieja (lotes de antes) también trae `chips`.
    pub fn normalizado(mut self) -> Self {
        if self.chips.is_empty() {
            if let Some(d) = &self.dispositivo {
                self.chips = vec![ChipOrigen {
                    dispositivo: d.clone(),
                    slot: self.slot.unwrap_or(0),
                    operador: self.operador.clone(),
                    numero: self.numero.clone(),
                    tipo: None,
                    receta: self.receta.clone(),
                }];
            }
        }
        self
    }

    /// ¿Ese chip es uno de los elegidos?
    pub fn tiene(&self, dispositivo: &str, slot: u32) -> bool {
        self.chips.iter().any(|c| c.dispositivo == dispositivo && c.slot == slot)
            || (self.chips.is_empty() && self.dispositivo.as_deref() == Some(dispositivo) && self.slot.unwrap_or(0) == slot)
    }
}

// ───────────────────────────── carriles ─────────────────────────────

/// El envío que un chip está haciendo ahora.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActualCarril {
    pub fila: u32,
    pub numero: Option<String>,
    pub monto: f64,
    pub desde_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SaldoCarril {
    /// `principal` o `dealer`.
    #[serde(deserialize_with = "tipo_actual")]
    pub tipo: String,
    pub bs: f64,
    pub ms: Option<i64>,
}

/// Un chip que participa (o puede participar) en un lote: `GET /api/lotes/{id}/carriles`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Carril {
    pub dispositivo: String,
    pub slot: u32,
    pub telefono: Option<String>,
    pub operador: Option<String>,
    pub numero: Option<String>,
    #[serde(default, deserialize_with = "tipo_actual_opcional")]
    pub tipo: Option<String>,
    pub receta: Option<String>,
    /// `enviando` · `esperando` · `libre` · `sin_capacidad` · `desconectado` · `excluido`.
    pub estado: String,
    pub actual: Option<ActualCarril>,
    pub ok: u32,
    pub fallidos: u32,
    pub inciertos: u32,
    pub enviado_bs: f64,
    /// `null` = sin límite.
    pub cupo_restante: Option<u32>,
    /// La receta del chip no tiene límite diario (distingue `cupo_restante: null` de "sin dato").
    #[serde(default)]
    pub cupo_sin_limite: bool,
    pub saldo: Option<SaldoCarril>,
    pub ms_promedio: Option<u64>,
    pub motivo: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Carriles {
    pub carriles: Vec<Carril>,
    /// Envíos pendientes o en curso.
    pub pendientes: u32,
    /// Estimación de lo que falta con el ritmo actual; `null` si el lote no está en curso o aún no hay ritmo.
    pub eta_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Lote {
    pub id: i64,
    pub nombre: String,
    pub creado_ms: i64,
    pub estado: EstadoLote,
    pub simular: bool,
    pub motivo_pausa: Option<String>,
    pub revision: Revision,
    /// Último resultado de `verificar` (se borra si el lote cambia).
    #[serde(default)]
    pub verificacion: Option<Verificacion>,
    pub progreso: Progreso,
    /// Chip y receta elegidos (modo "un chip"); `None` = reparto por operador entre todos los chips.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origen: Option<OrigenLote>,
    /// Archivado: no aparece en la lista por defecto. Nada más cambia.
    #[serde(default)]
    pub archivado: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filas: Option<Vec<FilaLote>>,
}

/// Números de las pestañas de `GET /api/lotes` (no dependen del filtro).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ConteosLotes {
    /// `listo` + `en_curso` + `pausado`, sin archivados.
    pub activos: u64,
    /// Con filas fallidas, inciertas o parciales, sin archivados.
    pub problemas: u64,
    /// `terminado` + `cancelado`, sin archivados.
    pub terminados: u64,
    /// Simulaciones, sin archivadas.
    pub simulaciones: u64,
    pub archivados: u64,
}

/// Respuesta de `GET /api/lotes`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ListaLotes {
    pub lotes: Vec<Lote>,
    /// Valor de `antes` para la página siguiente (`null` si no hay más).
    pub siguiente: Option<i64>,
    /// Lotes que cumplen el filtro (todas las páginas).
    pub total: u64,
    pub conteos: ConteosLotes,
}

/// Respuesta de `GET /api/lotes/{id}/filas`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PaginaFilas {
    pub filas: Vec<FilaLote>,
    /// Valor de `despues` para la página siguiente (`null` si no hay más).
    pub siguiente: Option<u32>,
    pub total: u64,
}

/// ¿Terminó todo bien? `terminado` solo dice que el ejecutor ya no tiene nada más que intentar.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Revision {
    pub completo: bool,
    pub motivos: Vec<String>,
}

impl Revision {
    pub fn de(estado: EstadoLote, p: &Progreso) -> Self {
        let mut motivos = vec![];
        if estado != EstadoLote::Terminado {
            motivos.push(format!("el lote está {}", estado.texto().replace('_', " ")));
        }
        let mut cuenta = |n: u32, uno: &str, varias: &str| match n {
            0 => {}
            1 => motivos.push(format!("1 fila {uno}")),
            n => motivos.push(format!("{n} filas {varias}")),
        };
        cuenta(p.pendientes, "pendiente", "pendientes");
        cuenta(p.inciertas, "incierta", "inciertas");
        cuenta(p.parcial, "parcial", "parciales");
        cuenta(p.fallidas, "fallida", "fallidas");
        cuenta(p.sin_capacidad, "para mañana", "para mañana");
        // Cualquier otra cosa que no sea ok/simulado también cuenta como incompleta.
        if p.filas == 0 {
            motivos.push("el lote no tiene filas para ejecutar".into());
        }
        Self { completo: motivos.is_empty(), motivos }
    }
}

// ───────────────────────────── verificación de saldos ─────────────────────────────

/// Diferencia tolerada (Bs) entre lo descontado esperado y el real.
pub const TOLERANCIA_BS: f64 = 0.10;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EstadoCuadre {
    Cuadra,
    Diferencia,
    SinDato,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CuadreChip {
    pub dispositivo: String,
    pub slot: u32,
    pub numero: Option<String>,
    /// Qué saldo se comparó en este chip: `principal` (lo transferido) o `dealer` (lo recargado desde un chip dealer).
    #[serde(default = "saldo_principal", deserialize_with = "tipo_actual")]
    pub saldo: String,
    pub estado: EstadoCuadre,
    pub saldo_inicial: Option<f64>,
    pub saldo_actual: Option<f64>,
    /// Σ (monto + comisión) de los envíos `ok` desde el último inicio.
    pub descontado_esperado: f64,
    pub descontado_real: Option<f64>,
    /// `descontado_real − descontado_esperado` (positivo: se descontó de más).
    pub diferencia_bs: Option<f64>,
    pub envios_ok: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Verificacion {
    pub lote_id: i64,
    pub verificado_ms: i64,
    /// Qué saldo se comparó: `principal` (transferencias), `dealer` (recargas desde chips dealer) o `mixto` (los dos).
    #[serde(default = "saldo_principal", deserialize_with = "tipo_actual")]
    pub saldo: String,
    pub completo: bool,
    pub motivos: Vec<String>,
    /// Todos los chips cuadran.
    pub cuadra: bool,
    /// `completo` y `cuadra`.
    pub confirmado: bool,
    pub chips: Vec<CuadreChip>,
    pub avisos: Vec<String>,
}

/// Estado de una fila a partir de sus envíos.
pub fn estado_fila(envios: &[EnvioLote]) -> EstadoFila {
    use EstadoEnvio as E;
    if envios.is_empty() {
        return EstadoFila::Omitida;
    }
    let cuenta = |e: E| envios.iter().filter(|x| x.estado == e).count();
    let total = envios.len();
    if cuenta(E::Incierto) > 0 {
        return EstadoFila::Incierta;
    }
    if cuenta(E::EnCurso) > 0 {
        return EstadoFila::EnCurso;
    }
    let ok = cuenta(E::Ok);
    let sim = cuenta(E::Simulado);
    if ok == total {
        return EstadoFila::Ok;
    }
    if sim == total {
        return EstadoFila::Simulado;
    }
    if cuenta(E::Pendiente) > 0 {
        return if ok + sim > 0 { EstadoFila::EnCurso } else { EstadoFila::Pendiente };
    }
    if ok + sim > 0 {
        return EstadoFila::Parcial;
    }
    if cuenta(E::Fallido) > 0 {
        return EstadoFila::Fallida;
    }
    EstadoFila::SinCapacidad
}
