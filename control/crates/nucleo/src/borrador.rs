//! Borrador de receta a partir de una sesión de la consola USSD (`POST /api/recetas/desde-sesion`). No guarda nada:
//! devuelve un YAML para que alguien lo revise y lo complete en el editor.
//!
//! - El primer paso es `marcar` con el código (y cualquier otro código USSD más adelante también).
//! - Una respuesta que es el número de una opción del menú anterior pasa a `elegir: '<texto de la opción>'`; si la
//!   opción es un monto (`10 Bs`), a `elegir: '{monto}\s*Bs'` con los montos del menú como `opciones`.
//! - Un teléfono (8 dígitos que empiezan con 6 o 7) pasa a `{numero}`; un monto, a `{monto}`; `{pin}` queda como
//!   está y se declara secreto. Nada se marca como que mueve dinero.
//! - Cada paso lleva un `esperar` sugerido: la primera línea de la pantalla que llegó (no una opción del menú).

use crate::menu;
use regex::Regex;
use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Debug, Clone, Deserialize)]
pub struct PasoSesion {
    pub enviado: String,
    #[serde(default)]
    pub texto: String,
}

/// Largo máximo de la frase sugerida para `esperar`.
const LARGO_ESPERAR: usize = 40;

fn re(cell: &'static OnceLock<Regex>, patron: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(patron).unwrap())
}

fn es_codigo(s: &str) -> bool {
    static R: OnceLock<Regex> = OnceLock::new();
    re(&R, r"^[*#][0-9*#]+#$").is_match(s)
}

fn es_telefono(s: &str) -> bool {
    static R: OnceLock<Regex> = OnceLock::new();
    re(&R, r"^[67]\d{7}$").is_match(s)
}

fn es_monto(s: &str) -> bool {
    static R: OnceLock<Regex> = OnceLock::new();
    re(&R, r"^\d{1,5}(?:[.,]\d{1,2})?$").is_match(s)
}

/// Opción de menú que es un monto: `10 Bs`, `5Bs.`, `20`. Devuelve (monto, lleva "Bs").
fn opcion_monto(t: &str) -> Option<(String, bool)> {
    static R: OnceLock<Regex> = OnceLock::new();
    let c = re(&R, r"(?i)^(\d{1,5}(?:[.,]\d{1,2})?)\s*(bs\.?)?$").captures(t.trim())?;
    Some((c[1].to_string(), c.get(2).is_some()))
}

/// Escapa lo que es especial en una regex, dejando el texto legible (espacios, guiones y `#` quedan igual).
pub fn escapar(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        if "\\.+*?()|[]{}^$".contains(ch) {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// Texto entre comillas simples de YAML.
fn yaml(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// Frase corta de la pantalla para `esperar`: la primera línea que no es una opción, recortada en una palabra.
/// Un teléfono que se envió en la sesión se cambia por `{numero}`.
fn frase(texto: &str, telefonos: &[String]) -> Option<String> {
    let opciones: Vec<String> = menu::opciones(texto).into_iter().map(|o| o.texto).collect();
    let linea = texto
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .find(|l| !opciones.iter().any(|o| l.ends_with(o.as_str()) && l.len() <= o.len() + 5))
        .or_else(|| opciones.first().map(String::as_str))?;
    let mut corta: String = linea.to_string();
    if corta.chars().count() > LARGO_ESPERAR {
        let cortada: String = corta.chars().take(LARGO_ESPERAR).collect();
        corta = match cortada.rfind(' ') {
            Some(i) if i >= 10 => cortada[..i].to_string(),
            _ => cortada,
        };
    }
    let corta = corta.trim_end_matches([' ', ':', ',', '.']).to_string();
    if corta.is_empty() {
        return None;
    }
    // Escapar no toca los dígitos: el teléfono se puede cambiar después.
    let mut s = escapar(&corta);
    for t in telefonos {
        s = s.replace(t.as_str(), "{numero}");
    }
    Some(s)
}

/// Arma el borrador. Error si no hay pasos o el primero no es un código USSD.
pub fn desde_sesion(operador: &str, pasos: &[PasoSesion]) -> Result<String, String> {
    let primero = pasos.first().ok_or("la sesión no tiene pasos")?;
    if !es_codigo(primero.enviado.trim()) {
        return Err(format!("el primer paso debe ser el código USSD marcado (p. ej. *105#), no '{}'", primero.enviado.trim()));
    }
    let operador = operador.trim().to_uppercase();
    let base: String = operador
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_lowercase() || c.is_ascii_digit() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|x| !x.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let nombre = if base.is_empty() { "receta-nueva".to_string() } else { format!("{base}-nueva") };

    let telefonos: Vec<String> =
        pasos.iter().skip(1).map(|p| p.enviado.trim().to_string()).filter(|e| es_telefono(e)).collect();
    let mut usa_numero = false;
    let mut usa_pin = false;
    // Monto: `Some(opciones)` si salió de un menú; `Some(vec![])` si se escribió.
    let mut monto: Option<Vec<String>> = None;
    let mut lineas: Vec<String> = vec![];
    let mut anterior = "";
    for (i, p) in pasos.iter().enumerate() {
        let e = p.enviado.trim();
        let accion = if i == 0 || es_codigo(e) {
            format!("marcar: {}", yaml(e))
        } else if e == "{pin}" {
            usa_pin = true;
            "enviar: '{pin}'".to_string()
        } else if let Some(op) = menu::opciones(anterior).into_iter().find(|o| o.numero == e) {
            match opcion_monto(&op.texto) {
                Some((_, bs)) => {
                    let del_menu: Vec<String> =
                        menu::opciones(anterior).iter().filter_map(|o| opcion_monto(&o.texto)).map(|(m, _)| m).collect();
                    let m = monto.get_or_insert_with(Vec::new);
                    for x in del_menu {
                        if !m.contains(&x) {
                            m.push(x);
                        }
                    }
                    if bs { r"elegir: '{monto}\s*Bs'".to_string() } else { "elegir: '{monto}'".to_string() }
                }
                None => format!("elegir: {}", yaml(&escapar(&op.texto))),
            }
        } else if es_telefono(e) {
            usa_numero = true;
            "enviar: '{numero}'".to_string()
        } else if es_monto(e) {
            monto.get_or_insert_with(Vec::new);
            "enviar: '{monto}'".to_string()
        } else {
            format!("enviar: {}", yaml(e))
        };
        lineas.push(format!("  - {accion}"));
        if let Some(f) = frase(&p.texto, &telefonos) {
            lineas.push(format!("    esperar: {}", yaml(&f)));
        }
        anterior = &p.texto;
    }

    let mut parametros: Vec<String> = vec![];
    if usa_numero {
        parametros.push(r"  - { nombre: numero, patron: '^[67]\d{7}$' }".to_string());
    }
    match &monto {
        Some(ops) if !ops.is_empty() => {
            let ops: Vec<String> = ops.iter().map(|o| format!("\"{o}\"")).collect();
            parametros.push(format!("  - {{ nombre: monto, opciones: [{}] }}", ops.join(", ")));
        }
        Some(_) => parametros.push(r"  - { nombre: monto, patron: '^\d+(\.\d{1,2})?$' }".to_string()),
        None => {}
    }
    if usa_pin {
        parametros.push("  - { nombre: pin, secreto: true }".to_string());
    }

    let mut y = String::new();
    y.push_str(&format!("nombre: {nombre}\n"));
    y.push_str("titulo: ''\n");
    let simple = !operador.is_empty() && operador.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    y.push_str(&format!("operador: {}\n", if simple { operador.clone() } else { yaml(&operador) }));
    y.push_str("descripcion: ''\n");
    y.push_str("modo: pantalla\n");
    if parametros.is_empty() {
        y.push_str("parametros: []\n");
    } else {
        y.push_str("parametros:\n");
        for p in parametros {
            y.push_str(&p);
            y.push('\n');
        }
    }
    y.push_str("pasos:\n");
    for l in lineas {
        y.push_str(&l);
        y.push('\n');
    }
    y.push_str("salida: {}\n");
    Ok(y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::receta::Receta;

    fn paso(e: &str, t: &str) -> PasoSesion {
        PasoSesion { enviado: e.into(), texto: t.into() }
    }

    #[test]
    fn sesion_tigo_con_menu_telefono_y_monto() {
        let pasos = vec![
            paso("*66#1#", "Bienvenido al servicio de transferencias\n1.-Transferir por unica vez\n2.-Ultimos movimientos"),
            paso("1", "Ingrese el numero de destino"),
            paso("69869869", "Elija el monto\n1.-1 Bs\n2.-2 Bs\n3.-5 Bs\n4.-10 Bs\n5.-20 Bs"),
            paso("4", "Confirma transferir por unica vez 10.00 Bs a 69869869 con un costo de 0.20?\n1.-Si\n2.-No"),
            paso("2", "Operacion cancelada (Tx: 1.5)"),
        ];
        let y = desde_sesion(" tigo ", &pasos).unwrap();
        let esperado = r#"nombre: tigo-nueva
titulo: ''
operador: TIGO
descripcion: ''
modo: pantalla
parametros:
  - { nombre: numero, patron: '^[67]\d{7}$' }
  - { nombre: monto, opciones: ["1", "2", "5", "10", "20"] }
pasos:
  - marcar: '*66#1#'
    esperar: 'Bienvenido al servicio de transferencias'
  - elegir: 'Transferir por unica vez'
    esperar: 'Ingrese el numero de destino'
  - enviar: '{numero}'
    esperar: 'Elija el monto'
  - elegir: '{monto}\s*Bs'
    esperar: 'Confirma transferir por unica vez 10\.00'
  - elegir: 'No'
    esperar: 'Operacion cancelada \(Tx: 1\.5\)'
salida: {}
"#;
        assert_eq!(y, esperado);
        // El borrador es una receta válida, y nada en él mueve dinero.
        let r = Receta::desde_yaml(&y).unwrap();
        assert_eq!(r.paso_que_mueve_dinero(), None);
        assert_eq!(r.pasos.len(), 5);
    }

    #[test]
    fn pin_monto_escrito_texto_libre_y_telefono_en_pantalla() {
        let pasos = vec![
            paso("*555#", "Billetera\n1. Pagar\n2. Enviar dinero"),
            paso("2", "Numero de destino:"),
            paso("71234567", "Monto a enviar:"),
            paso("15.50", "Enviar 15.50 a 71234567? Ingrese su PIN"),
            paso("{pin}", "Listo, enviaste a 71234567"),
            paso("hola 'mundo'", ""),
        ];
        let y = desde_sesion("Viva", &pasos).unwrap();
        assert!(y.contains("nombre: viva-nueva\n"));
        assert!(y.contains("  - elegir: 'Enviar dinero'\n"));
        assert!(y.contains("  - enviar: '{numero}'\n"));
        assert!(y.contains("  - enviar: '{monto}'\n"));
        assert!(y.contains("  - enviar: '{pin}'\n"));
        assert!(y.contains("  - { nombre: pin, secreto: true }\n"));
        assert!(y.contains(r"  - { nombre: monto, patron: '^\d+(\.\d{1,2})?$' }"));
        assert!(y.contains("    esperar: 'Listo, enviaste a {numero}'\n"), "{y}");
        assert!(y.contains("  - enviar: 'hola ''mundo'''\n"));
        assert!(!y.contains("mueve_dinero"));
        let r = Receta::desde_yaml(&y).unwrap();
        assert_eq!(r.pasos[5].enviar.as_deref(), Some("hola 'mundo'"));
        assert!(r.parametros.iter().any(|p| p.nombre == "pin" && p.secreto));
    }

    #[test]
    fn frase_larga_se_corta_en_una_palabra() {
        let f = frase("Estimado cliente su saldo principal es de 10 Bs y vence pronto", &[]).unwrap();
        assert!(f.len() <= LARGO_ESPERAR && !f.ends_with(' '), "{f}");
        assert_eq!(f, "Estimado cliente su saldo principal es");
    }

    #[test]
    fn primer_paso_debe_ser_codigo() {
        assert!(desde_sesion("TIGO", &[]).is_err());
        assert!(desde_sesion("TIGO", &[paso("70000000", "")]).is_err());
    }
}
