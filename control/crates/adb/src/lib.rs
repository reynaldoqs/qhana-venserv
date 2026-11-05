//! Teléfonos por ADB.
//!
//! El gestor revisa cada 3 s los teléfonos conectados por USB y:
//! - aplica `adb reverse tcp:47800 tcp:47800`, para que el agente llegue al control por el cable;
//! - lee `android_id` para cruzar el serial de ADB con el `dispositivo_id` del agente;
//! - avisa cambios (aparece, desaparece, `unauthorized`…) por el bus de eventos.
//!
//! Con agentes por Wi-Fi el gestor es opcional (`--sin-adb`): solo sirve para provisionar.

use nucleo::{Bus, Registro};
use protocolo::{tipos_evento as ev, InfoAdb};
use serde::Serialize;
use serde_json::json;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::process::Command;

const INTERVALO: Duration = Duration::from_secs(3);
/// Cada cuántas vueltas se vuelve a comprobar el reverse de un teléfono ya configurado (~30 s).
const VUELTAS_REVISION: u32 = 10;
const ESPERA_COMANDO: Duration = Duration::from_secs(20);
const ESPERA_INSTALAR: Duration = Duration::from_secs(180);
/// Si entre dos vueltas el reloj avanzó esto de más, el PC estuvo dormido.
const SALTO_SUENO: Duration = Duration::from_secs(15);

/// Ejecutable de ADB.
#[derive(Debug, Clone)]
pub struct Adb {
    ruta: PathBuf,
}

impl Adb {
    pub fn new(ruta: impl Into<PathBuf>) -> Self {
        Self { ruta: ruta.into() }
    }

    pub fn ruta(&self) -> &Path {
        &self.ruta
    }

    /// Busca adb: ruta dada → PATH → SDK de Android en su lugar por defecto (Windows, macOS, Linux).
    pub fn resolver(config: Option<PathBuf>) -> Option<Self> {
        if let Some(p) = config {
            return Some(Self::new(p));
        }
        let nombre = if cfg!(windows) { "adb.exe" } else { "adb" };
        if let Some(path) = std::env::var_os("PATH") {
            for dir in std::env::split_paths(&path) {
                let c = dir.join(nombre);
                if c.is_file() {
                    return Some(Self::new(c));
                }
            }
        }
        let mut candidatos = vec![];
        if let Some(sdk) = std::env::var_os("ANDROID_HOME").or_else(|| std::env::var_os("ANDROID_SDK_ROOT")) {
            candidatos.push(PathBuf::from(sdk).join("platform-tools").join(nombre));
        }
        if let Some(l) = std::env::var_os("LOCALAPPDATA") {
            candidatos.push(PathBuf::from(l).join("Android\\Sdk\\platform-tools").join(nombre));
        }
        if let Some(h) = std::env::var_os("HOME") {
            candidatos.push(PathBuf::from(&h).join("Library/Android/sdk/platform-tools").join(nombre));
            candidatos.push(PathBuf::from(&h).join("Android/Sdk/platform-tools").join(nombre));
        }
        candidatos.into_iter().find(|c| c.is_file()).map(Self::new)
    }

    /// Ejecuta adb y devuelve stdout. Error = stderr (o stdout) recortado.
    pub async fn ejecutar(&self, args: &[&str], espera: Duration) -> Result<String, String> {
        let mut cmd = Command::new(&self.ruta);
        cmd.args(args).kill_on_drop(true);
        #[cfg(windows)]
        {
            // Sin ventanas de consola cuando corre como servicio o detrás de la interfaz.
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let salida = tokio::time::timeout(espera, cmd.output())
            .await
            .map_err(|_| format!("adb {} tardó más de {}s", args.join(" "), espera.as_secs()))?
            .map_err(|e| format!("no pude ejecutar {}: {e}", self.ruta.display()))?;
        let out = String::from_utf8_lossy(&salida.stdout).to_string();
        if salida.status.success() {
            Ok(out)
        } else {
            let err = String::from_utf8_lossy(&salida.stderr).trim().to_string();
            Err(if err.is_empty() { out.trim().to_string() } else { err })
        }
    }

    async fn en(&self, serial: &str, args: &[&str]) -> Result<String, String> {
        let mut a = vec!["-s", serial];
        a.extend_from_slice(args);
        self.ejecutar(&a, ESPERA_COMANDO).await
    }

    pub async fn shell(&self, serial: &str, comando: &str) -> Result<String, String> {
        self.en(serial, &["shell", comando]).await
    }

    /// `adb devices -l` → (serial, estado, modelo).
    pub async fn dispositivos(&self) -> Result<Vec<(String, String, Option<String>)>, String> {
        let out = self.ejecutar(&["devices", "-l"], ESPERA_COMANDO).await?;
        Ok(parsear_devices(&out))
    }

    pub async fn reverse_activo(&self, serial: &str, puerto: u16) -> bool {
        let marca = format!("tcp:{puerto}");
        match self.en(serial, &["reverse", "--list"]).await {
            Ok(out) => out.lines().any(|l| l.split_whitespace().nth(1) == Some(marca.as_str())),
            Err(_) => false,
        }
    }

    pub async fn reverse(&self, serial: &str, puerto: u16) -> Result<(), String> {
        let t = format!("tcp:{puerto}");
        self.en(serial, &["reverse", &t, &t]).await.map(|_| ())
    }

    pub async fn android_id(&self, serial: &str) -> Option<String> {
        self.shell(serial, "settings get secure android_id")
            .await
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty() && s != "null")
    }

    pub async fn instalar(&self, serial: &str, apk: &Path) -> Result<String, String> {
        let apk = apk.to_string_lossy().to_string();
        self.ejecutar(&["-s", serial, "install", "-r", "-g", &apk], ESPERA_INSTALAR).await
    }

    /// Deja un teléfono listo para trabajar 24/7 con el agente. Devuelve el resultado de cada paso.
    pub async fn provisionar(&self, serial: &str, p: &Provision, puerto: u16, apk: Option<&Path>) -> Vec<PasoProvision> {
        let mut pasos = vec![];
        let mut anotar = |paso: &str, r: Result<String, String>| {
            pasos.push(PasoProvision {
                paso: paso.to_string(),
                ok: r.is_ok(),
                salida: r.unwrap_or_else(|e| e).trim().chars().take(300).collect(),
            })
        };
        if let Some(apk) = apk {
            anotar("instalar", self.instalar(serial, apk).await);
        }
        for permiso in &p.permisos {
            anotar(&format!("permiso {permiso}"), self.shell(serial, &format!("pm grant {} {permiso}", p.paquete)).await);
        }
        anotar(
            "segundo plano sin límites",
            self.shell(serial, &format!("cmd appops set {} RUN_ANY_IN_BACKGROUND allow", p.paquete)).await,
        );
        anotar(
            "sin optimización de batería",
            self.shell(serial, &format!("dumpsys deviceidle whitelist +{}", p.paquete)).await,
        );
        anotar("pantalla encendida con USB", self.shell(serial, "svc power stayon usb").await);
        // Accesibilidad: agregar nuestro servicio sin borrar los que ya estén activos.
        let actuales = self
            .shell(serial, "settings get secure enabled_accessibility_services")
            .await
            .unwrap_or_default();
        let actuales = actuales.trim();
        let lista = if actuales.is_empty() || actuales == "null" {
            p.servicio_accesibilidad.clone()
        } else if actuales.split(':').any(|s| s == p.servicio_accesibilidad) {
            actuales.to_string()
        } else {
            format!("{actuales}:{}", p.servicio_accesibilidad)
        };
        anotar(
            "activar accesibilidad",
            self.shell(serial, &format!("settings put secure enabled_accessibility_services {lista}")).await,
        );
        anotar("accesibilidad encendida", self.shell(serial, "settings put secure accessibility_enabled 1").await);
        anotar("adb reverse", self.reverse(serial, puerto).await.map(|_| "ok".into()));
        // El serial se le pasa a la app para que el control cruce teléfono ↔ USB (ver `adb_serial` en docs/protocolo.md).
        anotar("abrir la app", self.shell(serial, &format!("am start -n {} --es adb_serial {serial}", p.actividad)).await);
        pasos
    }
}

/// Identidad de la app agente en el teléfono (debe coincidir con `agente-android`).
#[derive(Debug, Clone, Serialize)]
pub struct Provision {
    pub paquete: String,
    /// `paquete/.Clase` del servicio de accesibilidad.
    pub servicio_accesibilidad: String,
    /// `paquete/.Clase` de la actividad principal.
    pub actividad: String,
    pub permisos: Vec<String>,
}

impl Provision {
    pub fn para_paquete(paquete: &str, servicio: &str, actividad: &str) -> Self {
        Self {
            paquete: paquete.into(),
            servicio_accesibilidad: servicio.into(),
            actividad: actividad.into(),
            permisos: [
                "CALL_PHONE",
                "READ_PHONE_STATE",
                "READ_PHONE_NUMBERS",
                "RECEIVE_SMS",
                "READ_SMS",
                "POST_NOTIFICATIONS",
            ]
            .iter()
            .map(|p| format!("android.permission.{p}"))
            .collect(),
        }
    }
}

/// Resultado de "Reconectar" para un teléfono de ADB.
#[derive(Debug, Clone, Serialize)]
pub struct ReconexionAdb {
    pub serial: String,
    pub reverse_ok: bool,
    /// Se abrió la app con `reconectar` (su agente no estaba conectado).
    pub app_abierta: bool,
    pub detalle: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PasoProvision {
    pub paso: String,
    pub ok: bool,
    pub salida: String,
}

pub fn parsear_devices(out: &str) -> Vec<(String, String, Option<String>)> {
    out.lines()
        .skip_while(|l| !l.starts_with("List of devices"))
        .skip(1)
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let serial = it.next()?.to_string();
            let estado = it.next()?.to_string();
            let modelo = it.find_map(|kv| kv.strip_prefix("model:")).map(|m| m.replace('_', " "));
            Some((serial, estado, modelo))
        })
        .collect()
}

#[derive(Debug, Clone)]
struct Seguimiento {
    info: InfoAdb,
    vueltas: u32,
}

/// Bucle de supervisión de ADB.
pub struct GestorAdb {
    adb: Adb,
    puerto: u16,
    estado: RwLock<BTreeMap<String, Seguimiento>>,
    registro: Arc<Registro>,
    bus: Bus,
    disponible: std::sync::atomic::AtomicBool,
}

impl GestorAdb {
    pub fn iniciar(adb: Adb, puerto: u16, registro: Arc<Registro>, bus: Bus) -> Arc<Self> {
        let g = Arc::new(Self {
            adb,
            puerto,
            estado: RwLock::new(BTreeMap::new()),
            registro,
            bus,
            disponible: std::sync::atomic::AtomicBool::new(false),
        });
        let g2 = g.clone();
        tokio::spawn(async move { g2.bucle().await });
        g
    }

    pub fn adb(&self) -> &Adb {
        &self.adb
    }

    pub fn disponible(&self) -> bool {
        self.disponible.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn listar(&self) -> Vec<InfoAdb> {
        self.estado.read().unwrap().values().map(|s| s.info.clone()).collect()
    }

    async fn bucle(self: Arc<Self>) {
        let mut avisado = false;
        let mut forzar = false;
        loop {
            match self.adb.dispositivos().await {
                Ok(lista) => {
                    if !self.disponible() {
                        tracing::info!(adb = %self.adb.ruta.display(), "ADB disponible");
                    }
                    self.disponible.store(true, std::sync::atomic::Ordering::Relaxed);
                    avisado = false;
                    self.vuelta(lista, std::mem::take(&mut forzar)).await;
                    // Si el reloj de pared avanzó mucho más que la pausa, el PC durmió: al despertar el túnel
                    // `adb reverse` puede haberse perdido, así que la próxima vuelta lo revisa en todos.
                    let antes = std::time::SystemTime::now();
                    tokio::time::sleep(INTERVALO).await;
                    let paso = std::time::SystemTime::now().duration_since(antes).unwrap_or_default();
                    if paso > INTERVALO + SALTO_SUENO {
                        tracing::info!(segundos = paso.as_secs(), "pausa larga (¿el PC durmió?): reviso el túnel de todos");
                        forzar = true;
                    }
                }
                Err(e) => {
                    self.disponible.store(false, std::sync::atomic::Ordering::Relaxed);
                    if !avisado {
                        tracing::warn!("ADB no responde ({e}); reintento cada 30 s");
                        avisado = true;
                    }
                    tokio::time::sleep(Duration::from_secs(30)).await;
                }
            }
        }
    }

    /// Botón "Reconectar": vuelve a aplicar `adb reverse` en cada teléfono listo y, si su agente no está
    /// conectado, abre la app con `reconectar` para que conecte ya (sin esperar su pausa de hasta 30 s).
    pub async fn reconectar(&self, p: &Provision) -> Vec<ReconexionAdb> {
        let listos: Vec<String> = self
            .estado
            .read()
            .unwrap()
            .iter()
            .filter(|(_, s)| s.info.estado == "device")
            .map(|(serial, _)| serial.clone())
            .collect();
        let mut out = vec![];
        for serial in listos {
            let r = self.adb.reverse(&serial, self.puerto).await;
            let reverse_ok = r.is_ok();
            let mut detalle = r.err();
            let cambio = {
                let mut estado = self.estado.write().unwrap();
                match estado.get_mut(&serial) {
                    Some(s) if s.info.reverse_ok != reverse_ok => {
                        s.info.reverse_ok = reverse_ok;
                        true
                    }
                    _ => false,
                }
            };
            if cambio {
                self.bus.emitir(ev::ADB, json!({ "accion": "reverse", "serial": serial, "ok": reverse_ok }));
            }
            let mut app_abierta = false;
            if reverse_ok && !self.registro.conectado_por_serial(&serial) {
                let cmd = format!("am start -n {} --es adb_serial {serial} --ez reconectar true", p.actividad);
                match self.adb.shell(&serial, &cmd).await {
                    // `am start` puede salir con 0 y aun así fallar ("Error: Activity class … does not exist").
                    Ok(salida) if salida.contains("Error") => detalle = Some(salida.trim().chars().take(200).collect()),
                    Ok(_) => app_abierta = true,
                    Err(e) => detalle = Some(e),
                }
            }
            tracing::info!(%serial, reverse_ok, app_abierta, ?detalle, "reconectar");
            out.push(ReconexionAdb { serial, reverse_ok, app_abierta, detalle });
        }
        out
    }

    async fn vuelta(&self, lista: Vec<(String, String, Option<String>)>, forzar: bool) {
        // Desaparecidos.
        let presentes: Vec<&String> = lista.iter().map(|(s, _, _)| s).collect();
        let idos: Vec<String> = self
            .estado
            .read()
            .unwrap()
            .keys()
            .filter(|s| !presentes.contains(s))
            .cloned()
            .collect();
        for serial in idos {
            self.estado.write().unwrap().remove(&serial);
            tracing::info!(%serial, "teléfono desconectado de ADB");
            if let Some(d) = self.registro.desvincular_adb(&serial) {
                self.bus.emitir(ev::DISPOSITIVO_ACTUALIZADO, json!({ "dispositivo_id": d.id, "dispositivo": d }));
            }
            self.bus.emitir(ev::ADB, json!({ "accion": "desaparecio", "serial": serial }));
        }

        for (serial, estado, modelo) in lista {
            let previo = self.estado.read().unwrap().get(&serial).cloned();
            let mut seg = previo.clone().unwrap_or(Seguimiento {
                info: InfoAdb { serial: serial.clone(), estado: estado.clone(), modelo: modelo.clone(), reverse_ok: false, dispositivo_id: None },
                vueltas: 0,
            });
            let cambio_estado = previo.as_ref().map(|p| p.info.estado != estado).unwrap_or(true);
            seg.info.estado = estado.clone();
            seg.info.modelo = modelo.or(seg.info.modelo);

            if estado == "device" {
                // Un teléfono sin agente conectado se revisa en cada vuelta: puede estar esperando el túnel.
                let revisar = forzar
                    || !seg.info.reverse_ok
                    || seg.vueltas % VUELTAS_REVISION == 0
                    || cambio_estado
                    || !self.registro.conectado_por_serial(&serial);
                if revisar {
                    let ok = if self.adb.reverse_activo(&serial, self.puerto).await {
                        true
                    } else {
                        match self.adb.reverse(&serial, self.puerto).await {
                            Ok(()) => {
                                tracing::info!(%serial, puerto = self.puerto, "adb reverse aplicado");
                                true
                            }
                            Err(e) => {
                                tracing::warn!(%serial, "adb reverse falló: {e}");
                                false
                            }
                        }
                    };
                    if ok != seg.info.reverse_ok {
                        self.bus.emitir(ev::ADB, json!({ "accion": "reverse", "serial": serial, "ok": ok }));
                    }
                    seg.info.reverse_ok = ok;
                }
                if seg.info.dispositivo_id.is_none() {
                    if let Some(id) = self.adb.android_id(&serial).await {
                        seg.info.dispositivo_id = Some(id.clone());
                        if let Some(d) = self.registro.vincular_adb(&id, &serial) {
                            self.bus.emitir(ev::DISPOSITIVO_ACTUALIZADO, json!({ "dispositivo_id": d.id, "dispositivo": d }));
                        }
                    }
                }
            } else {
                seg.info.reverse_ok = false;
            }
            seg.vueltas = seg.vueltas.wrapping_add(1);

            if cambio_estado {
                tracing::info!(%serial, %estado, "teléfono en ADB");
                self.bus.emitir(
                    ev::ADB,
                    json!({ "accion": if previo.is_none() { "aparecio" } else { "estado" }, "serial": serial, "info": seg.info }),
                );
            }
            self.estado.write().unwrap().insert(serial, seg);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsea_devices() {
        let out = "List of devices attached\n1026925361012456       device usb:1-1.3 product:X678B-OP model:Infinix_X678B device:Infinix-X678B transport_id:5\nemulator-5554\tunauthorized\n\n";
        let v = parsear_devices(out);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0], ("1026925361012456".into(), "device".into(), Some("Infinix X678B".into())));
        assert_eq!(v[1].1, "unauthorized");
    }

    #[test]
    fn parsea_devices_con_ruido_previo() {
        let out = "* daemon not running; starting now at tcp:5037\n* daemon started successfully\nList of devices attached\nabc\tdevice\n";
        assert_eq!(parsear_devices(out), vec![("abc".into(), "device".into(), None)]);
    }
}
