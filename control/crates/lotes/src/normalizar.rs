//! Limpieza de los valores del CSV: número, monto, operador y acción.

/// Quita tildes y diéresis comunes en español (á→a, ñ→n, ü→u…).
pub fn sin_acentos(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ä' => 'a',
            'Á' | 'À' | 'Â' | 'Ä' => 'A',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'É' | 'È' | 'Ê' | 'Ë' => 'E',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'Í' | 'Ì' | 'Î' | 'Ï' => 'I',
            'ó' | 'ò' | 'ô' | 'ö' => 'o',
            'Ó' | 'Ò' | 'Ô' | 'Ö' => 'O',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'Ú' | 'Ù' | 'Û' | 'Ü' => 'U',
            'ñ' => 'n',
            'Ñ' => 'N',
            otro => otro,
        })
        .collect()
}

/// `+591 7881-5232` → `78815232`. No valida el formato: eso lo hace la receta.
pub fn numero(s: &str) -> String {
    let mut n: String = s.chars().filter(|c| !matches!(c, ' ' | '-' | '.' | '(' | ')' | '\u{a0}')).collect();
    if let Some(r) = n.strip_prefix('+') {
        n = r.to_string();
    }
    if n.len() == 11 && n.starts_with("591") {
        n = n[3..].to_string();
    }
    n
}

/// `12`, `12.00`, `12,00`, `Bs 12`, `12 Bs` → 12. Error si tiene centavos o no es un número positivo.
pub fn monto(s: &str) -> Result<u64, String> {
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
            // "1.000" / "1,000" puede ser mil o uno: no se adivina (un error aquí mueve dinero de más).
            let frac = &t[d + 1..];
            if frac.len() == 3 {
                return Err(format!("monto ambiguo '{original}': escribe solo el entero (por ejemplo 1000 o 1)"));
            }
            (t[..d].to_string(), frac.to_string())
        }
        (None, None) => (t.clone(), String::new()),
    };
    if entera.is_empty() || !entera.chars().all(|c| c.is_ascii_digit()) || !decimal.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!("monto inválido '{original}'"));
    }
    if decimal.chars().any(|c| c != '0') {
        return Err(format!("monto con centavos '{original}': solo se aceptan montos enteros en Bs"));
    }
    let v: u64 = entera.parse().map_err(|_| format!("monto inválido '{original}'"))?;
    if v == 0 {
        return Err("el monto debe ser mayor que 0".into());
    }
    Ok(v)
}

/// `tigo` → `TIGO`, `Nuevatel` → `VIVA`. Error si no es un operador conocido.
pub fn operador(s: &str) -> Result<String, String> {
    let t = sin_acentos(s.trim()).to_uppercase();
    let t: String = t.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    match t.as_str() {
        "" => Err("falta el operador".into()),
        "TIGO" => Ok("TIGO".into()),
        "ENTEL" => Ok("ENTEL".into()),
        "VIVA" | "NUEVATEL" | "NUEVATELPCS" => Ok("VIVA".into()),
        _ => Err(format!("operador desconocido '{}' (TIGO, ENTEL o VIVA)", s.trim())),
    }
}

/// Nombre de red de un chip (`TIGO`, `Tigo 4G`, `NUEVATEL`) → operador normalizado si se reconoce.
pub fn operador_de_red(red: &str) -> String {
    let t = sin_acentos(red).to_uppercase();
    for (clave, op) in [("TIGO", "TIGO"), ("ENTEL", "ENTEL"), ("VIVA", "VIVA"), ("NUEVATEL", "VIVA")] {
        if t.contains(clave) {
            return op.into();
        }
    }
    red.trim().to_uppercase()
}

/// Operador de un chip: por el nombre de red si lo reconoce y, si no, por el código de red (MCC+MNC de Bolivia: 73601 Viva,
/// 73602 Entel, 73603 Tigo). Las marcas que corren sobre una red ajena ("FutbolSinLimites" sobre Entel) no llevan el
/// nombre del operador, pero sí su código.
pub fn operador_de_sim(nombre: &str, mcc_mnc: &str) -> String {
    let por_nombre = operador_de_red(nombre);
    if matches!(por_nombre.as_str(), "TIGO" | "ENTEL" | "VIVA") {
        return por_nombre;
    }
    match mcc_mnc.trim() {
        "73601" => "VIVA".into(),
        "73602" => "ENTEL".into(),
        "73603" => "TIGO".into(),
        _ => por_nombre,
    }
}

/// `Transferir` → `transferir`; vacío → `transferir`.
pub fn accion(s: &str) -> String {
    let t = sin_acentos(s.trim()).to_lowercase();
    if t.is_empty() { "transferir".into() } else { t }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeros() {
        assert_eq!(numero("+591 7881-5232"), "78815232");
        assert_eq!(numero("591 69869869"), "69869869");
        assert_eq!(numero("69.86.98.69"), "69869869");
        assert_eq!(numero("(6) 986-9869"), "69869869");
        assert_eq!(numero("12345"), "12345");
    }

    #[test]
    fn montos() {
        assert_eq!(monto("12"), Ok(12));
        assert_eq!(monto("12.00"), Ok(12));
        assert_eq!(monto("12,00"), Ok(12));
        assert_eq!(monto("Bs 12"), Ok(12));
        assert_eq!(monto("12 Bs"), Ok(12));
        assert_eq!(monto("Bs. 1.000,00"), Ok(1000));
        assert!(monto("1,000").unwrap_err().contains("ambiguo"));
        assert!(monto("1.000").unwrap_err().contains("ambiguo"));
        assert!(monto("12,50").unwrap_err().contains("centavos"));
        assert!(monto("doce").unwrap_err().contains("inválido"));
        assert!(monto("0").is_err());
        assert!(monto("-5").is_err());
        assert!(monto("").unwrap_err().contains("falta"));
    }

    #[test]
    fn operadores() {
        assert_eq!(operador(" tigo "), Ok("TIGO".into()));
        assert_eq!(operador("Nuevatel"), Ok("VIVA".into()));
        assert_eq!(operador("Éntel"), Ok("ENTEL".into()));
        assert!(operador("claro").is_err());
        assert_eq!(operador_de_red("Tigo 4G"), "TIGO");
        assert_eq!(operador_de_red("NUEVATEL"), "VIVA");
        assert_eq!(operador_de_sim("FutbolSinLimites", "73602"), "ENTEL");
        assert_eq!(operador_de_sim("Tigo 4G", "73602"), "TIGO"); // el nombre manda si se reconoce
        assert_eq!(operador_de_sim("Claro", "72405"), "CLARO"); // red desconocida: queda como vino
        assert_eq!(accion(""), "transferir");
        assert_eq!(accion("Transferir"), "transferir");
    }
}
