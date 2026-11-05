//! La carpeta de recetas: cargar, crear, reemplazar, retirar y ver versiones viejas (ver "Editar recetas desde la
//! interfaz" en `docs/protocolo.md`).
//!
//! - Un archivo por receta, `<nombre>.yaml`. Los de `viejo/` no se cargan: son el historial.
//! - Antes de reemplazar o retirar, el archivo vigente se copia (o se mueve) a `viejo/AAAA-MM-DD_HHMMSS_<nombre>.yaml`.
//!   Nunca se borra nada.
//! - Se escribe en un temporal de la misma carpeta y se renombra: nunca queda un archivo a medias.
//! - Quien llama serializa las escrituras (un candado) y vuelve a cargar la carpeta después.

use crate::receta::{version_de, ErrorReceta, Receta};
use serde::Serialize;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Carpeta del historial de versiones, dentro de la carpeta de recetas.
pub const VIEJO: &str = "viejo";

/// Una receta cargada con su archivo: el texto se guarda para mostrarlo y para la versión.
#[derive(Debug, Clone)]
pub struct RecetaCargada {
    pub receta: Receta,
    /// Nombre del archivo, p. ej. `tigo-saldo.yaml`.
    pub archivo: String,
    pub texto: String,
    /// SHA-256 del texto, en hex.
    pub version: String,
    /// Fecha de modificación del archivo (epoch ms).
    pub modificada_ms: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ErrorCarpeta {
    /// La receta no pasa la validación (los mismos errores de la carga).
    Invalida(Vec<ErrorReceta>),
    Parametro(String),
    Existe(String),
    Conflicto(String),
    NoExiste(String),
    Io(String),
}

impl std::fmt::Display for ErrorCarpeta {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ErrorCarpeta::Invalida(e) => {
                let v: Vec<String> = e.iter().map(|x| x.to_string()).collect();
                write!(f, "receta inválida: {}", v.join("; "))
            }
            ErrorCarpeta::Parametro(m)
            | ErrorCarpeta::Existe(m)
            | ErrorCarpeta::Conflicto(m)
            | ErrorCarpeta::NoExiste(m)
            | ErrorCarpeta::Io(m) => f.write_str(m),
        }
    }
}

fn io(ctx: &str, e: std::io::Error) -> ErrorCarpeta {
    ErrorCarpeta::Io(format!("{ctx}: {e}"))
}

/// Un `nombre` que sirve de nombre de archivo sin riesgo: `[a-z0-9-]+`, sin `..`, `/` ni `\`.
pub fn nombre_valido(n: &str) -> bool {
    !n.is_empty() && n.len() <= 80 && n.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn ms(t: SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn es_yaml(p: &Path) -> bool {
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
    let oculto = p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with('.'));
    (ext == "yaml" || ext == "yml") && !oculto
}

/// Carga las recetas de la carpeta (solo archivos de primer nivel: `viejo/` no). Las inválidas se reportan aparte.
pub fn cargar(dir: &Path) -> (BTreeMap<String, RecetaCargada>, Vec<String>) {
    let mut ok: BTreeMap<String, RecetaCargada> = BTreeMap::new();
    let mut errores = vec![];
    let Ok(rd) = std::fs::read_dir(dir) else {
        errores.push(format!("no pude leer la carpeta de recetas {}", dir.display()));
        return (ok, errores);
    };
    let mut rutas: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_file() && es_yaml(p)).collect();
    rutas.sort();
    for p in rutas {
        match cargar_archivo(&p) {
            Ok(c) => {
                if ok.contains_key(&c.receta.nombre) {
                    errores.push(format!("{}: nombre repetido '{}'", p.display(), c.receta.nombre));
                } else {
                    ok.insert(c.receta.nombre.clone(), c);
                }
            }
            Err(e) => errores.push(e),
        }
    }
    // Un nombre anterior que choca con otra receta no impide cargar (el nombre actual manda al buscar), pero se avisa.
    let todas: Vec<&Receta> = ok.values().map(|c| &c.receta).collect();
    for c in ok.values() {
        if let Some(m) = choque_de_nombres(&c.receta, todas.iter().copied()) {
            errores.push(format!("{}: {m}", c.archivo));
        }
    }
    (ok, errores)
}

/// ¿Algún nombre de `r` (el actual o uno anterior) es también el nombre o un nombre anterior de otra receta? Así
/// buscar por un nombre viejo nunca es ambiguo.
pub fn choque_de_nombres<'a>(r: &Receta, otras: impl IntoIterator<Item = &'a Receta>) -> Option<String> {
    for o in otras.into_iter().filter(|o| o.nombre != r.nombre) {
        for n in &r.nombres_anteriores {
            if *n == o.nombre {
                return Some(format!("el nombre anterior '{n}' es el nombre de otra receta"));
            }
            if o.nombres_anteriores.contains(n) {
                return Some(format!("el nombre anterior '{n}' también es nombre anterior de '{}'", o.nombre));
            }
        }
        if o.nombres_anteriores.contains(&r.nombre) {
            return Some(format!("'{}' es un nombre anterior de la receta '{}'", r.nombre, o.nombre));
        }
    }
    None
}

fn cargar_archivo(p: &Path) -> Result<RecetaCargada, String> {
    let texto = std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?;
    let mut receta = Receta::desde_yaml(&texto).map_err(|e| format!("{}: {e}", p.display()))?;
    let version = version_de(&texto);
    receta.version = Some(version.clone());
    let modificada_ms = std::fs::metadata(p).and_then(|m| m.modified()).map(ms).unwrap_or(0);
    let archivo = p.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
    Ok(RecetaCargada { receta, archivo, texto, version, modificada_ms })
}

/// Lee y valida un YAML. Todos los errores juntos.
pub fn leer_valida(yaml: &str) -> Result<Receta, ErrorCarpeta> {
    let r = Receta::leer(yaml).map_err(|e| ErrorCarpeta::Invalida(vec![e]))?;
    let errores = r.errores();
    if !errores.is_empty() {
        return Err(ErrorCarpeta::Invalida(errores));
    }
    Ok(r)
}

/// Escribe `texto` en `destino` por un temporal de la misma carpeta + renombrar.
fn escribir_atomico(destino: &Path, texto: &str) -> Result<(), ErrorCarpeta> {
    let dir = destino.parent().unwrap_or(Path::new("."));
    let nombre = destino.file_name().and_then(|n| n.to_str()).unwrap_or("receta");
    // Empieza con '.' y termina en '.tmp': la carga nunca lo toma por una receta.
    let tmp = dir.join(format!(".{nombre}.{}.tmp", std::process::id()));
    let r = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(texto.as_bytes())?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, destino)
    })();
    if let Err(e) = r {
        let _ = std::fs::remove_file(&tmp);
        return Err(io(&format!("no pude escribir {}", destino.display()), e));
    }
    Ok(())
}

/// Fecha local para los nombres de `viejo/`: `AAAA-MM-DD_HHMMSS`.
fn sello(t: SystemTime) -> String {
    chrono::DateTime::<chrono::Local>::from(t).format("%Y-%m-%d_%H%M%S").to_string()
}

/// Ruta libre en `viejo/` para guardar la versión de `nombre` (si el segundo ya está usado, el siguiente).
fn destino_viejo(dir: &Path, nombre: &str) -> Result<PathBuf, ErrorCarpeta> {
    let viejo = dir.join(VIEJO);
    std::fs::create_dir_all(&viejo).map_err(|e| io(&format!("no pude crear {}", viejo.display()), e))?;
    let mut t = SystemTime::now();
    for _ in 0..600 {
        let p = viejo.join(format!("{}_{nombre}.yaml", sello(t)));
        if !p.exists() {
            return Ok(p);
        }
        t += std::time::Duration::from_secs(1);
    }
    Err(ErrorCarpeta::Io(format!("no encontré un nombre libre en {}", viejo.display())))
}

/// Copia el archivo vigente a `viejo/` conservando su fecha (la fecha es cuándo se guardó esa versión).
fn copiar_a_viejo(dir: &Path, nombre: &str, origen: &Path) -> Result<PathBuf, ErrorCarpeta> {
    let destino = destino_viejo(dir, nombre)?;
    let fecha = std::fs::metadata(origen).and_then(|m| m.modified()).ok();
    std::fs::copy(origen, &destino).map_err(|e| io(&format!("no pude copiar {} a viejo/", origen.display()), e))?;
    if let Some(t) = fecha {
        let _ = std::fs::File::options().write(true).open(&destino).and_then(|f| f.set_modified(t));
    }
    Ok(destino)
}

/// Crea una receta nueva (`<nombre>.yaml`). `existentes`: las recetas cargadas.
pub fn crear(dir: &Path, yaml: &str, existentes: &BTreeMap<String, RecetaCargada>) -> Result<Receta, ErrorCarpeta> {
    let r = leer_valida(yaml)?;
    if !nombre_valido(&r.nombre) {
        return Err(ErrorCarpeta::Parametro(format!(
            "nombre '{}' no válido: solo minúsculas, dígitos y guiones (a-z, 0-9, -)",
            r.nombre
        )));
    }
    let ruta = dir.join(format!("{}.yaml", r.nombre));
    if existentes.contains_key(&r.nombre) || ruta.exists() || dir.join(format!("{}.yml", r.nombre)).exists() {
        return Err(ErrorCarpeta::Existe(format!("ya existe una receta '{}'", r.nombre)));
    }
    if let Some(m) = choque_de_nombres(&r, existentes.values().map(|c| &c.receta)) {
        return Err(ErrorCarpeta::Parametro(m));
    }
    escribir_atomico(&ruta, yaml)?;
    Ok(r)
}

/// Reemplaza la receta `actual` con `yaml`. `version_base` es la versión que se leyó: si el archivo cambió desde
/// entonces, `Conflicto` y no se toca nada. La versión anterior queda en `viejo/`.
pub fn reemplazar(
    dir: &Path,
    nombre: &str,
    yaml: &str,
    version_base: &str,
    actual: &RecetaCargada,
    existentes: &BTreeMap<String, RecetaCargada>,
) -> Result<Receta, ErrorCarpeta> {
    let r = leer_valida(yaml)?;
    if r.nombre != nombre {
        return Err(ErrorCarpeta::Parametro(format!(
            "el nombre del YAML ('{}') no es el de la ruta ('{nombre}'): para renombrar, crea una nueva y retira la vieja",
            r.nombre
        )));
    }
    if let Some(m) = choque_de_nombres(&r, existentes.values().map(|c| &c.receta)) {
        return Err(ErrorCarpeta::Parametro(m));
    }
    let ruta = dir.join(&actual.archivo);
    // Se compara con el archivo en disco, no con lo cargado: también detecta un cambio a mano sin recargar.
    let en_disco = std::fs::read_to_string(&ruta)
        .map_err(|_| ErrorCarpeta::Conflicto(format!("el archivo {} ya no existe", actual.archivo)))?;
    if version_de(&en_disco) != version_base.trim() {
        return Err(ErrorCarpeta::Conflicto(format!(
            "la receta '{nombre}' cambió desde que la leíste: recárgala y vuelve a aplicar tus cambios"
        )));
    }
    if en_disco == yaml {
        return Ok(r);
    }
    copiar_a_viejo(dir, nombre, &ruta)?;
    escribir_atomico(&ruta, yaml)?;
    Ok(r)
}

/// Retira la receta: mueve su archivo a `viejo/`. Devuelve el id de esa versión.
pub fn retirar(dir: &Path, actual: &RecetaCargada) -> Result<String, ErrorCarpeta> {
    let ruta = dir.join(&actual.archivo);
    if !ruta.is_file() {
        return Err(ErrorCarpeta::NoExiste(format!("el archivo {} ya no existe", actual.archivo)));
    }
    let destino = destino_viejo(dir, &actual.receta.nombre)?;
    std::fs::rename(&ruta, &destino).map_err(|e| io(&format!("no pude mover {} a viejo/", ruta.display()), e))?;
    Ok(destino.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string())
}

/// Una versión de una receta: la vigente o una copia en `viejo/`.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Version {
    pub id: String,
    pub guardada_ms: i64,
    pub version: String,
    pub actual: bool,
}

/// `AAAA-MM-DD_HHMMSS_<nombre>.yaml` → `nombre`.
fn nombre_de_copia(archivo: &str) -> Option<&str> {
    let b = archivo.as_bytes();
    let fecha_ok = b.len() > 18
        && b[..18].iter().enumerate().all(|(i, c)| match i {
            4 | 7 => *c == b'-',
            10 | 17 => *c == b'_',
            _ => c.is_ascii_digit(),
        });
    if !fecha_ok {
        return None;
    }
    archivo[18..].strip_suffix(".yaml").or_else(|| archivo[18..].strip_suffix(".yml"))
}

/// Versiones de `nombre`, la más nueva primero; la vigente (si la hay) va primera con `actual: true`. Incluye las
/// copias guardadas con los `nombres_anteriores` de la vigente (de antes de renombrarla).
pub fn versiones(dir: &Path, nombre: &str, actual: Option<&RecetaCargada>) -> Vec<Version> {
    let mut nombres: Vec<&str> = vec![nombre];
    if let Some(a) = actual {
        nombres.extend(a.receta.todos_los_nombres());
    }
    let mut v: Vec<Version> = vec![];
    if let Ok(rd) = std::fs::read_dir(dir.join(VIEJO)) {
        for e in rd.flatten() {
            let p = e.path();
            let Some(archivo) = p.file_name().and_then(|n| n.to_str()).map(str::to_string) else { continue };
            if !p.is_file() || !nombre_de_copia(&archivo).is_some_and(|n| nombres.contains(&n)) {
                continue;
            }
            let Ok(texto) = std::fs::read_to_string(&p) else { continue };
            let guardada_ms = e.metadata().and_then(|m| m.modified()).map(ms).unwrap_or(0);
            v.push(Version { id: archivo, guardada_ms, version: version_de(&texto), actual: false });
        }
    }
    // El nombre lleva la fecha en que se reemplazó: desempata entre copias con la misma fecha de archivo.
    v.sort_by(|a, b| b.guardada_ms.cmp(&a.guardada_ms).then_with(|| b.id.cmp(&a.id)));
    if let Some(a) = actual {
        v.insert(0, Version { id: a.archivo.clone(), guardada_ms: a.modificada_ms, version: a.version.clone(), actual: true });
    }
    v
}

/// El texto de una versión. `id` debe ser uno de los de [`versiones`] (así no se puede leer otro archivo).
pub fn leer_version(dir: &Path, nombre: &str, id: &str, actual: Option<&RecetaCargada>) -> Option<(String, String)> {
    let v = versiones(dir, nombre, actual).into_iter().find(|v| v.id == id)?;
    if v.actual {
        let a = actual?;
        return Some((a.texto.clone(), a.version.clone()));
    }
    let texto = std::fs::read_to_string(dir.join(VIEJO).join(&v.id)).ok()?;
    let version = version_de(&texto);
    Some((texto, version))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SALDO: &str = "nombre: prueba-saldo\ntitulo: Saldo\noperador: TIGO\npasos:\n  - marcar: '*105#'\n    esperar: 'Saldo'\n";

    fn temporal(n: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("qhana-carpeta-{n}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn nombres_seguros() {
        assert!(nombre_valido("tigo-saldo") && nombre_valido("entel2"));
        for n in ["", "../x", "a/b", "a\\b", "Tigo", "a.b", "a b", ".."] {
            assert!(!nombre_valido(n), "{n}");
        }
    }

    #[test]
    fn crear_reemplazar_retirar_y_versiones() {
        let d = temporal("ciclo");
        let (cargadas, _) = cargar(&d);
        crear(&d, SALDO, &cargadas).unwrap();
        let (cargadas, errores) = cargar(&d);
        assert!(errores.is_empty(), "{errores:?}");
        let c = cargadas["prueba-saldo"].clone();
        assert_eq!(c.archivo, "prueba-saldo.yaml");
        assert_eq!(c.version, version_de(SALDO));
        assert_eq!(c.receta.version.as_deref(), Some(c.version.as_str()));

        // Ya existe.
        assert!(matches!(crear(&d, SALDO, &cargadas), Err(ErrorCarpeta::Existe(_))));
        // Inválida: errores con paso.
        let mala = SALDO.replace("'Saldo'", "'Sal(do'");
        match crear(&d, &mala.replace("prueba-saldo", "otra"), &cargadas) {
            Err(ErrorCarpeta::Invalida(e)) => assert_eq!(e[0].paso, Some(1)),
            otro => panic!("{otro:?}"),
        }
        // Nombre con ruta: rechazado.
        assert!(matches!(
            crear(&d, &SALDO.replace("prueba-saldo", "../fuera"), &cargadas),
            Err(ErrorCarpeta::Parametro(_))
        ));
        assert!(!d.parent().unwrap().join("fuera.yaml").exists());

        // Reemplazar con versión vieja: conflicto, nada cambia.
        let nuevo = SALDO.replace("titulo: Saldo", "titulo: Saldo nuevo");
        assert!(matches!(reemplazar(&d, "prueba-saldo", &nuevo, "abc", &c, &BTreeMap::new()), Err(ErrorCarpeta::Conflicto(_))));
        // Nombre distinto al de la ruta.
        assert!(matches!(
            reemplazar(&d, "prueba-saldo", &nuevo.replace("prueba-saldo", "otra"), &c.version, &c, &BTreeMap::new()),
            Err(ErrorCarpeta::Parametro(_))
        ));
        assert_eq!(std::fs::read_to_string(d.join("prueba-saldo.yaml")).unwrap(), SALDO);
        reemplazar(&d, "prueba-saldo", &nuevo, &c.version, &c, &BTreeMap::new()).unwrap();
        assert_eq!(std::fs::read_to_string(d.join("prueba-saldo.yaml")).unwrap(), nuevo);

        // La copia en viejo/ no se carga como receta.
        let (cargadas, errores) = cargar(&d);
        assert!(errores.is_empty(), "{errores:?}");
        assert_eq!(cargadas.len(), 1);
        let c2 = cargadas["prueba-saldo"].clone();
        let vs = versiones(&d, "prueba-saldo", Some(&c2));
        assert_eq!(vs.len(), 2);
        assert!(vs[0].actual && vs[0].id == "prueba-saldo.yaml" && vs[0].version == version_de(&nuevo));
        assert!(!vs[1].actual && vs[1].version == version_de(SALDO));
        assert!(nombre_de_copia(&vs[1].id) == Some("prueba-saldo"));
        assert_eq!(leer_version(&d, "prueba-saldo", &vs[1].id, Some(&c2)).unwrap().0, SALDO);
        assert_eq!(leer_version(&d, "prueba-saldo", "prueba-saldo.yaml", Some(&c2)).unwrap().0, nuevo);
        // Ids que no están en la lista (rutas, otros archivos): nada.
        for id in ["../prueba-saldo.yaml", "viejo/../prueba-saldo.yaml", "/etc/passwd", "x.yaml"] {
            assert!(leer_version(&d, "prueba-saldo", id, Some(&c2)).is_none(), "{id}");
        }

        // Retirar: el archivo pasa a viejo/, nunca se borra.
        let id = retirar(&d, &c2).unwrap();
        assert!(!d.join("prueba-saldo.yaml").exists());
        assert!(d.join(VIEJO).join(&id).is_file());
        let (cargadas, _) = cargar(&d);
        assert!(cargadas.is_empty());
        let vs = versiones(&d, "prueba-saldo", None);
        assert_eq!(vs.len(), 2);
        assert!(vs.iter().all(|v| !v.actual));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn renombrada_ve_las_versiones_de_sus_nombres_anteriores() {
        let d = temporal("renombrada");
        std::fs::create_dir_all(d.join(VIEJO)).unwrap();
        // Historial de cuando se llamaba `prueba-vieja` (y de otra receta, que no cuenta).
        std::fs::write(d.join(VIEJO).join("2026-11-01_100000_prueba-vieja.yaml"), SALDO.replace("prueba-saldo", "prueba-vieja")).unwrap();
        std::fs::write(d.join(VIEJO).join("2026-11-02_100000_otra.yaml"), SALDO.replace("prueba-saldo", "otra")).unwrap();
        let nueva = SALDO.replace("nombre: prueba-saldo\n", "nombre: prueba-saldo\nnombres_anteriores: [prueba-vieja]\n");
        crear(&d, &nueva, &BTreeMap::new()).unwrap();
        let (cargadas, errores) = cargar(&d);
        assert!(errores.is_empty(), "{errores:?}");
        let c = &cargadas["prueba-saldo"];
        let vs = versiones(&d, "prueba-saldo", Some(c));
        let ids: Vec<&str> = vs.iter().map(|v| v.id.as_str()).collect();
        assert_eq!(ids, ["prueba-saldo.yaml", "2026-11-01_100000_prueba-vieja.yaml"]);
        assert!(leer_version(&d, "prueba-saldo", &vs[1].id, Some(c)).unwrap().0.contains("nombre: prueba-vieja"));
        // Por el nombre viejo (la API resuelve la receta renombrada): lo mismo.
        assert_eq!(versiones(&d, "prueba-vieja", Some(c)).len(), 2);

        // Un nombre anterior no puede ser el de otra receta, ni el de una nueva puede ser un nombre anterior.
        let otra = SALDO.replace("prueba-saldo", "otra-receta");
        crear(&d, &otra, &cargadas).unwrap();
        let (cargadas, _) = cargar(&d);
        let choca = nueva.replace("[prueba-vieja]", "[prueba-vieja, otra-receta]");
        let c = cargadas["prueba-saldo"].clone();
        assert!(matches!(reemplazar(&d, "prueba-saldo", &choca, &c.version, &c, &cargadas), Err(ErrorCarpeta::Parametro(_))));
        let robada = SALDO.replace("prueba-saldo", "prueba-vieja");
        match crear(&d, &robada, &cargadas) {
            Err(ErrorCarpeta::Parametro(m)) => assert!(m.contains("nombre anterior"), "{m}"),
            otro => panic!("{otro:?}"),
        }
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn reemplazar_igual_no_guarda_copia() {
        let d = temporal("igual");
        crear(&d, SALDO, &BTreeMap::new()).unwrap();
        let (c, _) = cargar(&d);
        let c = &c["prueba-saldo"];
        reemplazar(&d, "prueba-saldo", SALDO, &c.version, c, &BTreeMap::new()).unwrap();
        assert!(versiones(&d, "prueba-saldo", None).is_empty());
        let _ = std::fs::remove_dir_all(d);
    }
}
