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
    /// `normal` o `pda`. Decide qué receta usa el chip (ver "Chips PDA" en el protocolo).
    #[serde(default = "tipo_normal")]
    pub tipo: String,
}

pub const TIPO_NORMAL: &str = "normal";
pub const TIPO_PDA: &str = "pda";

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
    pub monto: u64,
    pub dispositivo: String,
    pub slot: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FilaValidada {
    pub fila: u32,
    pub numero: Option<String>,
    pub monto: Option<u64>,
    pub operador: Option<String>,
    pub accion: String,
    pub referencia: Option<String>,
    pub receta: Option<String>,
    pub estado: EstadoValidacion,
    pub errores: Vec<String>,
    pub avisos: Vec<String>,
    /// Montos de cada envío (12 → [10, 2]).
    pub envios: Vec<u64>,
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
    pub chips: u32,
    pub cupo_hoy: u32,
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
    pub total_bs: u64,
    pub envios: u32,
    pub comision_total: f64,
    pub planificados: u32,
    pub sin_capacidad: u32,
    pub operadores: Vec<ResumenOperador>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Validacion {
    pub filas: Vec<FilaValidada>,
    pub resumen: ResumenValidacion,
    pub chips: Vec<ChipInfo>,
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
    pub monto: u64,
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
    pub monto: Option<u64>,
    pub operador: Option<String>,
    pub accion: String,
    pub referencia: Option<String>,
    pub receta: Option<String>,
    pub estado: EstadoFila,
    pub enviado_bs: u64,
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
    pub enviado_bs: u64,
    pub comision_bs: f64,
    pub total_bs: u64,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filas: Option<Vec<FilaLote>>,
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
