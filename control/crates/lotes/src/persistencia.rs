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
    pub monto: Option<u64>,
    pub operador: Option<String>,
    pub accion: String,
    pub referencia: Option<String>,
    pub receta: Option<String>,
    pub comision: f64,
    /// `omitida` (inválida) o `pendiente`.
    pub estado: EstadoFila,
    pub detalle: Option<String>,
    pub original: BTreeMap<String, String>,
    pub envios: Vec<u64>,
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
                    saldo       REAL NOT NULL,
                    ts_ms       INTEGER NOT NULL,
                    PRIMARY KEY (lote_id, dispositivo, slot)
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
            // Bases anteriores a los chips PDA: `chips` no tiene `tipo`.
            let tiene_tipo = c
                .prepare("SELECT 1 FROM pragma_table_info('chips') WHERE name = 'tipo'")?
                .exists([])?;
            if !tiene_tipo {
                c.execute("ALTER TABLE chips ADD COLUMN tipo TEXT NOT NULL DEFAULT 'normal'", [])?;
            }
            Ok(())
        })?;
        Ok(Self { a })
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

    /// Tipo (`normal` | `pda`) de cada chip que ocupa una ranura.
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

    pub fn crear_lote(&self, nombre: &str, simular: bool, creado_ms: i64, filas: &[NuevaFila]) -> R<i64> {
        self.a.con(|c| {
            let tx = c.transaction()?;
            tx.execute(
                "INSERT INTO lotes (nombre, creado_ms, estado, simular) VALUES (?1, ?2, 'listo', ?3)",
                params![nombre, creado_ms, simular],
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
                        f.monto.map(|m| m as i64),
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
                        params![fila_id, lote, n as i64 + 1, *m as i64],
                    )?;
                }
            }
            tx.commit()?;
            Ok(lote)
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

    /// Lote con sus filas y envíos (o `None`).
    pub fn lote(&self, id: i64) -> R<Option<(Lote, BTreeMap<i64, f64>)>> {
        self.a.con(|c| {
            let cab = c
                .query_row(
                    "SELECT id, nombre, creado_ms, estado, simular, motivo_pausa FROM lotes WHERE id = ?1",
                    [id],
                    |r| {
                        Ok(Lote {
                            id: r.get(0)?,
                            nombre: r.get(1)?,
                            creado_ms: r.get(2)?,
                            estado: EstadoLote::desde(&r.get::<_, String>(3)?),
                            simular: r.get(4)?,
                            motivo_pausa: r.get(5)?,
                            revision: Revision::default(),
                            verificacion: None,
                            progreso: Progreso::default(),
                            filas: None,
                        })
                    },
                )
                .optional()?;
            let Some(mut lote) = cab else { return Ok(None) };

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
                                monto: r.get::<_, Option<i64>>(3)?.map(|m| m as u64),
                                operador: r.get(4)?,
                                accion: r.get(5)?,
                                referencia: r.get(6)?,
                                receta: r.get(7)?,
                                estado: EstadoFila::desde(&r.get::<_, String>(8)?),
                                enviado_bs: 0,
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
                let mut st = c.prepare(
                    "SELECT id, fila_id, n, monto, dispositivo, slot, numero_origen, estado, ejecucion_id, detalle, intentos, ts_ms
                     FROM lote_envios WHERE lote_id = ?1 ORDER BY fila_id, n",
                )?;
                let v = st.query_map([id], |r| {
                    Ok((
                        r.get::<_, i64>(1)?,
                        EnvioLote {
                            id: r.get(0)?,
                            n: r.get(2)?,
                            monto: r.get::<_, i64>(3)? as u64,
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
                })?;
                for x in v {
                    let (fila_id, e) = x?;
                    envios.entry(fila_id).or_default().push(e);
                }
            }
            for f in &mut filas {
                f.envios = envios.remove(&f.id).unwrap_or_default();
                f.enviado_bs = f.envios.iter().filter(|e| e.estado == EstadoEnvio::Ok).map(|e| e.monto).sum();
            }
            lote.progreso = progreso(&filas, &comisiones);
            lote.revision = Revision::de(lote.estado, &lote.progreso);
            lote.verificacion = c
                .query_row("SELECT json FROM lote_verificacion WHERE lote_id = ?1", [id], |r| r.get::<_, String>(0))
                .optional()?
                .and_then(|j| serde_json::from_str(&j).ok());
            lote.filas = Some(filas);
            Ok(Some((lote, comisiones)))
        })
    }

    /// Reemplaza el saldo inicial de los chips de un lote (se guarda en cada inicio o reanudación).
    pub fn guardar_saldos_iniciales(&self, lote_id: i64, saldos: &[SaldoChip], ts_ms: i64) -> R<()> {
        self.a.con(|c| {
            c.execute("DELETE FROM lote_saldo_inicial WHERE lote_id = ?1", [lote_id])?;
            for s in saldos {
                c.execute(
                    "INSERT INTO lote_saldo_inicial (lote_id, dispositivo, slot, saldo, ts_ms) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![lote_id, s.dispositivo, s.slot, s.saldo, ts_ms],
                )?;
            }
            Ok(())
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

    /// (dispositivo, slot, saldo, ts_ms) guardados al último inicio del lote.
    pub fn saldos_iniciales(&self, lote_id: i64) -> R<Vec<(String, u32, f64, i64)>> {
        self.a.con(|c| {
            let mut st =
                c.prepare("SELECT dispositivo, slot, saldo, ts_ms FROM lote_saldo_inicial WHERE lote_id = ?1 ORDER BY dispositivo, slot")?;
            let v = st
                .query_map([lote_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
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

    pub fn ids_en_estado(&self, estado: EstadoLote) -> R<Vec<i64>> {
        self.a.con(|c| {
            let mut st = c.prepare("SELECT id FROM lotes WHERE estado = ?1")?;
            let v = st.query_map([estado.texto()], |r| r.get(0))?.collect::<R<Vec<i64>>>()?;
            Ok(v)
        })
    }
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
        p.total_bs += f.monto.unwrap_or(0);
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
