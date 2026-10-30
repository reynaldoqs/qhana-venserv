//! Rutas HTTP/WS del contrato (`docs/protocolo.md`, sección "API del control").

use crate::error::{ApiError, ApiResult, Cuerpo};
use crate::{App, VERSION};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::StatusCode;
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
use std::path::{Path as RutaFs, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

type Ctx = State<Arc<App>>;

pub fn rutas(app: Arc<App>, ui: &RutaFs) -> Router {
    let api = Router::new()
        .route("/agente", get(ws_agente))
        .route("/api/salud", get(salud))
        .route("/api/dispositivos", get(dispositivos))
        .route("/api/dispositivos/:id/sms", get(sms))
        .route("/api/dispositivos/:id/refrescar", post(refrescar))
        .route("/api/adb", get(adb_listar))
        .route("/api/adb/:serial/provisionar", post(adb_provisionar))
        .route("/api/ussd/iniciar", post(ussd_iniciar))
        .route("/api/ussd/responder", post(ussd_responder))
        .route("/api/ussd/cancelar", post(ussd_cancelar))
        .route("/api/recetas", get(recetas))
        .route("/api/recetas/recargar", post(recetas_recargar))
        .route("/api/recetas/:nombre/ejecutar", post(receta_ejecutar))
        .route("/api/lotes", get(lotes_listar).post(lotes_crear))
        .route("/api/lotes/validar", post(lotes_validar))
        .route("/api/lotes/plantilla.csv", get(lotes_plantilla))
        .route("/api/lotes/:id", get(lotes_obtener))
        .route("/api/lotes/:id/iniciar", post(lotes_iniciar))
        .route("/api/lotes/:id/pausar", post(lotes_pausar))
        .route("/api/lotes/:id/cancelar", post(lotes_cancelar))
        .route("/api/lotes/:id/reintentar-fallidas", post(lotes_reintentar_fallidas))
        .route("/api/lotes/:id/verificar", post(lotes_verificar))
        .route("/api/lotes/:id/resultado.csv", get(lotes_resultado))
        .route("/api/chips", get(chips_listar))
        .route("/api/chips/saldos", post(chips_saldos))
        .route("/api/chips/:dispositivo/:slot", axum::routing::patch(chip_tipo))
        .route("/api/historial", get(historial))
        .route("/api/ejecuciones", get(ejecuciones))
        .route("/api/eventos", get(ws_eventos))
        // Cualquier otra ruta /api/* es un error de la API (no la interfaz).
        .route("/api/*resto", axum::routing::any(api_inexistente))
        .with_state(app)
        // Los CSV de lotes pueden pesar ~1 MB.
        .layer(axum::extract::DefaultBodyLimit::max(4 * 1024 * 1024))
        .layer(CorsLayer::permissive());

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

async fn salud(State(app): Ctx) -> Json<Value> {
    Json(json!({
        "version": VERSION,
        "agentes_conectados": app.registro.conectados(),
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

#[derive(Deserialize, Default)]
struct CuerpoProvisionar {
    apk: Option<PathBuf>,
}

async fn adb_provisionar(
    State(app): Ctx,
    Path(serial): Path<String>,
    cuerpo: Option<Json<CuerpoProvisionar>>,
) -> ApiResult<Value> {
    let g = app
        .adb
        .clone()
        .ok_or_else(|| ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "sin_adb", "ADB no está disponible"))?;
    if !g.listar().iter().any(|d| d.serial == serial && d.estado == "device") {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "sin_dispositivo", format!("'{serial}' no está listo en ADB")));
    }
    let apk = cuerpo.and_then(|Json(c)| c.apk).or_else(|| app.apk.clone());
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

fn marcar_ocupado(app: &App, id: &str, ocupado: bool) {
    app.lotes.ctx.marcar_ocupado(id, ocupado);
}

async fn ussd_iniciar(State(app): Ctx, Cuerpo(c): Cuerpo<CuerpoIniciar>) -> ApiResult<PasoUssd> {
    let codigo = c.codigo.trim().to_string();
    validar_codigo(&codigo)?;
    let canal = app.registro.canal(&c.dispositivo).ok_or_else(|| ApiError::sin_dispositivo(&c.dispositivo))?;
    let r = canal.iniciar(c.slot, &codigo, c.modo.unwrap_or_default(), espera(c.timeout_ms)).await?;
    marcar_ocupado(&app, &c.dispositivo, r.abierta);
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

async fn ussd_responder(State(app): Ctx, Cuerpo(c): Cuerpo<CuerpoResponder>) -> ApiResult<PasoUssd> {
    if c.texto.is_empty() || c.texto.len() > 182 {
        return Err(ApiError::parametro("la respuesta debe tener entre 1 y 182 caracteres"));
    }
    let canal = app.registro.canal(&c.dispositivo).ok_or_else(|| ApiError::sin_dispositivo(&c.dispositivo))?;
    let r = canal.responder(&c.sesion, &c.texto, espera(c.timeout_ms)).await;
    match &r {
        Ok(p) => marcar_ocupado(&app, &c.dispositivo, p.abierta),
        Err(_) => marcar_ocupado(&app, &c.dispositivo, false),
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
    let r = canal.cancelar(&c.sesion).await;
    marcar_ocupado(&app, &c.dispositivo, false);
    r?;
    Ok(Json(json!({ "ok": true })))
}

// ───────────────────────────── recetas ─────────────────────────────

async fn recetas(State(app): Ctx) -> Json<Vec<nucleo::receta::ResumenReceta>> {
    Json(app.recetas.read().unwrap().values().map(|r| r.resumen()).collect())
}

async fn recetas_recargar(State(app): Ctx) -> Json<Value> {
    let (n, errores) = app.cargar_recetas();
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
    let receta = app
        .recetas
        .read()
        .unwrap()
        .get(&nombre)
        .cloned()
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, protocolo::codigos::SIN_RECETA, format!("no existe la receta '{nombre}'")))?;
    if app.registro.canal(&c.dispositivo).is_none() {
        return Err(ApiError::sin_dispositivo(&c.dispositivo));
    }
    if app.registro.obtener(&c.dispositivo).map(|d| d.ocupado).unwrap_or(false) {
        return Err(ApiError::new(StatusCode::CONFLICT, protocolo::codigos::OCUPADO, "el dispositivo está ocupado con otra sesión"));
    }
    // Un chip PDA usa la variante PDA de la receta si existe; una receta PDA no corre en un chip normal.
    let tipo = app.lotes.ctx.tipo_chip(&c.dispositivo, c.slot);
    let receta = app
        .lotes
        .ctx
        .receta_efectiva(&receta, &tipo)
        .map_err(|e| ApiError::new(StatusCode::CONFLICT, protocolo::codigos::TIPO_CHIP, e))?;
    // Acepta números o textos en los parámetros: {"monto": 10} o {"monto": "10"}.
    let parametros: BTreeMap<String, String> = c
        .parametros
        .into_iter()
        .map(|(k, v)| {
            let s = match v {
                Value::String(s) => s,
                Value::Null => String::new(),
                otro => otro.to_string(),
            };
            (k, s)
        })
        .collect();

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
}

async fn lotes_validar(State(app): Ctx, Cuerpo(c): Cuerpo<CuerpoValidar>) -> ApiResult<lotes::Validacion> {
    app.lotes.validar(&c.csv, c.consultar_saldos).await.map(Json).map_err(error_lote)
}

#[derive(Deserialize)]
struct CuerpoCrearLote {
    #[serde(default)]
    nombre: String,
    csv: String,
    #[serde(default)]
    simular: bool,
}

async fn lotes_crear(State(app): Ctx, Cuerpo(c): Cuerpo<CuerpoCrearLote>) -> ApiResult<lotes::Lote> {
    app.lotes.crear(&c.nombre, &c.csv, c.simular).map(Json).map_err(error_lote)
}

async fn lotes_listar(State(app): Ctx) -> ApiResult<Vec<lotes::Lote>> {
    app.lotes.listar().map(Json).map_err(error_lote)
}

async fn lotes_obtener(State(app): Ctx, Path(id): Path<i64>) -> ApiResult<lotes::Lote> {
    app.lotes.obtener(id).map(Json).map_err(error_lote)
}

async fn lotes_iniciar(State(app): Ctx, Path(id): Path<i64>) -> ApiResult<lotes::Lote> {
    app.lotes.iniciar(id).map(Json).map_err(error_lote)
}

async fn lotes_pausar(State(app): Ctx, Path(id): Path<i64>) -> ApiResult<lotes::Lote> {
    app.lotes.pausar(id).map(Json).map_err(error_lote)
}

async fn lotes_reintentar_fallidas(State(app): Ctx, Path(id): Path<i64>) -> ApiResult<lotes::Lote> {
    app.lotes.reintentar_fallidas(id).map(Json).map_err(error_lote)
}

async fn lotes_verificar(State(app): Ctx, Path(id): Path<i64>) -> ApiResult<lotes::Verificacion> {
    app.lotes.verificar(id).await.map(Json).map_err(error_lote)
}

async fn lotes_cancelar(State(app): Ctx, Path(id): Path<i64>) -> ApiResult<lotes::Lote> {
    app.lotes.cancelar(id).map(Json).map_err(error_lote)
}

#[derive(Deserialize)]
struct ConsultaCsv {
    /// `;` para Excel en español. Por defecto `,` (contrato).
    separador: Option<String>,
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
    let sep = match q.separador.as_deref() {
        Some(";") => ';',
        Some("\t") | Some("tab") => '\t',
        _ => ',',
    };
    let (nombre, cuerpo) = app.lotes.resultado_csv(id, sep).map_err(error_lote)?;
    Ok(respuesta_csv(&nombre, cuerpo))
}

async fn lotes_plantilla() -> Response {
    respuesta_csv("plantilla-lote.csv", lotes::PLANTILLA.to_string())
}

async fn chips_listar(State(app): Ctx) -> Json<Vec<lotes::ChipInfo>> {
    Json(app.lotes.ctx.chips())
}

#[derive(Deserialize, Default)]
struct CuerpoSaldos {
    #[serde(default)]
    operadores: Option<Vec<String>>,
}

async fn chips_saldos(State(app): Ctx, cuerpo: Option<Json<CuerpoSaldos>>) -> Json<Vec<lotes::ChipInfo>> {
    let c = cuerpo.map(|Json(c)| c).unwrap_or_default();
    Json(app.lotes.ctx.consultar_saldos(c.operadores.as_deref(), None).await)
}

#[derive(Deserialize)]
struct CuerpoTipoChip {
    tipo: String,
}

/// Marca el chip que ocupa la ranura como `normal` o `pda`.
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
    Ok(Json(chip))
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
                    let s = json!({ "tipo": "perdidos", "ts_ms": protocolo::ahora_ms(), "cantidad": n }).to_string();
                    if tx.send(Message::Text(s)).await.is_err() { break; }
                }
                Err(_) => break,
            }
        }
    }
}
