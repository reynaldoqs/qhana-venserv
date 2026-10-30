//! Validación de un CSV contra las recetas y plan de reparto entre chips. No toca ningún teléfono.

use crate::csv::{self, Columna};
use crate::descomponer::descomponer;
use crate::modelo::*;
use crate::normalizar;
use crate::persistencia::NuevaFila;
use nucleo::Receta;
use regex::Regex;
use std::collections::{BTreeMap, HashMap};

/// Monto por fila a partir del cual se avisa.
pub const MONTO_ALTO: u64 = 100;

/// Lo que la validación necesita de una receta.
#[derive(Debug, Clone)]
pub struct InfoReceta {
    pub nombre: String,
    pub patron_numero: Option<Regex>,
    pub patron_texto: Option<String>,
    /// Montos permitidos (`parametros[monto].opciones`); `None` = cualquier monto en un solo envío.
    pub montos: Option<Vec<u64>>,
    pub comision: f64,
}

impl InfoReceta {
    pub fn de(r: &Receta) -> Self {
        let param = |n: &str| r.parametros.iter().find(|p| p.nombre == n);
        let patron_texto = param("numero").and_then(|p| p.patron.clone());
        let patron_numero = patron_texto.as_ref().and_then(|p| Regex::new(&format!("^(?:{p})$")).ok());
        let montos = param("monto").and_then(|p| p.opciones.as_ref()).map(|ops| {
            let mut v: Vec<u64> = ops.iter().filter_map(|o| o.trim().parse::<u64>().ok()).collect();
            v.sort_unstable();
            v
        });
        let comision = r.limites.as_ref().and_then(|l| l.comision).unwrap_or(0.0);
        Self { nombre: r.nombre.clone(), patron_numero, patron_texto, montos, comision }
    }
}

// ───────────────────────────── elección de chip ─────────────────────────────

/// Estado de un chip mientras se reparte (se descuenta cupo y saldo a medida que se asignan envíos).
#[derive(Debug, Clone)]
pub struct ChipReparto {
    pub info: ChipInfo,
    pub saldo: Option<f64>,
    pub cupo: u32,
    pub usados: u32,
}

impl ChipReparto {
    pub fn de(c: &ChipInfo) -> Self {
        Self { info: c.clone(), saldo: c.saldo, cupo: c.cupo_hoy, usados: c.enviados_hoy }
    }
    /// Descuenta un envío asignado.
    pub fn consumir(&mut self, monto: u64, comision: f64) {
        self.cupo = self.cupo.saturating_sub(1);
        self.usados += 1;
        if let Some(s) = &mut self.saldo {
            *s = ((*s - monto as f64 - comision).max(0.0) * 100.0).round() / 100.0;
        }
    }
}

/// ¿El chip es la misma línea que `destino`? Compara los últimos 8 dígitos (ignora +591).
pub fn es_mismo_numero(c: &ChipReparto, destino: &str) -> bool {
    let d: String = destino.chars().filter(|x| x.is_ascii_digit()).collect();
    let n: String = c.info.numero.as_deref().unwrap_or("").chars().filter(|x| x.is_ascii_digit()).collect();
    d.len() >= 8 && n.len() >= 8 && n[n.len() - 8..] == d[d.len() - 8..]
}

/// Elige el mejor chip para un envío: del operador, con cupo, con saldo para monto + comisión (o saldo
/// desconocido). Prefiere saldo conocido, después más saldo, después menos envíos hoy.
/// `filtro` permite excluir chips (p. ej. teléfonos ocupados). Error = motivo.
pub fn elegir_chip(
    chips: &[ChipReparto],
    operador: &str,
    destino: &str,
    monto: u64,
    comision: f64,
    filtro: impl Fn(&ChipReparto) -> bool,
) -> Result<usize, String> {
    let todos_op: Vec<usize> = (0..chips.len()).filter(|i| chips[*i].info.operador == operador).collect();
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
    let necesario = monto as f64 + comision;
    let alcanza = |c: &ChipReparto| c.saldo.map(|s| s + 1e-9 >= necesario).unwrap_or(true);
    let mut cand: Vec<usize> = del_op.iter().copied().filter(|i| chips[*i].cupo > 0 && alcanza(&chips[*i]) && filtro(&chips[*i])).collect();
    if cand.is_empty() {
        if del_op.iter().all(|i| chips[*i].cupo == 0) {
            return Err(format!("los chips de {operador} no tienen cupo hoy"));
        }
        if !del_op.iter().any(|i| chips[*i].cupo > 0 && alcanza(&chips[*i])) {
            return Err(format!("saldo insuficiente en los chips de {operador} para {monto} Bs + {comision:.2} de comisión"));
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

/// Valida `texto` (CSV). `recetas`: todas las recetas cargadas. `chips`: chips en línea para el plan.
pub fn validar(texto: &str, recetas: &BTreeMap<String, Receta>, chips: Vec<ChipInfo>) -> Result<Validado, String> {
    let csv = csv::leer(texto)?;
    let por_clave = |op: &str, acc: &str| -> Option<&Receta> {
        recetas.values().find(|r| {
            r.chip_tipo.is_none()
                && r.accion.as_deref().map(normalizar::accion).as_deref() == Some(acc)
                && r.operador.as_deref().map(normalizar::operador_de_red).as_deref() == Some(op)
        })
    };

    let mut filas: Vec<FilaValidada> = vec![];
    let mut comisiones: Vec<f64> = vec![];
    for f in &csv.filas {
        let crudo = |c: Columna| f.valores.get(&c).cloned().unwrap_or_default();
        let mut original = BTreeMap::new();
        for (c, v) in &f.valores {
            original.insert(c.nombre().to_string(), v.clone());
        }
        let mut errores = vec![];
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
        let operador = match normalizar::operador(&crudo(Columna::Operador)) {
            Ok(o) => Some(o),
            Err(e) => {
                errores.push(e);
                None
            }
        };
        let accion = normalizar::accion(&crudo(Columna::Accion));
        let referencia = Some(crudo(Columna::Referencia)).filter(|r| !r.is_empty());

        // Formato básico del número aunque no haya receta (8 dígitos).
        if let Some(n) = &numero {
            if !(n.len() == 8 && n.chars().all(|c| c.is_ascii_digit())) {
                errores.push(format!("número inválido '{}': debe tener 8 dígitos", crudo(Columna::Numero)));
            }
        }

        let mut receta_nombre = None;
        let mut envios = vec![];
        let mut comision = 0.0;
        if let Some(op) = &operador {
            match por_clave(op, &accion) {
                None => errores.push(format!("no hay receta de {accion} para {op}")),
                Some(r) => {
                    let info = InfoReceta::de(r);
                    receta_nombre = Some(info.nombre.clone());
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
                        match &info.montos {
                            None => envios = vec![m],
                            Some(ps) => match descomponer(m, ps) {
                                Some(v) => envios = v,
                                None => errores.push(format!(
                                    "no se puede formar {m} Bs con los montos de {op} ({})",
                                    ps.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(", ")
                                )),
                            },
                        }
                    }
                }
            }
        }
        let mut avisos = vec![];
        if let Some(m) = monto {
            if m > MONTO_ALTO {
                avisos.push(format!("monto alto ({m} Bs)"));
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

    // Plan de reparto.
    let mut reparto: Vec<ChipReparto> = chips.iter().map(ChipReparto::de).collect();
    for (f, com) in filas.iter_mut().zip(comisiones.iter()) {
        if !f.errores.is_empty() {
            continue;
        }
        if let Some(n) = f.numero.as_deref() {
            if reparto.iter().any(|c| es_mismo_numero(c, n)) {
                f.avisos.push("el destino es uno de nuestros chips: no saldrá de ese mismo chip".into());
            }
        }
        let op = f.operador.clone().unwrap_or_default();
        for m in f.envios.clone() {
            match elegir_chip(&reparto, &op, f.numero.as_deref().unwrap_or(""), m, *com, |_| true) {
                Ok(i) => {
                    reparto[i].consumir(m, *com);
                    f.plan.push(EnvioPlan { monto: m, dispositivo: reparto[i].info.dispositivo.clone(), slot: reparto[i].info.slot });
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
        total_bs: 0,
        envios: 0,
        comision_total: 0.0,
        planificados: 0,
        sin_capacidad: 0,
        operadores: vec![],
    };
    let mut ops: BTreeMap<String, ResumenOperador> = BTreeMap::new();
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
        let m = f.monto.unwrap_or(0);
        r.total_bs += m;
        r.envios += f.envios.len() as u32;
        r.comision_total += com * f.envios.len() as f64;
        r.planificados += f.plan.len() as u32;
        r.sin_capacidad += f.sin_capacidad;
        let op = f.operador.clone().unwrap_or_default();
        let e = ops.entry(op.clone()).or_insert_with(|| resumen_operador(&op, &chips));
        e.envios += f.envios.len() as u32;
        e.necesario_bs += m as f64 + com * f.envios.len() as f64;
    }
    r.comision_total = (r.comision_total * 100.0).round() / 100.0;
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

    Ok(Validado { validacion: Validacion { filas, resumen: r, chips }, filas: nuevas })
}

fn resumen_operador(op: &str, chips: &[ChipInfo]) -> ResumenOperador {
    let del_op: Vec<&ChipInfo> = chips.iter().filter(|c| c.operador == op).collect();
    ResumenOperador {
        operador: op.to_string(),
        chips: del_op.len() as u32,
        cupo_hoy: del_op.iter().map(|c| c.cupo_hoy).sum(),
        saldo_conocido: (del_op.iter().filter_map(|c| c.saldo).sum::<f64>() * 100.0).round() / 100.0,
        saldo_desconocido_chips: del_op.iter().filter(|c| c.saldo.is_none()).count() as u32,
        necesario_bs: 0.0,
        envios: 0,
    }
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
        })
    }

    #[test]
    fn nunca_elige_el_mismo_chip_como_origen() {
        // El chip 0 tiene más saldo, pero es el destino: debe salir del chip 1.
        let chips = vec![chip(0, "+59178815232", 50.0), chip(1, "+59169759675", 10.0)];
        assert_eq!(elegir_chip(&chips, "TIGO", "78815232", 5, 0.2, |_| true), Ok(1));
        // Si el único chip es el destino, no hay origen posible.
        let solo = vec![chip(0, "+59178815232", 50.0)];
        let e = elegir_chip(&solo, "TIGO", "78815232", 5, 0.2, |_| true).unwrap_err();
        assert!(e.contains("mismo número"), "{e}");
        // Otro destino: elige normal (más saldo).
        assert_eq!(elegir_chip(&chips, "TIGO", "69869869", 5, 0.2, |_| true), Ok(0));
    }
}
