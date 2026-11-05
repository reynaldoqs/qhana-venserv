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
    /// La fila no se puede leer con seguridad (p. ej. más campos que el encabezado): queda inválida.
    pub error: Option<String>,
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
/// Con `requiere_operador = false` (lote de un chip) basta `numero,monto`, y sin encabezado el orden es
/// `numero, monto, referencia`.
pub fn leer(texto: &str, requiere_operador: bool) -> Result<Csv, String> {
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
            if !mapeo.contains(&Some(req)) && (requiere_operador || req != Columna::Operador) {
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
        let orden: &[Columna] = if requiere_operador {
            &[Columna::Numero, Columna::Monto, Columna::Operador, Columna::Accion, Columna::Referencia]
        } else {
            &[Columna::Numero, Columna::Monto, Columna::Referencia]
        };
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
            // Más campos (con algo escrito) que columnas: nunca se descartan en silencio. El caso típico es
            // `69869869,12,50`, un monto con coma decimal sin comillas, que se leería como 12.
            let usados = campos.iter().rposition(|c| !c.is_empty()).map_or(0, |i| i + 1);
            let error = (usados > mapeo.len()).then(|| {
                let de = if con_encabezado { "el encabezado" } else { "se esperaban" };
                format!("la fila tiene {usados} campos y {de} {} (¿un monto con coma sin comillas?)", mapeo.len())
            });
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
            FilaCsv { linea: *linea, valores, error }
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
        let c = leer("numero,monto,operador,accion,referencia\n69869869,\"12,00\",TIGO,,\"Pedido \"\"A\"\", 1\"\n", true).unwrap();
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
        let c = leer(t, true).unwrap();
        assert_eq!(c.separador, ';');
        assert_eq!(c.filas.len(), 2);
        assert_eq!(v(&c.filas[0], Columna::Numero), "+591 7881-5232");
        assert_eq!(v(&c.filas[0], Columna::Monto), "Bs 12");
        assert_eq!(c.filas[1].linea, 5);
        assert_eq!(v(&c.filas[1], Columna::Monto), "12,00");
    }

    #[test]
    fn sin_encabezado() {
        let c = leer("69869869;12;TIGO\n78815232;5;TIGO;transferir;x\n", true).unwrap();
        assert!(!c.con_encabezado);
        assert_eq!(c.filas.len(), 2);
        assert_eq!(c.filas[0].linea, 1);
        assert_eq!(v(&c.filas[1], Columna::Referencia), "x");
    }

    #[test]
    fn errores_claros() {
        assert!(leer("", true).unwrap_err().contains("vacío"));
        assert!(leer("numero,monto\n1,2\n", true).unwrap_err().contains("operador"));
        assert!(leer("numero,monto,operador\n", true).unwrap_err().contains("filas"));
        // Lote de un chip: basta numero,monto (con o sin encabezado).
        let c = leer("numero,monto\n69869869,1.50\n", false).unwrap();
        assert_eq!(v(&c.filas[0], Columna::Monto), "1.50");
        let c = leer("69869869,1.50,Pedido 1\n", false).unwrap();
        assert_eq!(v(&c.filas[0], Columna::Referencia), "Pedido 1");
    }

    #[test]
    fn mas_campos_que_el_encabezado_es_error() {
        // Un chip: `numero,monto` y un monto con coma sin comillas.
        let c = leer("numero,monto\n69869869,12,50\n69869869,\"12,50\"\n70000000,5,\n", false).unwrap();
        assert_eq!(c.filas[0].error.as_deref(), Some("la fila tiene 3 campos y el encabezado 2 (¿un monto con coma sin comillas?)"));
        assert_eq!(c.filas[1].error, None, "con comillas está bien");
        assert_eq!(v(&c.filas[1], Columna::Monto), "12,50");
        assert_eq!(c.filas[2].error, None, "un separador de más al final, vacío, no es un campo");
        // Por operador, con `;`.
        let c = leer("numero;monto;operador\n69869869;12;TIGO;x\n", true).unwrap();
        assert!(c.filas[0].error.as_deref().unwrap().starts_with("la fila tiene 4 campos y el encabezado 3"));
        // Sin encabezado: más de las 5 columnas conocidas.
        let c = leer("69869869;12;TIGO;transferir;pedido;sobra\n", true).unwrap();
        assert!(c.filas[0].error.as_deref().unwrap().contains("6 campos y se esperaban 5"));
    }

    #[test]
    fn salto_de_linea_dentro_de_comillas() {
        let c = leer("numero,monto,operador,referencia\n69869869,5,TIGO,\"dos\nlineas\"\n78815232,1,TIGO,x\n", true).unwrap();
        assert_eq!(c.filas.len(), 2);
        assert_eq!(v(&c.filas[0], Columna::Referencia), "dos\nlineas");
        assert_eq!(c.filas[1].linea, 4);
    }
}
