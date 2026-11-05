//! Montos en Bs: formato canónico y valores calculados (contrato: "Montos: formato base y valores calculados").
//!
//! Todo se hace en **centavos enteros** (nunca con `f64`): `0.50` son 50 centavos y `{monto_cent3}` es `050`.

/// Valores calculados que el motor agrega cuando la receta declara `monto`. No se pueden declarar como parámetros.
pub const RESERVADOS: [&str; 4] = ["monto_cent", "monto_cent3", "monto_entero", "monto_mixto"];

/// Inicio del mensaje cuando un monto con centavos no se puede escribir en el código de la receta.
pub const SOLO_ENTEROS: &str = "esta receta solo acepta montos enteros";

/// `{monto_mixto}`: Bs enteros (`5`) o, por debajo de 1 Bs, centavos con 3 dígitos (`050`). Entel dealer lo marca
/// así (`*133*N*1#` = Bs 1). `None` para 1 Bs o más con centavos (`1.50`): no tiene forma en ese código.
fn mixto(c: u64) -> Option<String> {
    if c.is_multiple_of(100) {
        Some((c / 100).to_string())
    } else if c < 100 {
        Some(format!("{c:03}"))
    } else {
        None
    }
}

/// Error si la receta usa un valor calculado que este monto (centavos) no tiene. `usa(nombre)` dice si la receta
/// usa ese valor. Empieza con [`SOLO_ENTEROS`] (así se reconoce como `400 parametro`).
pub fn rechazo(c: u64, usa: impl Fn(&str) -> bool) -> Option<String> {
    let m = texto(c);
    if !c.is_multiple_of(100) && usa("monto_entero") {
        Some(format!("{SOLO_ENTEROS} (monto {m} Bs)"))
    } else if mixto(c).is_none() && usa("monto_mixto") {
        Some(format!("{SOLO_ENTEROS} o menores de 1 Bs (monto {m} Bs)"))
    } else {
        None
    }
}

/// Lee un monto en Bs y lo devuelve en centavos: `12`, `12.00`, `12,00`, `Bs 12`, `Bs. 0.50`, `0,5`, `.50`,
/// `1.000,00` (con separador de miles). Hasta 2 decimales. `1.000` o `1,000` (¿mil o uno?) es error: un error
/// aquí movería dinero de más.
pub fn centavos(s: &str) -> Result<u64, String> {
    let original = s.trim();
    if original.is_empty() {
        return Err("falta el monto".into());
    }
    let mut t = original.to_lowercase();
    for quitar in ["bob", "bs.", "bs", "$"] {
        t = t.replace(quitar, "");
    }
    let t: String = t.chars().filter(|c| !c.is_whitespace()).collect();
    // Separador decimal: el último de '.' o ','; el otro (si hay) es de miles.
    let (entera, decimal) = match (t.rfind('.'), t.rfind(',')) {
        (Some(p), Some(c)) => {
            let d = p.max(c);
            let miles = if d == p { ',' } else { '.' };
            (t[..d].replace(miles, ""), t[d + 1..].to_string())
        }
        (Some(d), None) | (None, Some(d)) => {
            let frac = &t[d + 1..];
            if frac.len() == 3 && !t[..d].is_empty() {
                return Err(format!("monto ambiguo '{original}': escribe solo el entero (por ejemplo 1000 o 1)"));
            }
            (t[..d].to_string(), frac.to_string())
        }
        (None, None) => (t.clone(), String::new()),
    };
    // `.50` es 0.50; un "." solo no es nada.
    if (entera.is_empty() && decimal.is_empty())
        || !entera.chars().all(|c| c.is_ascii_digit())
        || !decimal.chars().all(|c| c.is_ascii_digit())
    {
        return Err(format!("monto inválido '{original}'"));
    }
    let decimal = decimal.trim_end_matches('0');
    if decimal.len() > 2 {
        return Err(format!("monto inválido '{original}': máximo 2 decimales"));
    }
    let entera = entera.trim_start_matches('0');
    if entera.len() > 9 {
        return Err(format!("monto inválido '{original}': demasiado grande"));
    }
    let entera: u64 = if entera.is_empty() { 0 } else { entera.parse().map_err(|_| format!("monto inválido '{original}'"))? };
    let cent: u64 = format!("{decimal:0<2}").parse().unwrap_or(0);
    let total = entera * 100 + cent;
    if total == 0 {
        return Err("el monto debe ser mayor que 0".into());
    }
    Ok(total)
}

/// Formato canónico (`{monto}`): entero sin decimales (`5`) y, con centavos, con dos (`0.50`, `12.30`).
pub fn texto(c: u64) -> String {
    if c.is_multiple_of(100) { format!("{}", c / 100) } else { format!("{}.{:02}", c / 100, c % 100) }
}

/// Bs (f64, como los guarda un lote) → centavos, redondeando al centavo.
pub fn de_bs(m: f64) -> u64 {
    (m * 100.0).round().max(0.0) as u64
}

/// Valores calculados de un monto: (`monto_cent`, `monto_cent3`, `monto_entero` si no tiene centavos,
/// `monto_mixto` si tiene forma).
pub fn calculados(c: u64) -> Vec<(&'static str, String)> {
    let mut v = vec![("monto_cent", c.to_string()), ("monto_cent3", format!("{c:03}"))];
    if c.is_multiple_of(100) {
        v.push(("monto_entero", (c / 100).to_string()));
    }
    if let Some(m) = mixto(c) {
        v.push(("monto_mixto", m));
    }
    v
}

pub fn es_reservado(nombre: &str) -> bool {
    RESERVADOS.contains(&nombre)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabla_del_contrato() {
        // | Valor | Bs 0.50 | Bs 5 | Bs 12.30 |
        let fila = |s: &str| {
            let c = centavos(s).unwrap();
            let calc: std::collections::BTreeMap<_, _> = calculados(c).into_iter().collect();
            (texto(c), calc["monto_cent"].clone(), calc["monto_cent3"].clone(), calc.get("monto_entero").cloned())
        };
        assert_eq!(fila("0.50"), ("0.50".into(), "50".into(), "050".into(), None));
        assert_eq!(fila("5"), ("5".into(), "500".into(), "500".into(), Some("5".into())));
        assert_eq!(fila("12.30"), ("12.30".into(), "1230".into(), "1230".into(), None));
        let mixto = |s: &str| calculados(centavos(s).unwrap()).into_iter().find(|(k, _)| *k == "monto_mixto").map(|(_, v)| v);
        assert_eq!(mixto("0.50"), Some("050".into()));
        assert_eq!(mixto("1"), Some("1".into()), "Entel dealer: *133*N*1# es Bs 1");
        assert_eq!(mixto("100"), Some("100".into()));
        assert_eq!(mixto("12.30"), None);
    }

    #[test]
    fn rechazo_segun_el_valor_que_usa() {
        let usa = |n: &'static str| move |x: &str| x == n;
        assert!(rechazo(150, usa("monto_mixto")).unwrap().starts_with(SOLO_ENTEROS));
        assert_eq!(rechazo(50, usa("monto_mixto")), None);
        assert_eq!(rechazo(500, usa("monto_mixto")), None);
        assert!(rechazo(50, usa("monto_entero")).is_some());
        assert_eq!(rechazo(150, usa("monto_cent3")), None);
    }

    #[test]
    fn normaliza_lo_que_llegue() {
        for s in ["0.50", "0,5", "Bs 0.50", "Bs. 0.50", ".50", ",5", "0.5", "00.50"] {
            assert_eq!(centavos(s).map(texto), Ok("0.50".into()), "{s}");
        }
        assert_eq!(centavos("1.5").map(texto), Ok("1.50".into()));
        assert_eq!(centavos("12.00").map(texto), Ok("12".into()));
        assert_eq!(centavos("Bs. 1.000,00").map(texto), Ok("1000".into()));
        assert_eq!(centavos("0.01"), Ok(1));
        assert_eq!(centavos("050"), Ok(5000), "sin punto, 050 son 50 Bs: por eso las recetas usan {{monto_cent3}}");
        assert!(centavos("1.000").unwrap_err().contains("ambiguo"));
        assert!(centavos("1.5555").unwrap_err().contains("2 decimales"));
        assert!(centavos("0").is_err());
        assert!(centavos("-5").is_err());
        assert!(centavos(".").is_err());
        assert!(centavos("").unwrap_err().contains("falta"));
        assert!(centavos("99999999999").is_err());
    }

    #[test]
    fn redondeo_sin_sorpresas() {
        // 0.29 * 100 = 28.999999999999996 en f64: se redondea, no se trunca.
        assert_eq!(de_bs(0.29), 29);
        assert_eq!(de_bs(1.15), 115);
        assert_eq!(texto(de_bs(0.1 + 0.2)), "0.30");
    }
}
