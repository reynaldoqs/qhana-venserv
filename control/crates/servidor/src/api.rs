//! Rutas HTTP/WS del contrato (`docs/protocolo.md`, sección "API del control").

use crate::api_recetas;
use crate::error::{ApiError, ApiResult, Cuerpo};
use crate::{App, SesionManual, SESION_MANUAL_MAX, VERSION};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::{header, HeaderMap, Request, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use nucleo::motor;
use protocolo::{tipos_evento as ev, Dispositivo, InfoAdb, ModoUssd, PasoUssd, Resultado, Sms};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::Path as RutaFs;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tower_http::services::{ServeDir, ServeFile};

type Ctx = State<Arc<App>>;

pub fn rutas(app: Arc<App>, ui: &RutaFs) -> Router {
    let api = Router::new()
        .route("/agente", get(ws_agente))
        .route("/api/salud", get(salud))
        .route("/api/dispositivos", get(dispositivos))
        .route("/api/dispositivos/reconectar", post(reconectar))
        .route("/api/dispositivos/:id/sms", get(sms))
        .route("/api/dispositivos/:id/refrescar", post(refrescar))
        .route("/api/adb", get(adb_listar))
        .route("/api/adb/:serial/provisionar", post(adb_provisionar))
        .route("/api/ussd/iniciar", post(ussd_iniciar))
        .route("/api/ussd/responder", post(ussd_responder))
        .route("/api/ussd/cancelar", post(ussd_cancelar))
        .route("/api/recetas", get(api_recetas::listar).post(api_recetas::crear))
        .route("/api/recetas/recargar", post(recetas_recargar))
        .route("/api/recetas/validar", post(api_recetas::validar))
        .route("/api/recetas/probar-paso", post(api_recetas::probar_paso))
        .route("/api/recetas/desde-sesion", post(api_recetas::desde_sesion))
        .route(
            "/api/recetas/:nombre",
            get(api_recetas::detalle).put(api_recetas::reemplazar).delete(api_recetas::retirar),
        )
        .route("/api/recetas/:nombre/estadisticas", get(api_recetas::ver_estadisticas))
        .route("/api/recetas/:nombre/versiones", get(api_recetas::versiones))
        .route("/api/recetas/:nombre/versiones/:id", get(api_recetas::version))
        .route("/api/recetas/:nombre/ejecutar", post(receta_ejecutar))
        .route("/api/lotes", get(lotes_listar).post(lotes_crear))
        .route("/api/lotes/validar", post(lotes_validar))
        .route("/api/lotes/plantilla.csv", get(lotes_plantilla))
        .route("/api/lotes/:id", get(lotes_obtener))
        .route("/api/lotes/:id/filas", get(lotes_filas))
        .route("/api/lotes/:id/archivar", post(lotes_archivar))
        .route("/api/lotes/:id/iniciar", post(lotes_iniciar))
        .route("/api/lotes/:id/pausar", post(lotes_pausar))
        .route("/api/lotes/:id/cancelar", post(lotes_cancelar))
        .route("/api/lotes/:id/reintentar-fallidas", post(lotes_reintentar_fallidas))
        .route("/api/lotes/:id/verificar", post(lotes_verificar))
        .route("/api/lotes/:id/carriles", get(lotes_carriles))
        .route("/api/lotes/:id/resultado.csv", get(lotes_resultado))
        .route("/api/chips", get(chips_listar))
        .route("/api/chips/saldos", post(chips_saldos))
        .route("/api/chips/:dispositivo/:slot", axum::routing::patch(chip_tipo))
        .route("/api/chips/:dispositivo/:slot/pin", axum::routing::put(chip_pin))
        .route("/api/historial", get(historial))
        .route("/api/ejecuciones", get(ejecuciones))
        .route("/api/eventos", get(ws_eventos))
        // Cualquier otra ruta /api/* es un error de la API (no la interfaz).
        .route("/api/*resto", axum::routing::any(api_inexistente))
        .with_state(app.clone())
        // Los CSV de lotes pueden pesar ~1 MB.
        .layer(axum::extract::DefaultBodyLimit::max(4 * 1024 * 1024))
        // Token de la API para pedidos desde otra máquina (ver "Autenticación de la API").
        .layer(middleware::from_fn_with_state(app, autenticar))
        // Sin CORS: la interfaz va por el mismo origen. Un sitio web cualquiera no puede llamar a la API desde el navegador.
        .layer(middleware::from_fn(solo_mismo_origen));

    let indice = ui.join("index.html");
    if indice.is_file() {
        tracing::info!(ui = %ui.display(), "sirviendo la interfaz");
        api.fallback_service(ServeDir::new(ui).fallback(ServeFile::new(indice)))
    } else {
        tracing::info!(ui = %ui.display(), "interfaz no compilada; solo API");
        let ui = ui.display().to_string();
        api.fallback(move || async move { sin_ui(&ui) })
    }
}

async fn api_inexistente(uri: axum::http::Uri) -> ApiError {
    ApiError::new(StatusCode::NOT_FOUND, "ruta_inexistente", format!("no existe {}", uri.path()))
}

fn sin_ui(ui: &str) -> Response {
    (
        StatusCode::OK,
        Html(format!(
            "<!doctype html><meta charset=utf-8><title>qhana-control</title>\
             <body style='font-family:system-ui;padding:2rem'>\
             <h1>qhana-control {VERSION}</h1><p>La interfaz no está compilada (<code>{ui}</code>). \
             La API está en <a href='/api/salud'>/api/salud</a>.</p>"
        )),
    )
        .into_response()
}

// ───────────────────────────── agentes ─────────────────────────────

async fn ws_agente(ws: WebSocketUpgrade, State(app): Ctx, ConnectInfo(addr): ConnectInfo<SocketAddr>) -> Response {
    let agentes = app.agentes.clone();
    ws.on_upgrade(move |socket| agentes.atender(socket, addr.to_string()))
}

// ───────────────────────────── estado ─────────────────────────────

async fn salud(State(app): Ctx, ConnectInfo(addr): ConnectInfo<SocketAddr>) -> Json<Value> {
    Json(json!({
        "version": VERSION,
        "agentes_conectados": app.registro.conectados(),
        "requiere_token": app.api_token.is_some() && !es_local(&addr),
        "adb": app.adb.as_ref().map(|g| g.disponible()).unwrap_or(false),
        "recetas": app.recetas.read().unwrap().len(),
        "puerto": app.puerto,
    }))
}

async fn dispositivos(State(app): Ctx) -> Json<Vec<Dispositivo>> {
    Json(app.registro.listar())
}

async fn adb_listar(State(app): Ctx) -> Json<Vec<InfoAdb>> {
    Json(app.adb.as_ref().map(|g| g.listar()).unwrap_or_default())
}

/// El APK sale solo de la configuración del control (`--apk`): nunca de la petición.
async fn adb_provisionar(State(app): Ctx, Path(serial): Path<String>) -> ApiResult<Value> {
    let g = app
        .adb
        .clone()
        .ok_or_else(|| ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "sin_adb", "ADB no está disponible"))?;
    if !g.listar().iter().any(|d| d.serial == serial && d.estado == "device") {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "sin_dispositivo", format!("'{serial}' no está listo en ADB")));
    }
    let apk = app.apk.clone();
    if let Some(a) = &apk {
        if !a.is_file() {
            return Err(ApiError::parametro(format!("no existe el APK {}", a.display())));
        }
    }
    if apk.is_none() {
        let instalada = g.adb().shell(&serial, &format!("pm path {}", app.provision.paquete)).await.is_ok_and(|s| s.contains("package:"));
        if !instalada {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "sin_apk",
                "el control no tiene APK configurado y la app no está instalada en el teléfono: reinícialo con --apk o QHANA_APK",
            ));
        }
    }
    let pasos = g.adb().provisionar(&serial, &app.provision, app.puerto, apk.as_deref()).await;
    let ok = pasos.iter().all(|p| p.ok);
    app.bus.emitir(ev::ADB, json!({ "accion": "provisionar", "serial": serial, "ok": ok, "pasos": pasos }));
    Ok(Json(json!({ "ok": ok, "pasos": pasos })))
}

async fn refrescar(State(app): Ctx, Path(id): Path<String>) -> ApiResult<Value> {
    let c = app.agentes.conexion(&id).ok_or_else(|| ApiError::sin_dispositivo(&id))?;
    c.refrescar().await?;
    Ok(Json(json!({ "ok": true })))
}

/// Botón "Reconectar" (p. ej. tras dormir el PC). Ver `docs/protocolo.md`.
async fn reconectar(State(app): Ctx) -> Json<Value> {
    let adb = match &app.adb {
        Some(g) => g.reconectar(&app.provision).await,
        None => vec![],
    };
    // A los conectados se les pide `refrescar`; los ocupados (corriendo una receta) no se tocan.
    let mut ocupados = vec![];
    let mut tareas = vec![];
    for id in app.agentes.ids() {
        if app.registro.obtener(&id).is_some_and(|d| d.ocupado) {
            ocupados.push(id);
        } else if let Some(c) = app.agentes.conexion(&id) {
            tareas.push(async move { (id, c.refrescar().await, c) });
        }
    }
    let mut refrescados = vec![];
    let mut sin_respuesta = vec![];
    for (id, r, c) in futures_util::future::join_all(tareas).await {
        match r {
            Ok(()) => refrescados.push(id),
            Err(e) => {
                // Sin respuesta = conexión muerta que aún no se detectó: se cierra para que el agente reconecte.
                if e.codigo == protocolo::codigos::TIMEOUT {
                    tracing::warn!(%id, "no respondió a refrescar: cierro su conexión");
                    c.cortar();
                }
                sin_respuesta.push(id);
            }
        }
    }
    Json(json!({ "adb": adb, "refrescados": refrescados, "sin_respuesta": sin_respuesta, "ocupados": ocupados }))
}

#[derive(Deserialize)]
struct ConsultaSms {
    limite: Option<u32>,
    slot: Option<u32>,
    desde_ms: Option<i64>,
}

async fn sms(State(app): Ctx, Path(id): Path<String>, Query(q): Query<ConsultaSms>) -> ApiResult<Vec<Sms>> {
    let c = app.agentes.conexion(&id).ok_or_else(|| ApiError::sin_dispositivo(&id))?;
    Ok(Json(c.listar_sms(q.slot, q.desde_ms, Some(q.limite.unwrap_or(50))).await?))
}

// ───────────────────────────── USSD manual ─────────────────────────────

#[derive(Deserialize)]
struct CuerpoIniciar {
    dispositivo: String,
    #[serde(default)]
    slot: u32,
    codigo: String,
    #[serde(default)]
    modo: Option<ModoUssd>,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

fn validar_codigo(c: &str) -> Result<(), ApiError> {
    let ok = !c.is_empty() && c.len() <= 64 && c.chars().all(|ch| ch.is_ascii_digit() || "*#+".contains(ch));
    if ok {
        Ok(())
    } else {
        Err(ApiError::parametro(format!("código USSD inválido: '{c}' (solo dígitos, *, # y +)")))
    }
}

fn espera(ms: Option<u64>) -> Duration {
    Duration::from_millis(ms.unwrap_or(motor::TIMEOUT_POR_DEFECTO_MS).clamp(1_000, 120_000))
}

fn ocupado() -> ApiError {
    ApiError::new(StatusCode::CONFLICT, protocolo::codigos::OCUPADO, "el teléfono está ocupado con otra sesión")
}

fn sin_sesion(sesion: &str) -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        protocolo::codigos::SESION_INEXISTENTE,
        format!("la sesión '{sesion}' no está abierta (se cierra sola a los 60 s sin actividad)"),
    )
}

fn sesiones(app: &App) -> std::sync::MutexGuard<'_, std::collections::HashMap<String, SesionManual>> {
    app.sesiones.lock().unwrap_or_else(|e| e.into_inner())
}

/// Abre una sesión USSD a mano. Reserva el teléfono (si un lote o una receta lo tiene, `409 ocupado`); si la
/// sesión queda abierta, sigue reservado hasta `cancelar`, una respuesta que la cierra o 60 s sin actividad.
async fn ussd_iniciar(State(app): Ctx, Cuerpo(c): Cuerpo<CuerpoIniciar>) -> ApiResult<PasoUssd> {
    let codigo = c.codigo.trim().to_string();
    validar_codigo(&codigo)?;
    let canal = app.registro.canal(&c.dispositivo).ok_or_else(|| ApiError::sin_dispositivo(&c.dispositivo))?;
    let reserva = app.lotes.ctx.reservar(&c.dispositivo).ok_or_else(ocupado)?;
    let r = canal.iniciar(c.slot, &codigo, c.modo.unwrap_or_default(), espera(c.timeout_ms)).await?;
    if r.abierta {
        let s = SesionManual { sesion: r.sesion.clone(), ultima: Instant::now(), en_uso: false, reserva };
        sesiones(&app).insert(c.dispositivo.clone(), s);
    }
    Ok(Json(r))
}

#[derive(Deserialize)]
struct CuerpoResponder {
    dispositivo: String,
    sesion: String,
    texto: String,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

/// Solo contesta la sesión manual abierta de ese teléfono: nunca la de una receta o un lote en curso.
async fn ussd_responder(State(app): Ctx, Cuerpo(c): Cuerpo<CuerpoResponder>) -> ApiResult<PasoUssd> {
    if c.texto.is_empty() || c.texto.len() > 182 {
        return Err(ApiError::parametro("la respuesta debe tener entre 1 y 182 caracteres"));
    }
    let canal = app.registro.canal(&c.dispositivo).ok_or_else(|| ApiError::sin_dispositivo(&c.dispositivo))?;
    {
        let mut m = sesiones(&app);
        match m.get_mut(&c.dispositivo) {
            Some(s) if s.sesion == c.sesion => {
                s.ultima = Instant::now();
                s.en_uso = true;
            }
            Some(_) => return Err(sin_sesion(&c.sesion)),
            None if app.lotes.ctx.ocupado(&c.dispositivo) => return Err(ocupado()),
            None => return Err(sin_sesion(&c.sesion)),
        }
    }
    let r = canal.responder(&c.sesion, &c.texto, espera(c.timeout_ms)).await;
    let mut m = sesiones(&app);
    let sigue = matches!(&r, Ok(p) if p.abierta);
    if sigue {
        if let Some(s) = m.get_mut(&c.dispositivo).filter(|s| s.sesion == c.sesion) {
            s.ultima = Instant::now();
            s.en_uso = false;
        }
    } else if m.get(&c.dispositivo).is_some_and(|s| s.sesion == c.sesion) {
        // Cerrada (o falló): se libera el teléfono.
        m.remove(&c.dispositivo);
    }
    Ok(Json(r?))
}

#[derive(Deserialize)]
struct CuerpoCancelar {
    dispositivo: String,
    sesion: String,
}

async fn ussd_cancelar(State(app): Ctx, Cuerpo(c): Cuerpo<CuerpoCancelar>) -> ApiResult<Value> {
    let canal = app.registro.canal(&c.dispositivo).ok_or_else(|| ApiError::sin_dispositivo(&c.dispositivo))?;
    let manual = sesiones(&app).get(&c.dispositivo).is_some_and(|s| s.sesion == c.sesion);
    if !manual && app.lotes.ctx.ocupado(&c.dispositivo) {
        // Es la sesión de una receta o un lote: la cierra quien la abrió.
        return Err(ocupado());
    }
    let r = canal.cancelar(&c.sesion).await;
    let mut m = sesiones(&app);
    if m.get(&c.dispositivo).is_some_and(|s| s.sesion == c.sesion) {
        m.remove(&c.dispositivo);
    }
    drop(m);
    r?;
    Ok(Json(json!({ "ok": true })))
}

/// Cierra las sesiones manuales sin actividad por 60 s (el agente también las cierra) y libera el teléfono.
pub async fn vencer_sesiones(app: &App) {
    let vencidas: Vec<(String, SesionManual)> = {
        let mut m = sesiones(app);
        let ids: Vec<String> =
            m.iter().filter(|(_, s)| !s.en_uso && s.ultima.elapsed() > SESION_MANUAL_MAX).map(|(d, _)| d.clone()).collect();
        ids.into_iter().filter_map(|d| m.remove(&d).map(|s| (d, s))).collect()
    };
    for (d, s) in vencidas {
        tracing::info!(dispositivo = %d, "sesión USSD manual sin actividad por 60 s: la cierro");
        if let Some(canal) = app.registro.canal(&d) {
            let _ = tokio::time::timeout(Duration::from_secs(15), canal.cancelar(&s.sesion)).await;
        }
        drop(s);
    }
}

// ───────────────────────────── recetas ─────────────────────────────

/// Vuelve a leer la carpeta (p. ej. tras editar un archivo a mano). Emite `receta_cambiada` por lo que cambió.
async fn recetas_recargar(State(app): Ctx) -> Json<Value> {
    let (n, errores) = app.cargar_recetas(true);
    Json(json!({ "recetas": n, "errores": errores }))
}

#[derive(Deserialize)]
struct CuerpoEjecutar {
    dispositivo: String,
    #[serde(default)]
    slot: u32,
    #[serde(default)]
    parametros: BTreeMap<String, Value>,
    #[serde(default)]
    simular: bool,
}

async fn receta_ejecutar(State(app): Ctx, Path(nombre): Path<String>, Cuerpo(c): Cuerpo<CuerpoEjecutar>) -> ApiResult<Resultado> {
    // También por un nombre anterior de la receta (`nombres_anteriores`).
    let receta = nucleo::receta::buscar(&app.recetas.read().unwrap(), &nombre)
        .cloned()
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, protocolo::codigos::SIN_RECETA, format!("no existe la receta '{nombre}'")))?;
    if app.registro.canal(&c.dispositivo).is_none() {
        return Err(ApiError::sin_dispositivo(&c.dispositivo));
    }
    // Pista rápida; la reserva atómica la hace `ejecutar_receta` (si otro la ganó, responde `ocupado`).
    if app.lotes.ctx.ocupado(&c.dispositivo) {
        return Err(ocupado());
    }
    // Un chip dealer usa la variante dealer de la receta si existe; una receta dealer no corre en un chip normal.
    let tipo = app.lotes.ctx.tipo_chip(&c.dispositivo, c.slot);
    let receta = app
        .lotes
        .ctx
        .receta_efectiva(&receta, &tipo)
        .map_err(|e| ApiError::new(StatusCode::CONFLICT, protocolo::codigos::TIPO_CHIP, e))?;
    // Acepta números o textos en los parámetros: {"monto": 10} o {"monto": "10"}.
    let parametros = api_recetas::parametros_texto(c.parametros);

    // La receta corre en su propia tarea: si el cliente HTTP se corta, la ejecución termina igual
    // (nunca dejar una transferencia a medias sin registrar). Mismo camino que los lotes: guarda la
    // ejecución y actualiza el saldo conocido del chip.
    let ctx = app.lotes.ctx.clone();
    let id = c.dispositivo.clone();
    let slot = c.slot;
    let simular = c.simular;
    let tarea = tokio::spawn(async move { ctx.ejecutar_receta(&receta, &id, slot, &parametros, simular).await });
    let (r, _) = tarea
        .await
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, protocolo::codigos::DESCONOCIDO, e.to_string()))??;
    Ok(Json(r))
}

// ───────────────────────────── lotes y chips ─────────────────────────────

fn error_lote(e: lotes::ErrorLote) -> ApiError {
    let status = StatusCode::from_u16(e.http).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    ApiError::new(status, e.codigo, e.detalle)
}

#[derive(Deserialize)]
struct CuerpoValidar {
    csv: String,
    #[serde(default)]
    consultar_saldos: bool,
    /// Chips elegidos: `{chips: [{dispositivo, slot}], accion}` o la forma vieja `{dispositivo, slot, receta}`.
    #[serde(default)]
    origen: Option<lotes::OrigenLote>,
}

async fn lotes_validar(State(app): Ctx, Cuerpo(c): Cuerpo<CuerpoValidar>) -> ApiResult<lotes::Validacion> {
    app.lotes.validar(&c.csv, c.consultar_saldos, c.origen.as_ref()).await.map(Json).map_err(error_lote)
}

#[derive(Deserialize)]
struct CuerpoCrearLote {
    #[serde(default)]
    nombre: String,
    csv: String,
    #[serde(default)]
    simular: bool,
    #[serde(default)]
    origen: Option<lotes::OrigenLote>,
    /// Crear aunque sea igual a otro lote real reciente (ver "Duplicados").
    #[serde(default)]
    forzar: bool,
}

async fn lotes_crear(State(app): Ctx, Cuerpo(c): Cuerpo<CuerpoCrearLote>) -> ApiResult<lotes::Lote> {
    app.lotes.crear(&c.nombre, &c.csv, c.simular, c.origen.as_ref(), c.forzar).map(Json).map_err(error_lote)
}

#[derive(Deserialize)]
struct ConsultaLotes {
    limite: Option<u32>,
    antes: Option<i64>,
    estado: Option<String>,
    problemas: Option<String>,
    simular: Option<String>,
    q: Option<String>,
    archivados: Option<String>,
}

fn booleano(nombre: &str, v: Option<&str>) -> Result<Option<bool>, ApiError> {
    match v.map(str::trim) {
        None | Some("") => Ok(None),
        Some("true") => Ok(Some(true)),
        Some("false") => Ok(Some(false)),
        Some(otro) => Err(ApiError::parametro(format!("{nombre}='{otro}': usa true o false"))),
    }
}

/// "a,b" → estados; uno desconocido es `400 parametro`.
fn lista<T>(v: Option<&str>, validos: &[&str], de: impl Fn(&str) -> T) -> Result<Vec<T>, ApiError> {
    let mut out = vec![];
    for x in v.unwrap_or("").split(',').map(str::trim).filter(|x| !x.is_empty()) {
        if !validos.contains(&x) {
            return Err(ApiError::parametro(format!("estado '{x}' no válido ({})", validos.join(", "))));
        }
        out.push(de(x));
    }
    Ok(out)
}

const ESTADOS_LOTE: [&str; 5] = ["listo", "en_curso", "pausado", "terminado", "cancelado"];
const ESTADOS_FILA: [&str; 9] =
    ["omitida", "pendiente", "en_curso", "ok", "simulado", "parcial", "fallida", "incierta", "sin_capacidad"];

async fn lotes_listar(State(app): Ctx, Query(q): Query<ConsultaLotes>) -> ApiResult<lotes::ListaLotes> {
    let archivados = match q.archivados.as_deref().map(str::trim) {
        None | Some("") | Some("false") => Some(false),
        Some("true") => Some(true),
        Some("todos") => None,
        Some(otro) => return Err(ApiError::parametro(format!("archivados='{otro}': usa false, true o todos"))),
    };
    let filtro = lotes::FiltroLotes {
        limite: q.limite.unwrap_or(50).clamp(1, 200),
        antes: q.antes,
        estados: lista(q.estado.as_deref(), &ESTADOS_LOTE, lotes::EstadoLote::desde)?,
        problemas: booleano("problemas", q.problemas.as_deref())?,
        simular: booleano("simular", q.simular.as_deref())?,
        q: q.q,
        archivados,
    };
    app.lotes.listar(&filtro).map(Json).map_err(error_lote)
}

#[derive(Deserialize)]
struct ConsultaLote {
    filas: Option<String>,
}

async fn lotes_obtener(State(app): Ctx, Path(id): Path<i64>, Query(q): Query<ConsultaLote>) -> ApiResult<lotes::Lote> {
    if booleano("filas", q.filas.as_deref())? == Some(false) {
        return app.lotes.obtener_sin_filas(id).map(Json).map_err(error_lote);
    }
    app.lotes.obtener(id).map(Json).map_err(error_lote)
}

#[derive(Deserialize)]
struct ConsultaFilas {
    estado: Option<String>,
    q: Option<String>,
    despues: Option<u32>,
    limite: Option<u32>,
}

async fn lotes_filas(State(app): Ctx, Path(id): Path<i64>, Query(q): Query<ConsultaFilas>) -> ApiResult<lotes::PaginaFilas> {
    let filtro = lotes::FiltroFilas {
        limite: q.limite.unwrap_or(200).clamp(1, 1000),
        despues: q.despues,
        estados: lista(q.estado.as_deref(), &ESTADOS_FILA, lotes::EstadoFila::desde)?,
        q: q.q,
    };
    app.lotes.filas(id, &filtro).map(Json).map_err(error_lote)
}

#[derive(Deserialize)]
struct CuerpoArchivar {
    #[serde(default = "si")]
    archivado: bool,
}

fn si() -> bool {
    true
}

async fn lotes_archivar(State(app): Ctx, Path(id): Path<i64>, cuerpo: Option<Json<CuerpoArchivar>>) -> ApiResult<lotes::Lote> {
    let archivado = cuerpo.map(|Json(c)| c.archivado).unwrap_or(true);
    app.lotes.archivar(id, archivado).map(Json).map_err(error_lote)
}

async fn lotes_iniciar(State(app): Ctx, Path(id): Path<i64>) -> ApiResult<lotes::Lote> {
    app.lotes.iniciar(id).map(Json).map_err(error_lote)
}

async fn lotes_pausar(State(app): Ctx, Path(id): Path<i64>) -> ApiResult<lotes::Lote> {
    app.lotes.pausar(id).map(Json).map_err(error_lote)
}

#[derive(Deserialize, Default)]
struct CuerpoReintentar {
    /// También re-encolar los envíos `incierto` (el operador revisó el saldo).
    #[serde(default)]
    inciertas: bool,
}

async fn lotes_reintentar_fallidas(
    State(app): Ctx,
    Path(id): Path<i64>,
    cuerpo: Option<Json<CuerpoReintentar>>,
) -> ApiResult<lotes::Lote> {
    let inciertas = cuerpo.map(|Json(c)| c.inciertas).unwrap_or(false);
    app.lotes.reintentar_fallidas(id, inciertas).map(Json).map_err(error_lote)
}

async fn lotes_verificar(State(app): Ctx, Path(id): Path<i64>) -> ApiResult<lotes::Verificacion> {
    app.lotes.verificar(id).await.map(Json).map_err(error_lote)
}

async fn lotes_carriles(State(app): Ctx, Path(id): Path<i64>) -> ApiResult<lotes::Carriles> {
    app.lotes.carriles(id).map(Json).map_err(error_lote)
}

async fn lotes_cancelar(State(app): Ctx, Path(id): Path<i64>) -> ApiResult<lotes::Lote> {
    app.lotes.cancelar(id).map(Json).map_err(error_lote)
}

#[derive(Deserialize)]
struct ConsultaCsv {
    /// `;` para Excel en español. Por defecto `,` (contrato).
    separador: Option<String>,
    /// Solo la plantilla: `transferencias` (por defecto), `recargas` o `chips`.
    tipo: Option<String>,
}

fn separador(q: &ConsultaCsv) -> char {
    match q.separador.as_deref() {
        Some(";") => ';',
        Some("\t") | Some("tab") => '\t',
        _ => ',',
    }
}

fn respuesta_csv(nombre: &str, cuerpo: String) -> Response {
    (
        [
            (axum::http::header::CONTENT_TYPE, "text/csv; charset=utf-8".to_string()),
            (axum::http::header::CONTENT_DISPOSITION, format!("attachment; filename=\"{nombre}\"")),
        ],
        cuerpo,
    )
        .into_response()
}

async fn lotes_resultado(State(app): Ctx, Path(id): Path<i64>, Query(q): Query<ConsultaCsv>) -> Result<Response, ApiError> {
    let (nombre, cuerpo) = app.lotes.resultado_csv(id, separador(&q)).map_err(error_lote)?;
    Ok(respuesta_csv(&nombre, cuerpo))
}

async fn lotes_plantilla(Query(q): Query<ConsultaCsv>) -> Result<Response, ApiError> {
    let tipo = q.tipo.as_deref().map(str::trim).filter(|t| !t.is_empty()).unwrap_or("transferencias");
    let cuerpo = lotes::plantilla(Some(tipo), separador(&q))
        .ok_or_else(|| ApiError::parametro(format!("tipo de plantilla '{tipo}' no existe (transferencias, recargas o chips)")))?;
    let nombre = if tipo == "transferencias" { "plantilla-lote.csv".to_string() } else { format!("plantilla-lote-{tipo}.csv") };
    Ok(respuesta_csv(&nombre, cuerpo))
}

async fn chips_listar(State(app): Ctx) -> Json<Vec<lotes::ChipInfo>> {
    Json(app.lotes.ctx.chips().into_iter().map(lotes::ChipInfo::para_api).collect())
}

#[derive(Deserialize, Default)]
struct CuerpoSaldos {
    #[serde(default)]
    operadores: Option<Vec<String>>,
}

async fn chips_saldos(State(app): Ctx, cuerpo: Option<Json<CuerpoSaldos>>) -> Json<Vec<lotes::ChipInfo>> {
    let c = cuerpo.map(|Json(c)| c).unwrap_or_default();
    Json(app.lotes.ctx.consultar_saldos(c.operadores.as_deref(), None, true).await.into_iter().map(lotes::ChipInfo::para_api).collect())
}

#[derive(Deserialize)]
struct CuerpoTipoChip {
    tipo: String,
}

/// Marca el chip que ocupa la ranura como `normal` o `dealer`.
async fn chip_tipo(
    State(app): Ctx,
    Path((dispositivo, slot)): Path<(String, u32)>,
    Cuerpo(c): Cuerpo<CuerpoTipoChip>,
) -> ApiResult<lotes::ChipInfo> {
    let ctx = &app.lotes.ctx;
    if app.registro.obtener(&dispositivo).is_none() {
        return Err(ApiError::sin_dispositivo(&dispositivo));
    }
    ctx.fijar_tipo(&dispositivo, slot, &c.tipo)
        .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, protocolo::codigos::PARAMETRO, e))?;
    let chip = ctx
        .chips()
        .into_iter()
        .find(|x| x.dispositivo == dispositivo && x.slot == slot)
        .ok_or_else(|| ApiError::sin_dispositivo(&dispositivo))?;
    Ok(Json(chip.para_api()))
}

#[derive(Deserialize)]
struct CuerpoPin {
    #[serde(default)]
    pin: Option<String>,
}

/// Guarda el PIN del chip que ocupa la ranura (`null` o vacío lo borra). El PIN nunca se devuelve.
async fn chip_pin(
    State(app): Ctx,
    Path((dispositivo, slot)): Path<(String, u32)>,
    Cuerpo(c): Cuerpo<CuerpoPin>,
) -> ApiResult<lotes::ChipInfo> {
    let ctx = &app.lotes.ctx;
    if app.registro.obtener(&dispositivo).is_none() {
        return Err(ApiError::sin_dispositivo(&dispositivo));
    }
    ctx.fijar_pin(&dispositivo, slot, c.pin.as_deref())
        .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, protocolo::codigos::PARAMETRO, e))?;
    let chip = ctx
        .chips()
        .into_iter()
        .find(|x| x.dispositivo == dispositivo && x.slot == slot)
        .ok_or_else(|| ApiError::sin_dispositivo(&dispositivo))?;
    Ok(Json(chip.para_api()))
}

// ───────────────────────────── historial y eventos ─────────────────────────────

#[derive(Deserialize)]
struct ConsultaHistorial {
    limite: Option<u32>,
    tipo: Option<String>,
}

async fn historial(State(app): Ctx, Query(q): Query<ConsultaHistorial>) -> ApiResult<Vec<protocolo::Evento>> {
    let limite = q.limite.unwrap_or(100).min(5_000);
    app.almacen
        .eventos(limite, q.tipo.as_deref())
        .map(Json)
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "almacen", e.to_string()))
}

async fn ejecuciones(State(app): Ctx, Query(q): Query<ConsultaHistorial>) -> ApiResult<Vec<Value>> {
    let limite = q.limite.unwrap_or(50).min(1_000);
    app.almacen
        .ejecuciones(limite)
        .map(Json)
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "almacen", e.to_string()))
}

/// ¿El pedido viene de esta misma máquina?
fn es_local(addr: &SocketAddr) -> bool {
    addr.ip().to_canonical().is_loopback()
}

/// Token de la API (ver "Autenticación de la API"): si el control tiene token, todo `/api/*` que no venga de
/// esta máquina debe traer `Authorization: Bearer <token>`. Solo el WebSocket `/api/eventos` acepta además
/// `?token=` (el navegador no deja poner cabeceras ahí). `/api/salud` no lo pide. Nunca se anota la URL.
async fn autenticar(State(app): Ctx, req: Request<axum::body::Body>, siguiente: Next) -> Response {
    let ruta = req.uri().path();
    let Some(token) = app.api_token.as_deref() else { return siguiente.run(req).await };
    if !ruta.starts_with("/api/") || ruta == "/api/salud" {
        return siguiente.run(req).await;
    }
    let local = req.extensions().get::<ConnectInfo<SocketAddr>>().is_some_and(|ConnectInfo(a)| es_local(a));
    if local {
        return siguiente.run(req).await;
    }
    let iguales = |t: &str| protocolo::iguales_tiempo_constante(t.as_bytes(), token.as_bytes());
    let por_cabecera = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .is_some_and(|t| iguales(t.trim()));
    let por_url = ruta == "/api/eventos" && token_en_url(req.uri().query()).is_some_and(|t| iguales(&t));
    if por_cabecera || por_url {
        return siguiente.run(req).await;
    }
    ApiError::new(StatusCode::UNAUTHORIZED, protocolo::codigos::NO_AUTORIZADO, "falta el token de la API o no es válido")
        .into_response()
}

/// `token` de la query, con `%XX` decodificado.
fn token_en_url(query: Option<&str>) -> Option<String> {
    let v = query?.split('&').find_map(|p| p.strip_prefix("token="))?;
    let b = v.as_bytes();
    let mut out = vec![];
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' => {
                let h = std::str::from_utf8(b.get(i + 1..i + 3)?).ok().and_then(|h| u8::from_str_radix(h, 16).ok())?;
                out.push(h);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

/// Defensa contra páginas web ajenas que llamen a la API desde el navegador del operador (CSRF, WebSocket desde otro sitio, DNS rebinding):
/// - `Host` debe ser `localhost` o una IP (un nombre de dominio es la firma de DNS rebinding);
/// - si hay `Origin` (los navegadores siempre lo mandan en POST y WebSocket), debe ser el propio servidor
///   (o la interfaz en desarrollo, en :5173). Los agentes y `curl` no mandan `Origin`.
async fn solo_mismo_origen(cabeceras: HeaderMap, req: Request<axum::body::Body>, siguiente: Next) -> Response {
    let prohibido = |motivo: &str| ApiError::new(StatusCode::FORBIDDEN, "origen_no_permitido", motivo.to_string()).into_response();
    let host = cabeceras.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("");
    if !host_permitido(host) {
        return prohibido("Host no permitido: usa localhost o la IP del PC");
    }
    if let Some(origen) = cabeceras.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        let propio = origen.strip_prefix("http://").is_some_and(|h| h.eq_ignore_ascii_case(host));
        let desarrollo = ["http://localhost:5173", "http://127.0.0.1:5173"].contains(&origen);
        if !propio && !desarrollo {
            return prohibido("Origin no permitido");
        }
    }
    siguiente.run(req).await
}

fn host_permitido(host: &str) -> bool {
    let nombre = match host.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or(""),
        None => host.rsplit_once(':').map_or(host, |(n, _)| n),
    };
    nombre.eq_ignore_ascii_case("localhost") || nombre.parse::<std::net::IpAddr>().is_ok()
}

async fn ws_eventos(ws: WebSocketUpgrade, State(app): Ctx) -> Response {
    ws.on_upgrade(move |socket| eventos(socket, app))
}

async fn eventos(socket: WebSocket, app: Arc<App>) {
    let (mut tx, mut rx_ws) = socket.split();
    let mut rx = app.bus.suscribir();
    loop {
        tokio::select! {
            m = rx_ws.next() => match m {
                None | Some(Err(_)) | Some(Ok(Message::Close(_))) => break,
                _ => {}
            },
            e = rx.recv() => match e {
                Ok(e) => {
                    let s = serde_json::to_string(&e).unwrap_or_default();
                    if tx.send(Message::Text(s)).await.is_err() { break; }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    let s = json!({ "tipo": "perdidos", "ts_ms": protocolo::ahora_ms(), "n": n }).to_string();
                    if tx.send(Message::Text(s)).await.is_err() { break; }
                }
                Err(_) => break,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_de_la_url() {
        assert_eq!(token_en_url(Some("token=abc%2Bdef&x=1")).as_deref(), Some("abc+def"));
        assert_eq!(token_en_url(Some("x=1&token=a+b")).as_deref(), Some("a b"));
        assert_eq!(token_en_url(Some("x=1")), None);
        assert_eq!(token_en_url(Some("token=%zz")), None);
        assert_eq!(token_en_url(None), None);
    }

    #[test]
    fn local_incluye_ipv6_y_mapeadas() {
        assert!(es_local(&"127.0.0.1:1".parse().unwrap()));
        assert!(es_local(&"[::1]:1".parse().unwrap()));
        assert!(es_local(&"[::ffff:127.0.0.1]:1".parse().unwrap()));
        assert!(!es_local(&"192.168.1.5:1".parse().unwrap()));
    }
}
