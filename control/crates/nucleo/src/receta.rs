//! Recetas: flujos USSD de un operador, escritos en YAML (ver `docs/protocolo.md`).

use protocolo::ModoUssd;
use regex::Regex;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

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
    /// `pda`: receta propia de los chips PDA. Sin valor, vale para todos los chips.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chip_tipo: Option<String>,
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
    /// true si algún paso mueve dinero (`confirmar`).
    pub mueve_dinero: bool,
    pub accion: Option<String>,
    pub limites: Option<Limites>,
    pub chip_tipo: Option<String>,
}

impl Receta {
    pub fn desde_yaml(texto: &str) -> Result<Self, String> {
        let r: Receta = serde_yaml::from_str(texto).map_err(|e| e.to_string())?;
        r.validar()?;
        Ok(r)
    }

    pub fn desde_archivo(ruta: &Path) -> Result<Self, String> {
        let t = std::fs::read_to_string(ruta).map_err(|e| format!("{}: {e}", ruta.display()))?;
        Self::desde_yaml(&t).map_err(|e| format!("{}: {e}", ruta.display()))
    }

    /// Carga todas las `*.yaml` / `*.yml` de una carpeta. Las inválidas se reportan aparte.
    pub fn cargar_carpeta(dir: &Path) -> (BTreeMap<String, Receta>, Vec<String>) {
        let mut ok = BTreeMap::new();
        let mut errores = vec![];
        let Ok(rd) = std::fs::read_dir(dir) else {
            errores.push(format!("no pude leer la carpeta de recetas {}", dir.display()));
            return (ok, errores);
        };
        let mut rutas: Vec<_> = rd.flatten().map(|e| e.path()).collect();
        rutas.sort();
        for p in rutas {
            let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ext != "yaml" && ext != "yml" {
                continue;
            }
            match Receta::desde_archivo(&p) {
                Ok(r) => {
                    if ok.contains_key(&r.nombre) {
                        errores.push(format!("{}: nombre repetido '{}'", p.display(), r.nombre));
                    } else {
                        ok.insert(r.nombre.clone(), r);
                    }
                }
                Err(e) => errores.push(e),
            }
        }
        (ok, errores)
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
            mueve_dinero: self.pasos.iter().any(|p| p.confirmar.is_some()),
            accion: self.accion.clone(),
            limites: self.limites.clone(),
            chip_tipo: self.chip_tipo.clone(),
        }
    }

    pub fn validar(&self) -> Result<(), String> {
        if self.nombre.trim().is_empty() {
            return Err("falta 'nombre'".into());
        }
        if let Some(t) = &self.chip_tipo {
            if t != "pda" {
                return Err(format!("chip_tipo '{t}' no válido (solo 'pda')"));
            }
        }
        if let Some(l) = &self.limites {
            if l.comision.is_some_and(|c| !(0.0..1_000.0).contains(&c)) {
                return Err("limites.comision fuera de rango".into());
            }
        }
        if self.pasos.is_empty() {
            return Err("la receta no tiene pasos".into());
        }
        let nombres: Vec<&str> = self.parametros.iter().map(|p| p.nombre.as_str()).collect();
        for p in &self.parametros {
            if let Some(pat) = &p.patron {
                Regex::new(pat).map_err(|e| format!("parámetro '{}': patrón inválido: {e}", p.nombre))?;
            }
        }
        for (i, paso) in self.pasos.iter().enumerate() {
            let n = i + 1;
            let a = paso.accion().map_err(|e| format!("paso {n}: {e}"))?;
            if i == 0 && !matches!(a, Accion::Marcar(_)) {
                return Err("el primer paso debe ser 'marcar'".into());
            }
            // Un segundo 'marcar' vale si la sesión anterior ya cerró; eso solo se sabe al correr (ver motor).
            // Las regex deben compilar con valores de ejemplo en los parámetros.
            let ejemplo: BTreeMap<String, String> =
                nombres.iter().map(|n| (n.to_string(), "0".to_string())).collect();
            let regexes = [
                match a {
                    Accion::Elegir(r) | Accion::Verificar(r) | Accion::Confirmar(r) => Some(r),
                    _ => None,
                },
                paso.esperar.as_deref(),
                paso.error_si.as_deref(),
                paso.exito.as_deref(),
            ];
            for r in regexes.into_iter().flatten() {
                let s = sustituir(r, &ejemplo, true);
                Regex::new(&s).map_err(|e| format!("paso {n}: regex inválida '{r}': {e}"))?;
            }
        }
        if self.modo == ModoUssd::Api && self.pasos.len() != 1 {
            return Err("modo 'api' solo admite un paso 'marcar' (no se puede contestar menús)".into());
        }
        for (k, r) in &self.salida {
            let re = Regex::new(r).map_err(|e| format!("salida '{k}': regex inválida: {e}"))?;
            if re.captures_len() < 2 {
                return Err(format!("salida '{k}': la regex necesita un grupo (…)"));
            }
        }
        Ok(())
    }
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
    fn opciones_numericas() {
        let y = "nombre: x\nparametros:\n  - { nombre: monto, opciones: [1, 2, 5] }\npasos:\n  - marcar: '*1#'\n";
        let r = Receta::desde_yaml(y).unwrap();
        assert_eq!(r.parametros[0].opciones.as_ref().unwrap(), &vec!["1", "2", "5"]);
    }
}
