//! agente-simulado: se conecta al control como si fuera un teléfono Android con dos chips.
//! Responde los flujos reales de Tigo (y Entel en el chip 2 si se pide) usando `nucleo::simulacion`.
//!
//!     agente-simulado --id sim-1 --saldo 14
//!     agente-simulado --id sim-2 --chip2 entel --limite-diario 0   # todo transferir topa el límite
//!     agente-simulado --pin-dealer 4321 --saldo-dealer 50             # chip 1 es Tigo dealer (*555#)
//!     agente-simulado --chip2 entel --entel-dealer                    # chip 2 es Entel dealer (*133*número*050#)

use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use nucleo::simulacion::{LineaSimulada, Operador};
use protocolo::{ahora_ms, codigos, AlAgente, DeAgente, EstadoDispositivo, Hola, ModoUssd, Sim, Sms, PROTOCOLO};
use std::time::{Duration, Instant};
use tokio_tungstenite::tungstenite::Message;

#[derive(Parser, Debug)]
#[command(name = "agente-simulado", about = "Teléfono falso para probar el control sin gastar saldo")]
struct Args {
    #[arg(long, default_value = "ws://127.0.0.1:47800/agente")]
    url: String,
    #[arg(long, default_value = "dev")]
    token: String,
    /// dispositivo_id (en un teléfono real es el ANDROID_ID)
    #[arg(long, default_value = "simulado-1")]
    id: String,
    /// Saldo principal del chip 1 (Tigo)
    #[arg(long, default_value_t = 14.0)]
    saldo: f64,
    /// Operador del chip 2: tigo | entel | ninguno
    #[arg(long, default_value = "tigo")]
    chip2: String,
    /// Transferencias permitidas por día
    #[arg(long, default_value_t = 5)]
    limite_diario: u32,
    /// Números de los chips 1 y 2. Con un teléfono real conectado al mismo control, usar otros: los chips
    /// simulados no tienen ICCID y el control los confundiría por número con los reales.
    #[arg(long, default_value = "78815232")]
    numero1: String,
    #[arg(long, default_value = "69759675")]
    numero2: String,
    /// Hace del chip 1 un Tigo dealer con este PIN para `*555#`
    #[arg(long)]
    pin_dealer: Option<String>,
    /// Saldo de dealer del chip 1 (con --pin-dealer)
    #[arg(long, default_value_t = 50.0)]
    saldo_dealer: f64,
    /// Número del chip 2 cuando es Entel
    #[arg(long, default_value = "71227468")]
    numero_entel: String,
    /// Hace del chip 2 (con --chip2 entel) un Entel dealer que acepta *133*número*monto#
    #[arg(long)]
    entel_dealer: bool,
    /// Saldo de dealer del chip 2 Entel (con --entel-dealer)
    #[arg(long, default_value_t = 50.0)]
    saldo_dealer_entel: f64,
    /// Demora simulada de la red por paso (ms)
    #[arg(long, default_value_t = 700)]
    retardo_ms: u64,
}

struct Telefono {
    args: Args,
    lineas: Vec<LineaSimulada>,
    /// (id de sesión, slot, última actividad)
    sesion: Option<(String, usize, Instant)>,
    sms: Vec<Sms>,
}

impl Telefono {
    fn sims(&self) -> Vec<Sim> {
        self.lineas
            .iter()
            .enumerate()
            .map(|(i, l)| {
                let (op, mccmnc) = match l.operador {
                    Operador::Tigo => ("TIGO", "73603"),
                    Operador::Entel => ("ENTEL", "73602"),
                };
                Sim {
                    slot: i as u32,
                    sub_id: 4 + i as i64,
                    operador: op.into(),
                    mcc_mnc: mccmnc.into(),
                    // Sale del número: dos simuladores con números distintos son chips distintos para el control.
                    iccid: Some(format!("895910300{}{i}", l.linea)),
                    numero: Some(l.linea.clone()),
                    estado: "listo".into(),
                    voz: Some("en_servicio".into()),
                    red: Some("LTE".into()),
                }
            })
            .collect()
    }

    fn estado(&self) -> EstadoDispositivo {
        EstadoDispositivo {
            bateria: Some(80),
            cargando: Some(true),
            pantalla_encendida: Some(true),
            accesibilidad_activa: Some(true),
            permisos_faltantes: vec![],
        }
    }

    fn hola(&self) -> DeAgente {
        DeAgente::Hola(Hola {
            protocolo: PROTOCOLO,
            token: self.args.token.clone(),
            dispositivo_id: self.args.id.clone(),
            fabricante: "SIMULADO".into(),
            modelo: format!("Teléfono simulado ({} chips)", self.lineas.len()),
            android: "14".into(),
            sdk: 34,
            app_version: env!("CARGO_PKG_VERSION").into(),
            sims: self.sims(),
            capacidades: vec!["ussd_api".into(), "ussd_pantalla".into(), "sms".into()],
            adb_serial: None,
        })
    }

    fn error(req: String, codigo: &str, detalle: &str) -> DeAgente {
        DeAgente::Error { req: Some(req), codigo: codigo.into(), detalle: Some(detalle.into()) }
    }

    /// Atiende un mensaje del control. Devuelve los mensajes a enviar.
    async fn atender(&mut self, m: AlAgente) -> Vec<DeAgente> {
        let retardo = Duration::from_millis(self.args.retardo_ms);
        match m {
            AlAgente::Bienvenido { servidor, .. } => {
                tracing::info!("conectado a {servidor}");
                vec![DeAgente::Estado(self.estado())]
            }
            AlAgente::Rechazado { motivo } => {
                tracing::error!("el control me rechazó: {motivo}");
                std::process::exit(2);
            }
            AlAgente::Ping { req } => vec![DeAgente::Pong { req }],
            AlAgente::Refrescar { req } => {
                vec![DeAgente::Sims { sims: self.sims() }, DeAgente::Estado(self.estado()), DeAgente::Ok { req }]
            }
            AlAgente::UssdIniciar { req, slot, codigo, modo, .. } => {
                if self.sesion.is_some() {
                    return vec![Self::error(req, codigos::OCUPADO, "ya hay una sesión USSD abierta")];
                }
                let Some(l) = self.lineas.get_mut(slot as usize) else {
                    return vec![Self::error(req, codigos::SIM_INVALIDA, "no hay chip en esa ranura")];
                };
                let t = Instant::now();
                tokio::time::sleep(retardo * 3).await; // el primer paso tarda más (~2.3 s reales)
                let p = l.marcar(&codigo);
                let abierta = p.abierta && modo == ModoUssd::Pantalla;
                if !abierta {
                    l.cancelar();
                }
                let sesion = uuid::Uuid::new_v4().to_string();
                if abierta {
                    self.sesion = Some((sesion.clone(), slot as usize, Instant::now()));
                }
                tracing::info!(%codigo, slot, abierta, "ussd iniciar");
                vec![DeAgente::Ussd { req, sesion, texto: p.texto, abierta, ms: t.elapsed().as_millis() as u64 }]
            }
            AlAgente::UssdResponder { req, sesion, texto, .. } => {
                let slot = match &self.sesion {
                    Some((s, slot, _)) if *s == sesion => *slot,
                    _ => return vec![Self::error(req, codigos::SESION_INEXISTENTE, "no hay sesión con ese id")],
                };
                let t = Instant::now();
                tokio::time::sleep(retardo).await;
                let Some(p) = self.lineas[slot].responder(&texto) else {
                    self.sesion = None;
                    return vec![Self::error(req, codigos::SESION_INEXISTENTE, "la sesión ya terminó")];
                };
                if p.abierta {
                    self.sesion = Some((sesion.clone(), slot, Instant::now()));
                } else {
                    self.sesion = None;
                }
                tracing::info!(%texto, slot, abierta = p.abierta, "ussd responder");
                let mut out =
                    vec![DeAgente::Ussd { req, sesion, texto: p.texto, abierta: p.abierta, ms: t.elapsed().as_millis() as u64 }];
                if let Some(t) = self.lineas[slot].sms_pendiente.take() {
                    let sms = Sms {
                        id: format!("{}", self.sms.len() + 1),
                        slot: Some(slot as u32),
                        de: "TIGO".into(),
                        texto: t,
                        fecha_ms: ahora_ms(),
                        leido: false,
                    };
                    self.sms.push(sms.clone());
                    out.push(DeAgente::SmsRecibido { sms });
                }
                out
            }
            AlAgente::UssdCancelar { req, sesion } => match &self.sesion {
                Some((s, slot, _)) if *s == sesion => {
                    self.lineas[*slot].cancelar();
                    self.sesion = None;
                    vec![DeAgente::Ok { req }]
                }
                _ => vec![Self::error(req, codigos::SESION_INEXISTENTE, "no hay sesión con ese id")],
            },
            AlAgente::SmsListar { req, slot, desde_ms, limite } => {
                let mut v: Vec<Sms> = self
                    .sms
                    .iter()
                    .rev()
                    .filter(|s| slot.is_none() || s.slot == slot)
                    .filter(|s| desde_ms.map(|d| s.fecha_ms >= d).unwrap_or(true))
                    .cloned()
                    .collect();
                v.truncate(limite.unwrap_or(50) as usize);
                vec![DeAgente::SmsLista { req, sms: v }]
            }
        }
    }

    /// Cierra sesiones sin actividad por 60 s (como el agente real).
    fn vencer_sesiones(&mut self) {
        if let Some((_, slot, t)) = &self.sesion {
            if t.elapsed() > Duration::from_secs(60) {
                tracing::info!("sesión inactiva 60 s: la cancelo");
                self.lineas[*slot].cancelar();
                self.sesion = None;
            }
        }
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();
    let args = Args::parse();
    let mut l1 = LineaSimulada::tigo(&args.numero1, args.saldo);
    l1.limite_diario = args.limite_diario;
    l1.pin_dealer = args.pin_dealer.clone();
    l1.saldo_dealer = args.saldo_dealer;
    let mut lineas = vec![l1];
    match args.chip2.as_str() {
        "tigo" => {
            let mut l = LineaSimulada::tigo(&args.numero2, 8.0);
            l.limite_diario = args.limite_diario;
            lineas.push(l);
        }
        "entel" => {
            let mut l = LineaSimulada::entel(&args.numero_entel, 0.0);
            l.dealer_entel = args.entel_dealer;
            l.saldo_dealer = args.saldo_dealer_entel;
            lineas.push(l);
        }
        _ => {}
    }
    let sms = vec![Sms {
        id: "0".into(),
        slot: Some(0),
        de: "300".into(),
        texto: "Bienvenido a Tigo. Tu linea esta activa.".into(),
        fecha_ms: ahora_ms() - 3_600_000,
        leido: true,
    }];
    let mut tel = Telefono { args, lineas, sesion: None, sms };

    let mut espera = 1u64;
    loop {
        tracing::info!(url = %tel.args.url, "conectando…");
        match tokio_tungstenite::connect_async(tel.args.url.as_str()).await {
            Ok((ws, _)) => {
                espera = 1;
                if let Err(e) = sesion(&mut tel, ws).await {
                    tracing::warn!("conexión terminada: {e}");
                }
                // Al reconectar se descartan las sesiones abiertas.
                if let Some((_, slot, _)) = tel.sesion.take() {
                    tel.lineas[slot].cancelar();
                }
            }
            Err(e) => tracing::warn!("no pude conectar: {e}"),
        }
        tracing::info!("reintento en {espera} s");
        tokio::time::sleep(Duration::from_secs(espera)).await;
        espera = (espera * 2).min(30);
    }
}

async fn sesion(
    tel: &mut Telefono,
    ws: tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
) -> anyhow::Result<()> {
    let (mut tx, mut rx) = ws.split();
    tx.send(Message::Text(serde_json::to_string(&tel.hola())?)).await?;
    let mut tic = tokio::time::interval(Duration::from_secs(30));
    let mut vencer = tokio::time::interval(Duration::from_secs(5));
    loop {
        tokio::select! {
            _ = tic.tick() => {
                tx.send(Message::Text(serde_json::to_string(&DeAgente::Estado(tel.estado()))?)).await?;
            }
            _ = vencer.tick() => tel.vencer_sesiones(),
            m = rx.next() => {
                let m = match m {
                    None => return Ok(()),
                    Some(m) => m?,
                };
                match m {
                    Message::Text(t) => {
                        let msg: AlAgente = match serde_json::from_str(&t) {
                            Ok(x) => x,
                            Err(e) => { tracing::debug!("mensaje no reconocido ({e}): {t}"); continue; }
                        };
                        for r in tel.atender(msg).await {
                            tx.send(Message::Text(serde_json::to_string(&r)?)).await?;
                        }
                    }
                    Message::Close(_) => return Ok(()),
                    _ => {}
                }
            }
        }
    }
}
