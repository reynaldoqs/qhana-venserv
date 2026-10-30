//! Descomposición de un monto en los montos que ofrece el menú del operador, con la menor cantidad
//! de envíos (cada envío cobra comisión). Programación dinámica: funciona aunque los montos no sean
//! "canónicos" (con {1, 3, 4}, 6 = 3 + 3 y no 4 + 1 + 1).

/// Tope para no armar tablas enormes con un monto absurdo.
pub const MONTO_MAXIMO: u64 = 10_000;

/// Montos de mayor a menor, o `None` si no se puede formar `monto` con `permitidos`.
pub fn descomponer(monto: u64, permitidos: &[u64]) -> Option<Vec<u64>> {
    let mut ps: Vec<u64> = permitidos.iter().copied().filter(|p| *p > 0).collect();
    ps.sort_unstable();
    ps.dedup();
    if monto == 0 || monto > MONTO_MAXIMO || ps.is_empty() {
        return None;
    }
    let m = monto as usize;
    // mejor[v] = (cantidad mínima de envíos para v, último monto usado)
    let mut mejor: Vec<Option<(u32, u64)>> = vec![None; m + 1];
    mejor[0] = Some((0, 0));
    for v in 1..=m {
        // Recorre de mayor a menor: ante empate prefiere el monto grande.
        for &p in ps.iter().rev() {
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
        out.push(p);
        v -= p as usize;
    }
    out.sort_unstable_by(|a, b| b.cmp(a));
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TIGO: [u64; 5] = [1, 2, 5, 10, 20];

    #[test]
    fn tigo() {
        assert_eq!(descomponer(12, &TIGO), Some(vec![10, 2]));
        assert_eq!(descomponer(37, &TIGO), Some(vec![20, 10, 5, 2]));
        assert_eq!(descomponer(3, &TIGO), Some(vec![2, 1]));
        assert_eq!(descomponer(20, &TIGO), Some(vec![20]));
        assert_eq!(descomponer(100, &TIGO), Some(vec![20; 5]));
    }

    #[test]
    fn no_canonicos_y_imposibles() {
        assert_eq!(descomponer(6, &[1, 3, 4]), Some(vec![3, 3]));
        assert_eq!(descomponer(7, &[5, 10]), None);
        assert_eq!(descomponer(0, &TIGO), None);
        assert_eq!(descomponer(MONTO_MAXIMO + 1, &TIGO), None);
        assert_eq!(descomponer(5, &[]), None);
    }
}
