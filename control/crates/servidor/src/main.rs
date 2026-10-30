//! qhana-control: el servidor que corre en el PC (Windows en producción).
//!
//! Un solo puerto (47800) para:
//! - `/agente`        WebSocket de los teléfonos (por `adb reverse` o Wi-Fi)
//! - `/api/*`         API para la interfaz
//! - `/api/eventos`   WebSocket de eventos en vivo
//! - `/`              la interfaz compilada (`ui/dist`), si existe

mod api;
mod error;

use adb::{Adb, GestorAdb, Provision};
use agentes::Agentes;
use almacen::Almacen;
use clap::Parser;
use nucleo::{Bus, Receta, Registro};
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

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
    /// Actividad principal de la app agente (paquete/.Clase)
    #[arg(long, env = "QHANA_ACTIVIDAD", default_value = "bo.qhana.agente/.ui.MainActivity")]
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
    pub recetas: Arc<RwLock<BTreeMap<String, Receta>>>,
    /// Lotes desde CSV y estado de los chips (saldo, cupo).
    pub lotes: Arc<lotes::Lotes>,
    pub ruta_recetas: PathBuf,
    pub provision: Provision,
    pub apk: Option<PathBuf>,
    pub puerto: u16,
}

impl App {
    pub fn cargar_recetas(&self) -> (usize, Vec<String>) {
        let (r, errores) = Receta::cargar_carpeta(&self.ruta_recetas);
        for e in &errores {
            tracing::warn!("receta ignorada: {e}");
        }
        let n = r.len();
        *self.recetas.write().unwrap() = r;
        (n, errores)
    }
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
        lotes,
        ruta_recetas: args.recetas.clone(),
        provision: Provision::para_paquete(&args.paquete, &args.servicio_accesibilidad, &args.actividad),
        apk: args.apk.clone(),
        puerto,
    });
    let (n, _) = app.cargar_recetas();
    tracing::info!(recetas = n, carpeta = %args.recetas.display(), "recetas cargadas");

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
    if args.escuchar.ip().is_unspecified() && args.token.is_empty() {
        tracing::warn!("escuchando en todas las interfaces SIN token: cualquier equipo de la red puede conectarse como agente");
    }
    axum::serve(listener, rutas.into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
            tracing::info!("cerrando…");
        })
        .await?;
    Ok(())
}
