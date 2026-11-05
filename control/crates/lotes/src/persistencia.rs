//! Esquema SQLite de lotes y chips (vive en la misma base que el historial).

use crate::modelo::*;
use almacen::Almacen;
use protocolo::Sim;
use serde::Serialize;
use rusqlite::{params, OptionalExtension};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

type R<T> = rusqlite::Result<T>;

/// Fila nueva al crear un lote.
#[derive(Debug, Clone)]
pub struct NuevaFila {
    pub fila: u32,
    pub numero: Option<String>,
    pub monto: Option<f64>,
    pub operador: Option<String>,
    pub accion: String,
    pub referencia: Option<String>,
    pub receta: Option<String>,
    pub comision: f64,
    /// `omitida` (inválida) o `pendiente`.
    pub estado: EstadoFila,
    pub detalle: Option<String>,
    pub original: BTreeMap<String, String>,
    pub envios: Vec<f64>,
}

/// Saldo conocido de un chip.
#[derive(Debug, Clone, PartialEq)]
pub struct SaldoChip {
    pub dispositivo: String,
    pub slot: u32,
    pub numero: Option<String>,
    pub operador: Option<String>,
    pub saldo: f64,
    pub ts_ms: i64,
}

/// Saldo de un chip guardado al iniciar un lote, de un tipo (`principal` o `dealer`).
#[derive(Debug, Clone, PartialEq)]
pub struct SaldoInicial {
    pub dispositivo: String,
    pub slot: u32,
    pub tipo: String,
    pub saldo: f64,
    pub ts_ms: i64,
}

/// Lo que identifica a un chip: su ICCID y su número (a 8 dígitos).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct IdentChip {
    pub iccid: Option<String>,
    pub numero: Option<String>,
}

impl IdentChip {
    pub fn de(s: &Sim) -> Self {
        let iccid = s.iccid.as_deref().map(str::trim).filter(|x| !x.is_empty()).map(String::from);
        let digitos: String = s.numero.as_deref().unwrap_or("").chars().filter(|c| c.is_ascii_digit()).collect();
        let numero = (!digitos.is_empty()).then(|| digitos[digitos.len().saturating_sub(8)..].to_string());
        Self { iccid, numero }
    }

    pub fn vacia(&self) -> bool {
        self.iccid.is_none() && self.numero.is_none()
    }

    /// Son chips distintos: el ICCID decide si ambos lo traen; si no, el número. Un dato que falta no cuenta.
    pub fn choca(&self, otra: &Self) -> bool {
        match (&self.iccid, &otra.iccid) {
            (Some(a), Some(b)) => a != b,
            _ => matches!((&self.numero, &otra.numero), (Some(a), Some(b)) if a != b),
        }
    }
}

/// Un chip que entró a una ranura (o cambió el que había).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CambioChip {
    pub dispositivo: String,
    pub slot: u32,
    pub chip_id: i64,
    /// El chip que estaba antes en la ranura (si había uno identificable).
    pub anterior: Option<IdentChip>,
    pub nuevo: IdentChip,
    /// Primera vez que se ve esa ranura: no es un cambio, conserva lo que ya tenía.
    pub primera_vez: bool,
    /// Ese chip ya se había visto antes (en cualquier ranura).
    pub regreso: bool,
}

/// Quién ocupa hoy una ranura.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ocupante {
    pub chip_id: i64,
    /// `false` desde que entró un chip distinto hasta que se consulta su saldo.
    pub verificado: bool,
}

/// Lo que la tabla `chips` sabe de un chip (para `Contexto::chips`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DatosChip {
    pub tipo: String,
    pub tiene_pin: bool,
    pub saldo_dealer: Option<f64>,
    pub saldo_dealer_ms: Option<i64>,
}

/// Filtros de `GET /api/lotes`.
#[derive(Debug, Clone, Default)]
pub struct FiltroLotes {
    pub limite: u32,
    /// Cursor: solo lotes con `id` menor.
    pub antes: Option<i64>,
    pub estados: Vec<EstadoLote>,
    pub problemas: Option<bool>,
    pub simular: Option<bool>,
    pub q: Option<String>,
    /// `Some(false)` (por defecto) oculta los archivados, `Some(true)` solo ellos, `None` todos.
    pub archivados: Option<bool>,
}

/// Filtros de `GET /api/lotes/{id}/filas`.
#[derive(Debug, Clone, Default)]
pub struct FiltroFilas {
    pub limite: u32,
    /// Cursor: solo filas con número mayor.
    pub despues: Option<u32>,
    pub estados: Vec<EstadoFila>,
    pub q: Option<String>,
}

/// Un lote igual a otro (mismo hash) creado hace poco.
#[derive(Debug, Clone, PartialEq)]
pub struct Duplicado {
    pub id: i64,
    pub nombre: String,
    pub creado_ms: i64,
}

#[derive(Clone)]
pub struct Persistencia {
    a: Arc<Almacen>,
}

impl Persistencia {
    pub fn new(a: Arc<Almacen>) -> R<Self> {
        a.con(|c| {
            c.execute_batch(
                "CREATE TABLE IF NOT EXISTS lotes (
                    id           INTEGER PRIMARY KEY,
                    nombre       TEXT NOT NULL,
                    creado_ms    INTEGER NOT NULL,
                    estado       TEXT NOT NULL,
                    simular      INTEGER NOT NULL,
                    motivo_pausa TEXT
                 );
                 CREATE TABLE IF NOT EXISTS lote_filas (
                    id         INTEGER PRIMARY KEY,
                    lote_id    INTEGER NOT NULL REFERENCES lotes(id),
                    fila       INTEGER NOT NULL,
                    numero     TEXT,
                    monto      INTEGER,
                    operador   TEXT,
                    accion     TEXT NOT NULL,
                    referencia TEXT,
                    receta     TEXT,
                    comision   REAL NOT NULL DEFAULT 0,
                    estado     TEXT NOT NULL,
                    detalle    TEXT,
                    original   TEXT NOT NULL
                 );
                 CREATE INDEX IF NOT EXISTS lote_filas_lote ON lote_filas(lote_id);
                 CREATE TABLE IF NOT EXISTS lote_envios (
                    id            INTEGER PRIMARY KEY,
                    fila_id       INTEGER NOT NULL REFERENCES lote_filas(id),
                    lote_id       INTEGER NOT NULL REFERENCES lotes(id),
                    n             INTEGER NOT NULL,
                    monto         INTEGER NOT NULL,
                    dispositivo   TEXT,
                    slot          INTEGER,
                    numero_origen TEXT,
                    estado        TEXT NOT NULL,
                    ejecucion_id  INTEGER,
                    detalle       TEXT,
                    intentos      INTEGER NOT NULL DEFAULT 0,
                    ts_ms         INTEGER
                 );
                 CREATE INDEX IF NOT EXISTS lote_envios_lote ON lote_envios(lote_id);
                 CREATE TABLE IF NOT EXISTS chips_saldo (
                    dispositivo TEXT NOT NULL,
                    slot        INTEGER NOT NULL,
                    numero      TEXT,
                    operador    TEXT,
                    saldo       REAL NOT NULL,
                    ts_ms       INTEGER NOT NULL,
                    PRIMARY KEY (dispositivo, slot)
                 );
                 CREATE TABLE IF NOT EXISTS lote_saldo_inicial (
                    lote_id     INTEGER NOT NULL REFERENCES lotes(id),
                    dispositivo TEXT NOT NULL,
                    slot        INTEGER NOT NULL,
                    tipo        TEXT NOT NULL DEFAULT 'principal',
                    saldo       REAL NOT NULL,
                    ts_ms       INTEGER NOT NULL,
                    PRIMARY KEY (lote_id, dispositivo, slot, tipo)
                 );
                 CREATE TABLE IF NOT EXISTS lote_verificacion (
                    lote_id INTEGER PRIMARY KEY REFERENCES lotes(id),
                    ts_ms   INTEGER NOT NULL,
                    json    TEXT NOT NULL
                 );
                 CREATE TABLE IF NOT EXISTS chips_agotado (
                    dispositivo TEXT NOT NULL,
                    slot        INTEGER NOT NULL,
                    fecha       TEXT NOT NULL,
                    PRIMARY KEY (dispositivo, slot, fecha)
                 );
                 CREATE TABLE IF NOT EXISTS chips (
                    id       INTEGER PRIMARY KEY AUTOINCREMENT,
                    iccid    TEXT,
                    numero   TEXT,
                    visto_ms INTEGER NOT NULL,
                    tipo     TEXT NOT NULL DEFAULT 'normal'
                 );
                 CREATE INDEX IF NOT EXISTS chips_iccid ON chips(iccid);
                 CREATE INDEX IF NOT EXISTS chips_numero ON chips(numero);
                 CREATE TABLE IF NOT EXISTS chips_tenencia (
                    id          INTEGER PRIMARY KEY AUTOINCREMENT,
                    chip_id     INTEGER NOT NULL REFERENCES chips(id),
                    dispositivo TEXT NOT NULL,
                    slot        INTEGER NOT NULL,
                    desde_ms    INTEGER NOT NULL,
                    hasta_ms    INTEGER,
                    verificado  INTEGER NOT NULL DEFAULT 1
                 );
                 CREATE INDEX IF NOT EXISTS chips_tenencia_slot ON chips_tenencia(dispositivo, slot);
                 CREATE INDEX IF NOT EXISTS chips_tenencia_chip ON chips_tenencia(chip_id);
                 CREATE TABLE IF NOT EXISTS chips_agotado_chip (
                    chip_id INTEGER NOT NULL REFERENCES chips(id),
                    fecha   TEXT NOT NULL,
                    PRIMARY KEY (chip_id, fecha)
                 );
                 CREATE INDEX IF NOT EXISTS ejecuciones_cupo ON ejecuciones(dispositivo, slot, inicio_ms);",
            )?;
            // Bases anteriores a los chips dealer: `chips` no tiene `tipo`.
            let tiene_tipo = c
                .prepare("SELECT 1 FROM pragma_table_info('chips') WHERE name = 'tipo'")?
                .exists([])?;
            if !tiene_tipo {
                c.execute("ALTER TABLE chips ADD COLUMN tipo TEXT NOT NULL DEFAULT 'normal'", [])?;
            }
            // Lotes de un solo chip (`OrigenLote` en JSON). Bases anteriores no lo tienen.
            let tiene_origen = c
                .prepare("SELECT 1 FROM pragma_table_info('lotes') WHERE name = 'origen'")?
                .exists([])?;
            if !tiene_origen {
                c.execute("ALTER TABLE lotes ADD COLUMN origen TEXT", [])?;
            }
            // PIN de los chips dealer. Bases anteriores no lo tienen.
            let tiene_pin = c
                .prepare("SELECT 1 FROM pragma_table_info('chips') WHERE name = 'pin'")?
                .exists([])?;
            if !tiene_pin {
                c.execute("ALTER TABLE chips ADD COLUMN pin TEXT", [])?;
            }
            // Saldo dealer de los chips dealer. Bases de antes del cambio de nombre lo tienen como `saldo_pda`
            // (se renombra la columna, con sus datos); bases más viejas no lo tienen.
            let columna = |c: &rusqlite::Connection, n: &str| -> R<bool> {
                c.prepare("SELECT 1 FROM pragma_table_info('chips') WHERE name = ?1")?.exists([n])
            };
            if !columna(c, "saldo_dealer")? {
                if columna(c, "saldo_pda")? {
                    c.execute_batch(
                        "ALTER TABLE chips RENAME COLUMN saldo_pda TO saldo_dealer;
                         ALTER TABLE chips RENAME COLUMN saldo_pda_ms TO saldo_dealer_ms;",
                    )?;
                } else {
                    c.execute_batch("ALTER TABLE chips ADD COLUMN saldo_dealer REAL; ALTER TABLE chips ADD COLUMN saldo_dealer_ms INTEGER;")?;
                }
            }
            // Lista paginada sin leer filas: progreso guardado, marca de problemas, archivo y hash (duplicados).
            let tiene_progreso = c
                .prepare("SELECT 1 FROM pragma_table_info('lotes') WHERE name = 'progreso'")?
                .exists([])?;
            if !tiene_progreso {
                c.execute_batch(
                    "ALTER TABLE lotes ADD COLUMN progreso TEXT;
                     ALTER TABLE lotes ADD COLUMN problemas INTEGER NOT NULL DEFAULT 0;
                     ALTER TABLE lotes ADD COLUMN archivado INTEGER NOT NULL DEFAULT 0;
                     ALTER TABLE lotes ADD COLUMN hash TEXT;",
                )?;
            }
            // Saldo inicial por tipo (`principal` o `dealer`): un chip puede tener los dos en un lote mixto. Bases
            // anteriores guardaban uno solo por chip: el de los lotes de un chip que no transfieren era el dealer.
            let tiene_tipo_saldo = c
                .prepare("SELECT 1 FROM pragma_table_info('lote_saldo_inicial') WHERE name = 'tipo'")?
                .exists([])?;
            if !tiene_tipo_saldo {
                let tx = c.transaction()?;
                tx.execute_batch(
                    "CREATE TABLE lote_saldo_inicial_v2 (
                        lote_id     INTEGER NOT NULL REFERENCES lotes(id),
                        dispositivo TEXT NOT NULL,
                        slot        INTEGER NOT NULL,
                        tipo        TEXT NOT NULL DEFAULT 'principal',
                        saldo       REAL NOT NULL,
                        ts_ms       INTEGER NOT NULL,
                        PRIMARY KEY (lote_id, dispositivo, slot, tipo)
                     );
                     INSERT INTO lote_saldo_inicial_v2 (lote_id, dispositivo, slot, tipo, saldo, ts_ms)
                        SELECT s.lote_id, s.dispositivo, s.slot,
                               CASE WHEN l.origen IS NOT NULL AND NOT EXISTS (
                                        SELECT 1 FROM lote_filas f
                                        WHERE f.lote_id = s.lote_id AND f.estado != 'omitida' AND f.accion = 'transferir')
                                    THEN 'dealer' ELSE 'principal' END,
                               s.saldo, s.ts_ms
                        FROM lote_saldo_inicial s LEFT JOIN lotes l ON l.id = s.lote_id;
                     DROP TABLE lote_saldo_inicial;
                     ALTER TABLE lote_saldo_inicial_v2 RENAME TO lote_saldo_inicial;",
                )?;
                tx.commit()?;
            }
            c.execute_batch(
                "CREATE INDEX IF NOT EXISTS lotes_estado ON lotes(estado, id);
                 CREATE INDEX IF NOT EXISTS lotes_hash ON lotes(hash);
                 CREATE INDEX IF NOT EXISTS lote_envios_estado ON lote_envios(lote_id, estado);
                 CREATE INDEX IF NOT EXISTS lote_filas_fila ON lote_filas(lote_id, fila);
                 CREATE INDEX IF NOT EXISTS lote_envios_fila ON lote_envios(fila_id);",
            )?;
            Ok(())
        })?;
        let p = Self { a };
        // Una sola vez: los lotes de antes no tienen el progreso guardado.
        let sin_progreso: Vec<i64> = p.a.con(|c| {
            let mut st = c.prepare("SELECT id FROM lotes WHERE progreso IS NULL")?;
            let v = st.query_map([], |r| r.get(0))?.collect::<R<Vec<i64>>>()?;
            Ok(v)
        })?;
        for id in sin_progreso {
            p.refrescar_progreso(id)?;
        }
        p.migrar_origenes()?;
        p.migrar_dealer()?;
        Ok(p)
    }

    /// Cambio de nombre "PDA" → "dealer" (ver "Compatibilidad" en el protocolo). Idempotente: solo toca lo que
    /// todavía dice `pda`, así que se puede correr en cada arranque. No toca el PIN ni ninguna otra columna:
    /// - `chips.tipo` y `lote_saldo_inicial.tipo`: `pda` → `dealer`;
    /// - JSON guardado de los lotes (`origen`, `progreso`, verificación): `"tipo"`/`"saldo"` `pda` → `dealer`, y
    ///   "PDA" → "dealer" en los textos (avisos, motivos);
    /// - eventos `chip_tipo`: `tipo`/`anterior` `pda` → `dealer`.
    ///
    /// Los nombres de receta guardados (`tigo-pda-recargar`) no cambian: se resuelven con `nombres_anteriores`.
    fn migrar_dealer(&self) -> R<()> {
        self.a.con(|c| {
            let tx = c.transaction()?;
            let chips = tx.execute("UPDATE chips SET tipo = 'dealer' WHERE tipo = 'pda'", [])?;
            let saldos = tx.execute("UPDATE lote_saldo_inicial SET tipo = 'dealer' WHERE tipo = 'pda'", [])?;
            let mut jsons = 0;
            for (tabla, columna, clave) in
                [("lotes", "origen", "id"), ("lotes", "progreso", "id"), ("lote_verificacion", "json", "lote_id")]
            {
                let filas: Vec<(i64, String)> = {
                    let mut st = tx.prepare(&format!(
                        "SELECT {clave}, {columna} FROM {tabla} WHERE {columna} LIKE '%pda%'"
                    ))?;
                    let v = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<R<Vec<_>>>()?;
                    v
                };
                for (id, texto) in filas {
                    let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&texto) else { continue };
                    if !json_a_dealer(&mut v, None) {
                        continue;
                    }
                    let nuevo = serde_json::to_string(&v).unwrap_or(texto);
                    tx.execute(&format!("UPDATE {tabla} SET {columna} = ?2 WHERE {clave} = ?1"), params![id, nuevo])?;
                    jsons += 1;
                }
            }
            let eventos = tx.execute(
                "UPDATE eventos SET datos = replace(datos, '\"pda\"', '\"dealer\"')
                 WHERE tipo = 'chip_tipo' AND datos LIKE '%\"pda\"%'",
                [],
            )?;
            tx.commit()?;
            if chips + saldos + jsons + eventos > 0 {
                tracing::info!(chips, saldos, jsons, eventos, "migración PDA → dealer");
            }
            Ok(())
        })
    }

    /// Una sola vez: los lotes de un chip guardados con la forma vieja del origen (`{dispositivo, slot, receta}`)
    /// pasan a la nueva (`chips` + `accion`), conservando los campos viejos.
    fn migrar_origenes(&self) -> R<()> {
        let viejos: Vec<(i64, String)> = self.a.con(|c| {
            let mut st = c.prepare("SELECT id, origen FROM lotes WHERE origen IS NOT NULL AND origen NOT LIKE '%\"chips\"%'")?;
            let v = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<R<Vec<_>>>()?;
            Ok(v)
        })?;
        for (id, json) in viejos {
            let Ok(o) = serde_json::from_str::<OrigenLote>(&json) else { continue };
            let mut o = o.normalizado();
            if o.accion.is_none() {
                o.accion = self.a.con(|c| {
                    c.query_row(
                        "SELECT accion FROM lote_filas WHERE lote_id = ?1 ORDER BY (estado = 'omitida'), fila LIMIT 1",
                        [id],
                        |r| r.get::<_, String>(0),
                    )
                    .optional()
                })?;
            }
            let nuevo = serde_json::to_string(&o).unwrap_or(json);
            self.a.con(|c| c.execute("UPDATE lotes SET origen = ?2 WHERE id = ?1", params![id, nuevo]))?;
        }
        Ok(())
    }

    /// Duración promedio (ms) y cantidad de las ejecuciones de los envíos terminados de un lote, por chip.
    pub fn duraciones(&self, lote_id: i64) -> R<HashMap<(String, u32), (u64, u32)>> {
        self.a.con(|c| {
            let mut st = c.prepare(
                "SELECT e.dispositivo, e.slot, AVG(x.duracion_ms), COUNT(*) FROM lote_envios e
                 JOIN ejecuciones x ON x.id = e.ejecucion_id
                 WHERE e.lote_id = ?1 AND e.dispositivo IS NOT NULL AND e.estado IN ('ok', 'simulado', 'fallido', 'incierto')
                 GROUP BY e.dispositivo, e.slot",
            )?;
            let v = st
                .query_map([lote_id], |r| {
                    Ok(((r.get::<_, String>(0)?, r.get::<_, u32>(1)?), (r.get::<_, f64>(2)?.round() as u64, r.get::<_, u32>(3)?)))
                })?
                .collect::<R<HashMap<_, _>>>()?;
            Ok(v)
        })
    }

    pub fn almacen(&self) -> &Arc<Almacen> {
        &self.a
    }

    // ───────────── chips ─────────────

    /// Envíos `ok`, no simulados, de las recetas indicadas, desde `desde_ms`, hechos por ese chip: en cualquier
    /// ranura o teléfono donde haya estado (lo que hizo otro chip en la misma ranura no cuenta).
    pub fn enviados_ok(&self, recetas: &[String], chip_id: i64, desde_ms: i64) -> R<u32> {
        if recetas.is_empty() {
            return Ok(0);
        }
        self.a.con(|c| {
            let marcas = vec!["?"; recetas.len()].join(",");
            let sql = format!(
                "SELECT COUNT(*) FROM ejecuciones e JOIN chips_tenencia t
                   ON e.dispositivo = t.dispositivo AND e.slot = t.slot AND e.inicio_ms >= t.desde_ms
                  AND (t.hasta_ms IS NULL OR e.inicio_ms < t.hasta_ms)
                 WHERE t.chip_id = ? AND e.inicio_ms >= ? AND e.estado = 'ok' AND e.simular = 0 AND e.receta IN ({marcas})"
            );
            let mut st = c.prepare(&sql)?;
            let mut ps: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(chip_id), Box::new(desde_ms)];
            for r in recetas {
                ps.push(Box::new(r.clone()));
            }
            st.query_row(rusqlite::params_from_iter(ps.iter().map(|b| b.as_ref())), |r| r.get::<_, u32>(0))
        })
    }

    pub fn guardar_saldo(&self, s: &SaldoChip) -> R<()> {
        self.a.con(|c| {
            c.execute(
                "INSERT INTO chips_saldo (dispositivo, slot, numero, operador, saldo, ts_ms) VALUES (?1,?2,?3,?4,?5,?6)
                 ON CONFLICT(dispositivo, slot) DO UPDATE SET numero = COALESCE(excluded.numero, numero),
                   operador = COALESCE(excluded.operador, operador), saldo = excluded.saldo, ts_ms = excluded.ts_ms",
                params![s.dispositivo, s.slot, s.numero, s.operador, s.saldo, s.ts_ms],
            )?;
            Ok(())
        })
    }

    pub fn saldos(&self) -> R<Vec<SaldoChip>> {
        self.a.con(|c| {
            let mut st = c.prepare("SELECT dispositivo, slot, numero, operador, saldo, ts_ms FROM chips_saldo")?;
            let v = st
                .query_map([], |r| {
                    Ok(SaldoChip {
                        dispositivo: r.get(0)?,
                        slot: r.get(1)?,
                        numero: r.get(2)?,
                        operador: r.get(3)?,
                        saldo: r.get(4)?,
                        ts_ms: r.get(5)?,
                    })
                })?
                .collect::<R<Vec<_>>>()?;
            Ok(v)
        })
    }

    pub fn marcar_agotado(&self, chip_id: i64, fecha: &str) -> R<()> {
        self.a.con(|c| {
            c.execute("INSERT OR IGNORE INTO chips_agotado_chip (chip_id, fecha) VALUES (?1, ?2)", params![chip_id, fecha])?;
            Ok(())
        })
    }

    pub fn agotados(&self, fecha: &str) -> R<HashSet<i64>> {
        self.a.con(|c| {
            let mut st = c.prepare("SELECT chip_id FROM chips_agotado_chip WHERE fecha = ?1")?;
            let v = st.query_map([fecha], |r| r.get(0))?.collect::<R<HashSet<_>>>()?;
            Ok(v)
        })
    }

    pub fn borrar_saldo(&self, dispositivo: &str, slot: u32) -> R<()> {
        self.a.con(|c| {
            c.execute("DELETE FROM chips_saldo WHERE dispositivo = ?1 AND slot = ?2", params![dispositivo, slot])?;
            Ok(())
        })
    }

    // ───────────── identidad de chips ─────────────

    /// Quién ocupa cada ranura de un teléfono.
    pub fn ocupantes(&self, dispositivo: Option<&str>) -> R<Vec<(String, u32, Ocupante)>> {
        self.a.con(|c| {
            let mut st = c.prepare(
                "SELECT dispositivo, slot, chip_id, verificado FROM chips_tenencia
                 WHERE hasta_ms IS NULL AND (?1 IS NULL OR dispositivo = ?1)",
            )?;
            let v = st
                .query_map([dispositivo], |r| {
                    Ok((r.get(0)?, r.get(1)?, Ocupante { chip_id: r.get(2)?, verificado: r.get::<_, i64>(3)? != 0 }))
                })?
                .collect::<R<Vec<_>>>()?;
            Ok(v)
        })
    }

    /// Tipo, PIN (si hay) y saldo dealer de cada chip, en una consulta.
    pub fn datos_chips(&self) -> R<HashMap<i64, DatosChip>> {
        self.a.con(|c| {
            let mut st = c.prepare("SELECT id, tipo, pin IS NOT NULL, saldo_dealer, saldo_dealer_ms FROM chips")?;
            let v = st
                .query_map([], |r| {
                    Ok((
                        r.get(0)?,
                        DatosChip { tipo: r.get(1)?, tiene_pin: r.get(2)?, saldo_dealer: r.get(3)?, saldo_dealer_ms: r.get(4)? },
                    ))
                })?
                .collect::<R<HashMap<_, _>>>()?;
            Ok(v)
        })
    }

    /// Envíos `ok`, no simulados, desde `desde_ms`, de cada chip y receta: chip → receta → cantidad. Igual que
    /// [`Persistencia::enviados_ok`] pero para todos los chips en una consulta.
    pub fn enviados_ok_por_chip(&self, desde_ms: i64) -> R<HashMap<i64, HashMap<String, u32>>> {
        self.a.con(|c| {
            let mut st = c.prepare(
                "SELECT t.chip_id, e.receta, COUNT(*) FROM ejecuciones e JOIN chips_tenencia t
                   ON e.dispositivo = t.dispositivo AND e.slot = t.slot AND e.inicio_ms >= t.desde_ms
                  AND (t.hasta_ms IS NULL OR e.inicio_ms < t.hasta_ms)
                 WHERE e.inicio_ms >= ?1 AND e.estado = 'ok' AND e.simular = 0
                 GROUP BY t.chip_id, e.receta",
            )?;
            let mut m: HashMap<i64, HashMap<String, u32>> = HashMap::new();
            for x in st.query_map([desde_ms], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, u32>(2)?)))? {
                let (chip, receta, n) = x?;
                m.entry(chip).or_default().insert(receta, n);
            }
            Ok(m)
        })
    }

    /// Tipo (`normal` | `dealer`) de cada chip que ocupa una ranura.
    pub fn tipos(&self) -> R<HashMap<i64, String>> {
        self.a.con(|c| {
            let mut st = c.prepare("SELECT id, tipo FROM chips")?;
            let v = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<R<HashMap<_, _>>>()?;
            Ok(v)
        })
    }

    /// Fija el tipo de un chip. Devuelve el tipo que tenía.
    pub fn fijar_tipo(&self, chip_id: i64, tipo: &str) -> R<String> {
        self.a.con(|c| {
            let anterior: String = c.query_row("SELECT tipo FROM chips WHERE id = ?1", [chip_id], |r| r.get(0))?;
            c.execute("UPDATE chips SET tipo = ?2 WHERE id = ?1", params![chip_id, tipo])?;
            Ok(anterior)
        })
    }

    /// PIN guardado de un chip.
    pub fn pin(&self, chip_id: i64) -> R<Option<String>> {
        self.a.con(|c| c.query_row("SELECT pin FROM chips WHERE id = ?1", [chip_id], |r| r.get(0)))
    }

    /// Ids de los chips con PIN guardado.
    pub fn con_pin(&self) -> R<HashSet<i64>> {
        self.a.con(|c| {
            let mut st = c.prepare("SELECT id FROM chips WHERE pin IS NOT NULL")?;
            let v = st.query_map([], |r| r.get(0))?.collect::<R<HashSet<_>>>()?;
            Ok(v)
        })
    }

    /// Saldo dealer conocido de cada chip: chip → (saldo, ts_ms).
    pub fn saldos_dealer(&self) -> R<HashMap<i64, (f64, i64)>> {
        self.a.con(|c| {
            let mut st = c.prepare("SELECT id, saldo_dealer, saldo_dealer_ms FROM chips WHERE saldo_dealer IS NOT NULL")?;
            let v = st.query_map([], |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?))))?.collect::<R<HashMap<_, _>>>()?;
            Ok(v)
        })
    }

    pub fn fijar_saldo_dealer(&self, chip_id: i64, saldo: f64, ts_ms: i64) -> R<()> {
        self.a.con(|c| {
            c.execute("UPDATE chips SET saldo_dealer = ?2, saldo_dealer_ms = ?3 WHERE id = ?1", params![chip_id, saldo, ts_ms])?;
            Ok(())
        })
    }

    /// Guarda (o borra, con `None`) el PIN de un chip.
    pub fn fijar_pin(&self, chip_id: i64, pin: Option<&str>) -> R<()> {
        self.a.con(|c| {
            c.execute("UPDATE chips SET pin = ?2 WHERE id = ?1", params![chip_id, pin])?;
            Ok(())
        })
    }

    pub fn marcar_verificado(&self, dispositivo: &str, slot: u32) -> R<()> {
        self.a.con(|c| {
            c.execute(
                "UPDATE chips_tenencia SET verificado = 1 WHERE dispositivo = ?1 AND slot = ?2 AND hasta_ms IS NULL",
                params![dispositivo, slot],
            )?;
            Ok(())
        })
    }

    /// Compara los chips que informa un teléfono con quién ocupaba cada ranura y registra los cambios.
    /// Una ranura vacía, ausente o que ya no aparece cierra la estadía del chip que tenía.
    pub fn reconciliar_sims(&self, dispositivo: &str, sims: &[Sim], ahora: i64) -> R<Vec<CambioChip>> {
        self.a.con(|c| {
            let tx = c.transaction()?;
            let mut cambios = vec![];
            let mut presentes: Vec<u32> = vec![];
            for sim in sims.iter().filter(|s| s.estado != "ausente") {
                presentes.push(sim.slot);
                let ident = IdentChip::de(sim);
                let actual = tenencia_abierta(&tx, dispositivo, sim.slot)?;
                let hallado = if ident.vacia() { None } else { buscar_chip(&tx, &ident)? };
                match (&actual, hallado) {
                    (Some((_, cid, _)), Some(h)) if *cid == h => enriquecer(&tx, h, &ident, ahora)?,
                    (Some((_, cid, cur)), None) if ident.vacia() || !cur.choca(&ident) => enriquecer(&tx, *cid, &ident, ahora)?,
                    _ => {
                        let primera_vez = actual.is_none() && !hay_tenencias(&tx, dispositivo, sim.slot)?;
                        let anterior = match &actual {
                            Some((tid, _, cur)) => {
                                cerrar(&tx, *tid, ahora)?;
                                Some(cur.clone()).filter(|x| !x.vacia())
                            }
                            None => None,
                        };
                        let regreso = hallado.is_some();
                        let chip_id = match hallado {
                            Some(h) => {
                                enriquecer(&tx, h, &ident, ahora)?;
                                h
                            }
                            None => {
                                tx.execute(
                                    "INSERT INTO chips (iccid, numero, visto_ms) VALUES (?1, ?2, ?3)",
                                    params![ident.iccid, ident.numero, ahora],
                                )?;
                                tx.last_insert_rowid()
                            }
                        };
                        // Un chip no está en dos ranuras a la vez: si quedó abierta en otro teléfono, ya no está ahí.
                        tx.execute(
                            "UPDATE chips_tenencia SET hasta_ms = ?2 WHERE chip_id = ?1 AND hasta_ms IS NULL",
                            params![chip_id, ahora],
                        )?;
                        tx.execute(
                            "INSERT INTO chips_tenencia (chip_id, dispositivo, slot, desde_ms, verificado) VALUES (?1,?2,?3,?4,?5)",
                            // La primera estadía hereda todo el historial anterior de la ranura (desde 0).
                            params![chip_id, dispositivo, sim.slot, if primera_vez { 0 } else { ahora }, primera_vez],
                        )?;
                        cambios.push(CambioChip {
                            dispositivo: dispositivo.into(),
                            slot: sim.slot,
                            chip_id,
                            anterior,
                            nuevo: ident,
                            primera_vez,
                            regreso,
                        });
                    }
                }
            }
            let abiertas: Vec<(i64, u32)> = {
                let mut st = tx.prepare("SELECT id, slot FROM chips_tenencia WHERE dispositivo = ?1 AND hasta_ms IS NULL")?;
                let v = st.query_map([dispositivo], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<R<Vec<_>>>()?;
                v
            };
            for (tid, slot) in abiertas {
                if !presentes.contains(&slot) {
                    cerrar(&tx, tid, ahora)?;
                }
            }
            tx.commit()?;
            Ok(cambios)
        })
    }

    // ───────────── lotes ─────────────

    #[allow(clippy::too_many_arguments)]
    pub fn crear_lote(
        &self,
        nombre: &str,
        simular: bool,
        creado_ms: i64,
        origen: Option<&OrigenLote>,
        filas: &[NuevaFila],
        hash: Option<&str>,
    ) -> R<i64> {
        let origen = origen.and_then(|o| serde_json::to_string(o).ok());
        let progreso = progreso_nuevo(filas);
        let progreso_json = serde_json::to_string(&progreso).unwrap_or_default();
        self.a.con(|c| {
            let tx = c.transaction()?;
            tx.execute(
                "INSERT INTO lotes (nombre, creado_ms, estado, simular, origen, progreso, problemas, hash)
                 VALUES (?1, ?2, 'listo', ?3, ?4, ?5, 0, ?6)",
                params![nombre, creado_ms, simular, origen, progreso_json, hash],
            )?;
            let lote = tx.last_insert_rowid();
            for f in filas {
                tx.execute(
                    "INSERT INTO lote_filas (lote_id, fila, numero, monto, operador, accion, referencia, receta, comision, estado, detalle, original)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
                    params![
                        lote,
                        f.fila,
                        f.numero,
                        f.monto,
                        f.operador,
                        f.accion,
                        f.referencia,
                        f.receta,
                        f.comision,
                        f.estado.texto(),
                        f.detalle,
                        serde_json::to_string(&f.original).unwrap_or_default()
                    ],
                )?;
                let fila_id = tx.last_insert_rowid();
                for (n, m) in f.envios.iter().enumerate() {
                    tx.execute(
                        "INSERT INTO lote_envios (fila_id, lote_id, n, monto, estado) VALUES (?1, ?2, ?3, ?4, 'pendiente')",
                        params![fila_id, lote, n as i64 + 1, *m],
                    )?;
                }
            }
            tx.commit()?;
            Ok(lote)
        })
    }

    /// Lote real, no cancelado, con ese hash y creado desde `desde_ms` (el más nuevo).
    pub fn duplicado(&self, hash: &str, desde_ms: i64) -> R<Option<Duplicado>> {
        self.a.con(|c| {
            c.query_row(
                "SELECT id, nombre, creado_ms FROM lotes
                 WHERE hash = ?1 AND simular = 0 AND estado != 'cancelado' AND creado_ms >= ?2
                 ORDER BY id DESC LIMIT 1",
                params![hash, desde_ms],
                |r| Ok(Duplicado { id: r.get(0)?, nombre: r.get(1)?, creado_ms: r.get(2)? }),
            )
            .optional()
        })
    }

    /// Guarda el progreso del lote (lo que lee la lista sin tocar las filas).
    pub fn guardar_progreso(&self, id: i64, p: &Progreso) -> R<()> {
        let json = serde_json::to_string(p).unwrap_or_default();
        self.a.con(|c| {
            c.execute(
                "UPDATE lotes SET progreso = ?2, problemas = ?3 WHERE id = ?1",
                params![id, json, tiene_problemas(p)],
            )?;
            Ok(())
        })
    }

    /// Recalcula el progreso desde las filas y lo guarda (fuera del ejecutor: crear, recuperar, reintentar).
    pub fn refrescar_progreso(&self, id: i64) -> R<()> {
        if let Some((lote, _)) = self.lote(id)? {
            self.guardar_progreso(id, &lote.progreso)?;
        }
        Ok(())
    }

    pub fn archivar(&self, id: i64, archivado: bool) -> R<()> {
        self.a.con(|c| {
            c.execute("UPDATE lotes SET archivado = ?2 WHERE id = ?1", params![id, archivado])?;
            Ok(())
        })
    }

    pub fn estado_lote(&self, id: i64, estado: EstadoLote, motivo: Option<&str>) -> R<()> {
        self.a.con(|c| {
            c.execute(
                "UPDATE lotes SET estado = ?2, motivo_pausa = ?3 WHERE id = ?1",
                params![id, estado.texto(), motivo],
            )?;
            Ok(())
        })
    }

    pub fn guardar_envio(&self, e: &EnvioLote) -> R<()> {
        self.a.con(|c| {
            c.execute(
                "UPDATE lote_envios SET dispositivo = ?2, slot = ?3, numero_origen = ?4, estado = ?5, ejecucion_id = ?6,
                 detalle = ?7, intentos = ?8, ts_ms = ?9 WHERE id = ?1",
                params![
                    e.id,
                    e.dispositivo,
                    e.slot,
                    e.numero_origen,
                    e.estado.texto(),
                    e.ejecucion_id,
                    e.detalle,
                    e.intentos,
                    e.ts_ms
                ],
            )?;
            Ok(())
        })
    }

    pub fn guardar_fila(&self, f: &FilaLote) -> R<()> {
        self.a.con(|c| {
            c.execute(
                "UPDATE lote_filas SET estado = ?2, detalle = ?3 WHERE id = ?1",
                params![f.id, f.estado.texto(), f.detalle],
            )?;
            Ok(())
        })
    }

    /// Lote sin filas, con el progreso guardado (no lee las filas).
    pub fn cabecera(&self, id: i64) -> R<Option<Lote>> {
        self.a.con(|c| {
            c.query_row(&format!("SELECT {COLUMNAS_LOTE} FROM lotes l LEFT JOIN lote_verificacion v ON v.lote_id = l.id WHERE l.id = ?1"), [id], leer_lote)
                .optional()
        })
    }

    /// Página de lotes (sin filas), más nuevos primero: (lotes, siguiente, total).
    pub fn listar(&self, f: &FiltroLotes) -> R<(Vec<Lote>, Option<i64>, u64)> {
        let mut conds: Vec<String> = vec![];
        let mut ps: Vec<rusqlite::types::Value> = vec![];
        if !f.estados.is_empty() {
            conds.push(format!("l.estado IN ({})", vec!["?"; f.estados.len()].join(",")));
            ps.extend(f.estados.iter().map(|e| rusqlite::types::Value::Text(e.texto().into())));
        }
        if let Some(p) = f.problemas {
            conds.push("l.problemas = ?".into());
            ps.push(rusqlite::types::Value::Integer(p as i64));
        }
        if let Some(sim) = f.simular {
            conds.push("l.simular = ?".into());
            ps.push(rusqlite::types::Value::Integer(sim as i64));
        }
        if let Some(a) = f.archivados {
            conds.push("l.archivado = ?".into());
            ps.push(rusqlite::types::Value::Integer(a as i64));
        }
        if let Some(q) = f.q.as_deref().map(str::trim).filter(|q| !q.is_empty()) {
            let como_id = q.trim_start_matches('#').trim().parse::<i64>().ok();
            conds.push("(l.nombre LIKE ? ESCAPE '\\' OR l.id = ?)".into());
            ps.push(rusqlite::types::Value::Text(format!("%{}%", escapar_like(q))));
            ps.push(rusqlite::types::Value::Integer(como_id.unwrap_or(-1)));
        }
        let donde = |extra: Option<&str>| -> String {
            let mut c = conds.clone();
            if let Some(x) = extra {
                c.push(x.into());
            }
            if c.is_empty() { String::new() } else { format!("WHERE {}", c.join(" AND ")) }
        };
        let limite = f.limite.max(1);
        self.a.con(|c| {
            let total: u64 = c.query_row(
                &format!("SELECT COUNT(*) FROM lotes l {}", donde(None)),
                rusqlite::params_from_iter(ps.iter()),
                |r| r.get(0),
            )?;
            let mut ps2 = ps.clone();
            let w = match f.antes {
                Some(a) => {
                    ps2.push(rusqlite::types::Value::Integer(a));
                    donde(Some("l.id < ?"))
                }
                None => donde(None),
            };
            ps2.push(rusqlite::types::Value::Integer(limite as i64 + 1));
            let mut st = c.prepare(&format!(
                "SELECT {COLUMNAS_LOTE} FROM lotes l LEFT JOIN lote_verificacion v ON v.lote_id = l.id {w} ORDER BY l.id DESC LIMIT ?"
            ))?;
            let mut lotes = st.query_map(rusqlite::params_from_iter(ps2.iter()), leer_lote)?.collect::<R<Vec<_>>>()?;
            let siguiente = if lotes.len() > limite as usize {
                lotes.truncate(limite as usize);
                lotes.last().map(|l| l.id)
            } else {
                None
            };
            Ok((lotes, siguiente, total))
        })
    }

    /// Números de las pestañas (no dependen del filtro).
    pub fn conteos(&self) -> R<ConteosLotes> {
        self.a.con(|c| {
            c.query_row(
                "SELECT
                   COALESCE(SUM(archivado = 0 AND estado IN ('listo','en_curso','pausado')), 0),
                   COALESCE(SUM(archivado = 0 AND problemas = 1), 0),
                   COALESCE(SUM(archivado = 0 AND estado IN ('terminado','cancelado')), 0),
                   COALESCE(SUM(archivado = 0 AND simular = 1), 0),
                   COALESCE(SUM(archivado = 1), 0)
                 FROM lotes",
                [],
                |r| {
                    Ok(ConteosLotes {
                        activos: r.get(0)?,
                        problemas: r.get(1)?,
                        terminados: r.get(2)?,
                        simulaciones: r.get(3)?,
                        archivados: r.get(4)?,
                    })
                },
            )
        })
    }

    /// Página de filas de un lote, por número de fila: (filas, siguiente, total).
    pub fn filas(&self, lote_id: i64, f: &FiltroFilas) -> R<(Vec<FilaLote>, Option<u32>, u64)> {
        let mut conds: Vec<String> = vec!["lote_id = ?".into()];
        let mut ps: Vec<rusqlite::types::Value> = vec![rusqlite::types::Value::Integer(lote_id)];
        if !f.estados.is_empty() {
            conds.push(format!("estado IN ({})", vec!["?"; f.estados.len()].join(",")));
            ps.extend(f.estados.iter().map(|e| rusqlite::types::Value::Text(e.texto().into())));
        }
        if let Some(q) = f.q.as_deref().map(str::trim).filter(|q| !q.is_empty()) {
            conds.push("(numero LIKE ? ESCAPE '\\' OR referencia LIKE ? ESCAPE '\\')".into());
            let patron = format!("%{}%", escapar_like(q));
            ps.push(rusqlite::types::Value::Text(patron.clone()));
            ps.push(rusqlite::types::Value::Text(patron));
        }
        let limite = f.limite.max(1);
        self.a.con(|c| {
            let total: u64 = c.query_row(
                &format!("SELECT COUNT(*) FROM lote_filas WHERE {}", conds.join(" AND ")),
                rusqlite::params_from_iter(ps.iter()),
                |r| r.get(0),
            )?;
            let mut conds2 = conds.clone();
            let mut ps2 = ps.clone();
            if let Some(d) = f.despues {
                conds2.push("fila > ?".into());
                ps2.push(rusqlite::types::Value::Integer(d as i64));
            }
            ps2.push(rusqlite::types::Value::Integer(limite as i64 + 1));
            let mut st = c.prepare(&format!(
                "SELECT id, fila, numero, monto, operador, accion, referencia, receta, estado, detalle
                 FROM lote_filas WHERE {} ORDER BY fila, id LIMIT ?",
                conds2.join(" AND ")
            ))?;
            let mut filas = st
                .query_map(rusqlite::params_from_iter(ps2.iter()), |r| {
                    Ok(FilaLote {
                        id: r.get(0)?,
                        fila: r.get(1)?,
                        numero: r.get(2)?,
                        monto: r.get::<_, Option<f64>>(3)?,
                        operador: r.get(4)?,
                        accion: r.get(5)?,
                        referencia: r.get(6)?,
                        receta: r.get(7)?,
                        estado: EstadoFila::desde(&r.get::<_, String>(8)?),
                        enviado_bs: 0.0,
                        detalle: r.get(9)?,
                        envios: vec![],
                    })
                })?
                .collect::<R<Vec<_>>>()?;
            let siguiente = if filas.len() > limite as usize {
                filas.truncate(limite as usize);
                filas.last().map(|f| f.fila)
            } else {
                None
            };
            let mut st = c.prepare(&format!("SELECT {COLUMNAS_ENVIO} FROM lote_envios WHERE fila_id = ?1 ORDER BY n"))?;
            for f in &mut filas {
                f.envios = st.query_map([f.id], |r| leer_envio(r).map(|(_, e)| e))?.collect::<R<Vec<_>>>()?;
                f.enviado_bs = enviado(&f.envios);
            }
            Ok((filas, siguiente, total))
        })
    }

    /// Lote con sus filas y envíos (o `None`).
    pub fn lote(&self, id: i64) -> R<Option<(Lote, BTreeMap<i64, f64>)>> {
        let Some(mut lote) = self.cabecera(id)? else { return Ok(None) };
        self.a.con(|c| {

            let mut comisiones = BTreeMap::new();
            let mut filas: Vec<FilaLote> = {
                let mut st = c.prepare(
                    "SELECT id, fila, numero, monto, operador, accion, referencia, receta, estado, detalle, comision
                     FROM lote_filas WHERE lote_id = ?1 ORDER BY fila, id",
                )?;
                let v = st
                    .query_map([id], |r| {
                        Ok((
                            FilaLote {
                                id: r.get(0)?,
                                fila: r.get(1)?,
                                numero: r.get(2)?,
                                monto: r.get::<_, Option<f64>>(3)?,
                                operador: r.get(4)?,
                                accion: r.get(5)?,
                                referencia: r.get(6)?,
                                receta: r.get(7)?,
                                estado: EstadoFila::desde(&r.get::<_, String>(8)?),
                                enviado_bs: 0.0,
                                detalle: r.get(9)?,
                                envios: vec![],
                            },
                            r.get::<_, f64>(10)?,
                        ))
                    })?
                    .collect::<R<Vec<_>>>()?;
                v.into_iter()
                    .map(|(f, com)| {
                        comisiones.insert(f.id, com);
                        f
                    })
                    .collect()
            };
            let mut envios: BTreeMap<i64, Vec<EnvioLote>> = BTreeMap::new();
            {
                let mut st =
                    c.prepare(&format!("SELECT {COLUMNAS_ENVIO} FROM lote_envios WHERE lote_id = ?1 ORDER BY fila_id, n"))?;
                for x in st.query_map([id], leer_envio)? {
                    let (fila_id, e) = x?;
                    envios.entry(fila_id).or_default().push(e);
                }
            }
            for f in &mut filas {
                f.envios = envios.remove(&f.id).unwrap_or_default();
                f.enviado_bs = enviado(&f.envios);
            }
            // El lote completo se calcula desde las filas (la verdad); el guardado es para la lista.
            lote.progreso = progreso(&filas, &comisiones);
            lote.revision = Revision::de(lote.estado, &lote.progreso);
            lote.filas = Some(filas);
            Ok(Some((lote, comisiones)))
        })
    }

    /// Reemplaza el saldo inicial de los chips de un lote (se guarda en cada inicio o reanudación). `ts_ms` de
    /// cada saldo se ignora: todos quedan con `ts_ms` (la hora del inicio).
    pub fn guardar_saldos_iniciales(&self, lote_id: i64, saldos: &[SaldoInicial], ts_ms: i64) -> R<()> {
        self.a.con(|c| {
            let tx = c.transaction()?;
            tx.execute("DELETE FROM lote_saldo_inicial WHERE lote_id = ?1", [lote_id])?;
            for s in saldos {
                tx.execute(
                    "INSERT OR REPLACE INTO lote_saldo_inicial (lote_id, dispositivo, slot, tipo, saldo, ts_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![lote_id, s.dispositivo, s.slot, s.tipo, s.saldo, ts_ms],
                )?;
            }
            tx.commit()
        })
    }

    /// Guarda el resultado de `verificar` (reemplaza el anterior).
    pub fn guardar_verificacion(&self, v: &Verificacion) -> R<()> {
        let json = serde_json::to_string(v).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        self.a.con(|c| {
            c.execute(
                "INSERT INTO lote_verificacion (lote_id, ts_ms, json) VALUES (?1, ?2, ?3)
                 ON CONFLICT(lote_id) DO UPDATE SET ts_ms = excluded.ts_ms, json = excluded.json",
                params![v.lote_id, v.verificado_ms, json],
            )?;
            Ok(())
        })
    }

    /// El lote cambió: la verificación guardada ya no describe lo que hay.
    pub fn borrar_verificacion(&self, lote_id: i64) -> R<()> {
        self.a.con(|c| {
            c.execute("DELETE FROM lote_verificacion WHERE lote_id = ?1", [lote_id])?;
            Ok(())
        })
    }

    /// Saldos guardados al último inicio del lote.
    pub fn saldos_iniciales(&self, lote_id: i64) -> R<Vec<SaldoInicial>> {
        self.a.con(|c| {
            let mut st = c.prepare(
                "SELECT dispositivo, slot, tipo, saldo, ts_ms FROM lote_saldo_inicial WHERE lote_id = ?1 ORDER BY dispositivo, slot, tipo",
            )?;
            let v = st
                .query_map([lote_id], |r| {
                    Ok(SaldoInicial { dispositivo: r.get(0)?, slot: r.get(1)?, tipo: r.get(2)?, saldo: r.get(3)?, ts_ms: r.get(4)? })
                })?
                .collect::<R<Vec<_>>>()?;
            Ok(v)
        })
    }

    pub fn ids_lotes(&self) -> R<Vec<i64>> {
        self.a.con(|c| {
            let mut st = c.prepare("SELECT id FROM lotes ORDER BY id DESC")?;
            let v = st.query_map([], |r| r.get(0))?.collect::<R<Vec<i64>>>()?;
            Ok(v)
        })
    }

    /// Lotes (en cualquier estado) con algún envío `en_curso`.
    pub fn ids_con_envios_en_curso(&self) -> R<Vec<i64>> {
        self.a.con(|c| {
            let mut st = c.prepare("SELECT DISTINCT lote_id FROM lote_envios WHERE estado = 'en_curso' ORDER BY lote_id")?;
            let v = st.query_map([], |r| r.get(0))?.collect::<R<Vec<i64>>>()?;
            Ok(v)
        })
    }

    pub fn ids_en_estado(&self, estado: EstadoLote) -> R<Vec<i64>> {
        self.a.con(|c| {
            let mut st = c.prepare("SELECT id FROM lotes WHERE estado = ?1")?;
            let v = st.query_map([estado.texto()], |r| r.get(0))?.collect::<R<Vec<i64>>>()?;
            Ok(v)
        })
    }
}

const COLUMNAS_LOTE: &str =
    "l.id, l.nombre, l.creado_ms, l.estado, l.simular, l.motivo_pausa, l.origen, l.progreso, l.archivado, v.json";

fn leer_lote(r: &rusqlite::Row) -> R<Lote> {
    let estado = EstadoLote::desde(&r.get::<_, String>(3)?);
    let progreso: Progreso = r.get::<_, Option<String>>(7)?.and_then(|p| serde_json::from_str(&p).ok()).unwrap_or_default();
    Ok(Lote {
        id: r.get(0)?,
        nombre: r.get(1)?,
        creado_ms: r.get(2)?,
        estado,
        simular: r.get(4)?,
        motivo_pausa: r.get(5)?,
        revision: Revision::de(estado, &progreso),
        verificacion: r.get::<_, Option<String>>(9)?.and_then(|j| serde_json::from_str(&j).ok()),
        progreso,
        origen: r.get::<_, Option<String>>(6)?.and_then(|o| serde_json::from_str::<OrigenLote>(&o).ok()).map(OrigenLote::normalizado),
        archivado: r.get(8)?,
        filas: None,
    })
}

const COLUMNAS_ENVIO: &str =
    "id, fila_id, n, monto, dispositivo, slot, numero_origen, estado, ejecucion_id, detalle, intentos, ts_ms";

fn leer_envio(r: &rusqlite::Row) -> R<(i64, EnvioLote)> {
    Ok((
        r.get::<_, i64>(1)?,
        EnvioLote {
            id: r.get(0)?,
            n: r.get(2)?,
            monto: r.get::<_, f64>(3)?,
            dispositivo: r.get(4)?,
            slot: r.get(5)?,
            numero_origen: r.get(6)?,
            estado: EstadoEnvio::desde(&r.get::<_, String>(7)?),
            ejecucion_id: r.get(8)?,
            detalle: r.get(9)?,
            intentos: r.get(10)?,
            ts_ms: r.get(11)?,
        },
    ))
}

fn enviado(envios: &[EnvioLote]) -> f64 {
    crate::normalizar::centavos(envios.iter().filter(|e| e.estado == EstadoEnvio::Ok).map(|e| e.monto).sum())
}

/// `%`, `_` y `\` literales en un LIKE.
fn escapar_like(q: &str) -> String {
    q.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

/// ¿Alguna fila fallida, incierta o parcial? (pestaña "problemas").
pub fn tiene_problemas(p: &Progreso) -> bool {
    p.fallidas + p.inciertas + p.parcial > 0
}

/// Progreso de un lote recién creado (todo pendiente, sin envíos hechos).
fn progreso_nuevo(filas: &[NuevaFila]) -> Progreso {
    let mut p = Progreso::default();
    for f in filas {
        if f.estado == EstadoFila::Omitida {
            p.omitidas += 1;
        } else {
            p.filas += 1;
            p.pendientes += 1;
            p.total_bs += f.monto.unwrap_or(0.0);
        }
    }
    p.total_bs = crate::normalizar::centavos(p.total_bs);
    p
}

/// Progreso a partir de las filas (con su comisión por envío).
pub fn progreso(filas: &[FilaLote], comisiones: &BTreeMap<i64, f64>) -> Progreso {
    let mut p = Progreso::default();
    for f in filas {
        if f.estado == EstadoFila::Omitida {
            p.omitidas += 1;
            continue;
        }
        p.filas += 1;
        p.total_bs += f.monto.unwrap_or(0.0);
        match f.estado {
            EstadoFila::Ok => p.ok += 1,
            EstadoFila::Simulado => p.simulado += 1,
            EstadoFila::Parcial => p.parcial += 1,
            EstadoFila::Fallida => p.fallidas += 1,
            EstadoFila::Incierta => p.inciertas += 1,
            EstadoFila::SinCapacidad => p.sin_capacidad += 1,
            EstadoFila::Pendiente | EstadoFila::EnCurso => p.pendientes += 1,
            EstadoFila::Omitida => {}
        }
        let com = comisiones.get(&f.id).copied().unwrap_or(0.0);
        for e in &f.envios {
            if e.estado == EstadoEnvio::Ok {
                p.enviado_bs += e.monto;
                p.comision_bs += com;
            }
        }
    }
    p.comision_bs = (p.comision_bs * 100.0).round() / 100.0;
    p.total_bs = crate::normalizar::centavos(p.total_bs);
    p.enviado_bs = crate::normalizar::centavos(p.enviado_bs);
    p
}

// ───────────── identidad de chips: ayudas ─────────────

fn tenencia_abierta(c: &rusqlite::Connection, dispositivo: &str, slot: u32) -> R<Option<(i64, i64, IdentChip)>> {
    c.query_row(
        "SELECT t.id, t.chip_id, h.iccid, h.numero FROM chips_tenencia t JOIN chips h ON h.id = t.chip_id
         WHERE t.dispositivo = ?1 AND t.slot = ?2 AND t.hasta_ms IS NULL ORDER BY t.id DESC LIMIT 1",
        params![dispositivo, slot],
        |r| Ok((r.get(0)?, r.get(1)?, IdentChip { iccid: r.get(2)?, numero: r.get(3)? })),
    )
    .optional()
}

fn hay_tenencias(c: &rusqlite::Connection, dispositivo: &str, slot: u32) -> R<bool> {
    c.query_row("SELECT COUNT(*) FROM chips_tenencia WHERE dispositivo = ?1 AND slot = ?2", params![dispositivo, slot], |r| {
        r.get::<_, i64>(0)
    })
    .map(|n| n > 0)
}

fn cerrar(c: &rusqlite::Connection, tenencia: i64, ahora: i64) -> R<()> {
    c.execute("UPDATE chips_tenencia SET hasta_ms = ?2 WHERE id = ?1", params![tenencia, ahora])?;
    Ok(())
}

/// El chip ya conocido que corresponde a esta identidad. Con ICCID: ese ICCID, o uno sin ICCID guardado con el
/// mismo número. Sin ICCID: el último chip con ese número.
fn buscar_chip(c: &rusqlite::Connection, ident: &IdentChip) -> R<Option<i64>> {
    if let Some(i) = &ident.iccid {
        if let Some(id) = c.query_row("SELECT id FROM chips WHERE iccid = ?1 ORDER BY id DESC LIMIT 1", [i], |r| r.get(0)).optional()? {
            return Ok(Some(id));
        }
        return match &ident.numero {
            Some(n) => c
                .query_row("SELECT id FROM chips WHERE numero = ?1 AND iccid IS NULL ORDER BY id DESC LIMIT 1", [n], |r| r.get(0))
                .optional(),
            None => Ok(None),
        };
    }
    match &ident.numero {
        Some(n) => c.query_row("SELECT id FROM chips WHERE numero = ?1 ORDER BY visto_ms DESC, id DESC LIMIT 1", [n], |r| r.get(0)).optional(),
        None => Ok(None),
    }
}

/// Completa lo que faltaba del chip (ICCID nuevo, número) con lo que se acaba de leer.
fn enriquecer(c: &rusqlite::Connection, chip_id: i64, ident: &IdentChip, ahora: i64) -> R<()> {
    c.execute(
        "UPDATE chips SET iccid = COALESCE(iccid, ?2), numero = COALESCE(?3, numero), visto_ms = ?4 WHERE id = ?1",
        params![chip_id, ident.iccid, ident.numero, ahora],
    )?;
    Ok(())
}

/// Cambia en un JSON guardado lo que dice `pda` por `dealer`: el valor `"pda"` de las claves `tipo` y `saldo`, y la
/// palabra "PDA" en los textos. Devuelve si cambió algo.
fn json_a_dealer(v: &mut serde_json::Value, clave: Option<&str>) -> bool {
    use serde_json::Value;
    match v {
        Value::String(s) => {
            if matches!(clave, Some("tipo" | "saldo")) && s == "pda" {
                *s = "dealer".into();
                return true;
            }
            let nuevo = palabra_pda_a_dealer(s);
            let cambio = nuevo != *s;
            *s = nuevo;
            cambio
        }
        Value::Array(xs) => xs.iter_mut().fold(false, |c, x| json_a_dealer(x, clave) | c),
        Value::Object(m) => m.iter_mut().fold(false, |c, (k, x)| json_a_dealer(x, Some(k.as_str())) | c),
        _ => false,
    }
}

/// "saldo PDA" → "saldo dealer": solo la palabra entera "PDA" (no "UPDATE").
fn palabra_pda_a_dealer(s: &str) -> String {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"\bPDA\b").unwrap()).replace_all(s, "dealer").into_owned()
}
