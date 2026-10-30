//! Contrato v1 entre el agente Android y el control (ver `docs/protocolo.md`).
//!
//! Reglas que se respetan aquí:
//! - Cada mensaje WebSocket lleva `"tipo"` en `snake_case`.
//! - Campos desconocidos se ignoran (serde los ignora por defecto: no usamos `deny_unknown_fields`).
//! - Campos opcionales llevan `#[serde(default)]` para tolerar agentes viejos.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Versión del protocolo que habla este control.
pub const PROTOCOLO: u32 = 1;
/// Puerto por defecto para agente, API e interfaz.
pub const PUERTO: u16 = 47800;

/// Códigos de error del contrato (más los que agrega el control para la API).
pub mod codigos {
    pub const OCUPADO: &str = "ocupado";
    pub const SIN_PERMISO: &str = "sin_permiso";
    pub const SIN_ACCESIBILIDAD: &str = "sin_accesibilidad";
    pub const SIM_INVALIDA: &str = "sim_invalida";
    pub const TIMEOUT: &str = "timeout";
    pub const RED: &str = "red";
    pub const SESION_INEXISTENTE: &str = "sesion_inexistente";
    pub const PARAMETRO: &str = "parametro";
    pub const DESCONOCIDO: &str = "desconocido";
    /// Agregado por el control: el dispositivo no existe o no está conectado.
    pub const SIN_DISPOSITIVO: &str = "sin_dispositivo";
    /// Agregado por el control: la conexión con el agente se cortó durante la petición.
    pub const DESCONECTADO: &str = "desconectado";
    /// Agregado por el control: receta inexistente.
    pub const SIN_RECETA: &str = "sin_receta";
    /// Agregado por el control: la receta es de otro tipo de chip (p. ej. una receta PDA en un chip normal).
    pub const TIPO_CHIP: &str = "tipo_chip";
}

// ───────────────────────────── tipos compartidos ─────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Sim {
    pub slot: u32,
    #[serde(default)]
    pub sub_id: i64,
    #[serde(default)]
    pub operador: String,
    #[serde(default)]
    pub mcc_mnc: String,
    #[serde(default)]
    pub iccid: Option<String>,
    #[serde(default)]
    pub numero: Option<String>,
    /// listo | ausente | pin | puk | bloqueada | desconocido (se guarda como texto para tolerar valores nuevos)
    #[serde(default = "desconocido")]
    pub estado: String,
    /// en_servicio | sin_servicio | solo_emergencia | apagado
    #[serde(default)]
    pub voz: Option<String>,
    #[serde(default)]
    pub red: Option<String>,
}

fn desconocido() -> String {
    "desconocido".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Sms {
    pub id: String,
    #[serde(default)]
    pub slot: Option<u32>,
    #[serde(default)]
    pub de: String,
    #[serde(default)]
    pub texto: String,
    #[serde(default)]
    pub fecha_ms: i64,
    #[serde(default)]
    pub leido: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ModoUssd {
    /// `TelephonyManager.sendUssdRequest`: un paso, sin ventana.
    Api,
    /// Ventana del sistema + accesibilidad: admite menús de varios pasos.
    #[default]
    Pantalla,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct EstadoDispositivo {
    #[serde(default)]
    pub bateria: Option<u8>,
    #[serde(default)]
    pub cargando: Option<bool>,
    #[serde(default)]
    pub pantalla_encendida: Option<bool>,
    #[serde(default)]
    pub accesibilidad_activa: Option<bool>,
    #[serde(default)]
    pub permisos_faltantes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Hola {
    pub protocolo: u32,
    #[serde(default)]
    pub token: String,
    pub dispositivo_id: String,
    #[serde(default)]
    pub fabricante: String,
    #[serde(default)]
    pub modelo: String,
    #[serde(default)]
    pub android: String,
    #[serde(default)]
    pub sdk: u32,
    #[serde(default)]
    pub app_version: String,
    #[serde(default)]
    pub sims: Vec<Sim>,
    #[serde(default)]
    pub capacidades: Vec<String>,
    /// Serial de ADB que la app recibió al provisionarse (opcional; null por Wi-Fi sin ADB).
    #[serde(default)]
    pub adb_serial: Option<String>,
}

// ───────────────────────────── agente → control ─────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "tipo", rename_all = "snake_case")]
pub enum DeAgente {
    Hola(Hola),
    Sims {
        #[serde(default)]
        sims: Vec<Sim>,
    },
    Estado(EstadoDispositivo),
    Ussd {
        req: String,
        sesion: String,
        #[serde(default)]
        texto: String,
        #[serde(default)]
        abierta: bool,
        #[serde(default)]
        ms: u64,
    },
    Ok {
        req: String,
    },
    Error {
        #[serde(default)]
        req: Option<String>,
        codigo: String,
        #[serde(default)]
        detalle: Option<String>,
    },
    SmsRecibido {
        sms: Sms,
    },
    SmsLista {
        req: String,
        #[serde(default)]
        sms: Vec<Sms>,
    },
    Pong {
        req: String,
    },
}

impl DeAgente {
    /// `req` de las respuestas a peticiones (para correlacionar).
    pub fn req(&self) -> Option<&str> {
        match self {
            DeAgente::Ussd { req, .. }
            | DeAgente::Ok { req }
            | DeAgente::SmsLista { req, .. }
            | DeAgente::Pong { req } => Some(req),
            DeAgente::Error { req, .. } => req.as_deref(),
            _ => None,
        }
    }
}

// ───────────────────────────── control → agente ─────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "tipo", rename_all = "snake_case")]
pub enum AlAgente {
    Bienvenido {
        servidor: String,
        hora_ms: i64,
    },
    Rechazado {
        motivo: String,
    },
    UssdIniciar {
        req: String,
        slot: u32,
        codigo: String,
        modo: ModoUssd,
        timeout_ms: u64,
    },
    UssdResponder {
        req: String,
        sesion: String,
        texto: String,
        timeout_ms: u64,
    },
    UssdCancelar {
        req: String,
        sesion: String,
    },
    SmsListar {
        req: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        slot: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        desde_ms: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        limite: Option<u32>,
    },
    Refrescar {
        req: String,
    },
    Ping {
        req: String,
    },
}

// ───────────────────────────── API para la interfaz ─────────────────────────────

/// Un paso USSD: lo que devuelve `POST /api/ussd/iniciar` y `/responder`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PasoUssd {
    pub sesion: String,
    pub texto: String,
    pub abierta: bool,
    pub ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Dispositivo {
    pub id: String,
    pub conectado: bool,
    pub adb_serial: Option<String>,
    pub fabricante: String,
    pub modelo: String,
    pub android: String,
    pub app_version: String,
    pub sims: Vec<Sim>,
    pub estado: Option<EstadoDispositivo>,
    pub ultima_vez_ms: i64,
    pub ocupado: bool,
    #[serde(default)]
    pub capacidades: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InfoAdb {
    pub serial: String,
    /// device | unauthorized | offline | …
    pub estado: String,
    pub modelo: Option<String>,
    pub reverse_ok: bool,
    pub dispositivo_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EstadoResultado {
    /// Terminó bien.
    Ok,
    /// Se recorrió hasta la confirmación y se respondió "No".
    Simulado,
    /// Se cortó antes de mover dinero.
    Abortado,
    /// Se confirmó pero no hubo respuesta clara: hay que revisar el saldo.
    Incierto,
    /// Receta o parámetros inválidos, o fallo antes de empezar.
    Error,
}

impl EstadoResultado {
    pub fn como_texto(&self) -> &'static str {
        match self {
            EstadoResultado::Ok => "ok",
            EstadoResultado::Simulado => "simulado",
            EstadoResultado::Abortado => "abortado",
            EstadoResultado::Incierto => "incierto",
            EstadoResultado::Error => "error",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PasoResultado {
    /// marcar | enviar | elegir | verificar | confirmar | cancelar
    pub accion: String,
    pub enviado: Option<String>,
    pub texto: String,
    pub abierta: bool,
    pub ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Resultado {
    pub ok: bool,
    pub estado: EstadoResultado,
    pub detalle: Option<String>,
    pub pasos: Vec<PasoResultado>,
    pub salida: BTreeMap<String, String>,
    // Extras (el contrato tolera campos adicionales).
    #[serde(default)]
    pub receta: String,
    #[serde(default)]
    pub dispositivo: Option<String>,
    #[serde(default)]
    pub slot: u32,
    #[serde(default)]
    pub simular: bool,
    #[serde(default)]
    pub inicio_ms: i64,
    #[serde(default)]
    pub duracion_ms: u64,
}

/// Evento para la interfaz: `{ "tipo", "ts_ms", …datos }`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Evento {
    pub tipo: String,
    pub ts_ms: i64,
    #[serde(flatten)]
    pub datos: serde_json::Map<String, serde_json::Value>,
}

pub mod tipos_evento {
    pub const DISPOSITIVO_CONECTADO: &str = "dispositivo_conectado";
    pub const DISPOSITIVO_DESCONECTADO: &str = "dispositivo_desconectado";
    pub const DISPOSITIVO_ACTUALIZADO: &str = "dispositivo_actualizado";
    pub const CHIP_CAMBIADO: &str = "chip_cambiado";
    pub const CHIP_TIPO: &str = "chip_tipo";
    pub const USSD: &str = "ussd";
    pub const SMS: &str = "sms";
    pub const ADB: &str = "adb";
    pub const RECETA: &str = "receta";
}

/// Cuerpo de error de la API: `{"error": {"codigo", "detalle"}}`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CuerpoError {
    pub error: DetalleError,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DetalleError {
    pub codigo: String,
    pub detalle: String,
}

pub fn ahora_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hola_tolera_campos_desconocidos_y_faltantes() {
        let j = r#"{"tipo":"hola","protocolo":1,"dispositivo_id":"abc","extra":42,
                    "sims":[{"slot":0,"operador":"TIGO","nuevo":"x"}]}"#;
        let m: DeAgente = serde_json::from_str(j).unwrap();
        match m {
            DeAgente::Hola(h) => {
                assert_eq!(h.dispositivo_id, "abc");
                assert_eq!(h.sims[0].estado, "desconocido");
            }
            _ => panic!(),
        }
    }

    #[test]
    fn ussd_iniciar_serializa_snake_case() {
        let m = AlAgente::UssdIniciar {
            req: "r1".into(),
            slot: 0,
            codigo: "*105#".into(),
            modo: ModoUssd::Api,
            timeout_ms: 30000,
        };
        let s = serde_json::to_string(&m).unwrap();
        assert!(s.contains(r#""tipo":"ussd_iniciar""#), "{s}");
        assert!(s.contains(r#""modo":"api""#), "{s}");
    }

    #[test]
    fn error_sin_req() {
        let m: DeAgente =
            serde_json::from_str(r#"{"tipo":"error","codigo":"ocupado"}"#).unwrap();
        assert_eq!(m.req(), None);
    }

    #[test]
    fn evento_aplana_datos() {
        let mut datos = serde_json::Map::new();
        datos.insert("dispositivo".into(), "x".into());
        let e = Evento { tipo: "ussd".into(), ts_ms: 1, datos };
        let s = serde_json::to_string(&e).unwrap();
        assert_eq!(s, r#"{"tipo":"ussd","ts_ms":1,"dispositivo":"x"}"#);
    }
}
