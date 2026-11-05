//! Recetas: flujos USSD de un operador, escritos en YAML (ver `docs/protocolo.md`).

use protocolo::ModoUssd;
use regex::Regex;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::OnceLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Receta {
    pub nombre: String,
    #[serde(default)]
    pub titulo: String,
    #[serde(default)]
    pub operador: Option<String>,
    #[serde(default)]
    pub descripcion: Option<String>,
    #[serde(default)]
    pub modo: ModoUssd,
    #[serde(default)]
    pub parametros: Vec<Parametro>,
    pub pasos: Vec<Paso>,
    #[serde(default)]
    pub salida: BTreeMap<String, String>,
    /// Qué hace la receta (`transferir`, `saldo`, `consulta`…). Los lotes buscan la receta por
    /// (operador, accion).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accion: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limites: Option<Limites>,
    /// `dealer`: receta propia de los chips dealer. Sin valor, vale para todos los chips. Acepta el nombre viejo
    /// `pda` y lo guarda como `dealer` (ver "Compatibilidad" en el protocolo).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chip_tipo: Option<String>,
    /// Nombres que tuvo antes la receta. Buscarla por uno de ellos (lotes guardados, `ejecutar`, versiones,
    /// estadísticas) da esta receta.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nombres_anteriores: Vec<String>,
    /// Versión del archivo del que se cargó (SHA-256 del texto, en hex). No es parte del YAML: la pone quien
    /// carga la carpeta, y cada ejecución la guarda en el historial.
    #[serde(skip)]
    pub version: Option<String>,
}

/// Un error de una receta, con el paso (1-based) y el campo cuando se sabe.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ErrorReceta {
    pub paso: Option<usize>,
    pub campo: Option<String>,
    pub mensaje: String,
}

impl ErrorReceta {
    fn nuevo(paso: Option<usize>, campo: Option<&str>, mensaje: impl Into<String>) -> Self {
        Self { paso, campo: campo.map(str::to_string), mensaje: mensaje.into() }
    }
}

impl std::fmt::Display for ErrorReceta {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.paso {
            Some(n) => write!(f, "paso {n}: {}", self.mensaje),
            None => f.write_str(&self.mensaje),
        }
    }
}

/// Algo raro que no impide guardar ni correr la receta.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AvisoReceta {
    pub paso: Option<usize>,
    pub mensaje: String,
}

impl std::fmt::Display for AvisoReceta {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.paso {
            Some(n) => write!(f, "paso {n}: {}", self.mensaje),
            None => f.write_str(&self.mensaje),
        }
    }
}

/// Tipo de chip de las recetas propias de los chips dealer.
pub const CHIP_DEALER: &str = "dealer";

/// Acción de las recetas que leen el saldo dealer (su `salida` trae `saldo_dealer`).
pub const ACCION_SALDO_DEALER: &str = "saldo_dealer";

/// Tipo de chip con los nombres viejos ya traducidos: `pda` → `dealer` (sin importar mayúsculas ni espacios).
/// Cualquier otro valor queda tal cual (sin espacios alrededor).
pub fn tipo_chip_actual(tipo: &str) -> String {
    let t = tipo.trim();
    if t.eq_ignore_ascii_case("pda") || t.eq_ignore_ascii_case(CHIP_DEALER) {
        CHIP_DEALER.to_string()
    } else {
        t.to_string()
    }
}

/// Acción con los nombres viejos ya traducidos: `saldo_pda` → `saldo_dealer`. Cualquier otra queda tal cual.
pub fn accion_actual(accion: &str) -> String {
    if accion.trim().eq_ignore_ascii_case("saldo_pda") {
        ACCION_SALDO_DEALER.to_string()
    } else {
        accion.to_string()
    }
}

/// Busca una receta por su nombre o, si no hay ninguna con ese nombre, por uno de sus `nombres_anteriores`.
pub fn buscar<'a>(recetas: &'a BTreeMap<String, Receta>, nombre: &str) -> Option<&'a Receta> {
    recetas.get(nombre).or_else(|| recetas.values().find(|r| r.nombres_anteriores.iter().any(|n| n == nombre)))
}

/// Versión de una receta: SHA-256 del texto del archivo, en hex.
pub fn version_de(texto: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(texto.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// Límites del operador para esta receta (los usa el reparto de lotes).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Limites {
    /// Envíos permitidos por chip y por día.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub por_dia: Option<u32>,
    /// Costo por envío, en Bs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comision: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Parametro {
    pub nombre: String,
    #[serde(default)]
    pub patron: Option<String>,
    #[serde(default, deserialize_with = "opciones_como_texto")]
    pub opciones: Option<Vec<String>>,
    #[serde(default)]
    pub descripcion: Option<String>,
    /// Valor secreto (PIN): no se guarda en el historial ni en los eventos. Un parámetro `pin` secreto
    /// se completa solo con el PIN guardado del chip.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub secreto: bool,
}

/// Acepta `opciones: ["1", "2"]` y también `opciones: [1, 2]`.
fn opciones_como_texto<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<String>>, D::Error> {
    let v: Option<Vec<serde_yaml::Value>> = Option::deserialize(d)?;
    Ok(v.map(|xs| {
        xs.into_iter()
            .map(|x| match x {
                serde_yaml::Value::String(s) => s,
                serde_yaml::Value::Number(n) => n.to_string(),
                serde_yaml::Value::Bool(b) => b.to_string(),
                otro => serde_yaml::to_string(&otro).unwrap_or_default().trim().to_string(),
            })
            .collect()
    }))
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Paso {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub marcar: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enviar: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elegir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verificar: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirmar: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub esperar: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_si: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exito: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    /// Solo en `enviar`: este paso es el que mueve dinero (p. ej. enviar el PIN). Nunca se reintenta y en
    /// `simular` no se envía.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub mueve_dinero: bool,
}

/// La acción de un paso, ya validada.
#[derive(Debug, Clone, PartialEq)]
pub enum Accion<'a> {
    Marcar(&'a str),
    Enviar(&'a str),
    Elegir(&'a str),
    Verificar(&'a str),
    Confirmar(&'a str),
}

impl Accion<'_> {
    pub fn nombre(&self) -> &'static str {
        match self {
            Accion::Marcar(_) => "marcar",
            Accion::Enviar(_) => "enviar",
            Accion::Elegir(_) => "elegir",
            Accion::Verificar(_) => "verificar",
            Accion::Confirmar(_) => "confirmar",
        }
    }
}

impl Paso {
    pub fn accion(&self) -> Result<Accion<'_>, String> {
        let mut v = vec![];
        if let Some(x) = &self.marcar {
            v.push(Accion::Marcar(x));
        }
        if let Some(x) = &self.enviar {
            v.push(Accion::Enviar(x));
        }
        if let Some(x) = &self.elegir {
            v.push(Accion::Elegir(x));
        }
        if let Some(x) = &self.verificar {
            v.push(Accion::Verificar(x));
        }
        if let Some(x) = &self.confirmar {
            v.push(Accion::Confirmar(x));
        }
        match v.len() {
            1 => Ok(v.remove(0)),
            0 => Err("el paso no tiene acción (marcar/enviar/elegir/verificar/confirmar)".into()),
            _ => Err("el paso tiene más de una acción".into()),
        }
    }
}

/// Info pública de una receta para `GET /api/recetas`.
#[derive(Debug, Clone, Serialize)]
pub struct ResumenReceta {
    pub nombre: String,
    pub titulo: String,
    pub operador: Option<String>,
    pub descripcion: Option<String>,
    pub modo: ModoUssd,
    pub parametros: Vec<Parametro>,
    pub pasos: usize,
    /// true si algún paso mueve dinero (`confirmar` o `mueve_dinero`).
    pub mueve_dinero: bool,
    pub accion: Option<String>,
    pub limites: Option<Limites>,
    pub chip_tipo: Option<String>,
    /// Nombres que tuvo antes la receta (`[]` si nunca se renombró).
    pub nombres_anteriores: Vec<String>,
}

impl Receta {
    pub fn desde_yaml(texto: &str) -> Result<Self, String> {
        let r = Self::leer(texto).map_err(|e| e.to_string())?;
        r.validar()?;
        Ok(r)
    }

    /// Lee el YAML sin validar la receta. Si el error es de un paso (`pasos[2].esperar: …`), lo dice.
    pub fn leer(texto: &str) -> Result<Self, ErrorReceta> {
        let mut r = Self::leer_tal_cual(texto)?;
        r.normalizar();
        Ok(r)
    }

    /// Traduce los valores viejos: `chip_tipo: pda` → `dealer` y `accion: saldo_pda` → `saldo_dealer`.
    pub fn normalizar(&mut self) {
        if let Some(t) = &self.chip_tipo {
            self.chip_tipo = Some(tipo_chip_actual(t));
        }
        if let Some(a) = &self.accion {
            self.accion = Some(accion_actual(a));
        }
    }

    /// Este nombre y los anteriores: con todos ellos pudo quedar en el historial.
    pub fn todos_los_nombres(&self) -> Vec<&str> {
        std::iter::once(self.nombre.as_str()).chain(self.nombres_anteriores.iter().map(String::as_str)).collect()
    }

    fn leer_tal_cual(texto: &str) -> Result<Self, ErrorReceta> {
        serde_yaml::from_str::<Receta>(texto).map_err(|e| {
            let m = e.to_string();
            static RE: OnceLock<Regex> = OnceLock::new();
            let re = RE.get_or_init(|| Regex::new(r"^pasos\[(\d+)\](?:\.([a-z_]+))?: ").unwrap());
            match re.captures(&m) {
                Some(c) => ErrorReceta::nuevo(
                    c[1].parse::<usize>().ok().map(|i| i + 1),
                    c.get(2).map(|x| x.as_str()),
                    m[c[0].len()..].to_string(),
                ),
                None => ErrorReceta::nuevo(None, None, format!("YAML mal formado: {m}")),
            }
        })
    }

    pub fn desde_archivo(ruta: &Path) -> Result<Self, String> {
        let t = std::fs::read_to_string(ruta).map_err(|e| format!("{}: {e}", ruta.display()))?;
        let mut r = Self::desde_yaml(&t).map_err(|e| format!("{}: {e}", ruta.display()))?;
        r.version = Some(version_de(&t));
        Ok(r)
    }

    /// Carga todas las `*.yaml` / `*.yml` de una carpeta (no las de `viejo/`). Las inválidas se reportan aparte.
    pub fn cargar_carpeta(dir: &Path) -> (BTreeMap<String, Receta>, Vec<String>) {
        let (ok, errores) = crate::carpeta::cargar(dir);
        (ok.into_iter().map(|(k, c)| (k, c.receta)).collect(), errores)
    }

    pub fn resumen(&self) -> ResumenReceta {
        ResumenReceta {
            nombre: self.nombre.clone(),
            titulo: if self.titulo.is_empty() { self.nombre.clone() } else { self.titulo.clone() },
            operador: self.operador.clone(),
            descripcion: self.descripcion.clone(),
            modo: self.modo,
            parametros: self.parametros.clone(),
            pasos: self.pasos.len(),
            mueve_dinero: self.pasos.iter().any(|p| p.confirmar.is_some() || p.mueve_dinero),
            accion: self.accion.clone(),
            limites: self.limites.clone(),
            chip_tipo: self.chip_tipo.clone(),
            nombres_anteriores: self.nombres_anteriores.clone(),
        }
    }

    /// Valores de los parámetros secretos, para taparlos antes de guardar o mostrar algo.
    pub fn secretos(&self, params: &BTreeMap<String, String>) -> Vec<String> {
        self.parametros
            .iter()
            .filter(|p| p.secreto)
            .filter_map(|p| params.get(&p.nombre).cloned())
            .filter(|v| !v.is_empty())
            .collect()
    }

    /// `params` sin los valores secretos (para historial y eventos).
    pub fn sin_secretos(&self, params: &BTreeMap<String, String>) -> BTreeMap<String, String> {
        let mut p = params.clone();
        for q in self.parametros.iter().filter(|q| q.secreto) {
            if let Some(v) = p.get_mut(&q.nombre) {
                *v = TAPADO.into();
            }
        }
        p
    }

    /// Valida la receta. El error es el primero de [`Receta::errores`].
    pub fn validar(&self) -> Result<(), String> {
        match self.errores().into_iter().next() {
            Some(e) => Err(e.to_string()),
            None => Ok(()),
        }
    }

    /// Todos los errores que impiden cargar la receta, en orden, con su paso (1-based) y campo.
    pub fn errores(&self) -> Vec<ErrorReceta> {
        let mut v = vec![];
        let mut err = |paso: Option<usize>, campo: Option<&str>, m: String| v.push(ErrorReceta::nuevo(paso, campo, m));
        if self.nombre.trim().is_empty() {
            err(None, Some("nombre"), "falta 'nombre'".into());
        }
        if let Some(t) = &self.chip_tipo {
            if t != CHIP_DEALER {
                err(None, Some("chip_tipo"), format!("chip_tipo '{t}' no válido (solo 'dealer')"));
            }
        }
        for (i, n) in self.nombres_anteriores.iter().enumerate() {
            if !crate::carpeta::nombre_valido(n) {
                err(None, Some("nombres_anteriores"), format!("nombre anterior '{n}' no válido: solo minúsculas, dígitos y guiones"));
            } else if *n == self.nombre {
                err(None, Some("nombres_anteriores"), format!("'{n}' es el nombre actual: no va en 'nombres_anteriores'"));
            } else if self.nombres_anteriores[..i].contains(n) {
                err(None, Some("nombres_anteriores"), format!("'{n}' está repetido en 'nombres_anteriores'"));
            }
        }
        if let Some(l) = &self.limites {
            if l.comision.is_some_and(|c| !(0.0..1_000.0).contains(&c)) {
                err(None, Some("limites"), "limites.comision fuera de rango".into());
            }
        }
        if self.pasos.is_empty() {
            err(None, Some("pasos"), "la receta no tiene pasos".into());
        }
        for p in &self.parametros {
            if crate::monto::es_reservado(&p.nombre) {
                err(
                    None,
                    Some("parametros"),
                    format!("'{}' es un nombre reservado (valor calculado de 'monto'): no se puede declarar", p.nombre),
                );
            }
            if let Some(pat) = &p.patron {
                if let Err(e) = Regex::new(pat) {
                    err(None, Some("parametros"), format!("parámetro '{}': patrón inválido: {e}", p.nombre));
                }
            }
        }
        // Las regex deben compilar con valores de ejemplo en los parámetros.
        let mut ejemplo: BTreeMap<String, String> =
            self.parametros.iter().map(|p| (p.nombre.clone(), "0".to_string())).collect();
        for r in crate::monto::RESERVADOS {
            ejemplo.insert(r.to_string(), "0".to_string());
        }
        for (i, paso) in self.pasos.iter().enumerate() {
            let n = Some(i + 1);
            let a = match paso.accion() {
                Ok(a) => a,
                Err(e) => {
                    err(n, None, e);
                    continue;
                }
            };
            if paso.mueve_dinero && !matches!(a, Accion::Enviar(_)) {
                err(n, Some("mueve_dinero"), "'mueve_dinero' solo va en un paso 'enviar'".into());
            }
            if i == 0 && !matches!(a, Accion::Marcar(_)) {
                err(n, Some(a.nombre()), "el primer paso debe ser 'marcar'".into());
            }
            // Un segundo 'marcar' vale si la sesión anterior ya cerró; eso solo se sabe al correr (ver motor).
            let regexes = [
                match a {
                    Accion::Elegir(r) | Accion::Verificar(r) | Accion::Confirmar(r) => Some((a.nombre(), r)),
                    _ => None,
                },
                paso.esperar.as_deref().map(|r| ("esperar", r)),
                paso.error_si.as_deref().map(|r| ("error_si", r)),
                paso.exito.as_deref().map(|r| ("exito", r)),
            ];
            for (campo, r) in regexes.into_iter().flatten() {
                if let Err(e) = Regex::new(&sustituir(r, &ejemplo, true)) {
                    err(n, Some(campo), format!("regex inválida '{r}': {e}"));
                }
            }
        }
        if self.modo == ModoUssd::Api && self.pasos.len() > 1 {
            err(None, Some("modo"), "modo 'api' solo admite un paso 'marcar' (no se puede contestar menús)".into());
        }
        for (k, r) in &self.salida {
            match Regex::new(r) {
                Err(e) => err(None, Some("salida"), format!("salida '{k}': regex inválida: {e}")),
                Ok(re) if re.captures_len() < 2 => {
                    err(None, Some("salida"), format!("salida '{k}': la regex necesita un grupo (…)"))
                }
                Ok(_) => {}
            }
        }
        v
    }

    /// ¿Algún paso o la `salida` usa `{nombre}`?
    pub fn usa_parametro(&self, nombre: &str) -> bool {
        let pasos = self.pasos.iter().flat_map(|p| {
            [&p.marcar, &p.enviar, &p.elegir, &p.verificar, &p.confirmar, &p.esperar, &p.error_si, &p.exito]
                .into_iter()
                .flatten()
        });
        pasos.chain(self.salida.values()).any(|t| referencias(t).iter().any(|r| r == nombre))
    }

    /// Índice (0-based) del primer paso que mueve dinero (`confirmar` o `enviar` con `mueve_dinero`).
    pub fn paso_que_mueve_dinero(&self) -> Option<usize> {
        self.pasos.iter().position(|p| p.confirmar.is_some() || p.mueve_dinero)
    }

    /// Avisos que no impiden guardar (ver "Validar sin guardar" en el protocolo). `otras`: las demás recetas
    /// cargadas, para detectar dos con el mismo (operador, accion, chip_tipo).
    pub fn avisos(&self, otras: &[&Receta]) -> Vec<AvisoReceta> {
        let mut v = vec![];
        let mut aviso = |paso: Option<usize>, m: String| v.push(AvisoReceta { paso, mensaje: m });
        if let Some(m) = self.paso_que_mueve_dinero() {
            if !self.pasos[..m].iter().any(|p| p.verificar.is_some()) {
                aviso(
                    Some(m + 1),
                    "la receta mueve dinero y no tiene un paso 'verificar' antes del paso que mueve dinero".into(),
                );
            }
        }
        for (i, p) in self.pasos.iter().enumerate() {
            if (p.confirmar.is_some() || p.mueve_dinero) && p.exito.is_none() {
                aviso(Some(i + 1), "el paso que mueve dinero no tiene 'exito': todo resultado quedará incierto".into());
            }
            if p.verificar.is_some() && (p.esperar.is_some() || p.error_si.is_some() || p.exito.is_some()) {
                aviso(Some(i + 1), "en un paso 'verificar' no se usan 'esperar', 'error_si' ni 'exito' (no llega pantalla nueva)".into());
            }
        }
        // Parámetros usados y declarados.
        let declarados: BTreeSet<&str> = self.parametros.iter().map(|p| p.nombre.as_str()).collect();
        let mut usados: BTreeSet<String> = BTreeSet::new();
        for (i, p) in self.pasos.iter().enumerate() {
            let textos = [&p.marcar, &p.enviar, &p.elegir, &p.verificar, &p.confirmar, &p.esperar, &p.error_si, &p.exito];
            let mut del_paso: BTreeSet<String> = BTreeSet::new();
            for t in textos.into_iter().flatten() {
                del_paso.extend(referencias(t));
            }
            for x in &del_paso {
                if crate::monto::es_reservado(x) {
                    // Valor calculado del monto: no se declara, pero necesita 'monto'.
                    if !declarados.contains("monto") {
                        aviso(Some(i + 1), format!("'{{{x}}}' es un valor calculado de 'monto' y 'monto' no está declarado"));
                    }
                } else if !declarados.contains(x.as_str()) {
                    aviso(Some(i + 1), format!("'{{{x}}}' no está declarado en 'parametros'"));
                }
            }
            usados.extend(del_paso);
        }
        for r in self.salida.values() {
            usados.extend(referencias(r));
        }
        // Usar un valor calculado es usar 'monto'.
        if usados.iter().any(|u| crate::monto::es_reservado(u)) {
            usados.insert("monto".to_string());
        }
        for d in &declarados {
            if !usados.contains(*d) {
                aviso(None, format!("el parámetro '{d}' está declarado y no se usa en ningún paso"));
            }
        }
        let accion = self.accion.as_deref().map(|a| a.trim().to_lowercase());
        match accion.as_deref() {
            Some(a @ ("transferir" | "recargar")) if self.limites.is_none() => aviso(
                None,
                format!("accion '{a}' sin 'limites' (por_dia, comision): los lotes no sabrán el cupo ni la comisión"),
            ),
            Some("saldo") if !self.salida.contains_key("saldo_principal") => {
                aviso(None, "accion 'saldo' sin 'salida.saldo_principal': no servirá para consultar el saldo de los chips".into())
            }
            _ => {}
        }
        if let Some(a) = &accion {
            let mismo_operador = |o: &Receta| {
                o.operador.as_deref().map(|x| x.trim().to_uppercase())
                    == self.operador.as_deref().map(|x| x.trim().to_uppercase())
            };
            for o in otras.iter().filter(|o| o.nombre != self.nombre) {
                if mismo_operador(o)
                    && o.accion.as_deref().map(|x| x.trim().to_lowercase()).as_ref() == Some(a)
                    && o.chip_tipo == self.chip_tipo
                {
                    aviso(
                        None,
                        format!(
                            "la receta '{}' tiene el mismo operador, acción y chip_tipo: el lote no sabría cuál usar",
                            o.nombre
                        ),
                    );
                }
            }
        }
        v
    }
}

/// Nombres de parámetro `{x}` que aparecen en un texto (no `\d{7}` ni `{2,3}`).
pub fn referencias(texto: &str) -> Vec<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"\{([A-Za-z_][A-Za-z0-9_]*)\}").unwrap());
    re.captures_iter(texto).map(|c| c[1].to_string()).collect()
}

/// Campos del YAML que la receta no conoce (un error de tipeo como `esprar:` se ignoraría en silencio).
pub fn campos_desconocidos(texto: &str) -> Vec<AvisoReceta> {
    const RECETA: &[&str] = &[
        "nombre", "titulo", "operador", "descripcion", "modo", "parametros", "pasos", "salida", "accion", "limites",
        "chip_tipo", "nombres_anteriores",
    ];
    const PASO: &[&str] = &[
        "marcar", "enviar", "elegir", "verificar", "confirmar", "esperar", "error_si", "exito", "timeout_ms",
        "mueve_dinero",
    ];
    const PARAMETRO: &[&str] = &["nombre", "patron", "opciones", "descripcion", "secreto"];
    const LIMITES: &[&str] = &["por_dia", "comision"];
    let Ok(serde_yaml::Value::Mapping(m)) = serde_yaml::from_str::<serde_yaml::Value>(texto) else { return vec![] };
    let mut v = vec![];
    let mut revisar = |mapa: &serde_yaml::Mapping, validos: &[&str], paso: Option<usize>, donde: &str| {
        for k in mapa.keys() {
            let k = k.as_str().unwrap_or("?");
            if !validos.contains(&k) {
                v.push(AvisoReceta { paso, mensaje: format!("campo desconocido '{k}'{donde}: se ignora") });
            }
        }
    };
    revisar(&m, RECETA, None, "");
    if let Some(serde_yaml::Value::Sequence(pasos)) = m.get("pasos") {
        for (i, p) in pasos.iter().enumerate() {
            if let serde_yaml::Value::Mapping(p) = p {
                revisar(p, PASO, Some(i + 1), "");
            }
        }
    }
    if let Some(serde_yaml::Value::Sequence(ps)) = m.get("parametros") {
        for p in ps {
            if let serde_yaml::Value::Mapping(p) = p {
                revisar(p, PARAMETRO, None, " en 'parametros'");
            }
        }
    }
    if let Some(serde_yaml::Value::Mapping(l)) = m.get("limites") {
        revisar(l, LIMITES, None, " en 'limites'");
    }
    v
}

/// Cómo se muestra un valor secreto.
pub const TAPADO: &str = "••••";

/// Tapa en `texto` cada valor secreto.
pub fn tapar(texto: &str, secretos: &[String]) -> String {
    secretos.iter().fold(texto.to_string(), |t, s| t.replace(s.as_str(), TAPADO))
}

/// Reemplaza `{param}` por su valor. Solo nombres de parámetros conocidos: así `\d{7}` queda intacto.
/// Con `escapar`, el valor se escapa para usarse dentro de una regex.
pub fn sustituir(plantilla: &str, params: &BTreeMap<String, String>, escapar: bool) -> String {
    let mut s = plantilla.to_string();
    for (k, v) in params {
        let marca = format!("{{{k}}}");
        if s.contains(&marca) {
            let valor = if escapar { regex::escape(v) } else { v.clone() };
            s = s.replace(&marca, &valor);
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sustituye_solo_parametros_conocidos() {
        let mut p = BTreeMap::new();
        p.insert("monto".to_string(), "1.5".to_string());
        assert_eq!(sustituir(r"^\d{7} {monto} {otro}", &p, true), r"^\d{7} 1\.5 {otro}");
        assert_eq!(sustituir("{monto}", &p, false), "1.5");
    }

    #[test]
    fn tapa_secretos() {
        let y = "nombre: x\nparametros:\n  - { nombre: pin, secreto: true }\n  - { nombre: monto }\npasos:\n  - marcar: '*1#'\n";
        let r = Receta::desde_yaml(y).unwrap();
        let mut p = BTreeMap::new();
        p.insert("pin".to_string(), "1234".to_string());
        p.insert("monto".to_string(), "5".to_string());
        assert_eq!(r.sin_secretos(&p)["pin"], TAPADO);
        assert_eq!(r.sin_secretos(&p)["monto"], "5");
        assert_eq!(tapar("pin 1234 ok", &r.secretos(&p)), format!("pin {TAPADO} ok"));
    }

    #[test]
    fn mueve_dinero_solo_en_enviar() {
        let y = "nombre: x\npasos:\n  - marcar: '*1#'\n    mueve_dinero: true\n";
        assert!(Receta::desde_yaml(y).unwrap_err().contains("mueve_dinero"));
    }

    #[test]
    fn rechaza_api_con_varios_pasos() {
        let y = "nombre: x\nmodo: api\npasos:\n  - marcar: '*1#'\n  - enviar: '1'\n";
        assert!(Receta::desde_yaml(y).unwrap_err().contains("api"));
    }

    #[test]
    fn rechaza_paso_con_dos_acciones() {
        let y = "nombre: x\npasos:\n  - marcar: '*1#'\n    enviar: '1'\n";
        assert!(Receta::desde_yaml(y).is_err());
    }

    #[test]
    fn errores_con_paso_y_campo() {
        let y = "nombre: x\nchip_tipo: otro\npasos:\n  - enviar: '1'\n  - marcar: '*1#'\n    esperar: 'a(b'\n  - elegir: '[x'\n    exito: 'ok'\n  - {}\nsalida:\n  costo: 'sin grupo'\n";
        let r = Receta::leer(y).unwrap();
        let e = r.errores();
        let resumen: Vec<(Option<usize>, Option<&str>)> = e.iter().map(|e| (e.paso, e.campo.as_deref())).collect();
        assert_eq!(
            resumen,
            vec![
                (None, Some("chip_tipo")),
                (Some(1), Some("enviar")),
                (Some(2), Some("esperar")),
                (Some(3), Some("elegir")),
                (Some(4), None),
                (None, Some("salida")),
            ],
            "{e:?}"
        );
        assert!(e[1].mensaje.contains("primer paso"));
        // `validar` da el primero, con el paso delante si lo hay.
        let y2 = "nombre: x\npasos:\n  - marcar: '*1#'\n  - elegir: '[x'\n";
        assert!(Receta::desde_yaml(y2).unwrap_err().starts_with("paso 2: regex inválida"));
    }

    #[test]
    fn error_de_yaml_con_paso() {
        let e = Receta::leer("nombre: x\npasos:\n  - marcar: '*1#'\n  - enviar: '1'\n    timeout_ms: mucho\n").unwrap_err();
        assert_eq!((e.paso, e.campo.as_deref()), (Some(2), Some("timeout_ms")), "{e:?}");
        let e = Receta::leer("nombre: [x\n").unwrap_err();
        assert_eq!(e.paso, None);
        assert!(e.mensaje.starts_with("YAML mal formado"));
    }

    #[test]
    fn avisos() {
        let y = r#"nombre: t
operador: TIGO
accion: transferir
parametros:
  - { nombre: numero }
  - { nombre: sobra }
pasos:
  - marcar: '*1#'
    esperar: 'x'
  - enviar: '{numero}'
  - enviar: '{monto}'
  - confirmar: 'Si'
  - verificar: 'x'
    esperar: 'y'
"#;
        let r = Receta::desde_yaml(y).unwrap();
        let otra = Receta::desde_yaml("nombre: otra\noperador: tigo\naccion: Transferir\npasos:\n  - marcar: '*1#'\n").unwrap();
        let dealer = Receta::desde_yaml("nombre: dealer\noperador: TIGO\naccion: transferir\nchip_tipo: dealer\npasos:\n  - marcar: '*1#'\n").unwrap();
        let a = r.avisos(&[&otra, &dealer, &r]);
        let ver: Vec<String> = a.iter().map(|a| a.to_string()).collect();
        let tiene = |paso: Option<usize>, txt: &str| a.iter().any(|x| x.paso == paso && x.mensaje.contains(txt));
        assert!(tiene(Some(4), "no tiene un paso 'verificar' antes"), "{ver:?}");
        assert!(tiene(Some(4), "no tiene 'exito'"), "{ver:?}");
        assert!(tiene(Some(3), "'{monto}' no está declarado"), "{ver:?}");
        assert!(tiene(None, "'sobra' está declarado y no se usa"), "{ver:?}");
        assert!(tiene(None, "sin 'limites'"), "{ver:?}");
        assert!(tiene(None, "'otra' tiene el mismo operador"), "{ver:?}");
        assert!(!tiene(None, "'dealer' tiene"), "otro chip_tipo no choca: {ver:?}");
        assert!(tiene(Some(5), "'verificar' no se usan"), "{ver:?}");
        assert_eq!(a.len(), 7, "{ver:?}");

        let saldo = Receta::desde_yaml("nombre: s\naccion: saldo\npasos:\n  - marcar: '*1#'\nsalida:\n  total: 'T: (\\d+)'\n").unwrap();
        assert!(saldo.avisos(&[]).iter().any(|a| a.mensaje.contains("saldo_principal")));
        // Las recetas reales no tienen avisos de parámetros.
        let real = Receta::desde_yaml(include_str!("../../../recetas/tigo-transferir.yaml")).unwrap();
        assert!(real.avisos(&[]).is_empty(), "{:?}", real.avisos(&[]));
    }

    #[test]
    fn valores_calculados_del_monto() {
        // Nombres reservados: no se declaran.
        let y = "nombre: x\nparametros:\n  - { nombre: monto_cent3 }\npasos:\n  - marcar: '*1#'\n";
        assert!(Receta::desde_yaml(y).unwrap_err().contains("reservado"));
        // Usarlos no avisa "no declarado", y cuentan como uso de 'monto'.
        let y = "nombre: x\nparametros:\n  - { nombre: monto }\npasos:\n  - marcar: '*1*{monto_cent3}#'\n    esperar: 'Bs {monto_cent}'\n";
        let r = Receta::desde_yaml(y).unwrap();
        assert!(r.avisos(&[]).is_empty(), "{:?}", r.avisos(&[]));
        assert!(r.usa_parametro("monto_cent3") && !r.usa_parametro("monto_entero"));
        // Sin 'monto' declarado sí avisa.
        let y = "nombre: x\npasos:\n  - marcar: '*1*{monto_entero}#'\n";
        let a = Receta::desde_yaml(y).unwrap().avisos(&[]);
        assert!(a.iter().any(|x| x.mensaje.contains("valor calculado")), "{a:?}");
        // La receta Entel dealer no tiene avisos de parámetros.
        let entel = Receta::desde_yaml(include_str!("../../../recetas/entel-dealer-recargar.yaml")).unwrap();
        assert!(entel.avisos(&[]).iter().all(|a| !a.mensaje.contains("parámetro") && !a.mensaje.contains("declarado")), "{:?}", entel.avisos(&[]));
    }

    #[test]
    fn campos_desconocidos_avisan() {
        let y = "nombre: x\ntitlo: y\nparametros:\n  - { nombre: a, patorn: 'x' }\npasos:\n  - marcar: '*1#'\n    esprar: 'z'\n";
        let a = campos_desconocidos(y);
        assert_eq!(a.len(), 3, "{a:?}");
        assert!(a.iter().any(|x| x.paso == Some(1) && x.mensaje.contains("'esprar'")));
        assert!(a.iter().any(|x| x.paso.is_none() && x.mensaje.contains("'titlo'")));
    }

    #[test]
    fn version_es_sha256() {
        assert_eq!(version_de("").len(), 64);
        assert_eq!(version_de("abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn nombres_viejos_pda_se_leen_como_dealer() {
        let y = "nombre: x\noperador: TIGO\naccion: saldo_pda\nchip_tipo: pda\npasos:\n  - marcar: '*1#'\n";
        let r = Receta::desde_yaml(y).unwrap();
        assert_eq!((r.chip_tipo.as_deref(), r.accion.as_deref()), (Some("dealer"), Some("saldo_dealer")));
        assert_eq!(r.resumen().chip_tipo.as_deref(), Some("dealer"));
        let r = Receta::desde_yaml(&y.replace("chip_tipo: pda", "chip_tipo: PDA").replace("saldo_pda", "transferir")).unwrap();
        assert_eq!((r.chip_tipo.as_deref(), r.accion.as_deref()), (Some("dealer"), Some("transferir")));
        assert_eq!(tipo_chip_actual("pda"), "dealer");
        assert_eq!(tipo_chip_actual("normal"), "normal");
        assert_eq!(accion_actual("saldo_pda"), "saldo_dealer");
        // Las recetas reales ya usan los nombres nuevos.
        let saldo = Receta::desde_yaml(include_str!("../../../recetas/tigo-dealer-saldo.yaml")).unwrap();
        assert_eq!((saldo.accion.as_deref(), saldo.chip_tipo.as_deref()), (Some(ACCION_SALDO_DEALER), Some(CHIP_DEALER)));
        assert_eq!(saldo.nombres_anteriores, ["tigo-pda-saldo"]);
    }

    #[test]
    fn nombres_anteriores_validos_y_busqueda() {
        let y = "nombre: nueva\nnombres_anteriores: [vieja, otra-vieja]\npasos:\n  - marcar: '*1#'\n";
        let r = Receta::desde_yaml(y).unwrap();
        assert_eq!(r.todos_los_nombres(), ["nueva", "vieja", "otra-vieja"]);
        assert_eq!(r.resumen().nombres_anteriores, ["vieja", "otra-vieja"]);
        assert!(campos_desconocidos(y).is_empty());
        let mut m = BTreeMap::new();
        m.insert(r.nombre.clone(), r.clone());
        assert_eq!(buscar(&m, "nueva").unwrap().nombre, "nueva");
        assert_eq!(buscar(&m, "vieja").unwrap().nombre, "nueva");
        assert!(buscar(&m, "otra").is_none());
        // El nombre actual manda sobre un nombre anterior de otra.
        let v = Receta::desde_yaml("nombre: vieja\npasos:\n  - marcar: '*1#'\n").unwrap();
        m.insert(v.nombre.clone(), v);
        assert_eq!(buscar(&m, "vieja").unwrap().nombre, "vieja");
        // Sin nombres anteriores no se serializa el campo.
        assert!(!serde_json::to_string(&m["vieja"]).unwrap().contains("nombres_anteriores"));
        // Errores: nombre inválido, el mismo nombre actual o repetido.
        for (lista, txt) in [("[Vieja]", "no válido"), ("[nueva]", "nombre actual"), ("[a, a]", "repetido")] {
            let e = Receta::desde_yaml(&y.replace("[vieja, otra-vieja]", lista)).unwrap_err();
            assert!(e.contains(txt), "{lista}: {e}");
        }
    }

    #[test]
    fn opciones_numericas() {
        let y = "nombre: x\nparametros:\n  - { nombre: monto, opciones: [1, 2, 5] }\npasos:\n  - marcar: '*1#'\n";
        let r = Receta::desde_yaml(y).unwrap();
        assert_eq!(r.parametros[0].opciones.as_ref().unwrap(), &vec!["1", "2", "5"]);
    }
}
