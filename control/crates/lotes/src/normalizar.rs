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

/// `12`, `12.00`, `12,00`, `Bs 12`, `12 Bs` → 12; `1.50`, `1,5`, `.50` → 1.5 / 0.5. Hasta 2 decimales. Error si no
/// es un número positivo. Que la receta acepte centavos se revisa aparte. Se lee en centavos enteros
/// ([`nucleo::monto::centavos`]): el f64 que sale es exacto al centavo.
pub fn monto(s: &str) -> Result<f64, String> {
    nucleo::monto::centavos(s).map(|c| c as f64 / 100.0)
}

/// Bs como texto para una receta y para mostrar: `5`, `1.50`.
pub fn bs(m: f64) -> String {
    nucleo::monto::texto(nucleo::monto::de_bs(m))
}

pub fn es_entero(m: f64) -> bool {
    (m - m.round()).abs() < 1e-9
}

/// Redondea a centavos.
pub fn centavos(m: f64) -> f64 {
    // `+ 0.0`: una suma vacía de f64 da -0.0, que saldría como "-0.0" en el JSON.
    (m * 100.0).round() / 100.0 + 0.0
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
        assert_eq!(monto("12"), Ok(12.0));
        assert_eq!(monto("12.00"), Ok(12.0));
        assert_eq!(monto("12,00"), Ok(12.0));
        assert_eq!(monto("Bs 12"), Ok(12.0));
        assert_eq!(monto("12 Bs"), Ok(12.0));
        assert_eq!(monto("Bs. 1.000,00"), Ok(1000.0));
        assert!(monto("1,000").unwrap_err().contains("ambiguo"));
        assert!(monto("1.000").unwrap_err().contains("ambiguo"));
        assert_eq!(monto("12,50"), Ok(12.5));
        assert_eq!(monto("1.5"), Ok(1.5));
        assert!(monto("0.00").is_err());
        assert_eq!(bs(1.5), "1.50");
        assert_eq!(bs(5.0), "5");
        assert!(monto("doce").unwrap_err().contains("inválido"));
        assert!(monto("0").is_err());
        assert!(monto("-5").is_err());
        assert!(monto("").unwrap_err().contains("falta"));
        assert_eq!(monto(".50"), Ok(0.5));
        assert_eq!(monto("Bs. 0,50"), Ok(0.5));
        assert_eq!(bs(0.5), "0.50");
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
