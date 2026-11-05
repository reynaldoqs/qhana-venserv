//! qhana-control: el servidor que corre en el PC (Windows en producción).
//!
//! Un solo puerto (47800) para:
//! - `/agente`        WebSocket de los teléfonos (por `adb reverse` o Wi-Fi)
//! - `/api/*`         API para la interfaz
//! - `/api/eventos`   WebSocket de eventos en vivo
//! - `/`              la interfaz compilada (`ui/dist`), si existe

mod api;
mod api_recetas;
mod error;

use adb::{Adb, GestorAdb, Provision};
use agentes::Agentes;
use almacen::Almacen;
use clap::Parser;
use nucleo::carpeta::{self, RecetaCargada};
use nucleo::{Bus, Registro};
use std::collections::{BTreeMap, HashMap};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Parser, Debug)]
#[command(name = "qhana-control", version, about = "Control de teléfonos/módems para USSD y SMS")]
pub struct Args {
    /// Dirección de escucha. Para agentes por Wi-Fi: 0.0.0.0:47800
    #[arg(long, env = "QHANA_ESCUCHAR", default_value = "127.0.0.1:47800")]
    pub escuchar: SocketAddr,
    /// Token que deben mandar los agentes en `hola` (vacío = sin token)
    #[arg(long, env = "QHANA_TOKEN", default_value = "dev")]
    pub token: String,
    /// Token de la API para pedidos desde otra máquina (16+ caracteres). Obligatorio si no se escucha en 127.0.0.1
    #[arg(long, env = "QHANA_API_TOKEN", hide_env_values = true)]
    pub api_token: Option<String>,
    /// Carpeta de recetas YAML
    #[arg(long, env = "QHANA_RECETAS", default_value = "recetas")]
    pub recetas: PathBuf,
    /// Base de datos SQLite del historial
    #[arg(long, env = "QHANA_DB", default_value = "datos/control.db")]
    pub db: PathBuf,
    /// Ruta a adb (por defecto: PATH o el SDK de Android)
    #[arg(long, env = "QHANA_ADB")]
    pub adb: Option<PathBuf>,
    /// No usar ADB (agentes solo por Wi-Fi)
    #[arg(long, env = "QHANA_SIN_ADB")]
    pub sin_adb: bool,
    /// No buscar módems USB (solo teléfonos)
    #[arg(long, env = "QHANA_SIN_MODEMS")]
    pub sin_modems: bool,
    /// Puertos de módems fijos, separados por coma (p. ej. COM5,COM6). Por defecto se buscan solos
    #[arg(long, env = "QHANA_MODEM_PUERTOS", value_delimiter = ',')]
    pub modem_puertos: Option<Vec<String>>,
    /// Probar módems de cualquier fabricante USB, no solo los conocidos (Huawei, ZTE, Quectel, SIMCom…)
    #[arg(long, env = "QHANA_MODEM_TODOS_USB")]
    pub modem_todos_usb: bool,
    /// Carpeta de la interfaz compilada
    #[arg(long, env = "QHANA_UI", default_value = "ui/dist")]
    pub ui: PathBuf,
    /// Paquete de la app agente (para provisionar)
    #[arg(long, env = "QHANA_PAQUETE", default_value = "bo.qhana.agente")]
    pub paquete: String,
    /// Servicio de accesibilidad de la app agente (paquete/.Clase)
    #[arg(long, env = "QHANA_SERVICIO_ACCESIBILIDAD", default_value = "bo.qhana.agente/.ussd.ServicioAccesibilidad")]
    pub servicio_accesibilidad: String,
    /// Actividad de configuración de la app agente (paquete/.Clase): recibe url, token, adb_serial y reconectar
    #[arg(long, env = "QHANA_ACTIVIDAD", default_value = "bo.qhana.agente/.ui.ActividadConfig")]
    pub actividad: String,
    /// APK de la app agente para provisionar teléfonos nuevos
    #[arg(long, env = "QHANA_APK")]
    pub apk: Option<PathBuf>,
}

pub struct App {
    pub registro: Arc<Registro>,
    pub bus: Bus,
    pub agentes: Arc<Agentes>,
    pub adb: Option<Arc<GestorAdb>>,
    pub almacen: Arc<Almacen>,
    /// Recetas cargadas (las comparte `lotes`). Se reemplaza entero al recargar.
    pub recetas: Arc<RwLock<BTreeMap<String, nucleo::Receta>>>,
    /// Las mismas recetas con su archivo, texto, versión y fecha (para la API de edición).
    pub cargadas: RwLock<BTreeMap<String, RecetaCargada>>,
    /// Serializa las escrituras de la carpeta de recetas (guardar, retirar, recargar): dos no se intercalan.
    pub escritura_recetas: Mutex<()>,
    /// Lotes desde CSV y estado de los chips (saldo, cupo).
    pub lotes: Arc<lotes::Lotes>,
    pub ruta_recetas: PathBuf,
    pub provision: Provision,
    pub apk: Option<PathBuf>,
    pub puerto: u16,
    /// Token de la API para pedidos que no vienen de esta máquina (`None` = solo loopback, sin token).
    pub api_token: Option<String>,
    /// Sesiones USSD manuales abiertas (`/api/ussd/iniciar`), por dispositivo: el teléfono queda reservado.
    pub sesiones: Mutex<HashMap<String, SesionManual>>,
}

/// Una sesión USSD manual abierta. Mientras exista, el teléfono figura `ocupado` y los lotes no lo usan.
pub struct SesionManual {
    pub sesion: String,
    pub ultima: Instant,
    /// Hay un `responder` esperando al agente: no se vence en medio.
    pub en_uso: bool,
    pub reserva: lotes::contexto::Reserva,
}

/// Sin actividad por este tiempo, la sesión manual se da por cerrada y el teléfono se libera.
pub const SESION_MANUAL_MAX: Duration = Duration::from_secs(60);

/// Eventos más viejos que esto se borran del historial (las ejecuciones y los lotes quedan).
const EVENTOS_DIAS: i64 = 90;

impl App {
    /// Candado de escritura de la carpeta de recetas.
    pub fn candado_recetas(&self) -> std::sync::MutexGuard<'_, ()> {
        self.escritura_recetas.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Vuelve a cargar la carpeta de recetas (toma el candado de escritura). Con `emitir`, avisa `receta_cambiada`
    /// por cada receta nueva, cambiada o que ya no está.
    pub fn cargar_recetas(&self, emitir: bool) -> (usize, Vec<String>) {
        let _g = self.candado_recetas();
        self.recargar_con_candado(emitir)
    }

    /// Como [`App::cargar_recetas`], para quien ya tiene el candado (después de escribir).
    pub fn recargar_con_candado(&self, emitir: bool) -> (usize, Vec<String>) {
        let (nuevas, errores) = carpeta::cargar(&self.ruta_recetas);
        for e in &errores {
            tracing::warn!("receta ignorada: {e}");
        }
        let n = nuevas.len();
        *self.recetas.write().unwrap() = nuevas.iter().map(|(k, c)| (k.clone(), c.receta.clone())).collect();
        let viejas = std::mem::replace(&mut *self.cargadas.write().unwrap(), nuevas.clone());
        if emitir {
            for (nombre, cambio, version) in cambios(&viejas, &nuevas) {
                tracing::info!(%nombre, cambio, "receta cambiada");
                self.bus.emitir(
                    protocolo::tipos_evento::RECETA_CAMBIADA,
                    serde_json::json!({ "nombre": nombre, "cambio": cambio, "version": version }),
                );
            }
        }
        (n, errores)
    }
}

/// Diferencias entre dos cargas: (nombre, `creada` | `editada` | `retirada`, versión nueva o `None`).
pub fn cambios(
    viejas: &BTreeMap<String, RecetaCargada>,
    nuevas: &BTreeMap<String, RecetaCargada>,
) -> Vec<(String, &'static str, Option<String>)> {
    let mut v = vec![];
    for (k, n) in nuevas {
        match viejas.get(k) {
            None => v.push((k.clone(), "creada", Some(n.version.clone()))),
            Some(a) if a.version != n.version => v.push((k.clone(), "editada", Some(n.version.clone()))),
            Some(_) => {}
        }
    }
    for k in viejas.keys().filter(|k| !nuevas.contains_key(*k)) {
        v.push((k.clone(), "retirada", None));
    }
    v
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,tower_http=warn".into()),
        )
        .init();
    let args = Args::parse();
    let api_token = args.api_token.clone().map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
    if !args.escuchar.ip().is_loopback() {
        // Escuchar en la red (Wi-Fi): sin tokens fuertes, cualquiera de la red podría mover dinero.
        if api_token.as_ref().is_none_or(|t| t.chars().count() < 16) {
            anyhow::bail!(
                "escuchando en {} hace falta --api-token (o QHANA_API_TOKEN) de 16 caracteres o más",
                args.escuchar
            );
        }
        if args.token.trim().is_empty() || args.token == "dev" {
            anyhow::bail!("escuchando en {} el token de los agentes (--token) no puede ser vacío ni 'dev'", args.escuchar);
        }
    }

    let registro = Arc::new(Registro::new());
    let bus = Bus::default();
    let almacen = Arc::new(Almacen::abrir(&args.db)?);
    let agentes = Agentes::new(registro.clone(), bus.clone(), args.token.clone(), format!("qhana-control {VERSION}"));
    let puerto = args.escuchar.port();

    let adb = if args.sin_adb {
        tracing::info!("ADB desactivado (--sin-adb)");
        None
    } else {
        match Adb::resolver(args.adb.clone()) {
            Some(a) => {
                tracing::info!(adb = %a.ruta().display(), "usando ADB");
                Some(GestorAdb::iniciar(a, puerto, registro.clone(), bus.clone()))
            }
            None => {
                tracing::warn!("no encontré adb; sigo sin ADB (usa --adb <ruta>)");
                None
            }
        }
    };

    if args.sin_modems {
        tracing::info!("módems desactivados (--sin-modems)");
    } else {
        modem::iniciar(
            registro.clone(),
            bus.clone(),
            modem::Opciones {
                puertos: args.modem_puertos.clone(),
                todos_usb: args.modem_todos_usb,
                ..Default::default()
            },
        );
    }

    let recetas = Arc::new(RwLock::new(BTreeMap::new()));
    let ctx = lotes::Contexto::new(registro.clone(), bus.clone(), almacen.clone(), recetas.clone())?;
    ctx.vigilar_chips();
    let lotes = lotes::Lotes::new(ctx, lotes::Esperas::default())?;

    let app = Arc::new(App {
        registro,
        bus: bus.clone(),
        agentes,
        adb,
        almacen: almacen.clone(),
        recetas,
        cargadas: RwLock::new(BTreeMap::new()),
        escritura_recetas: Mutex::new(()),
        lotes,
        ruta_recetas: args.recetas.clone(),
        provision: Provision::para_paquete(&args.paquete, &args.servicio_accesibilidad, &args.actividad),
        apk: args.apk.clone(),
        puerto,
        api_token,
        sesiones: Mutex::new(HashMap::new()),
    });
    let (n, _) = app.cargar_recetas(false);
    tracing::info!(recetas = n, carpeta = %args.recetas.display(), "recetas cargadas");

    // Sesiones USSD manuales olvidadas: a los 60 s sin actividad se cierran y el teléfono se libera.
    let app_s = app.clone();
    tokio::spawn(async move {
        let mut t = tokio::time::interval(Duration::from_secs(5));
        loop {
            t.tick().await;
            api::vencer_sesiones(&app_s).await;
        }
    });

    // Mantenimiento de la base: respaldo diario y poda de eventos viejos. Al arrancar y cada 24 h, en un hilo
    // aparte (no frena al ejecutor: el respaldo usa su propia conexión).
    let a = almacen.clone();
    tokio::spawn(async move {
        loop {
            let a2 = a.clone();
            let r = tokio::task::spawn_blocking(move || mantenimiento(&a2)).await;
            if let Err(e) = r {
                tracing::warn!("mantenimiento de la base: {e}");
            }
            tokio::time::sleep(Duration::from_secs(24 * 60 * 60)).await;
        }
    });

    // Historial: todo evento del bus va a SQLite.
    let mut rx = bus.suscribir();
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(e) => {
                    if let Err(err) = almacen.guardar_evento(&e) {
                        tracing::warn!("no pude guardar evento: {err}");
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!("historial: se perdieron {n} eventos")
                }
                Err(_) => break,
            }
        }
    });

    let rutas = api::rutas(app.clone(), &args.ui);
    let listener = tokio::net::TcpListener::bind(args.escuchar).await?;
    tracing::info!("qhana-control {VERSION} escuchando en http://{}", args.escuchar);
    if app.api_token.is_some() && !args.escuchar.ip().is_loopback() {
        tracing::info!("la API pide token a los pedidos que no vienen de esta máquina");
    }
    axum::serve(listener, rutas.into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
            tracing::info!("cerrando…");
        })
        .await?;
    Ok(())
}

/// Respaldo del día (`datos/backup/AAAA-MM-DD_control.db`, quedan los últimos 14) y poda de eventos de más de 90 días.
fn mantenimiento(a: &Almacen) {
    match a.respaldar(&lotes::contexto::hoy()) {
        Ok(Some(r)) => tracing::info!(respaldo = %r.display(), "base respaldada"),
        Ok(None) => {}
        Err(e) => tracing::warn!("no pude respaldar la base: {e}"),
    }
    match a.podar_eventos(protocolo::ahora_ms() - EVENTOS_DIAS * 24 * 60 * 60 * 1000) {
        Ok(0) => {}
        Ok(n) => tracing::info!("historial: borré {n} eventos de más de {EVENTOS_DIAS} días"),
        Err(e) => tracing::warn!("no pude podar el historial: {e}"),
    }
}
