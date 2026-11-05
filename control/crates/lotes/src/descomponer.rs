//! Descomposición de un monto en los montos que ofrece el menú del operador, con la menor cantidad
//! de envíos (cada envío cobra comisión). Programación dinámica: funciona aunque los montos no sean
//! "canónicos" (con {1, 3, 4}, 6 = 3 + 3 y no 4 + 1 + 1).
//!
//! Todo en **centavos** (12 Bs = 1200; 0.50 Bs = 50): así una receta con `opciones: ["0.50", "1"]` descompone
//! 1.50 = 1 + 0.50 sin errores de redondeo.

/// Tope para no armar tablas enormes con un monto absurdo: 10 000 Bs, en centavos.
pub const MONTO_MAXIMO: u64 = 10_000 * 100;

fn mcd(a: u64, b: u64) -> u64 {
    if b == 0 { a } else { mcd(b, a % b) }
}

/// Montos (centavos) de mayor a menor, o `None` si no se puede formar `monto` (centavos) con `permitidos`.
pub fn descomponer(monto: u64, permitidos: &[u64]) -> Option<Vec<u64>> {
    let mut ps: Vec<u64> = permitidos.iter().copied().filter(|p| *p > 0).collect();
    ps.sort_unstable();
    ps.dedup();
    if monto == 0 || monto > MONTO_MAXIMO || ps.is_empty() {
        return None;
    }
    // Se trabaja en unidades del máximo común divisor: con montos enteros (múltiplos de 100) la tabla es la de
    // siempre, en Bs, y no 100 veces más grande.
    let g = ps.iter().fold(monto, |g, p| mcd(g, *p));
    let unidades: Vec<u64> = ps.iter().map(|p| p / g).collect();
    let m = (monto / g) as usize;
    // mejor[v] = (cantidad mínima de envíos para v, último monto usado)
    let mut mejor: Vec<Option<(u32, u64)>> = vec![None; m + 1];
    mejor[0] = Some((0, 0));
    for v in 1..=m {
        // Recorre de mayor a menor: ante empate prefiere el monto grande.
        for &p in unidades.iter().rev() {
            let p_us = p as usize;
            if p_us > v {
                continue;
            }
            if let Some((c, _)) = mejor[v - p_us] {
                if mejor[v].map(|(cv, _)| c + 1 < cv).unwrap_or(true) {
                    mejor[v] = Some((c + 1, p));
                }
            }
        }
    }
    mejor[m]?;
    let mut out = vec![];
    let mut v = m;
    while v > 0 {
        let (_, p) = mejor[v]?;
        out.push(p * g);
        v -= p as usize;
    }
    out.sort_unstable_by(|a, b| b.cmp(a));
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bs enteros → centavos.
    fn c(xs: &[u64]) -> Vec<u64> {
        xs.iter().map(|x| x * 100).collect()
    }

    const TIGO: [u64; 5] = [100, 200, 500, 1000, 2000];

    #[test]
    fn tigo() {
        assert_eq!(descomponer(1200, &TIGO), Some(c(&[10, 2])));
        assert_eq!(descomponer(3700, &TIGO), Some(c(&[20, 10, 5, 2])));
        assert_eq!(descomponer(300, &TIGO), Some(c(&[2, 1])));
        assert_eq!(descomponer(2000, &TIGO), Some(c(&[20])));
        assert_eq!(descomponer(10_000, &TIGO), Some(c(&[20; 5])));
        assert_eq!(descomponer(1250, &TIGO), None, "con centavos no se forma con montos enteros");
    }

    #[test]
    fn con_centavos() {
        // Entel dealer: solo 0.50.
        assert_eq!(descomponer(50, &[50]), Some(vec![50]));
        assert_eq!(descomponer(150, &[50]), Some(vec![50, 50, 50]));
        assert_eq!(descomponer(120, &[50]), None);
        assert_eq!(descomponer(250, &[50, 100]), Some(vec![100, 100, 50]));
        assert_eq!(descomponer(30, &[20, 50]), None);
        assert_eq!(descomponer(90, &[20, 50]), Some(vec![50, 20, 20]));
    }

    #[test]
    fn no_canonicos_y_imposibles() {
        assert_eq!(descomponer(600, &c(&[1, 3, 4])), Some(c(&[3, 3])));
        assert_eq!(descomponer(700, &c(&[5, 10])), None);
        assert_eq!(descomponer(0, &TIGO), None);
        assert_eq!(descomponer(MONTO_MAXIMO + 1, &TIGO), None);
        assert_eq!(descomponer(500, &[]), None);
    }
}
