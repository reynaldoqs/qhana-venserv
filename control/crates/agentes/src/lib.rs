//! Agentes Android conectados por WebSocket (`/agente`).
//!
//! Da igual si la conexión llega por `adb reverse` (USB) o por Wi-Fi: el agente siempre es el cliente.
//! Cada conexión implementa [`CanalUssd`], así el motor de recetas no sabe que habla con un teléfono.

use async_trait::async_trait;
use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use nucleo::{Bus, CanalUssd, ErrorCanal, Registro};
use protocolo::{
    ahora_ms, codigos, tipos_evento as ev, AlAgente, DeAgente, Hola, ModoUssd, PasoUssd, Sms, PROTOCOLO,
};
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, Notify};

pub const INTERVALO_PING: Duration = Duration::from_secs(15);
pub const CAIDA_SIN_MENSAJES: Duration = Duration::from_secs(45);
const ESPERA_HOLA: Duration = Duration::from_secs(10);
/// Margen sobre el timeout del agente: él corta primero y nos manda `error: timeout`.
const MARGEN: Duration = Duration::from_secs(5);
const ESPERA_CORTA: Duration = Duration::from_secs(15);

/// Todos los agentes conectados.
pub struct Agentes {
    conexiones: RwLock<HashMap<String, Arc<ConexionAgente>>>,
    registro: Arc<Registro>,
    bus: Bus,
    token: String,
    servidor: String,
}

/// Una conexión viva con un agente.
pub struct ConexionAgente {
    pub id: String,
    tx: mpsc::UnboundedSender<AlAgente>,
    pendientes: Mutex<HashMap<String, oneshot::Sender<DeAgente>>>,
    cerrar: Notify,
    bus: Bus,
}

fn nuevo_req() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

impl ConexionAgente {
    /// Envía una petición y espera la respuesta con el mismo `req`.
    async fn pedir(&self, crear: impl FnOnce(String) -> AlAgente, espera: Duration) -> Result<DeAgente, ErrorCanal> {
        let req = nuevo_req();
        let (tx, rx) = oneshot::channel();
        self.pendientes.lock().unwrap().insert(req.clone(), tx);
        if self.tx.send(crear(req.clone())).is_err() {
            self.pendientes.lock().unwrap().remove(&req);
            return Err(ErrorCanal::desconectado());
        }
        match tokio::time::timeout(espera, rx).await {
            Ok(Ok(DeAgente::Error { codigo, detalle, .. })) => {
                Err(ErrorCanal::nuevo(&codigo, detalle.unwrap_or_default()))
            }
            Ok(Ok(m)) => Ok(m),
            Ok(Err(_)) => Err(ErrorCanal::desconectado()),
            Err(_) => {
                self.pendientes.lock().unwrap().remove(&req);
                Err(ErrorCanal::timeout())
            }
        }
    }

    fn evento_ussd(&self, accion: &str, slot: Option<u32>, enviado: &str, r: &Result<PasoUssd, ErrorCanal>) {
        let mut d = json!({ "dispositivo_id": self.id, "accion": accion, "enviado": enviado });
        if let Some(s) = slot {
            d["slot"] = json!(s);
        }
        match r {
            Ok(p) => {
                d["sesion"] = json!(p.sesion);
                d["texto"] = json!(p.texto);
                d["abierta"] = json!(p.abierta);
                d["ms"] = json!(p.ms);
            }
            Err(e) => {
                d["error"] = json!({ "codigo": e.codigo, "detalle": e.detalle });
            }
        }
        self.bus.emitir(ev::USSD, d);
    }

    fn a_paso(m: DeAgente) -> Result<PasoUssd, ErrorCanal> {
        match m {
            DeAgente::Ussd { sesion, texto, abierta, ms, .. } => Ok(PasoUssd { sesion, texto, abierta, ms }),
            otro => Err(ErrorCanal::nuevo(codigos::DESCONOCIDO, format!("respuesta inesperada: {otro:?}"))),
        }
    }

    pub async fn listar_sms(&self, slot: Option<u32>, desde_ms: Option<i64>, limite: Option<u32>) -> Result<Vec<Sms>, ErrorCanal> {
        match self.pedir(|req| AlAgente::SmsListar { req, slot, desde_ms, limite }, ESPERA_CORTA).await? {
            DeAgente::SmsLista { sms, .. } => Ok(sms),
            otro => Err(ErrorCanal::nuevo(codigos::DESCONOCIDO, format!("respuesta inesperada: {otro:?}"))),
        }
    }

    pub async fn refrescar(&self) -> Result<(), ErrorCanal> {
        self.pedir(|req| AlAgente::Refrescar { req }, ESPERA_CORTA).await.map(|_| ())
    }

    /// Cierra la conexión (el agente, si sigue vivo, vuelve a conectar solo).
    pub fn cortar(&self) {
        self.cerrar.notify_one();
    }

    /// Ida y vuelta medida en ms.
    pub async fn ping(&self) -> Result<u64, ErrorCanal> {
        let t = std::time::Instant::now();
        self.pedir(|req| AlAgente::Ping { req }, ESPERA_CORTA).await?;
        Ok(t.elapsed().as_millis() as u64)
    }
}

#[async_trait]
impl CanalUssd for ConexionAgente {
    async fn iniciar(&self, slot: u32, codigo: &str, modo: ModoUssd, timeout: Duration) -> Result<PasoUssd, ErrorCanal> {
        let timeout_ms = timeout.as_millis() as u64;
        let r = self
            .pedir(
                |req| AlAgente::UssdIniciar { req, slot, codigo: codigo.to_string(), modo, timeout_ms },
                timeout + MARGEN,
            )
            .await
            .and_then(Self::a_paso);
        self.evento_ussd("iniciar", Some(slot), codigo, &r);
        r
    }

    async fn responder(&self, sesion: &str, texto: &str, timeout: Duration) -> Result<PasoUssd, ErrorCanal> {
        let timeout_ms = timeout.as_millis() as u64;
        let r = self
            .pedir(
                |req| AlAgente::UssdResponder { req, sesion: sesion.to_string(), texto: texto.to_string(), timeout_ms },
                timeout + MARGEN,
            )
            .await
            .and_then(Self::a_paso);
        self.evento_ussd("responder", None, texto, &r);
        r
    }

    async fn cancelar(&self, sesion: &str) -> Result<(), ErrorCanal> {
        let r = self
            .pedir(|req| AlAgente::UssdCancelar { req, sesion: sesion.to_string() }, ESPERA_CORTA)
            .await
            .map(|_| ());
        self.bus.emitir(
            ev::USSD,
            json!({ "dispositivo_id": self.id, "accion": "cancelar", "sesion": sesion,
                    "error": r.as_ref().err().map(|e| json!({"codigo": e.codigo, "detalle": e.detalle})) }),
        );
        r
    }
}

impl Agentes {
    pub fn new(registro: Arc<Registro>, bus: Bus, token: impl Into<String>, servidor: impl Into<String>) -> Arc<Self> {
        Arc::new(Self {
            conexiones: RwLock::new(HashMap::new()),
            registro,
            bus,
            token: token.into(),
            servidor: servidor.into(),
        })
    }

    pub fn conexion(&self, id: &str) -> Option<Arc<ConexionAgente>> {
        self.conexiones.read().unwrap().get(id).cloned()
    }

    pub fn ids(&self) -> Vec<String> {
        self.conexiones.read().unwrap().keys().cloned().collect()
    }

    /// Atiende un WebSocket de agente hasta que se cierre.
    pub async fn atender(self: Arc<Self>, socket: WebSocket, origen: String) {
        let (mut ws_tx, mut ws_rx) = socket.split();

        // 1. Saludo.
        let hola = match tokio::time::timeout(ESPERA_HOLA, esperar_hola(&mut ws_rx)).await {
            Ok(Some(h)) => h,
            Ok(None) => {
                tracing::warn!(%origen, "el agente se desconectó sin saludar");
                return;
            }
            Err(_) => {
                tracing::warn!(%origen, "el agente no mandó 'hola' a tiempo");
                let _ = enviar(&mut ws_tx, &AlAgente::Rechazado { motivo: "se esperaba 'hola'".into() }).await;
                return;
            }
        };
        let rechazo = if !self.token.is_empty() && !protocolo::iguales_tiempo_constante(hola.token.as_bytes(), self.token.as_bytes()) {
            Some("token inválido".to_string())
        } else if hola.protocolo != PROTOCOLO {
            Some(format!("protocolo {} no soportado (el control habla {PROTOCOLO})", hola.protocolo))
        } else if hola.dispositivo_id.trim().is_empty() {
            Some("falta dispositivo_id".to_string())
        } else {
            None
        };
        if let Some(motivo) = rechazo {
            tracing::warn!(%origen, id = %hola.dispositivo_id, %motivo, "agente rechazado");
            let _ = enviar(&mut ws_tx, &AlAgente::Rechazado { motivo }).await;
            let _ = ws_tx.close().await;
            return;
        }
        if enviar(&mut ws_tx, &AlAgente::Bienvenido { servidor: self.servidor.clone(), hora_ms: ahora_ms() })
            .await
            .is_err()
        {
            return;
        }

        // 2. Registro.
        let id = hola.dispositivo_id.clone();
        let (tx, mut rx) = mpsc::unbounded_channel::<AlAgente>();
        let con = Arc::new(ConexionAgente {
            id: id.clone(),
            tx,
            pendientes: Mutex::new(HashMap::new()),
            cerrar: Notify::new(),
            bus: self.bus.clone(),
        });
        let generacion = self.registro.conectar(&hola, con.clone());
        // La app informa su serial de ADB (lo recibe al provisionar). Es la forma fiable de cruzar
        // teléfono ↔ USB: desde Android 8 cada app ve un ANDROID_ID distinto al de `adb shell`.
        if let Some(serial) = hola.adb_serial.as_deref().filter(|s| !s.is_empty()) {
            self.registro.vincular_adb(&id, serial);
        }
        if let Some(vieja) = self.conexiones.write().unwrap().insert(id.clone(), con.clone()) {
            tracing::info!(%id, "reconexión: cierro la conexión anterior");
            vieja.cerrar.notify_one();
        }
        tracing::info!(%id, %origen, modelo = %hola.modelo, sims = hola.sims.len(), "agente conectado");
        self.bus.emitir(ev::DISPOSITIVO_CONECTADO, json!({ "dispositivo_id": id, "dispositivo": self.registro.obtener(&id), "origen": origen }));

        // 3. Escritor y pings.
        let escritor = tokio::spawn(async move {
            while let Some(m) = rx.recv().await {
                if enviar(&mut ws_tx, &m).await.is_err() {
                    break;
                }
            }
            let _ = ws_tx.close().await;
        });
        let con_ping = con.clone();
        let pinger = tokio::spawn(async move {
            let mut t = tokio::time::interval(INTERVALO_PING);
            t.tick().await;
            loop {
                t.tick().await;
                if con_ping.tx.send(AlAgente::Ping { req: format!("ping-{}", nuevo_req()) }).is_err() {
                    break;
                }
            }
        });

        // 4. Lector. Sin ningún mensaje en 45 s (ni pong), el agente se da por caído.
        loop {
            tokio::select! {
                _ = con.cerrar.notified() => break,
                m = tokio::time::timeout(CAIDA_SIN_MENSAJES, ws_rx.next()) => match m {
                    Err(_) => { tracing::warn!(%id, "sin mensajes en 45 s: agente caído"); break; }
                    Ok(None) | Ok(Some(Err(_))) | Ok(Some(Ok(Message::Close(_)))) => break,
                    Ok(Some(Ok(Message::Text(t)))) => self.manejar(&con, &t),
                    Ok(Some(Ok(_))) => {}
                }
            }
        }

        // 5. Limpieza.
        pinger.abort();
        escritor.abort();
        con.pendientes.lock().unwrap().clear(); // los que esperan reciben "desconectado"
        {
            let mut c = self.conexiones.write().unwrap();
            if c.get(&id).map(|x| Arc::ptr_eq(x, &con)).unwrap_or(false) {
                c.remove(&id);
            }
        }
        if self.registro.desconectar(&id, generacion) {
            tracing::info!(%id, "agente desconectado");
            self.bus.emitir(ev::DISPOSITIVO_DESCONECTADO, json!({ "dispositivo_id": id, "dispositivo": self.registro.obtener(&id) }));
        }
    }

    fn manejar(&self, con: &ConexionAgente, texto: &str) {
        let m: DeAgente = match serde_json::from_str(texto) {
            Ok(m) => m,
            Err(e) => {
                tracing::debug!(id = %con.id, "mensaje no reconocido ({e}): {texto}");
                self.registro.tocar(&con.id);
                return;
            }
        };
        if let Some(req) = m.req() {
            let espera = con.pendientes.lock().unwrap().remove(req);
            match espera {
                Some(tx) => {
                    let _ = tx.send(m);
                }
                None if req.starts_with("ping-") => {}
                None => tracing::debug!(id = %con.id, %req, "respuesta sin petición pendiente (¿llegó tarde?)"),
            }
            self.registro.tocar(&con.id);
            return;
        }
        match m {
            DeAgente::Sims { sims } => {
                if let Some(d) = self.registro.actualizar_sims(&con.id, sims) {
                    self.bus.emitir(ev::DISPOSITIVO_ACTUALIZADO, json!({ "dispositivo_id": con.id, "dispositivo": d }));
                }
            }
            DeAgente::Estado(e) => {
                let antes = self.registro.obtener(&con.id).and_then(|d| d.estado);
                if let Some(d) = self.registro.actualizar_estado(&con.id, e.clone()) {
                    if antes.as_ref() != Some(&e) {
                        self.bus.emitir(ev::DISPOSITIVO_ACTUALIZADO, json!({ "dispositivo_id": con.id, "dispositivo": d }));
                    }
                }
            }
            DeAgente::SmsRecibido { sms } => {
                self.registro.tocar(&con.id);
                self.bus.emitir(ev::SMS, json!({ "dispositivo_id": con.id, "sms": sms }));
            }
            DeAgente::Error { codigo, detalle, .. } => {
                tracing::warn!(id = %con.id, %codigo, ?detalle, "error del agente sin petición");
            }
            DeAgente::Hola(_) => tracing::debug!(id = %con.id, "'hola' repetido, ignorado"),
            _ => self.registro.tocar(&con.id),
        }
    }
}

async fn esperar_hola(rx: &mut futures_util::stream::SplitStream<WebSocket>) -> Option<Hola> {
    while let Some(m) = rx.next().await {
        match m {
            Ok(Message::Text(t)) => match serde_json::from_str::<DeAgente>(&t) {
                Ok(DeAgente::Hola(h)) => return Some(h),
                Ok(_) => tracing::debug!("mensaje antes de 'hola', ignorado"),
                Err(e) => tracing::debug!("primer mensaje no reconocido: {e}"),
            },
            Ok(Message::Close(_)) | Err(_) => return None,
            Ok(_) => {}
        }
    }
    None
}

async fn enviar(
    tx: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    m: &AlAgente,
) -> Result<(), axum::Error> {
    let s = serde_json::to_string(m).expect("serializar mensaje");
    tx.send(Message::Text(s)).await
}
