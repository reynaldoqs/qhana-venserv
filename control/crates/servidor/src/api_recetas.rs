//! Rutas para leer, validar, probar y editar recetas (ver "Editar recetas desde la interfaz" en
//! `docs/protocolo.md`). La carpeta y sus reglas están en `nucleo::carpeta`; aquí solo la API.

use crate::error::{ApiError, ApiResult, Cuerpo};
use crate::App;
use almacen::{FilaEstadistica, UsoReceta};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use nucleo::borrador::{self, PasoSesion};
use nucleo::carpeta::{self, ErrorCarpeta, RecetaCargada};
use nucleo::receta::{campos_desconocidos, version_de, AvisoReceta, ErrorReceta, ResumenReceta};
use nucleo::{probar, Receta};
use protocolo::{ahora_ms, codigos};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::Arc;

type Ctx = State<Arc<App>>;

const DIA_MS: i64 = 24 * 60 * 60 * 1000;

/// `ResumenReceta` de `GET /api/recetas`: el de siempre más versión, archivo y uso.
#[derive(Debug, Clone, Serialize)]
pub struct ResumenCompleto {
    #[serde(flatten)]
    pub base: ResumenReceta,
    pub version: String,
    pub archivo: String,
    /// Fecha del archivo; `null` si el texto todavía no está guardado (en `validar`).
    pub modificada_ms: Option<i64>,
    /// La versión actual nunca terminó `ok` ni `simulado`. Solo avisa.
    pub sin_probar: bool,
    pub uso: Option<UsoReceta>,
}

fn sin_receta(nombre: &str) -> ApiError {
    ApiError::new(StatusCode::NOT_FOUND, codigos::SIN_RECETA, format!("no existe la receta '{nombre}'"))
}

fn error_carpeta(e: ErrorCarpeta) -> ApiError {
    match e {
        ErrorCarpeta::Invalida(errores) => ApiError::receta_invalida(&errores),
        ErrorCarpeta::Parametro(m) => ApiError::parametro(m),
        ErrorCarpeta::Existe(m) => ApiError::new(StatusCode::CONFLICT, codigos::EXISTE, m),
        ErrorCarpeta::Conflicto(m) => ApiError::new(StatusCode::CONFLICT, codigos::CONFLICTO, m),
        ErrorCarpeta::NoExiste(m) => ApiError::new(StatusCode::NOT_FOUND, codigos::SIN_RECETA, m),
        ErrorCarpeta::Io(m) => ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, codigos::DESCONOCIDO, m),
    }
}

/// El `nombre` de la ruta debe ser un nombre de archivo seguro (sin `..`, `/`…).
fn nombre_ruta(nombre: &str) -> Result<(), ApiError> {
    if carpeta::nombre_valido(nombre) {
        Ok(())
    } else {
        Err(ApiError::parametro(format!("nombre de receta no válido: '{nombre}' (solo a-z, 0-9 y -)")))
    }
}

fn cargada(app: &App, nombre: &str) -> Option<RecetaCargada> {
    app.cargadas.read().unwrap().get(nombre).cloned()
}

/// Como [`cargada`], pero también por un nombre anterior (`nombres_anteriores`) de una receta renombrada.
fn cargada_o_anterior(app: &App, nombre: &str) -> Option<RecetaCargada> {
    let cargadas = app.cargadas.read().unwrap();
    cargadas
        .get(nombre)
        .or_else(|| cargadas.values().find(|c| c.receta.nombres_anteriores.iter().any(|n| n == nombre)))
        .cloned()
}

/// Resumen con los datos del historial. `archivo`/`modificada_ms` salen del archivo cargado si es esa versión.
fn resumen_de(app: &App, r: &Receta, version: &str, archivo: &str, modificada_ms: Option<i64>) -> ResumenCompleto {
    // El historial guarda el nombre que tenía la receta al correr: cuentan también sus nombres anteriores.
    let nombres = r.todos_los_nombres();
    let probada = app.almacen.version_probada(&nombres, version).unwrap_or(true);
    ResumenCompleto {
        base: r.resumen(),
        version: version.to_string(),
        archivo: archivo.to_string(),
        modificada_ms,
        sin_probar: !probada,
        uso: app.almacen.uso_receta(&nombres, ahora_ms() - 30 * DIA_MS).ok().flatten(),
    }
}

fn resumen_cargada(app: &App, c: &RecetaCargada) -> ResumenCompleto {
    resumen_de(app, &c.receta, &c.version, &c.archivo, Some(c.modificada_ms))
}

/// Resumen de un texto que quizá no está guardado.
fn resumen_texto(app: &App, r: &Receta, yaml: &str) -> ResumenCompleto {
    let version = version_de(yaml);
    match cargada(app, &r.nombre) {
        Some(c) if c.version == version => resumen_cargada(app, &c),
        Some(c) => resumen_de(app, r, &version, &c.archivo, None),
        None => resumen_de(app, r, &version, &format!("{}.yaml", r.nombre), None),
    }
}

/// Avisos de la receta contra las demás cargadas, más los campos desconocidos del YAML.
fn avisos(app: &App, r: &Receta, yaml: &str) -> Vec<AvisoReceta> {
    let cargadas = app.cargadas.read().unwrap();
    let otras: Vec<&Receta> = cargadas.values().map(|c| &c.receta).collect();
    let mut v = r.avisos(&otras);
    v.extend(campos_desconocidos(yaml));
    v
}

// ───────────────────────────── lectura ─────────────────────────────

pub async fn listar(State(app): Ctx) -> Json<Vec<ResumenCompleto>> {
    let cargadas: Vec<RecetaCargada> = app.cargadas.read().unwrap().values().cloned().collect();
    Json(cargadas.iter().map(|c| resumen_cargada(&app, c)).collect())
}

pub async fn detalle(State(app): Ctx, Path(nombre): Path<String>) -> ApiResult<Value> {
    nombre_ruta(&nombre)?;
    let c = cargada_o_anterior(&app, &nombre).ok_or_else(|| sin_receta(&nombre))?;
    let avisos: Vec<String> = avisos(&app, &c.receta, &c.texto).iter().map(|a| a.to_string()).collect();
    Ok(Json(json!({
        "resumen": resumen_cargada(&app, &c),
        "yaml": c.texto,
        "receta": c.receta,
        "avisos": avisos,
    })))
}

#[derive(Deserialize)]
pub struct ConsultaEstadisticas {
    dias: Option<u32>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct PasoFallido {
    pub paso: usize,
    pub veces: u32,
    pub ultimo_detalle: Option<String>,
}

/// Cuenta por estado, duración (promedio y p90 de las que terminaron) y en qué paso fallaron. `filas`: la más
/// nueva primero.
pub fn estadisticas(filas: &[FilaEstadistica]) -> Value {
    let mut por_estado: BTreeMap<String, u32> =
        ["ok", "simulado", "abortado", "incierto", "error"].iter().map(|e| (e.to_string(), 0)).collect();
    for f in filas {
        *por_estado.entry(f.estado.clone()).or_default() += 1;
    }
    let mut ms: Vec<i64> = filas.iter().filter(|f| f.estado != almacen::EN_CURSO).map(|f| f.duracion_ms).collect();
    ms.sort_unstable();
    let promedio = (!ms.is_empty()).then(|| ms.iter().sum::<i64>() / ms.len() as i64);
    let p90 = (!ms.is_empty()).then(|| ms[(ms.len() * 9).div_ceil(10).saturating_sub(1)]);
    let re = regex_paso();
    let mut fallidos: Vec<PasoFallido> = vec![];
    for f in filas.iter().filter(|f| matches!(f.estado.as_str(), "abortado" | "incierto" | "error")) {
        let Some(paso) = f.detalle.as_deref().and_then(|d| re.captures(d)).and_then(|c| c[1].parse::<usize>().ok()) else {
            continue;
        };
        match fallidos.iter_mut().find(|x| x.paso == paso) {
            Some(x) => x.veces += 1,
            None => fallidos.push(PasoFallido { paso, veces: 1, ultimo_detalle: f.detalle.clone() }),
        }
    }
    fallidos.sort_by(|a, b| b.veces.cmp(&a.veces).then(a.paso.cmp(&b.paso)));
    json!({
        "ejecuciones": filas.len(),
        "por_estado": por_estado,
        "ms_promedio": promedio,
        "ms_p90": p90,
        "por_paso_fallido": fallidos,
    })
}

fn regex_paso() -> &'static regex::Regex {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"\bpaso (\d+)\b").unwrap())
}

pub async fn ver_estadisticas(
    State(app): Ctx,
    Path(nombre): Path<String>,
    Query(q): Query<ConsultaEstadisticas>,
) -> ApiResult<Value> {
    nombre_ruta(&nombre)?;
    let dias = q.dias.unwrap_or(30).clamp(1, 3650);
    // Con los nombres anteriores de la receta: el historial guarda el nombre que tenía al correr.
    let actual = cargada_o_anterior(&app, &nombre);
    let nombres: Vec<&str> = match &actual {
        Some(c) => c.receta.todos_los_nombres(),
        None => vec![nombre.as_str()],
    };
    let filas = app.almacen.filas_estadistica(&nombres, ahora_ms() - dias as i64 * DIA_MS).map_err(interno)?;
    let ultimas = app.almacen.ultimas_de_receta(&nombres, 20).map_err(interno)?;
    let ultima_ok_ms = app.almacen.uso_receta(&nombres, 0).map_err(interno)?.and_then(|u| u.ultima_ok_ms);
    let mut v = estadisticas(&filas);
    v["dias"] = json!(dias);
    v["ultima_ok_ms"] = json!(ultima_ok_ms);
    v["ultimas"] = json!(ultimas);
    Ok(Json(v))
}

fn interno(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "almacen", e.to_string())
}

// ───────────────────────────── validar, probar, borrador ─────────────────────────────

#[derive(Deserialize)]
pub struct CuerpoYaml {
    yaml: String,
}

#[derive(Serialize)]
struct Validacion {
    ok: bool,
    errores: Vec<ErrorReceta>,
    avisos: Vec<AvisoReceta>,
    receta: Option<Receta>,
    resumen: Option<ResumenCompleto>,
}

pub async fn validar(State(app): Ctx, Cuerpo(c): Cuerpo<CuerpoYaml>) -> Json<Value> {
    let v = match Receta::leer(&c.yaml) {
        Err(e) => Validacion { ok: false, errores: vec![e], avisos: vec![], receta: None, resumen: None },
        Ok(r) => {
            let mut errores = r.errores();
            if !r.nombre.trim().is_empty() && !carpeta::nombre_valido(&r.nombre) {
                errores.insert(
                    0,
                    ErrorReceta {
                        paso: None,
                        campo: Some("nombre".into()),
                        mensaje: format!("nombre '{}' no válido: solo minúsculas, dígitos y guiones", r.nombre),
                    },
                );
            }
            // Un nombre anterior no puede ser el de otra receta (buscar por él sería ambiguo).
            let choque = {
                let cargadas = app.cargadas.read().unwrap();
                carpeta::choque_de_nombres(&r, cargadas.values().map(|c| &c.receta))
            };
            if let Some(m) = choque {
                errores.push(ErrorReceta { paso: None, campo: Some("nombres_anteriores".into()), mensaje: m });
            }
            let avisos = avisos(&app, &r, &c.yaml);
            let resumen = Some(resumen_texto(&app, &r, &c.yaml));
            Validacion { ok: errores.is_empty(), errores, avisos, receta: Some(r), resumen }
        }
    };
    Json(serde_json::to_value(v).unwrap_or_default())
}

/// Lee y valida un YAML; si no sirve, `400 receta_invalida`.
fn receta_valida(yaml: &str) -> Result<Receta, ApiError> {
    carpeta::leer_valida(yaml).map_err(error_carpeta)
}

/// Acepta números o textos en los parámetros: {"monto": 10} o {"monto": "10"}.
pub fn parametros_texto(p: BTreeMap<String, Value>) -> BTreeMap<String, String> {
    p.into_iter()
        .map(|(k, v)| {
            let s = match v {
                Value::String(s) => s,
                Value::Null => String::new(),
                otro => otro.to_string(),
            };
            (k, s)
        })
        .collect()
}

#[derive(Deserialize)]
pub struct CuerpoProbar {
    yaml: String,
    paso: usize,
    #[serde(default)]
    texto: String,
    #[serde(default)]
    texto_antes: Option<String>,
    #[serde(default)]
    parametros: BTreeMap<String, Value>,
}

pub async fn probar_paso(Cuerpo(c): Cuerpo<CuerpoProbar>) -> ApiResult<probar::Prueba> {
    let r = receta_valida(&c.yaml)?;
    let params = parametros_texto(c.parametros);
    probar::probar_paso(&r, c.paso, &c.texto, c.texto_antes.as_deref(), &params).map(Json).map_err(ApiError::parametro)
}

#[derive(Deserialize)]
pub struct CuerpoSesion {
    #[serde(default)]
    operador: String,
    pasos: Vec<PasoSesion>,
}

pub async fn desde_sesion(Cuerpo(c): Cuerpo<CuerpoSesion>) -> ApiResult<Value> {
    if c.pasos.len() > 100 {
        return Err(ApiError::parametro("la sesión tiene demasiados pasos (máximo 100)"));
    }
    let yaml = borrador::desde_sesion(&c.operador, &c.pasos).map_err(ApiError::parametro)?;
    Ok(Json(json!({ "yaml": yaml })))
}

// ───────────────────────────── guardar, crear, retirar ─────────────────────────────

pub async fn crear(State(app): Ctx, Cuerpo(c): Cuerpo<CuerpoYaml>) -> ApiResult<ResumenCompleto> {
    let nombre = {
        let _g = app.candado_recetas();
        let existentes = app.cargadas.read().unwrap().clone();
        let r = carpeta::crear(&app.ruta_recetas, &c.yaml, &existentes).map_err(error_carpeta)?;
        app.recargar_con_candado(true);
        r.nombre
    };
    tracing::info!(%nombre, "receta creada desde la API");
    let c = cargada(&app, &nombre).ok_or_else(|| sin_receta(&nombre))?;
    Ok(Json(resumen_cargada(&app, &c)))
}

#[derive(Deserialize)]
pub struct CuerpoReemplazar {
    yaml: String,
    version_base: String,
}

pub async fn reemplazar(
    State(app): Ctx,
    Path(nombre): Path<String>,
    Cuerpo(c): Cuerpo<CuerpoReemplazar>,
) -> ApiResult<ResumenCompleto> {
    nombre_ruta(&nombre)?;
    {
        let _g = app.candado_recetas();
        let actual = cargada(&app, &nombre).ok_or_else(|| sin_receta(&nombre))?;
        carpeta::reemplazar(&app.ruta_recetas, &nombre, &c.yaml, &c.version_base, &actual, &app.cargadas.read().unwrap().clone()).map_err(error_carpeta)?;
        app.recargar_con_candado(true);
    }
    tracing::info!(%nombre, "receta guardada desde la API");
    let c = cargada(&app, &nombre).ok_or_else(|| sin_receta(&nombre))?;
    Ok(Json(resumen_cargada(&app, &c)))
}

pub async fn retirar(State(app): Ctx, Path(nombre): Path<String>) -> ApiResult<Value> {
    nombre_ruta(&nombre)?;
    let id = {
        let _g = app.candado_recetas();
        let actual = cargada(&app, &nombre).ok_or_else(|| sin_receta(&nombre))?;
        let id = carpeta::retirar(&app.ruta_recetas, &actual).map_err(error_carpeta)?;
        app.recargar_con_candado(true);
        id
    };
    tracing::info!(%nombre, %id, "receta retirada desde la API");
    Ok(Json(json!({ "ok": true })))
}

// ───────────────────────────── versiones ─────────────────────────────

pub async fn versiones(State(app): Ctx, Path(nombre): Path<String>) -> ApiResult<Vec<carpeta::Version>> {
    nombre_ruta(&nombre)?;
    // Por un nombre anterior también: da las versiones de la receta renombrada (las de antes y las de ahora).
    let actual = cargada_o_anterior(&app, &nombre);
    let v = carpeta::versiones(&app.ruta_recetas, &nombre, actual.as_ref());
    if v.is_empty() {
        return Err(sin_receta(&nombre));
    }
    Ok(Json(v))
}

pub async fn version(State(app): Ctx, Path((nombre, id)): Path<(String, String)>) -> ApiResult<Value> {
    nombre_ruta(&nombre)?;
    let actual = cargada_o_anterior(&app, &nombre);
    let (yaml, version) = carpeta::leer_version(&app.ruta_recetas, &nombre, &id, actual.as_ref()).ok_or_else(|| {
        ApiError::new(StatusCode::NOT_FOUND, codigos::SIN_RECETA, format!("la receta '{nombre}' no tiene la versión '{id}'"))
    })?;
    Ok(Json(json!({ "id": id, "yaml": yaml, "version": version })))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fila(estado: &str, ms: i64, detalle: Option<&str>) -> FilaEstadistica {
        FilaEstadistica { inicio_ms: 0, estado: estado.into(), duracion_ms: ms, detalle: detalle.map(str::to_string) }
    }

    #[test]
    fn estadisticas_por_estado_duracion_y_paso() {
        let filas = vec![
            fila("abortado", 1000, Some("paso 3: respuesta inesperada (esperaba /monto/)")),
            fila("ok", 3000, None),
            fila("en_curso", 0, None),
            fila("error", 2000, Some("paso 3: viejo")),
            fila("incierto", 4000, Some("se confirmó pero no apareció la respuesta")),
            fila("abortado", 5000, Some("paso 1 (marcar \"*1#\"): timeout")),
        ];
        let v = estadisticas(&filas);
        assert_eq!(v["ejecuciones"], 6);
        assert_eq!(v["por_estado"]["ok"], 1);
        assert_eq!(v["por_estado"]["abortado"], 2);
        assert_eq!(v["por_estado"]["simulado"], 0);
        assert_eq!(v["por_estado"]["en_curso"], 1);
        assert_eq!(v["ms_promedio"], 3000, "sin la en curso");
        assert_eq!(v["ms_p90"], 5000);
        assert_eq!(v["por_paso_fallido"][0], json!({ "paso": 3, "veces": 2, "ultimo_detalle": "paso 3: respuesta inesperada (esperaba /monto/)" }));
        assert_eq!(v["por_paso_fallido"][1]["paso"], 1);
        assert_eq!(v["por_paso_fallido"].as_array().unwrap().len(), 2);
        let vacio = estadisticas(&[]);
        assert_eq!((vacio["ms_promedio"].clone(), vacio["ejecuciones"].clone()), (Value::Null, json!(0)));
    }

    #[test]
    fn cambios_entre_cargas() {
        let d = std::env::temp_dir().join(format!("qhana-cambios-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let y = |n: &str, t: &str| format!("nombre: {n}\ntitulo: {t}\npasos:\n  - marcar: '*1#'\n");
        std::fs::write(d.join("a.yaml"), y("a", "A")).unwrap();
        std::fs::write(d.join("b.yaml"), y("b", "B")).unwrap();
        let (antes, _) = carpeta::cargar(&d);
        std::fs::write(d.join("a.yaml"), y("a", "A2")).unwrap();
        std::fs::remove_file(d.join("b.yaml")).unwrap();
        std::fs::write(d.join("c.yaml"), y("c", "C")).unwrap();
        let (despues, _) = carpeta::cargar(&d);
        let c: Vec<(String, &str)> = crate::cambios(&antes, &despues).into_iter().map(|(n, k, _)| (n, k)).collect();
        assert_eq!(c, vec![("a".into(), "editada"), ("c".into(), "creada"), ("b".into(), "retirada")]);
        assert!(crate::cambios(&despues, &despues).is_empty());
        let _ = std::fs::remove_dir_all(d);
    }
}
