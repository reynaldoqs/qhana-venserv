use protocolo::{ahora_ms, Evento};
use serde_json::{Map, Value};
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

/// Bus de eventos: la interfaz (WS `/api/eventos`) y el historial (SQLite) se suscriben.
#[derive(Clone)]
pub struct Bus {
    tx: broadcast::Sender<Evento>,
    /// Valores secretos (PIN) de las recetas en curso, con el teléfono y la ranura donde corren: se tapan en
    /// los eventos de ese teléfono (no en los de otros, para no estropear textos ajenos que tengan esos dígitos).
    secretos: Arc<Mutex<Vec<Secreto>>>,
}

impl Default for Bus {
    fn default() -> Self {
        Self::new(1024)
    }
}

impl Bus {
    pub fn new(capacidad: usize) -> Self {
        let (tx, _) = broadcast::channel(capacidad);
        Self { tx, secretos: Arc::default() }
    }

    /// Tapa `valores` en los eventos de `dispositivo` (y de su `slot`, si el evento lo dice) mientras viva la
    /// guarda devuelta.
    pub fn tapar(&self, dispositivo: &str, slot: u32, valores: Vec<String>) -> Tapados {
        let nuevos: Vec<Secreto> = valores
            .into_iter()
            .filter(|v| !v.is_empty())
            .map(|valor| Secreto { dispositivo: dispositivo.to_string(), slot, valor })
            .collect();
        bloquear(&self.secretos).extend(nuevos.iter().cloned());
        Tapados { secretos: self.secretos.clone(), valores: nuevos }
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
        let secretos: Vec<String> = {
            let s = bloquear(&self.secretos);
            if s.is_empty() {
                vec![]
            } else {
                let disp = datos.get("dispositivo_id").and_then(Value::as_str);
                let slot = datos.get("slot").and_then(Value::as_u64);
                s.iter()
                    .filter(|x| disp == Some(x.dispositivo.as_str()) && slot.is_none_or(|sl| sl == x.slot as u64))
                    .map(|x| x.valor.clone())
                    .collect()
            }
        };
        let datos = if secretos.is_empty() { datos } else { tapar_en(Value::Object(datos), &secretos).as_object().cloned().unwrap_or_default() };
        let _ = self.tx.send(Evento { tipo: tipo.into(), ts_ms: ahora_ms(), datos });
    }
}

#[derive(Clone, PartialEq)]
struct Secreto {
    dispositivo: String,
    slot: u32,
    valor: String,
}

/// Un candado envenenado (alguien entró en pánico con él tomado) no debe tumbar el bus.
fn bloquear<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Guarda de [`Bus::tapar`]: al soltarse, los valores dejan de taparse.
pub struct Tapados {
    secretos: Arc<Mutex<Vec<Secreto>>>,
    valores: Vec<Secreto>,
}

impl Drop for Tapados {
    fn drop(&mut self) {
        let mut s = bloquear(&self.secretos);
        for v in &self.valores {
            if let Some(i) = s.iter().position(|x| x == v) {
                s.remove(i);
            }
        }
    }
}

fn tapar_en(v: Value, secretos: &[String]) -> Value {
    match v {
        Value::String(t) => Value::String(crate::receta::tapar(&t, secretos)),
        Value::Array(xs) => Value::Array(xs.into_iter().map(|x| tapar_en(x, secretos)).collect()),
        Value::Object(m) => Value::Object(m.into_iter().map(|(k, x)| (k, tapar_en(x, secretos))).collect()),
        otro => otro,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tapa_secretos_mientras_vive_la_guarda() {
        let bus = Bus::default();
        let mut rx = bus.suscribir();
        let guarda = bus.tapar("tel1", 0, vec!["4321".into()]);
        bus.emitir("ussd", json!({ "dispositivo_id": "tel1", "enviado": "4321", "pasos": [{ "texto": "PIN 4321" }] }));
        let e = rx.try_recv().unwrap();
        assert_eq!(e.datos["enviado"], crate::receta::TAPADO);
        assert!(!serde_json::to_string(&e.datos).unwrap().contains("4321"));
        // Otro teléfono, u otra ranura del mismo, no se tocan.
        bus.emitir("ussd", json!({ "dispositivo_id": "tel2", "texto": "Saldo 4321 Bs" }));
        assert_eq!(rx.try_recv().unwrap().datos["texto"], "Saldo 4321 Bs");
        bus.emitir("ussd", json!({ "dispositivo_id": "tel1", "slot": 1, "texto": "Saldo 4321 Bs" }));
        assert_eq!(rx.try_recv().unwrap().datos["texto"], "Saldo 4321 Bs");
        bus.emitir("ussd", json!({ "dispositivo_id": "tel1", "slot": 0, "texto": "PIN 4321" }));
        assert!(!rx.try_recv().unwrap().datos["texto"].as_str().unwrap().contains("4321"));
        drop(guarda);
        bus.emitir("ussd", json!({ "dispositivo_id": "tel1", "enviado": "4321" }));
        assert_eq!(rx.try_recv().unwrap().datos["enviado"], "4321");
    }
}
