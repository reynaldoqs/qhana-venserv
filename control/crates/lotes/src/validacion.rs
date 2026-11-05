//! Validación de un CSV contra las recetas y plan de reparto entre chips. No toca ningún teléfono.

use crate::csv::{self, Columna};
use crate::descomponer::descomponer;
use crate::modelo::*;
use crate::normalizar;
use crate::persistencia::NuevaFila;
use nucleo::{monto, Receta};
use regex::Regex;
use std::collections::{BTreeMap, HashMap};

/// Monto por fila a partir del cual se avisa.
pub const MONTO_ALTO: f64 = 100.0;

/// Lo que la validación necesita de una receta.
#[derive(Debug, Clone)]
pub struct InfoReceta {
    pub nombre: String,
    pub patron_numero: Option<Regex>,
    pub patron_texto: Option<String>,
    /// Montos permitidos en **centavos** (`parametros[monto].opciones`, en Bs: "0.50" → 50); `None` = cualquier
    /// monto en un solo envío.
    pub montos: Option<Vec<u64>>,
    /// `parametros[monto].patron`, si la receta acepta montos libres (p. ej. con centavos). Se aplica al monto
    /// canónico (`1.50`).
    pub patron_monto: Option<Regex>,
    pub comision: f64,
}

impl InfoReceta {
    pub fn de(r: &Receta) -> Self {
        let param = |n: &str| r.parametros.iter().find(|p| p.nombre == n);
        let patron_texto = param("numero").and_then(|p| p.patron.clone());
        let patron_numero = patron_texto.as_ref().and_then(|p| Regex::new(&format!("^(?:{p})$")).ok());
        let montos = param("monto").and_then(|p| p.opciones.as_ref()).map(|ops| {
            let mut v: Vec<u64> = ops.iter().filter_map(|o| monto::centavos(o).ok()).collect();
            v.sort_unstable();
            v.dedup();
            v
        });
        let patron_monto =
            param("monto").and_then(|p| p.patron.as_ref()).and_then(|p| Regex::new(&format!("^(?:{p})$")).ok());
        let comision = r.limites.as_ref().and_then(|l| l.comision).unwrap_or(0.0);
        Self { nombre: r.nombre.clone(), patron_numero, patron_texto, montos, patron_monto, comision }
    }
}

/// Receta de un lote para (operador, acción) cuando la fila no viene de un chip elegido: la de la compañía y, si
/// no hay, una `chip_tipo: dealer` (entonces solo los chips dealer pueden hacer esas filas).
pub fn receta_de_lote<'a>(recetas: &'a BTreeMap<String, Receta>, operador: &str, accion: &str) -> Option<&'a Receta> {
    let de = |dealer: bool| {
        recetas.values().find(|r| {
            r.chip_tipo.is_some() == dealer
                && r.accion.as_deref().map(normalizar::accion).as_deref() == Some(accion)
                && r.operador.as_deref().map(normalizar::operador_de_red).as_deref() == Some(operador)
        })
    };
    de(false).or_else(|| de(true))
}

// ───────────────────────────── elección de chip ─────────────────────────────

/// Un chip listo para recibir envíos de una acción (se descuenta cupo y saldo a medida que se asignan envíos).
/// Un mismo chip puede aparecer una vez por acción (transferir con su saldo principal, recargar con su cupo).
#[derive(Debug, Clone)]
pub struct ChipReparto {
    pub info: ChipInfo,
    /// Acción que hace este chip en el reparto (`transferir`, `recargar`…).
    pub accion: String,
    /// La receta que correría este chip para esa acción (su variante dealer si corresponde).
    pub receta: Option<String>,
    /// Saldo que gasta esa acción: el principal al transferir; `None` (no se usa) en las demás.
    pub saldo: Option<f64>,
    pub cupo: u32,
    /// La receta no tiene `limites.por_dia`.
    pub sin_limite: bool,
    pub usados: u32,
}

impl ChipReparto {
    /// Para transferir: cupo y saldo principal del chip.
    pub fn de(c: &ChipInfo) -> Self {
        Self {
            info: c.clone(),
            accion: "transferir".into(),
            receta: None,
            saldo: c.saldo,
            cupo: c.cupo_hoy,
            sin_limite: false,
            usados: c.enviados_hoy,
        }
    }
    /// Descuenta un envío asignado.
    pub fn consumir(&mut self, monto: f64, comision: f64) {
        self.cupo = self.cupo.saturating_sub(1);
        self.usados += 1;
        if let Some(s) = &mut self.saldo {
            *s = ((*s - monto - comision).max(0.0) * 100.0).round() / 100.0;
        }
    }
}

/// Un chip que no puede hacer una acción del lote, con el motivo (p. ej. "sin PIN guardado").
#[derive(Debug, Clone)]
pub struct Excluido {
    pub info: ChipInfo,
    pub accion: String,
    pub motivo: String,
}

/// (operador, acción, receta de las filas): lo que un lote necesita de los chips.
#[derive(Debug, Clone)]
pub struct Par {
    pub operador: String,
    pub accion: String,
    pub receta: Receta,
}

/// Los chips con los que se reparte un lote.
#[derive(Debug, Clone, Default)]
pub struct Reparto {
    pub chips: Vec<ChipReparto>,
    pub excluidos: Vec<Excluido>,
}

impl Reparto {
    /// Como [`elegir_chip`], pero si ningún chip puede hacer esa acción en ese operador, el motivo dice por qué
    /// quedaron fuera (p. ej. "sin PIN guardado").
    pub fn elegir(
        &self,
        operador: &str,
        accion: &str,
        destino: &str,
        monto: f64,
        comision: f64,
        filtro: impl Fn(&ChipReparto) -> bool,
    ) -> Result<usize, String> {
        let r = elegir_chip(&self.chips, operador, accion, destino, monto, comision, filtro);
        if r.is_err() && !self.chips.iter().any(|c| c.info.operador == operador && c.accion == accion) {
            let mut motivos: Vec<String> = self
                .excluidos
                .iter()
                .filter(|e| e.info.operador == operador && e.accion == accion)
                .map(|e| format!("{} chip {}: {}", e.info.numero.as_deref().unwrap_or(&e.info.dispositivo), e.info.slot + 1, e.motivo))
                .collect();
            motivos.dedup();
            if !motivos.is_empty() {
                return Err(format!("ningún chip de {operador} puede {accion} ({})", motivos.join("; ")));
            }
        }
        r
    }
}

/// ¿El chip es la misma línea que `destino`? Compara los últimos 8 dígitos (ignora +591).
pub fn es_mismo_numero(c: &ChipReparto, destino: &str) -> bool {
    let d: String = destino.chars().filter(|x| x.is_ascii_digit()).collect();
    let n: String = c.info.numero.as_deref().unwrap_or("").chars().filter(|x| x.is_ascii_digit()).collect();
    d.len() >= 8 && n.len() >= 8 && n[n.len() - 8..] == d[d.len() - 8..]
}

/// Elige el mejor chip para un envío: del operador y la acción, con cupo, con saldo para monto + comisión (o saldo
/// desconocido, o una acción que no gasta el saldo principal). Prefiere saldo conocido, después más saldo, después
/// menos envíos hoy. `filtro` permite excluir chips (p. ej. teléfonos ocupados). Error = motivo.
pub fn elegir_chip(
    chips: &[ChipReparto],
    operador: &str,
    accion: &str,
    destino: &str,
    monto: f64,
    comision: f64,
    filtro: impl Fn(&ChipReparto) -> bool,
) -> Result<usize, String> {
    let todos_op: Vec<usize> =
        (0..chips.len()).filter(|i| chips[*i].info.operador == operador && chips[*i].accion == accion).collect();
    if todos_op.is_empty() {
        return Err(format!("no hay chips de {operador} en línea"));
    }
    // Un chip nunca se transfiere a sí mismo.
    let del_op: Vec<usize> = todos_op.into_iter().filter(|i| !es_mismo_numero(&chips[*i], destino)).collect();
    if del_op.is_empty() {
        return Err(format!("el único chip de {operador} en línea es el mismo número de destino"));
    }
    // Un chip recién cambiado no tiene saldo verificado: no se le asigna nada hasta consultarlo.
    let del_op: Vec<usize> = del_op.into_iter().filter(|i| !chips[*i].info.sin_verificar).collect();
    if del_op.is_empty() {
        return Err(format!("los chips de {operador} son nuevos y no tienen saldo verificado: consulta su saldo"));
    }
    let necesario = monto + comision;
    let alcanza = |c: &ChipReparto| c.saldo.map(|s| s + 1e-9 >= necesario).unwrap_or(true);
    let mut cand: Vec<usize> = del_op.iter().copied().filter(|i| chips[*i].cupo > 0 && alcanza(&chips[*i]) && filtro(&chips[*i])).collect();
    if cand.is_empty() {
        if del_op.iter().all(|i| chips[*i].cupo == 0) {
            return Err(format!("los chips de {operador} no tienen cupo hoy"));
        }
        if !del_op.iter().any(|i| chips[*i].cupo > 0 && alcanza(&chips[*i])) {
            return Err(format!(
                "saldo insuficiente en los chips de {operador} para {} Bs + {comision:.2} de comisión",
                normalizar::bs(monto)
            ));
        }
        return Err(format!("los chips de {operador} con capacidad están ocupados"));
    }
    cand.sort_by(|a, b| {
        let (x, y) = (&chips[*a], &chips[*b]);
        y.saldo
            .is_some()
            .cmp(&x.saldo.is_some())
            .then(y.saldo.unwrap_or(0.0).total_cmp(&x.saldo.unwrap_or(0.0)))
            .then(x.usados.cmp(&y.usados))
            .then(x.info.dispositivo.cmp(&y.info.dispositivo))
            .then(x.info.slot.cmp(&y.info.slot))
    });
    Ok(cand[0])
}

// ───────────────────────────── validación ─────────────────────────────

/// Resultado interno: la respuesta para la API y las filas listas para guardar como lote.
pub struct Validado {
    pub validacion: Validacion,
    pub filas: Vec<NuevaFila>,
}

/// Lote desde chips elegidos (o de un chip): la acción y la receta con que se valida cada operador.
pub struct Fijo {
    pub accion: String,
    /// Operador → receta de las filas (la de la compañía o, si no hay, la dealer; en un lote de un chip, la elegida).
    pub recetas: BTreeMap<String, Receta>,
    /// Operadores cuyos chips elegidos no sirven → motivo (para el error de sus filas).
    pub sin_chip: BTreeMap<String, String>,
    /// Origen viejo de un chip: los mensajes hablan de "el chip elegido".
    pub un_chip: bool,
}

/// Valida `texto` (CSV). `recetas`: todas las recetas cargadas. `reparto`: arma los chips para el plan según lo
/// que necesitan las filas válidas. Con `fijo`, el lote sale de los chips elegidos (ver [`OrigenLote`]).
pub fn validar(
    texto: &str,
    recetas: &BTreeMap<String, Receta>,
    reparto: impl FnOnce(&[Par]) -> (Reparto, Vec<ChipInfo>),
    fijo: Option<&Fijo>,
) -> Result<Validado, String> {
    let varios_operadores = fijo.is_some_and(|f| f.recetas.len() + f.sin_chip.len() > 1);
    let csv = csv::leer(texto, fijo.is_none() || varios_operadores)?;
    let unico_op = fijo.filter(|f| f.recetas.len() == 1 && f.sin_chip.is_empty()).and_then(|f| f.recetas.keys().next().cloned());

    let mut filas: Vec<FilaValidada> = vec![];
    let mut comisiones: Vec<f64> = vec![];
    let mut recetas_fila: Vec<Option<Receta>> = vec![];
    for f in &csv.filas {
        let crudo = |c: Columna| f.valores.get(&c).cloned().unwrap_or_default();
        let mut original = BTreeMap::new();
        for (c, v) in &f.valores {
            original.insert(c.nombre().to_string(), v.clone());
        }
        let mut errores: Vec<String> = f.error.iter().cloned().collect();
        let numero_txt = normalizar::numero(&crudo(Columna::Numero));
        let numero = if numero_txt.is_empty() {
            errores.push("falta el número".to_string());
            None
        } else {
            Some(numero_txt)
        };
        let monto = match normalizar::monto(&crudo(Columna::Monto)) {
            Ok(m) => Some(m),
            Err(e) => {
                errores.push(e);
                None
            }
        };
        let operador = match (fijo, normalizar::operador(&crudo(Columna::Operador))) {
            (None, Ok(o)) => Some(o),
            (None, Err(e)) => {
                errores.push(e);
                None
            }
            (Some(fj), Ok(o)) if fj.recetas.contains_key(&o) => Some(o),
            // La fila es de un operador sin chip elegido (o cuyo chip no sirve).
            (Some(fj), Ok(o)) => {
                errores.push(match (fj.un_chip, fj.sin_chip.get(&o)) {
                    (true, _) => format!("la fila es de {o} y el chip elegido es de {}", unico_op.as_deref().unwrap_or("otro operador")),
                    (false, Some(m)) => format!("los chips elegidos de {o} no sirven: {m}"),
                    (false, None) => format!("ninguno de los chips elegidos es {o}"),
                });
                None
            }
            // Sin operador (o uno que no se entiende): si los chips son de un solo operador, es ese.
            (Some(fj), Err(e)) => match &unico_op {
                Some(o) if fj.un_chip || crudo(Columna::Operador).trim().is_empty() => Some(o.clone()),
                _ if crudo(Columna::Operador).trim().is_empty() => {
                    errores.push("falta el operador (los chips elegidos son de varios operadores)".into());
                    None
                }
                _ => {
                    errores.push(e);
                    None
                }
            },
        };
        let accion = match fijo {
            Some(fj) => fj.accion.clone(),
            None => normalizar::accion(&crudo(Columna::Accion)),
        };
        let referencia = Some(crudo(Columna::Referencia)).filter(|r| !r.is_empty());

        // Formato básico del número aunque no haya receta (8 dígitos).
        if let Some(n) = &numero {
            if !(n.len() == 8 && n.chars().all(|c| c.is_ascii_digit())) {
                errores.push(format!("número inválido '{}': debe tener 8 dígitos", crudo(Columna::Numero)));
            }
        }

        let mut receta_nombre = None;
        let mut receta_usada = None;
        let mut envios = vec![];
        let mut comision = 0.0;
        if let Some(op) = &operador {
            let receta = match fijo {
                Some(fj) => fj.recetas.get(op),
                None => receta_de_lote(recetas, op, &accion),
            };
            match receta {
                None => errores.push(format!("no hay receta de {accion} para {op}")),
                Some(r) => {
                    let info = InfoReceta::de(r);
                    receta_nombre = Some(info.nombre.clone());
                    receta_usada = Some(r.clone());
                    comision = info.comision;
                    if let (Some(n), Some(re)) = (&numero, &info.patron_numero) {
                        if errores.iter().all(|e| !e.starts_with("número inválido")) && !re.is_match(n) {
                            errores.push(format!(
                                "el número {n} no es válido para {op} (formato {})",
                                info.patron_texto.as_deref().unwrap_or("?")
                            ));
                        }
                    }
                    if let Some(m) = monto {
                        match montos_de_envios(m, &info, op) {
                            Ok(v) => envios = v,
                            Err(e) => errores.push(e.replace("{crudo}", &crudo(Columna::Monto))),
                        }
                    }
                }
            }
        }
        let mut avisos = vec![];
        if let Some(m) = monto {
            if m > MONTO_ALTO {
                avisos.push(format!("monto alto ({} Bs)", normalizar::bs(m)));
            }
        }
        if !errores.is_empty() {
            envios.clear();
        }
        filas.push(FilaValidada {
            fila: f.linea,
            numero,
            monto,
            operador,
            accion,
            referencia,
            receta: receta_nombre,
            estado: EstadoValidacion::Valida,
            errores,
            avisos,
            envios,
            plan: vec![],
            sin_capacidad: 0,
            motivo_sin_capacidad: None,
            original,
        });
        comisiones.push(comision);
        recetas_fila.push(receta_usada);
    }

    // Números repetidos.
    let mut por_numero: HashMap<String, Vec<u32>> = HashMap::new();
    for f in &filas {
        if let Some(n) = &f.numero {
            por_numero.entry(n.clone()).or_default().push(f.fila);
        }
    }
    for f in &mut filas {
        if let Some(lineas) = f.numero.as_ref().and_then(|n| por_numero.get(n)) {
            if lineas.len() > 1 {
                let otras: Vec<String> = lineas.iter().filter(|l| **l != f.fila).map(|l| l.to_string()).collect();
                f.avisos.push(format!("número repetido (también en la fila {})", otras.join(", ")));
            }
        }
    }

    // Plan de reparto: los chips que sirven para lo que piden las filas válidas.
    let pares = pares_de(filas.iter().zip(recetas_fila.iter()).filter(|(f, _)| f.errores.is_empty()).filter_map(|(f, r)| {
        Some((f.operador.clone()?, f.accion.clone(), r.clone()?))
    }));
    let (mut reparto, chips) = reparto(&pares);
    // Cupo de cada chip por acción antes de asignar envíos (para el resumen y `chips`).
    let inicial = reparto.chips.clone();
    for (f, com) in filas.iter_mut().zip(comisiones.iter()) {
        if !f.errores.is_empty() {
            continue;
        }
        if let Some(n) = f.numero.as_deref() {
            if reparto.chips.iter().any(|c| c.info.operador == f.operador.clone().unwrap_or_default() && es_mismo_numero(c, n)) {
                f.avisos.push("el destino es uno de nuestros chips: no saldrá de ese mismo chip".into());
            }
        }
        let op = f.operador.clone().unwrap_or_default();
        for m in f.envios.clone() {
            match reparto.elegir(&op, &f.accion, f.numero.as_deref().unwrap_or(""), m, *com, |_| true) {
                Ok(i) => {
                    reparto.chips[i].consumir(m, *com);
                    f.plan.push(EnvioPlan { monto: m, dispositivo: reparto.chips[i].info.dispositivo.clone(), slot: reparto.chips[i].info.slot });
                }
                Err(motivo) => {
                    f.sin_capacidad += 1;
                    f.motivo_sin_capacidad.get_or_insert(motivo);
                }
            }
        }
        if f.sin_capacidad > 0 {
            f.avisos.push(format!(
                "{} de {} envío(s) no entran hoy: {}",
                f.sin_capacidad,
                f.envios.len(),
                f.motivo_sin_capacidad.as_deref().unwrap_or("sin capacidad")
            ));
        }
    }

    for f in &mut filas {
        f.estado = if !f.errores.is_empty() {
            EstadoValidacion::Invalida
        } else if !f.avisos.is_empty() {
            EstadoValidacion::Aviso
        } else {
            EstadoValidacion::Valida
        };
    }

    // Resumen.
    let mut r = ResumenValidacion {
        filas: filas.len() as u32,
        validas: 0,
        con_avisos: 0,
        invalidas: 0,
        total_bs: 0.0,
        envios: 0,
        comision_total: 0.0,
        planificados: 0,
        sin_capacidad: 0,
        operadores: vec![],
        avisos: vec![],
    };
    let mut ops: BTreeMap<(String, String), ResumenOperador> = BTreeMap::new();
    for (f, com) in filas.iter().zip(comisiones.iter()) {
        match f.estado {
            EstadoValidacion::Invalida => {
                r.invalidas += 1;
                continue;
            }
            EstadoValidacion::Aviso => {
                r.validas += 1;
                r.con_avisos += 1;
            }
            EstadoValidacion::Valida => r.validas += 1,
        }
        let m = f.monto.unwrap_or(0.0);
        r.total_bs += m;
        r.envios += f.envios.len() as u32;
        r.comision_total += com * f.envios.len() as f64;
        r.planificados += f.plan.len() as u32;
        r.sin_capacidad += f.sin_capacidad;
        let op = f.operador.clone().unwrap_or_default();
        let e = ops
            .entry((op.clone(), f.accion.clone()))
            .or_insert_with(|| resumen_operador(&op, &f.accion, &chips, &inicial));
        e.envios += f.envios.len() as u32;
        e.necesario_bs += m + com * f.envios.len() as f64;
    }
    r.comision_total = (r.comision_total * 100.0).round() / 100.0;
    r.total_bs = normalizar::centavos(r.total_bs);
    for e in ops.values_mut() {
        e.necesario_bs = (e.necesario_bs * 100.0).round() / 100.0;
    }
    r.operadores = ops.into_values().collect();

    let nuevas = filas
        .iter()
        .zip(comisiones.iter())
        .map(|(f, com)| NuevaFila {
            fila: f.fila,
            numero: f.numero.clone(),
            monto: f.monto,
            operador: f.operador.clone(),
            accion: f.accion.clone(),
            referencia: f.referencia.clone(),
            receta: f.receta.clone(),
            comision: *com,
            estado: if f.estado == EstadoValidacion::Invalida { EstadoFila::Omitida } else { EstadoFila::Pendiente },
            detalle: if f.errores.is_empty() { None } else { Some(f.errores.join("; ")) },
            original: f.original.clone(),
            envios: if f.estado == EstadoValidacion::Invalida { vec![] } else { f.envios.clone() },
        })
        .collect();

    Ok(Validado {
        validacion: Validacion { filas, resumen: r, chips: chips_por_accion(&chips, &inicial), origen: None, avisos_origen: vec![] },
        filas: nuevas,
    })
}

/// Montos (Bs) de los envíos de una fila, calculados en centavos: 12 → [10, 2]; 1.50 con montos libres → [1.50].
/// El error lleva `{crudo}` donde va el monto tal como vino.
pub fn montos_de_envios(m: f64, info: &InfoReceta, op: &str) -> Result<Vec<f64>, String> {
    let cent = monto::de_bs(m);
    let bs = |c: u64| c as f64 / 100.0;
    match &info.montos {
        None => match &info.patron_monto {
            Some(re) if !re.is_match(&monto::texto(cent)) => {
                Err(format!("el monto {} Bs no es válido para la receta {}", monto::texto(cent), info.nombre))
            }
            _ => Ok(vec![bs(cent)]),
        },
        // Montos fijos enteros (1, 2, 5…): un monto con centavos no se puede formar.
        Some(ps) if !cent.is_multiple_of(100) && ps.iter().all(|p| p.is_multiple_of(100)) => {
            Err(format!("monto con centavos '{{crudo}}': {} solo acepta montos enteros en Bs", info.nombre))
        }
        Some(ps) => match descomponer(cent, ps) {
            Some(v) => Ok(v.into_iter().map(bs).collect()),
            None => Err(format!(
                "no se puede formar {} Bs con los montos de {op} ({})",
                monto::texto(cent),
                ps.iter().map(|p| monto::texto(*p)).collect::<Vec<_>>().join(", ")
            )),
        },
    }
}

/// (operador, acción, receta) distintos, en orden.
pub fn pares_de(it: impl Iterator<Item = (String, String, Receta)>) -> Vec<Par> {
    let mut v: Vec<Par> = vec![];
    for (operador, accion, receta) in it {
        if !v.iter().any(|p| p.operador == operador && p.accion == accion && p.receta.nombre == receta.nombre) {
            v.push(Par { operador, accion, receta });
        }
    }
    v
}

/// Avisos de lote por recetas sin probar: las que usarían las filas válidas (con la variante dealer si el plan usa un
/// chip dealer) cuya versión actual nunca terminó `ok` ni `simulado`. `probada(receta)` lo consulta en el historial.
pub fn avisos_recetas(v: &Validacion, recetas: &BTreeMap<String, Receta>, probada: impl Fn(&Receta) -> bool) -> Vec<String> {
    let mut usadas: Vec<String> = vec![];
    let mut sumar = |n: &str| {
        if !usadas.iter().any(|u| u == n) {
            usadas.push(n.to_string());
        }
    };
    for f in v.filas.iter().filter(|f| f.estado != EstadoValidacion::Invalida) {
        let Some(nombre) = &f.receta else { continue };
        sumar(nombre);
        let Some(receta) = nucleo::receta::buscar(recetas, nombre) else { continue };
        for e in &f.plan {
            let tipo = v.chips.iter().find(|c| c.dispositivo == e.dispositivo && c.slot == e.slot).map(|c| c.tipo.as_str());
            if let Some(t) = tipo {
                if let Ok(efectiva) = crate::contexto::receta_efectiva_en(recetas, receta, t) {
                    sumar(&efectiva.nombre);
                }
            }
        }
    }
    usadas
        .iter()
        .filter_map(|n| nucleo::receta::buscar(recetas, n))
        .filter(|r| r.version.is_some() && !probada(r))
        .map(|r| format!("la receta {} cambió y no se ha probado", r.nombre))
        .collect()
}

/// Resumen de las filas (`op`, `accion`). Con `transferir`: los chips en línea del operador, su cupo de transferir y
/// su saldo principal (como siempre). Con otra acción: los chips que pueden hacerla (`inicial`, del reparto) con el
/// cupo de la receta que correrían, sin saldo principal (esas filas no lo gastan).
fn resumen_operador(op: &str, accion: &str, chips: &[ChipInfo], inicial: &[ChipReparto]) -> ResumenOperador {
    if accion == "transferir" {
        let del_op: Vec<&ChipInfo> = chips.iter().filter(|c| c.operador == op).collect();
        return ResumenOperador {
            operador: op.to_string(),
            accion: accion.to_string(),
            chips: del_op.len() as u32,
            cupo_hoy: del_op.iter().filter(|c| !c.sin_limite).map(|c| c.cupo_hoy).sum(),
            cupo_sin_limite: del_op.iter().any(|c| c.sin_limite),
            saldo_conocido: (del_op.iter().filter_map(|c| c.saldo).sum::<f64>() * 100.0).round() / 100.0,
            saldo_desconocido_chips: del_op.iter().filter(|c| c.saldo.is_none()).count() as u32,
            necesario_bs: 0.0,
            envios: 0,
        };
    }
    let del_op: Vec<&ChipReparto> = inicial.iter().filter(|c| c.info.operador == op && c.accion == accion).collect();
    ResumenOperador {
        operador: op.to_string(),
        accion: accion.to_string(),
        chips: del_op.len() as u32,
        cupo_hoy: del_op.iter().filter(|c| !c.sin_limite).map(|c| c.cupo).sum(),
        cupo_sin_limite: del_op.iter().any(|c| c.sin_limite),
        saldo_conocido: 0.0,
        saldo_desconocido_chips: 0,
        necesario_bs: 0.0,
        envios: 0,
    }
}

/// `Validacion.chips`: cada chip una vez por acción del lote que hace (con los números de esa acción), o una vez
/// sin `accion` (números de transferir) si no hace ninguna. Nunca con el cupo "infinito" interno.
fn chips_por_accion(chips: &[ChipInfo], inicial: &[ChipReparto]) -> Vec<ChipInfo> {
    let mut v = vec![];
    for c in chips {
        let mismas: Vec<&ChipReparto> =
            inicial.iter().filter(|x| x.info.dispositivo == c.dispositivo && x.info.slot == c.slot).collect();
        if mismas.is_empty() {
            v.push(c.clone().para_api());
            continue;
        }
        for x in mismas {
            let mut i = if x.accion == "transferir" { c.clone() } else { x.info.clone() };
            i.accion = Some(x.accion.clone());
            i.enviados_hoy = x.usados;
            i.cupo_hoy = x.cupo;
            i.sin_limite = x.sin_limite;
            v.push(i.para_api());
        }
    }
    v
}

#[cfg(test)]
mod prueba_mismo_numero {
    use super::*;

    fn chip(slot: u32, numero: &str, saldo: f64) -> ChipReparto {
        ChipReparto::de(&ChipInfo {
            dispositivo: "tel".into(),
            slot,
            operador: "TIGO".into(),
            numero: Some(numero.into()),
            saldo: Some(saldo),
            saldo_ms: None,
            enviados_hoy: 0,
            cupo_hoy: 5,
            agotado_hoy: false,
            sin_verificar: false,
            tipo: "normal".into(),
            tiene_pin: false,
            saldo_dealer: None,
            saldo_dealer_ms: None,
            sin_limite: false,
            accion: None,
        })
    }

    #[test]
    fn nunca_elige_el_mismo_chip_como_origen() {
        // El chip 0 tiene más saldo, pero es el destino: debe salir del chip 1.
        let chips = vec![chip(0, "+59178815232", 50.0), chip(1, "+59169759675", 10.0)];
        assert_eq!(elegir_chip(&chips, "TIGO", "transferir", "78815232", 5.0, 0.2, |_| true), Ok(1));
        // Si el único chip es el destino, no hay origen posible.
        let solo = vec![chip(0, "+59178815232", 50.0)];
        let e = elegir_chip(&solo, "TIGO", "transferir", "78815232", 5.0, 0.2, |_| true).unwrap_err();
        assert!(e.contains("mismo número"), "{e}");
        // Otro destino: elige normal (más saldo).
        assert_eq!(elegir_chip(&chips, "TIGO", "transferir", "69869869", 5.0, 0.2, |_| true), Ok(0));
        // Otra acción: ningún chip la hace.
        assert!(elegir_chip(&chips, "TIGO", "recargar", "69869869", 5.0, 0.0, |_| true).is_err());
    }
}
