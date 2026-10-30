use async_trait::async_trait;
use protocolo::{codigos, ModoUssd, PasoUssd};
use std::time::Duration;

/// Error de un canal USSD. `codigo` usa los códigos del contrato (`protocolo::codigos`).
#[derive(Debug, Clone, thiserror::Error, PartialEq)]
#[error("{codigo}: {detalle}")]
pub struct ErrorCanal {
    pub codigo: String,
    pub detalle: String,
}

impl ErrorCanal {
    pub fn nuevo(codigo: &str, detalle: impl Into<String>) -> Self {
        Self { codigo: codigo.into(), detalle: detalle.into() }
    }
    pub fn timeout() -> Self {
        Self::nuevo(codigos::TIMEOUT, "sin respuesta a tiempo")
    }
    pub fn desconectado() -> Self {
        Self::nuevo(codigos::DESCONECTADO, "se cortó la conexión con el dispositivo")
    }
}

/// Lo único que el motor de recetas necesita de un dispositivo.
///
/// Implementaciones: agente Android (crate `agentes`), canales falsos en tests.
/// Para sumar un módem AT basta implementar este trait (ver README).
#[async_trait]
pub trait CanalUssd: Send + Sync {
    /// Marca `codigo` por el chip de `slot`. Devuelve el primer texto y si la sesión quedó abierta.
    async fn iniciar(
        &self,
        slot: u32,
        codigo: &str,
        modo: ModoUssd,
        timeout: Duration,
    ) -> Result<PasoUssd, ErrorCanal>;

    /// Contesta `texto` en una sesión abierta.
    async fn responder(
        &self,
        sesion: &str,
        texto: &str,
        timeout: Duration,
    ) -> Result<PasoUssd, ErrorCanal>;

    /// Cierra la sesión (cancela la ventana de USSD).
    async fn cancelar(&self, sesion: &str) -> Result<(), ErrorCanal>;
}
