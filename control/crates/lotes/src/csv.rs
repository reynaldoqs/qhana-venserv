//! Lector de CSV tolerante: separador `,` o `;`, BOM, comillas (con `""` y saltos de línea dentro),
//! encabezados con alias en español y archivos sin encabezado.

use crate::normalizar::sin_acentos;
use std::collections::BTreeMap;

pub const MAX_FILAS: usize = 5_000;

/// Columnas que entiende un lote.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Columna {
    Numero,
    Monto,
    Operador,
    Accion,
    Referencia,
}

impl Columna {
    pub fn nombre(self) -> &'static str {
        match self {
            Columna::Numero => "numero",
            Columna::Monto => "monto",
            Columna::Operador => "operador",
            Columna::Accion => "accion",
            Columna::Referencia => "referencia",
        }
    }

    fn por_encabezado(h: &str) -> Option<Self> {
        // "Monto (Bs)" → "monto bs"; "N° de teléfono" → "n de telefono".
        let h = sin_acentos(h).to_lowercase();
        let h = h.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ");
        Some(match h.as_str() {
            "numero" | "nro" | "n" | "telefono" | "celular" | "linea" | "destino" | "numero destino" | "numero de telefono"
            | "n de telefono" => Columna::Numero,
            "monto" | "cantidad" | "importe" | "bs" | "carga" | "monto bs" | "monto en bs" => Columna::Monto,
            "operador" | "operadora" | "empresa" | "compania" => Columna::Operador,
            "accion" | "operacion" => Columna::Accion,
            "referencia" | "ref" | "nota" | "id" => Columna::Referencia,
            _ => return None,
        })
    }
}

/// Una fila de datos del CSV, con su número de línea en el archivo (1 = primera línea).
#[derive(Debug, Clone, PartialEq)]
pub struct FilaCsv {
    pub linea: u32,
    pub valores: BTreeMap<Columna, String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Csv {
    pub separador: char,
    pub con_encabezado: bool,
    pub filas: Vec<FilaCsv>,
}

/// Elige `;` o `,` contando en la primera línea con datos (fuera de comillas).
fn detectar_separador(texto: &str) -> char {
    let primera = texto.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let (mut comas, mut pyc, mut dentro) = (0, 0, false);
    for c in primera.chars() {
        match c {
            '"' => dentro = !dentro,
            ',' if !dentro => comas += 1,
            ';' if !dentro => pyc += 1,
            _ => {}
        }
    }
    if pyc > comas { ';' } else { ',' }
}

/// Divide en registros (línea de inicio, campos). Respeta comillas con `""` y saltos de línea dentro.
fn registros(texto: &str, sep: char) -> Vec<(u32, Vec<String>)> {
    let mut out = vec![];
    let mut campos: Vec<String> = vec![];
    let mut campo = String::new();
    let mut dentro = false;
    let mut linea = 1u32;
    let mut inicio = 1u32;
    let mut chars = texto.chars().peekable();
    while let Some(c) = chars.next() {
        if dentro {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    campo.push('"');
                }
                '"' => dentro = false,
                '\n' => {
                    linea += 1;
                    campo.push('\n');
                }
                _ => campo.push(c),
            }
            continue;
        }
        match c {
            '"' if campo.trim().is_empty() => {
                campo.clear();
                dentro = true;
            }
            '\r' => {}
            '\n' => {
                campos.push(std::mem::take(&mut campo));
                out.push((inicio, std::mem::take(&mut campos)));
                linea += 1;
                inicio = linea;
            }
            c if c == sep => campos.push(std::mem::take(&mut campo)),
            _ => campo.push(c),
        }
    }
    if !campo.is_empty() || !campos.is_empty() {
        campos.push(campo);
        out.push((inicio, campos));
    }
    out.into_iter()
        .map(|(l, cs)| (l, cs.into_iter().map(|c| c.trim().to_string()).collect::<Vec<_>>()))
        .filter(|(_, cs)| cs.iter().any(|c| !c.is_empty()))
        .collect()
}

/// Lee el CSV. Error si está vacío, si faltan columnas obligatorias o si supera [`MAX_FILAS`].
pub fn leer(texto: &str) -> Result<Csv, String> {
    let texto = texto.trim_start_matches('\u{feff}');
    let sep = detectar_separador(texto);
    let regs = registros(texto, sep);
    let Some((_, primera)) = regs.first() else {
        return Err("el CSV está vacío".into());
    };

    let mapeo: Vec<Option<Columna>> = primera.iter().map(|h| Columna::por_encabezado(h)).collect();
    let con_encabezado = mapeo.iter().any(|m| m.is_some());
    let (mapeo, datos) = if con_encabezado {
        for req in [Columna::Numero, Columna::Monto, Columna::Operador] {
            if !mapeo.contains(&Some(req)) {
                return Err(format!(
                    "al CSV le falta la columna '{}' (encabezados: {})",
                    req.nombre(),
                    primera.join(", ")
                ));
            }
        }
        (mapeo, &regs[1..])
    } else {
        // Sin encabezado: numero, monto, operador, accion, referencia.
        let orden = [Columna::Numero, Columna::Monto, Columna::Operador, Columna::Accion, Columna::Referencia];
        (orden.iter().map(|c| Some(*c)).collect(), &regs[..])
    };

    if datos.is_empty() {
        return Err("el CSV no tiene filas de datos".into());
    }
    if datos.len() > MAX_FILAS {
        return Err(format!("el CSV tiene {} filas; el máximo es {MAX_FILAS}", datos.len()));
    }

    let filas = datos
        .iter()
        .map(|(linea, campos)| {
            let mut valores = BTreeMap::new();
            for (i, v) in campos.iter().enumerate() {
                if let Some(Some(col)) = mapeo.get(i) {
                    // Si una columna aparece dos veces, gana la primera no vacía.
                    let e = valores.entry(*col).or_insert_with(String::new);
                    if e.is_empty() {
                        *e = v.clone();
                    }
                }
            }
            FilaCsv { linea: *linea, valores }
        })
        .collect();
    Ok(Csv { separador: sep, con_encabezado, filas })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(f: &FilaCsv, c: Columna) -> &str {
        f.valores.get(&c).map(|s| s.as_str()).unwrap_or("")
    }

    #[test]
    fn coma_con_encabezado_y_comillas() {
        let c = leer("numero,monto,operador,accion,referencia\n69869869,\"12,00\",TIGO,,\"Pedido \"\"A\"\", 1\"\n").unwrap();
        assert_eq!(c.separador, ',');
        assert!(c.con_encabezado);
        assert_eq!(c.filas.len(), 1);
        let f = &c.filas[0];
        assert_eq!(f.linea, 2);
        assert_eq!(v(f, Columna::Monto), "12,00");
        assert_eq!(v(f, Columna::Referencia), "Pedido \"A\", 1");
    }

    #[test]
    fn punto_y_coma_bom_alias_y_vacias() {
        let t = "\u{feff}Teléfono;Importe;Compañía\r\n+591 7881-5232;Bs 12;tigo\r\n\r\n;;\r\n69869869;12,00;Tigo\r\n";
        let c = leer(t).unwrap();
        assert_eq!(c.separador, ';');
        assert_eq!(c.filas.len(), 2);
        assert_eq!(v(&c.filas[0], Columna::Numero), "+591 7881-5232");
        assert_eq!(v(&c.filas[0], Columna::Monto), "Bs 12");
        assert_eq!(c.filas[1].linea, 5);
        assert_eq!(v(&c.filas[1], Columna::Monto), "12,00");
    }

    #[test]
    fn sin_encabezado() {
        let c = leer("69869869;12;TIGO\n78815232;5;TIGO;transferir;x\n").unwrap();
        assert!(!c.con_encabezado);
        assert_eq!(c.filas.len(), 2);
        assert_eq!(c.filas[0].linea, 1);
        assert_eq!(v(&c.filas[1], Columna::Referencia), "x");
    }

    #[test]
    fn errores_claros() {
        assert!(leer("").unwrap_err().contains("vacío"));
        assert!(leer("numero,monto\n1,2\n").unwrap_err().contains("operador"));
        assert!(leer("numero,monto,operador\n").unwrap_err().contains("filas"));
    }

    #[test]
    fn salto_de_linea_dentro_de_comillas() {
        let c = leer("numero,monto,operador,referencia\n69869869,5,TIGO,\"dos\nlineas\"\n78815232,1,TIGO,x\n").unwrap();
        assert_eq!(c.filas.len(), 2);
        assert_eq!(v(&c.filas[0], Columna::Referencia), "dos\nlineas");
        assert_eq!(c.filas[1].linea, 4);
    }
}
