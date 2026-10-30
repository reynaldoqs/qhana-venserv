use protocolo::{ahora_ms, Evento};
use serde_json::{Map, Value};
use tokio::sync::broadcast;

/// Bus de eventos: la interfaz (WS `/api/eventos`) y el historial (SQLite) se suscriben.
#[derive(Clone)]
pub struct Bus {
    tx: broadcast::Sender<Evento>,
}

impl Default for Bus {
    fn default() -> Self {
        Self::new(1024)
    }
}

impl Bus {
    pub fn new(capacidad: usize) -> Self {
        let (tx, _) = broadcast::channel(capacidad);
        Self { tx }
    }

    pub fn suscribir(&self) -> broadcast::Receiver<Evento> {
        self.tx.subscribe()
    }

    /// Emite un evento. `datos` debe ser un objeto JSON (si no, se envuelve en `{"valor": …}`).
    pub fn emitir(&self, tipo: &str, datos: Value) {
        let datos = match datos {
            Value::Object(m) => m,
            Value::Null => Map::new(),
            otro => {
                let mut m = Map::new();
                m.insert("valor".into(), otro);
                m
            }
        };
        let _ = self.tx.send(Evento { tipo: tipo.into(), ts_ms: ahora_ms(), datos });
    }
}
