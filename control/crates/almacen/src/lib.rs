//! Historial persistente: eventos y ejecuciones de recetas con sus pasos.

use protocolo::{Evento, Resultado};
use rusqlite::{params, Connection};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;

pub struct Almacen {
    conn: Mutex<Connection>,
}

const VERSION_ESQUEMA: i64 = 1;

impl Almacen {
    pub fn abrir(ruta: &Path) -> rusqlite::Result<Self> {
        if let Some(dir) = ruta.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        Self::iniciar(Connection::open(ruta)?)
    }

    pub fn en_memoria() -> rusqlite::Result<Self> {
        Self::iniciar(Connection::open_in_memory()?)
    }

    fn iniciar(conn: Connection) -> rusqlite::Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
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
        conn.pragma_update(None, "user_version", VERSION_ESQUEMA)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    /// Acceso directo a la conexión para módulos con su propio esquema (p. ej. `lotes`).
    pub fn con<R>(&self, f: impl FnOnce(&mut Connection) -> rusqlite::Result<R>) -> rusqlite::Result<R> {
        let mut c = self.conn.lock().unwrap();
        f(&mut c)
    }

    pub fn guardar_evento(&self, e: &Evento) -> rusqlite::Result<()> {
        let datos = serde_json::to_string(&e.datos).unwrap_or_else(|_| "{}".into());
        self.conn.lock().unwrap().execute(
            "INSERT INTO eventos (ts_ms, tipo, datos) VALUES (?1, ?2, ?3)",
            params![e.ts_ms, e.tipo, datos],
        )?;
        Ok(())
    }

    /// Eventos más recientes primero.
    pub fn eventos(&self, limite: u32, tipo: Option<&str>) -> rusqlite::Result<Vec<Evento>> {
        let conn = self.conn.lock().unwrap();
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

    pub fn guardar_ejecucion(&self, r: &Resultado, parametros: &BTreeMap<String, String>) -> rusqlite::Result<i64> {
        let mut conn = self.conn.lock().unwrap();
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
        let conn = self.conn.lock().unwrap();
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
}
