//! Historial persistente: eventos y ejecuciones de recetas con sus pasos.

use protocolo::{Evento, Resultado};
use rusqlite::types::Value;
use rusqlite::{params, Connection};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

pub struct Almacen {
    conn: Mutex<Connection>,
    /// Archivo de la base (`None` en memoria). Lo usa el respaldo diario.
    ruta: Option<PathBuf>,
}

const VERSION_ESQUEMA: i64 = 3;

/// Estado de una ejecución guardada antes de marcar y que todavía no terminó.
pub const EN_CURSO: &str = "en_curso";

/// Respaldos diarios que se conservan.
pub const RESPALDOS: usize = 14;

impl Almacen {
    pub fn abrir(ruta: &Path) -> rusqlite::Result<Self> {
        if let Some(dir) = ruta.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        Self::iniciar(Connection::open(ruta)?, Some(ruta.to_path_buf()))
    }

    pub fn en_memoria() -> rusqlite::Result<Self> {
        Self::iniciar(Connection::open_in_memory()?, None)
    }

    fn iniciar(conn: Connection, ruta: Option<PathBuf>) -> rusqlite::Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        // FULL: un corte de luz no deshace un envío ya marcado `en_curso` (ver "Ejecución" en el protocolo).
        conn.pragma_update(None, "synchronous", "FULL")?;
        let v: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if v < 1 {
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS eventos (
                    id      INTEGER PRIMARY KEY,
                    ts_ms   INTEGER NOT NULL,
                    tipo    TEXT NOT NULL,
                    datos   TEXT NOT NULL
                 );
                 CREATE INDEX IF NOT EXISTS eventos_ts ON eventos(ts_ms);
                 CREATE TABLE IF NOT EXISTS ejecuciones (
                    id          INTEGER PRIMARY KEY,
                    inicio_ms   INTEGER NOT NULL,
                    receta      TEXT NOT NULL,
                    dispositivo TEXT,
                    slot        INTEGER NOT NULL,
                    simular     INTEGER NOT NULL,
                    estado      TEXT NOT NULL,
                    ok          INTEGER NOT NULL,
                    detalle     TEXT,
                    parametros  TEXT NOT NULL,
                    salida      TEXT NOT NULL,
                    duracion_ms INTEGER NOT NULL
                 );
                 CREATE TABLE IF NOT EXISTS pasos (
                    id           INTEGER PRIMARY KEY,
                    ejecucion_id INTEGER NOT NULL REFERENCES ejecuciones(id),
                    n            INTEGER NOT NULL,
                    accion       TEXT NOT NULL,
                    enviado      TEXT,
                    texto        TEXT NOT NULL,
                    abierta      INTEGER NOT NULL,
                    ms           INTEGER NOT NULL
                 );",
            )?;
        }
        if v < 2 {
            // Cupo del día de todos los chips en una sola consulta (por fecha).
            conn.execute_batch("CREATE INDEX IF NOT EXISTS ejecuciones_inicio ON ejecuciones(inicio_ms);")?;
        }
        if v < 3 {
            // Versión de la receta con que corrió cada ejecución ("Editar recetas desde la interfaz").
            let tiene: bool = conn
                .prepare("SELECT 1 FROM pragma_table_info('ejecuciones') WHERE name = 'receta_version'")?
                .exists([])?;
            if !tiene {
                conn.execute_batch("ALTER TABLE ejecuciones ADD COLUMN receta_version TEXT;")?;
            }
            conn.execute_batch(
                "CREATE INDEX IF NOT EXISTS ejecuciones_receta_inicio ON ejecuciones(receta, inicio_ms);
                 CREATE INDEX IF NOT EXISTS ejecuciones_receta_version ON ejecuciones(receta, receta_version, estado);",
            )?;
        }
        conn.pragma_update(None, "user_version", VERSION_ESQUEMA)?;
        // Ejecuciones que quedaron a medias: el control se cortó mientras corrían. Pudieron haberse hecho.
        let n = conn.execute(
            "UPDATE ejecuciones SET estado = 'incierto', ok = 0,
               detalle = 'el control se reinició durante esta ejecución: revisa el saldo'
             WHERE estado = ?1",
            [EN_CURSO],
        )?;
        if n > 0 {
            tracing::warn!("{n} ejecución(es) quedaron en curso al cortarse el control: pasan a incierto");
        }
        Ok(Self { conn: Mutex::new(conn), ruta })
    }

    /// El candado de la conexión. Si alguien entró en pánico con él tomado, la conexión sigue sirviendo
    /// (SQLite deshace la transacción a medias): no se tumba todo el control por eso.
    fn bloquear(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Acceso directo a la conexión para módulos con su propio esquema (p. ej. `lotes`).
    pub fn con<R>(&self, f: impl FnOnce(&mut Connection) -> rusqlite::Result<R>) -> rusqlite::Result<R> {
        let mut c = self.bloquear();
        f(&mut c)
    }

    pub fn guardar_evento(&self, e: &Evento) -> rusqlite::Result<()> {
        let datos = serde_json::to_string(&e.datos).unwrap_or_else(|_| "{}".into());
        self.bloquear().execute(
            "INSERT INTO eventos (ts_ms, tipo, datos) VALUES (?1, ?2, ?3)",
            params![e.ts_ms, e.tipo, datos],
        )?;
        Ok(())
    }

    /// Eventos más recientes primero.
    pub fn eventos(&self, limite: u32, tipo: Option<&str>) -> rusqlite::Result<Vec<Evento>> {
        let conn = self.bloquear();
        let mut st = conn.prepare(
            "SELECT ts_ms, tipo, datos FROM eventos WHERE (?1 IS NULL OR tipo = ?1) ORDER BY id DESC LIMIT ?2",
        )?;
        let filas = st.query_map(params![tipo, limite], |r| {
            let datos: String = r.get(2)?;
            Ok(Evento {
                ts_ms: r.get(0)?,
                tipo: r.get(1)?,
                datos: serde_json::from_str(&datos).unwrap_or_default(),
            })
        })?;
        filas.collect()
    }

    /// Anota una ejecución **antes** de marcar, en estado `en_curso`. Si el control se cae en medio, al
    /// arrancar queda `incierto`. Se completa con [`Almacen::terminar_ejecucion`].
    #[allow(clippy::too_many_arguments)]
    pub fn iniciar_ejecucion(
        &self,
        inicio_ms: i64,
        receta: &str,
        dispositivo: &str,
        slot: u32,
        simular: bool,
        parametros: &BTreeMap<String, String>,
        receta_version: Option<&str>,
    ) -> rusqlite::Result<i64> {
        let conn = self.bloquear();
        conn.execute(
            "INSERT INTO ejecuciones (inicio_ms, receta, dispositivo, slot, simular, estado, ok, detalle, parametros, salida, duracion_ms, receta_version)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, NULL, ?7, '{}', 0, ?8)",
            params![inicio_ms, receta, dispositivo, slot, simular, EN_CURSO, serde_json::to_string(parametros).unwrap_or_default(), receta_version],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Completa una ejecución anotada con [`Almacen::iniciar_ejecucion`]: resultado y pasos.
    pub fn terminar_ejecucion(&self, id: i64, r: &Resultado) -> rusqlite::Result<()> {
        let mut conn = self.bloquear();
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE ejecuciones SET receta = ?2, estado = ?3, ok = ?4, detalle = ?5, salida = ?6, duracion_ms = ?7 WHERE id = ?1",
            params![
                id,
                r.receta,
                r.estado.como_texto(),
                r.ok,
                r.detalle,
                serde_json::to_string(&r.salida).unwrap_or_default(),
                r.duracion_ms as i64,
            ],
        )?;
        tx.execute("DELETE FROM pasos WHERE ejecucion_id = ?1", [id])?;
        for (n, p) in r.pasos.iter().enumerate() {
            tx.execute(
                "INSERT INTO pasos (ejecucion_id, n, accion, enviado, texto, abierta, ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![id, n as i64 + 1, p.accion, p.enviado, p.texto, p.abierta, p.ms as i64],
            )?;
        }
        tx.commit()
    }

    /// Una ejecución anotada que no llegó a terminar normalmente (la tarea se cayó): queda `incierto`.
    pub fn marcar_incierta(&self, id: i64, detalle: &str) -> rusqlite::Result<()> {
        self.bloquear().execute(
            "UPDATE ejecuciones SET estado = 'incierto', ok = 0, detalle = ?2 WHERE id = ?1 AND estado = ?3",
            params![id, detalle, EN_CURSO],
        )?;
        Ok(())
    }

    pub fn guardar_ejecucion(&self, r: &Resultado, parametros: &BTreeMap<String, String>) -> rusqlite::Result<i64> {
        let mut conn = self.bloquear();
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO ejecuciones (inicio_ms, receta, dispositivo, slot, simular, estado, ok, detalle, parametros, salida, duracion_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                r.inicio_ms,
                r.receta,
                r.dispositivo,
                r.slot,
                r.simular,
                r.estado.como_texto(),
                r.ok,
                r.detalle,
                serde_json::to_string(parametros).unwrap_or_default(),
                serde_json::to_string(&r.salida).unwrap_or_default(),
                r.duracion_ms as i64,
            ],
        )?;
        let id = tx.last_insert_rowid();
        for (n, p) in r.pasos.iter().enumerate() {
            tx.execute(
                "INSERT INTO pasos (ejecucion_id, n, accion, enviado, texto, abierta, ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![id, n as i64 + 1, p.accion, p.enviado, p.texto, p.abierta, p.ms as i64],
            )?;
        }
        tx.commit()?;
        Ok(id)
    }

    /// Ejecuciones recientes (sin pasos) como JSON para la API.
    pub fn ejecuciones(&self, limite: u32) -> rusqlite::Result<Vec<serde_json::Value>> {
        let conn = self.bloquear();
        let mut st = conn.prepare(
            "SELECT id, inicio_ms, receta, dispositivo, slot, simular, estado, ok, detalle, parametros, salida, duracion_ms
             FROM ejecuciones ORDER BY id DESC LIMIT ?1",
        )?;
        let filas = st.query_map(params![limite], |r| {
            let parametros: String = r.get(9)?;
            let salida: String = r.get(10)?;
            Ok(serde_json::json!({
                "id": r.get::<_, i64>(0)?,
                "inicio_ms": r.get::<_, i64>(1)?,
                "receta": r.get::<_, String>(2)?,
                "dispositivo": r.get::<_, Option<String>>(3)?,
                "slot": r.get::<_, i64>(4)?,
                "simular": r.get::<_, bool>(5)?,
                "estado": r.get::<_, String>(6)?,
                "ok": r.get::<_, bool>(7)?,
                "detalle": r.get::<_, Option<String>>(8)?,
                "parametros": serde_json::from_str::<serde_json::Value>(&parametros).unwrap_or_default(),
                "salida": serde_json::from_str::<serde_json::Value>(&salida).unwrap_or_default(),
                "duracion_ms": r.get::<_, i64>(11)?,
            }))
        })?;
        filas.collect()
    }
}

// ───────────────────────────── uso de las recetas ─────────────────────────────

/// Estados que cuentan como "salió bien" para el uso de una receta.
const BIEN: &str = "('ok', 'simulado')";

/// Uso reciente de una receta (`uso` en `GET /api/recetas`).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct UsoReceta {
    pub ejecuciones_30d: u32,
    /// `ok` y `simulado`.
    pub ok_30d: u32,
    pub ultima_ms: i64,
    pub ultimo_estado: String,
    /// Última que terminó `ok` o `simulado`.
    pub ultima_ok_ms: Option<i64>,
}

/// Una ejecución con sus pasos (`ultimas` en las estadísticas de una receta).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct EjecucionReceta {
    pub id: i64,
    pub inicio_ms: i64,
    pub estado: String,
    pub simular: bool,
    pub dispositivo: Option<String>,
    pub slot: u32,
    pub duracion_ms: i64,
    pub detalle: Option<String>,
    pub version: Option<String>,
    pub pasos: Vec<protocolo::PasoResultado>,
}

/// Una fila liviana para calcular estadísticas.
#[derive(Debug, Clone, PartialEq)]
pub struct FilaEstadistica {
    pub inicio_ms: i64,
    pub estado: String,
    pub duracion_ms: i64,
    pub detalle: Option<String>,
}

/// `?,?,?` para una lista de `n` valores en un `IN (…)`.
fn marcas(n: usize) -> String {
    vec!["?"; n.max(1)].join(",")
}

/// Parámetros: los nombres de la receta y después los demás valores.
fn con_nombres(nombres: &[&str], resto: &[Value]) -> Vec<Value> {
    let mut v: Vec<Value> = nombres.iter().map(|n| Value::Text(n.to_string())).collect();
    if nombres.is_empty() {
        // `IN ()` no vale en SQLite: un nombre que ninguna receta tiene.
        v.push(Value::Text(String::new()));
    }
    v.extend_from_slice(resto);
    v
}

impl Almacen {
    /// ¿Esa versión de la receta terminó alguna vez `ok` o `simulado`? `nombres`: el de la receta y sus
    /// `nombres_anteriores` (el historial guarda el nombre que tenía al correr).
    pub fn version_probada(&self, nombres: &[&str], version: &str) -> rusqlite::Result<bool> {
        let ps = con_nombres(nombres, &[Value::Text(version.to_string())]);
        self.bloquear()
            .prepare_cached(&format!(
                "SELECT 1 FROM ejecuciones WHERE receta IN ({}) AND receta_version = ? AND estado IN {BIEN} LIMIT 1",
                marcas(nombres.len())
            ))?
            .exists(rusqlite::params_from_iter(ps.iter()))
    }

    /// Uso de la receta (con todos sus `nombres`) desde `desde_ms` (y la última ejecución de siempre). `None` si
    /// nunca corrió.
    pub fn uso_receta(&self, nombres: &[&str], desde_ms: i64) -> rusqlite::Result<Option<UsoReceta>> {
        use rusqlite::OptionalExtension;
        let conn = self.bloquear();
        let m = marcas(nombres.len());
        let solo = con_nombres(nombres, &[]);
        let ultima: Option<(i64, String)> = conn
            .prepare_cached(&format!(
                "SELECT inicio_ms, estado FROM ejecuciones WHERE receta IN ({m}) ORDER BY inicio_ms DESC, id DESC LIMIT 1"
            ))?
            .query_row(rusqlite::params_from_iter(solo.iter()), |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?;
        let Some((ultima_ms, ultimo_estado)) = ultima else { return Ok(None) };
        let ps = con_nombres(nombres, &[Value::Integer(desde_ms)]);
        let (n, ok): (u32, u32) = conn
            .prepare_cached(&format!(
                "SELECT COUNT(*), COALESCE(SUM(estado IN {BIEN}), 0) FROM ejecuciones WHERE receta IN ({m}) AND inicio_ms >= ?"
            ))?
            .query_row(rusqlite::params_from_iter(ps.iter()), |r| Ok((r.get(0)?, r.get(1)?)))?;
        let ultima_ok_ms: Option<i64> = conn
            .prepare_cached(&format!("SELECT MAX(inicio_ms) FROM ejecuciones WHERE receta IN ({m}) AND estado IN {BIEN}"))?
            .query_row(rusqlite::params_from_iter(solo.iter()), |r| r.get(0))?;
        Ok(Some(UsoReceta { ejecuciones_30d: n, ok_30d: ok, ultima_ms, ultimo_estado, ultima_ok_ms }))
    }

    /// Ejecuciones de la receta (con todos sus `nombres`) desde `desde_ms` (sin pasos), para las estadísticas.
    pub fn filas_estadistica(&self, nombres: &[&str], desde_ms: i64) -> rusqlite::Result<Vec<FilaEstadistica>> {
        let conn = self.bloquear();
        let mut st = conn.prepare_cached(&format!(
            "SELECT inicio_ms, estado, duracion_ms, detalle FROM ejecuciones WHERE receta IN ({}) AND inicio_ms >= ?
             ORDER BY inicio_ms DESC, id DESC",
            marcas(nombres.len())
        ))?;
        let ps = con_nombres(nombres, &[Value::Integer(desde_ms)]);
        let filas = st.query_map(rusqlite::params_from_iter(ps.iter()), |r| {
            Ok(FilaEstadistica { inicio_ms: r.get(0)?, estado: r.get(1)?, duracion_ms: r.get(2)?, detalle: r.get(3)? })
        })?;
        filas.collect()
    }

    /// Las `limite` ejecuciones más recientes de la receta (con todos sus `nombres`), con sus pasos (ya guardados
    /// con los secretos tapados).
    pub fn ultimas_de_receta(&self, nombres: &[&str], limite: u32) -> rusqlite::Result<Vec<EjecucionReceta>> {
        let conn = self.bloquear();
        let mut st = conn.prepare_cached(&format!(
            "SELECT id, inicio_ms, estado, simular, dispositivo, slot, duracion_ms, detalle, receta_version
             FROM ejecuciones WHERE receta IN ({}) ORDER BY inicio_ms DESC, id DESC LIMIT ?",
            marcas(nombres.len())
        ))?;
        let ps = con_nombres(nombres, &[Value::Integer(limite as i64)]);
        let mut v: Vec<EjecucionReceta> = st
            .query_map(rusqlite::params_from_iter(ps.iter()), |r| {
                Ok(EjecucionReceta {
                    id: r.get(0)?,
                    inicio_ms: r.get(1)?,
                    estado: r.get(2)?,
                    simular: r.get(3)?,
                    dispositivo: r.get(4)?,
                    slot: r.get(5)?,
                    duracion_ms: r.get(6)?,
                    detalle: r.get(7)?,
                    version: r.get(8)?,
                    pasos: vec![],
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        let mut sp = conn.prepare_cached(
            "SELECT accion, enviado, texto, abierta, ms FROM pasos WHERE ejecucion_id = ?1 ORDER BY n",
        )?;
        for e in &mut v {
            e.pasos = sp
                .query_map([e.id], |r| {
                    Ok(protocolo::PasoResultado {
                        accion: r.get(0)?,
                        enviado: r.get(1)?,
                        texto: r.get(2)?,
                        abierta: r.get(3)?,
                        ms: r.get::<_, i64>(4)? as u64,
                    })
                })?
                .collect::<rusqlite::Result<_>>()?;
        }
        Ok(v)
    }
}

// ───────────────────────────── mantenimiento ─────────────────────────────

impl Almacen {
    /// Copia la base a `<carpeta de la base>/backup/<fecha>_control.db` con `VACUUM INTO` (si ya existe la de
    /// hoy, no hace nada) y deja solo los últimos [`RESPALDOS`]. Usa una conexión propia de solo lectura: con
    /// WAL no frena a nadie que esté escribiendo. Devuelve la ruta del respaldo nuevo.
    pub fn respaldar(&self, fecha: &str) -> Result<Option<PathBuf>, String> {
        let Some(ruta) = &self.ruta else { return Ok(None) };
        let dir = ruta.parent().unwrap_or(Path::new(".")).join("backup");
        std::fs::create_dir_all(&dir).map_err(|e| format!("no pude crear {}: {e}", dir.display()))?;
        let destino = dir.join(format!("{fecha}_control.db"));
        let mut nuevo = None;
        if !destino.exists() {
            let c = Connection::open_with_flags(ruta, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|e| e.to_string())?;
            c.execute("VACUUM INTO ?1", [destino.to_string_lossy()]).map_err(|e| format!("VACUUM INTO: {e}"))?;
            nuevo = Some(destino);
        }
        // Rotación: los nombres `AAAA-MM-DD_control.db` se ordenan por fecha.
        let mut viejos: Vec<PathBuf> = std::fs::read_dir(&dir)
            .map_err(|e| e.to_string())?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(es_respaldo))
            .collect();
        viejos.sort();
        let sobran = viejos.len().saturating_sub(RESPALDOS);
        for p in &viejos[..sobran] {
            if let Err(e) = std::fs::remove_file(p) {
                tracing::warn!("no pude borrar el respaldo viejo {}: {e}", p.display());
            }
        }
        Ok(nuevo)
    }

    /// Borra los eventos anteriores a `antes_ms`, de a poco para no retener el candado (las ejecuciones y
    /// los lotes no se tocan). Devuelve cuántos borró.
    pub fn podar_eventos(&self, antes_ms: i64) -> rusqlite::Result<usize> {
        let mut total = 0;
        loop {
            let n = self.bloquear().execute(
                "DELETE FROM eventos WHERE id IN (SELECT id FROM eventos WHERE ts_ms < ?1 LIMIT 2000)",
                [antes_ms],
            )?;
            total += n;
            if n == 0 {
                return Ok(total);
            }
            std::thread::yield_now();
        }
    }
}

fn es_respaldo(n: &str) -> bool {
    let b = n.as_bytes();
    n.len() == "2026-01-01_control.db".len()
        && n.ends_with("_control.db")
        && b[..10].iter().enumerate().all(|(i, c)| if i == 4 || i == 7 { *c == b'-' } else { c.is_ascii_digit() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocolo::{EstadoResultado, PasoResultado};

    #[test]
    fn guarda_y_lee() {
        let a = Almacen::en_memoria().unwrap();
        let mut datos = serde_json::Map::new();
        datos.insert("x".into(), 1.into());
        a.guardar_evento(&Evento { tipo: "ussd".into(), ts_ms: 5, datos }).unwrap();
        a.guardar_evento(&Evento { tipo: "sms".into(), ts_ms: 6, datos: Default::default() }).unwrap();
        let e = a.eventos(10, None).unwrap();
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].tipo, "sms");
        assert_eq!(a.eventos(10, Some("ussd")).unwrap()[0].datos["x"], 1);

        let r = Resultado {
            ok: true,
            estado: EstadoResultado::Simulado,
            detalle: None,
            pasos: vec![PasoResultado { accion: "marcar".into(), enviado: Some("*1#".into()), texto: "hola".into(), abierta: true, ms: 3 }],
            salida: BTreeMap::new(),
            receta: "r".into(),
            dispositivo: Some("d".into()),
            slot: 0,
            simular: true,
            inicio_ms: 1,
            duracion_ms: 2,
        };
        a.guardar_ejecucion(&r, &BTreeMap::new()).unwrap();
        let ej = a.ejecuciones(5).unwrap();
        assert_eq!(ej[0]["estado"], "simulado");
    }

    fn resultado(receta: &str, estado: EstadoResultado) -> Resultado {
        Resultado {
            ok: matches!(estado, EstadoResultado::Ok | EstadoResultado::Simulado),
            estado,
            detalle: Some("paso 3: no coincide".into()),
            pasos: vec![PasoResultado { accion: "marcar".into(), enviado: Some("*105#".into()), texto: "Saldo".into(), abierta: false, ms: 3 }],
            salida: BTreeMap::new(),
            receta: receta.into(),
            dispositivo: Some("tel1".into()),
            slot: 0,
            simular: false,
            inicio_ms: 0,
            duracion_ms: 100,
        }
    }

    #[test]
    fn version_probada_y_uso() {
        let a = Almacen::en_memoria().unwrap();
        let correr = |inicio: i64, version: Option<&str>, estado: EstadoResultado| {
            let id = a.iniciar_ejecucion(inicio, "tigo-saldo", "tel1", 0, false, &BTreeMap::new(), version).unwrap();
            a.terminar_ejecucion(id, &resultado("tigo-saldo", estado)).unwrap();
        };
        assert_eq!(a.uso_receta(&["tigo-saldo"], 0).unwrap(), None, "nunca corrió");
        assert!(!a.version_probada(&["tigo-saldo"], "v1").unwrap());
        correr(10, Some("v1"), EstadoResultado::Abortado);
        assert!(!a.version_probada(&["tigo-saldo"], "v1").unwrap(), "abortado no cuenta");
        correr(20, Some("v1"), EstadoResultado::Simulado);
        assert!(a.version_probada(&["tigo-saldo"], "v1").unwrap());
        assert!(!a.version_probada(&["tigo-saldo"], "v2").unwrap(), "otra versión no está probada");
        assert!(!a.version_probada(&["tigo-transferir"], "v1").unwrap());
        correr(30, None, EstadoResultado::Ok);
        correr(40, Some("v2"), EstadoResultado::Error);
        let u = a.uso_receta(&["tigo-saldo"], 15).unwrap().unwrap();
        assert_eq!(
            u,
            UsoReceta { ejecuciones_30d: 3, ok_30d: 2, ultima_ms: 40, ultimo_estado: "error".into(), ultima_ok_ms: Some(30) }
        );
        assert_eq!(a.filas_estadistica(&["tigo-saldo"], 15).unwrap().len(), 3);
        let ult = a.ultimas_de_receta(&["tigo-saldo"], 2).unwrap();
        assert_eq!(ult.len(), 2);
        assert_eq!((ult[0].inicio_ms, ult[0].version.as_deref()), (40, Some("v2")));
        assert_eq!(ult[0].pasos[0].enviado.as_deref(), Some("*105#"));
    }

    #[test]
    fn uso_y_estadisticas_cuentan_los_nombres_anteriores() {
        let a = Almacen::en_memoria().unwrap();
        let correr = |inicio: i64, receta: &str, version: &str, estado: EstadoResultado| {
            let id = a.iniciar_ejecucion(inicio, receta, "tel1", 0, false, &BTreeMap::new(), Some(version)).unwrap();
            a.terminar_ejecucion(id, &resultado(receta, estado)).unwrap();
        };
        // Antes de renombrar la receta y después.
        correr(10, "tigo-pda-recargar", "v1", EstadoResultado::Ok);
        correr(20, "tigo-pda-recargar", "v1", EstadoResultado::Abortado);
        correr(30, "tigo-dealer-recargar", "v2", EstadoResultado::Simulado);
        let nombres = ["tigo-dealer-recargar", "tigo-pda-recargar"];
        let u = a.uso_receta(&nombres, 0).unwrap().unwrap();
        assert_eq!((u.ejecuciones_30d, u.ok_30d, u.ultima_ms), (3, 2, 30));
        assert_eq!(a.uso_receta(&nombres[..1], 0).unwrap().unwrap().ejecuciones_30d, 1, "solo con el nombre nuevo");
        assert_eq!(a.filas_estadistica(&nombres, 0).unwrap().len(), 3);
        assert_eq!(a.ultimas_de_receta(&nombres, 10).unwrap().len(), 3);
        assert!(a.version_probada(&nombres, "v1").unwrap(), "probada con el nombre anterior");
        assert!(!a.version_probada(&nombres[..1], "v1").unwrap());
        assert_eq!(a.uso_receta(&[], 0).unwrap(), None);
    }

    #[test]
    fn migra_una_base_sin_receta_version() {
        let d = temporal("migra");
        let ruta = d.join("control.db");
        {
            // Base de la versión 2 del esquema, con una ejecución.
            let c = Connection::open(&ruta).unwrap();
            c.execute_batch(
                "CREATE TABLE ejecuciones (id INTEGER PRIMARY KEY, inicio_ms INTEGER NOT NULL, receta TEXT NOT NULL,
                   dispositivo TEXT, slot INTEGER NOT NULL, simular INTEGER NOT NULL, estado TEXT NOT NULL, ok INTEGER NOT NULL,
                   detalle TEXT, parametros TEXT NOT NULL, salida TEXT NOT NULL, duracion_ms INTEGER NOT NULL);
                 CREATE TABLE eventos (id INTEGER PRIMARY KEY, ts_ms INTEGER NOT NULL, tipo TEXT NOT NULL, datos TEXT NOT NULL);
                 CREATE TABLE pasos (id INTEGER PRIMARY KEY, ejecucion_id INTEGER NOT NULL, n INTEGER NOT NULL, accion TEXT NOT NULL,
                   enviado TEXT, texto TEXT NOT NULL, abierta INTEGER NOT NULL, ms INTEGER NOT NULL);
                 INSERT INTO ejecuciones VALUES (1, 5, 'tigo-saldo', 'tel1', 0, 0, 'ok', 1, NULL, '{}', '{}', 9);
                 PRAGMA user_version = 2;",
            )
            .unwrap();
        }
        let a = Almacen::abrir(&ruta).unwrap();
        assert!(!a.version_probada(&["tigo-saldo"], "v1").unwrap(), "las viejas no tienen versión");
        assert_eq!(a.uso_receta(&["tigo-saldo"], 0).unwrap().unwrap().ok_30d, 1);
        drop(a);
        // Reabrir no vuelve a migrar.
        Almacen::abrir(&ruta).unwrap();
        let _ = std::fs::remove_dir_all(d);
    }

    fn temporal(nombre: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("qhana-almacen-{nombre}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn ejecucion_en_curso_queda_incierta_al_reabrir() {
        let d = temporal("en-curso");
        let ruta = d.join("control.db");
        {
            let a = Almacen::abrir(&ruta).unwrap();
            let id = a.iniciar_ejecucion(1, "tigo-transferir", "tel1", 0, false, &BTreeMap::new(), None).unwrap();
            assert_eq!(a.ejecuciones(1).unwrap()[0]["estado"], EN_CURSO);
            let terminada = a.iniciar_ejecucion(2, "tigo-saldo", "tel1", 0, false, &BTreeMap::new(), Some("v1")).unwrap();
            let r = Resultado {
                ok: true,
                estado: EstadoResultado::Ok,
                detalle: None,
                pasos: vec![PasoResultado { accion: "marcar".into(), enviado: Some("*105#".into()), texto: "Saldo".into(), abierta: false, ms: 3 }],
                salida: BTreeMap::new(),
                receta: "tigo-saldo".into(),
                dispositivo: Some("tel1".into()),
                slot: 0,
                simular: false,
                inicio_ms: 2,
                duracion_ms: 9,
            };
            a.terminar_ejecucion(terminada, &r).unwrap();
            assert!(id < terminada);
        }
        let a = Almacen::abrir(&ruta).unwrap();
        let ej = a.ejecuciones(5).unwrap();
        assert_eq!(ej[0]["estado"], "ok");
        assert_eq!(ej[0]["duracion_ms"], 9);
        assert_eq!(ej[1]["estado"], "incierto");
        assert!(ej[1]["detalle"].as_str().unwrap().contains("reinició"));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn respaldo_diario_rota_y_poda_eventos() {
        let d = temporal("respaldo");
        let a = Almacen::abrir(&d.join("control.db")).unwrap();
        a.guardar_evento(&Evento { tipo: "viejo".into(), ts_ms: 10, datos: Default::default() }).unwrap();
        a.guardar_evento(&Evento { tipo: "nuevo".into(), ts_ms: 1_000, datos: Default::default() }).unwrap();
        assert_eq!(a.podar_eventos(500).unwrap(), 1);
        assert_eq!(a.eventos(10, None).unwrap().len(), 1);
        for dia in 1..=16 {
            a.respaldar(&format!("2026-10-{dia:02}")).unwrap();
        }
        // Mismo día: no rehace.
        assert!(a.respaldar("2026-10-16").unwrap().is_none());
        let mut v: Vec<String> = std::fs::read_dir(d.join("backup")).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        v.sort();
        assert_eq!(v.len(), RESPALDOS);
        assert_eq!(v[0], "2026-10-03_control.db");
        // El respaldo es una base válida con los datos.
        let r = Almacen::abrir(&d.join("backup").join("2026-10-16_control.db")).unwrap();
        assert_eq!(r.eventos(10, None).unwrap()[0].tipo, "nuevo");
        assert!(es_respaldo("2026-10-16_control.db") && !es_respaldo("control.db") && !es_respaldo("2026-1x-16_control.db"));
        let _ = std::fs::remove_dir_all(d);
    }
}
