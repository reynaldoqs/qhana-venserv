//! GSM de 7 bits en hex: el formato que el Huawei exige para el USSD (en texto plano responde
//! `+CME ERROR: UNKNOWN`) y en el que devuelve las respuestas, aunque diga DCS 8 o 15.
//! Solo la tabla básica (ASCII); alcanza para los menús de Tigo, Entel y Viva.

/// `*105#` → `AA18AC3602`
pub fn empaquetar(s: &str) -> String {
    let (mut bits, mut n) = (0u32, 0u32);
    let mut out = Vec::new();
    for b in s.bytes() {
        bits |= u32::from(b & 0x7F) << n;
        n += 7;
        while n >= 8 {
            out.push((bits & 0xFF) as u8);
            bits >>= 8;
            n -= 8;
        }
    }
    if n > 0 {
        out.push((bits & 0xFF) as u8);
    }
    out.iter().map(|b| format!("{b:02X}")).collect()
}

fn de_hex(h: &str) -> Option<Vec<u8>> {
    if !h.len().is_multiple_of(2) || !h.is_ascii() {
        return None;
    }
    (0..h.len()).step_by(2).map(|i| u8::from_str_radix(&h[i..i + 2], 16).ok()).collect()
}

/// Inverso de [`empaquetar`]. Quita el relleno final (`\r`, `\0`, `@`).
pub fn desempaquetar(hex: &str) -> Option<String> {
    let (mut bits, mut n) = (0u32, 0u32);
    let mut out = String::new();
    for x in de_hex(hex)? {
        bits |= u32::from(x) << n;
        n += 8;
        while n >= 7 {
            out.push(char::from((bits & 0x7F) as u8));
            bits >>= 7;
            n -= 7;
        }
    }
    Some(out.trim_end_matches(['\r', '\0', '@']).to_string())
}

/// UCS2 (DCS 72) es UTF-16 BE en hex; cualquier otro DCS viene empaquetado en 7 bits.
pub fn decodificar(hex: &str, dcs: u32) -> String {
    if dcs == 72 {
        let Some(b) = de_hex(hex) else { return String::new() };
        let u: Vec<u16> = b.chunks_exact(2).map(|p| u16::from_be_bytes([p[0], p[1]])).collect();
        return String::from_utf16_lossy(&u);
    }
    desempaquetar(hex).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codigo_conocido() {
        // Medido con el E303 real: *105# → AA18AC3602
        assert_eq!(empaquetar("*105#"), "AA18AC3602");
        assert_eq!(desempaquetar("AA18AC3602").as_deref(), Some("*105#"));
    }

    #[test]
    fn ida_y_vuelta() {
        for s in ["*66#1#", "69759675", "1", "Saldo Bs 10.00\nvence 12/11", "1234567"] {
            assert_eq!(desempaquetar(&empaquetar(s)).as_deref(), Some(s), "{s}");
        }
    }

    #[test]
    fn largo_multiplo_de_ocho_no_agrega_arroba() {
        // 8 caracteres caben exacto en 7 bytes: no hay relleno.
        assert_eq!(desempaquetar(&empaquetar("69759675")).as_deref(), Some("69759675"));
    }

    #[test]
    fn ucs2() {
        assert_eq!(decodificar("00530061006C0064006F", 72), "Saldo");
    }

    #[test]
    fn hex_invalido() {
        assert_eq!(desempaquetar("ZZ"), None);
        assert_eq!(desempaquetar("ABC"), None);
    }
}
