//! Lectura de menús USSD. Cada operador numera distinto:
//! Tigo `1.-Transferir`, Entel `1 Saldo para transferencia`, otros `1. X` o `1) X`.

use regex::Regex;
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq)]
pub struct Opcion {
    pub numero: String,
    pub texto: String,
}

fn re_opcion() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // número + (separador .- . ) - : con espacios opcionales | al menos un espacio) + texto
    RE.get_or_init(|| {
        Regex::new(r"(?m)^[ \t]*(\d{1,3})(?:[ \t]*(?:\.-|\.|\)|-|:)[ \t]*|[ \t]+)(\S.*?)[ \t\r]*$")
            .unwrap()
    })
}

/// Opciones numeradas del texto, en orden de aparición.
pub fn opciones(texto: &str) -> Vec<Opcion> {
    re_opcion()
        .captures_iter(texto)
        .map(|c| Opcion { numero: c[1].to_string(), texto: c[2].trim().to_string() })
        .collect()
}

/// Número de la primera opción cuyo texto coincide **completo** con `patron` (sin importar mayúsculas).
pub fn elegir(texto: &str, patron: &str) -> Result<Option<String>, regex::Error> {
    Ok(marcar_opciones(texto, patron)?.into_iter().find(|(_, si)| *si).map(|(o, _)| o.numero))
}

/// Las opciones del texto y si cada una coincide **completa** con `patron` (la regla de [`elegir`]).
pub fn marcar_opciones(texto: &str, patron: &str) -> Result<Vec<(Opcion, bool)>, regex::Error> {
    let re = Regex::new(&format!("(?i)^(?:{patron})$"))?;
    Ok(opciones(texto).into_iter().map(|o| {
        let si = re.is_match(&o.texto);
        (o, si)
    }).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tigo() {
        let t = "Bienvenido al servicio de transferencias\n\n1.-Transferir por unica vez\n2.-Ultimos movimientos\n";
        let o = opciones(t);
        assert_eq!(o.len(), 2);
        assert_eq!(o[0], Opcion { numero: "1".into(), texto: "Transferir por unica vez".into() });
        assert_eq!(elegir(t, "Ultimos movimientos").unwrap(), Some("2".into()));
    }

    #[test]
    fn montos_tigo() {
        let t = "Ingrese monto\n1.-1 Bs\n2.-2 Bs\n3.-5 Bs\n4.-10 Bs\n5.-20 Bs\n";
        assert_eq!(elegir(t, r"10\s*Bs").unwrap(), Some("4".into()));
        assert_eq!(elegir(t, r"1\s*Bs").unwrap(), Some("1".into()));
        assert_eq!(elegir(t, r"50\s*Bs").unwrap(), None);
    }

    #[test]
    fn entel_sin_separador_y_espacios_al_final() {
        let t = "Escoja una opcion\n1 Ultimas transferencias \n2 Total de transferencias";
        assert_eq!(elegir(t, "Ultimas transferencias").unwrap(), Some("1".into()));
        assert_eq!(elegir(t, "total de TRANSFERENCIAS").unwrap(), Some("2".into()));
    }

    #[test]
    fn otros_formatos() {
        assert_eq!(elegir("1. Si\n2. No", "No").unwrap(), Some("2".into()));
        assert_eq!(elegir("1) Si\n2) No", "Si").unwrap(), Some("1".into()));
        assert_eq!(elegir("1.-Si\r\n2.-No\r\n", "No").unwrap(), Some("2".into()));
    }

    #[test]
    fn coincidencia_completa_no_parcial() {
        // "Si" no debe coincidir con "Sin saldo"
        assert_eq!(elegir("1.-Sin saldo\n2.-Si", "Si").unwrap(), Some("2".into()));
    }
}
