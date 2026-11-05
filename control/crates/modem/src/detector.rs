//! Detector de módems: cada pocos segundos mira los puertos serie, prueba `AT` en los nuevos,
//! identifica el módem por su IMEI y lo registra como un dispositivo más.
//!
//! - Un módem expone varios puertos (el Huawei E303: `PC UI` y `Modem`, más uno de diagnóstico).
//!   Se queda con el primero que responde AT; los demás de ese mismo IMEI se ignoran.
//! - Se identifica por IMEI, nunca por el número de COM (cambia al reenchufar).
//! - Si el puerto desaparece o falla la E/S, se da de baja y se emite `dispositivo_desconectado`.

use crate::at::Modem;
use crate::canal::CanalModem;
use crate::enlace::{Enlace, Serie};
use nucleo::{Bus, Registro};
use protocolo::{tipos_evento as ev, Hola, Sim};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::task::JoinHandle;

/// Fabricantes de módems conocidos (VID USB): Huawei, ZTE, Qualcomm genérico, Quectel, SIMCom, Telit, Fibocom.
const VIDS: &[u16] = &[0x12D1, 0x19D2, 0x05C6, 0x2C7C, 0x1E0E, 0x1BC7, 0x2CB7];
const REVISAR_SIM: Duration = Duration::from_secs(30);
const REINTENTO_FALLIDO: Duration = Duration::from_secs(30);

#[derive(Debug, Clone)]
pub struct Opciones {
    pub intervalo: Duration,
    /// Puertos fijos (p. ej. `COM5,COM6` o `/dev/ttyUSB2`). `None` = buscar solos.
    pub puertos: Option<Vec<String>>,
    /// Probar también puertos USB de cualquier fabricante.
    pub todos_usb: bool,
}

impl Default for Opciones {
    fn default() -> Self {
        Self { intervalo: Duration::from_secs(3), puertos: None, todos_usb: false }
    }
}

struct Activo {
    canal: CanalModem,
    puerto: String,
    generacion: u64,
    proxima_sim: Instant,
}

enum Sonda {
    Nuevo { puerto: String, canal: Box<CanalModem>, sim: Sim },
    /// Otro puerto de un módem que ya tenemos.
    Duplicado { puerto: String },
    Falla { puerto: String, motivo: String },
}

/// Arranca el detector en una tarea de fondo.
pub fn iniciar(registro: Arc<Registro>, bus: Bus, opciones: Opciones) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut activos: HashMap<String, Activo> = HashMap::new();
        let mut ignorados: HashSet<String> = HashSet::new();
        let mut fallidos: HashMap<String, Instant> = HashMap::new();
        tracing::info!(puertos = ?opciones.puertos, "detector de módems activo");
        loop {
            tick(&registro, &bus, &opciones, &mut activos, &mut ignorados, &mut fallidos).await;
            tokio::time::sleep(opciones.intervalo).await;
        }
    })
}

async fn tick(
    registro: &Arc<Registro>,
    bus: &Bus,
    op: &Opciones,
    activos: &mut HashMap<String, Activo>,
    ignorados: &mut HashSet<String>,
    fallidos: &mut HashMap<String, Instant>,
) {
    let op2 = op.clone();
    let puertos = tokio::task::spawn_blocking(move || listar(&op2)).await.unwrap_or_default();

    // Bajas: el puerto ya no está o la E/S falló.
    let caidos: Vec<String> = activos
        .iter()
        .filter(|(_, a)| !puertos.contains(&a.puerto) || a.canal.muerto())
        .map(|(imei, _)| imei.clone())
        .collect();
    for imei in &caidos {
        if let Some(a) = activos.remove(imei) {
            if registro.desconectar(imei, a.generacion) {
                tracing::info!(%imei, puerto = %a.puerto, "módem desconectado");
                bus.emitir(
                    ev::DISPOSITIVO_DESCONECTADO,
                    json!({ "dispositivo_id": imei, "dispositivo": registro.obtener(imei), "origen": "modem" }),
                );
            }
        }
    }
    if !caidos.is_empty() {
        ignorados.clear(); // los otros puertos de ese módem vuelven a ser candidatos
    }
    ignorados.retain(|p| puertos.contains(p));
    fallidos.retain(|p, hasta| puertos.contains(p) && *hasta > Instant::now());

    // Altas.
    let en_uso: HashSet<String> = activos.values().map(|a| a.puerto.clone()).collect();
    let nuevos: Vec<String> = puertos
        .into_iter()
        .filter(|p| !en_uso.contains(p) && !ignorados.contains(p) && !fallidos.contains_key(p))
        .collect();
    if !nuevos.is_empty() {
        let conocidos: HashSet<String> = activos.keys().cloned().collect();
        let sondas = tokio::task::spawn_blocking(move || sondear(nuevos, conocidos)).await.unwrap_or_default();
        for s in sondas {
            match s {
                Sonda::Nuevo { puerto, canal, sim } => {
                    let canal = *canal;
                    let imei = canal.identidad.imei.clone();
                    let hola = Hola {
                        protocolo: protocolo::PROTOCOLO,
                        token: String::new(),
                        dispositivo_id: imei.clone(),
                        fabricante: canal.identidad.fabricante.to_uppercase(),
                        modelo: canal.identidad.modelo.clone(),
                        android: String::new(),
                        sdk: 0,
                        app_version: "modem".into(),
                        sims: vec![sim],
                        capacidades: vec!["ussd_api".into(), "modem".into()],
                        adb_serial: None,
                    };
                    let generacion = registro.conectar(&hola, Arc::new(canal.clone()));
                    tracing::info!(%imei, %puerto, modelo = %hola.modelo, "módem conectado");
                    bus.emitir(
                        ev::DISPOSITIVO_CONECTADO,
                        json!({ "dispositivo_id": imei, "dispositivo": registro.obtener(&imei), "origen": "modem" }),
                    );
                    activos.insert(imei, Activo { canal, puerto, generacion, proxima_sim: Instant::now() + REVISAR_SIM });
                }
                Sonda::Duplicado { puerto } => {
                    ignorados.insert(puerto);
                }
                Sonda::Falla { puerto, motivo } => {
                    tracing::debug!(%puerto, %motivo, "puerto descartado");
                    fallidos.insert(puerto, Instant::now() + REINTENTO_FALLIDO);
                }
            }
        }
    }

    // El chip puede cambiar (lo sacan y ponen otro) o perder señal: se relee cada tanto.
    let ahora = Instant::now();
    for (imei, a) in activos.iter_mut().filter(|(_, a)| a.proxima_sim <= ahora) {
        a.proxima_sim = ahora + REVISAR_SIM;
        let canal = a.canal.clone();
        let Ok(Some(Ok(sim))) = tokio::task::spawn_blocking(move || canal.leer_sim()).await else { continue };
        if registro.obtener(imei).map(|d| d.sims) != Some(vec![sim.clone()]) {
            if let Some(d) = registro.actualizar_sims(imei, vec![sim]) {
                bus.emitir(ev::DISPOSITIVO_ACTUALIZADO, json!({ "dispositivo_id": imei, "dispositivo": d }));
            }
        }
    }
}

fn listar(op: &Opciones) -> Vec<String> {
    if let Some(p) = &op.puertos {
        return p.clone();
    }
    match serialport::available_ports() {
        Ok(ps) => ps
            .into_iter()
            .filter(|p| match &p.port_type {
                serialport::SerialPortType::UsbPort(u) => op.todos_usb || VIDS.contains(&u.vid),
                _ => false,
            })
            .map(|p| p.port_name)
            .collect(),
        // Linux sin libudev: aparecen como /dev/ttyUSB* y /dev/ttyACM*.
        Err(_) => std::fs::read_dir("/dev")
            .map(|d| {
                let mut v: Vec<String> = d
                    .filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .filter(|n| n.starts_with("ttyUSB") || n.starts_with("ttyACM"))
                    .map(|n| format!("/dev/{n}"))
                    .collect();
                v.sort();
                v
            })
            .unwrap_or_default(),
    }
}

/// Abre cada puerto, prueba AT y lee identidad y chip. Bloqueante.
fn sondear(puertos: Vec<String>, mut conocidos: HashSet<String>) -> Vec<Sonda> {
    let mut out = Vec::new();
    for puerto in puertos {
        let falla = |motivo: String| Sonda::Falla { puerto: puerto.clone(), motivo };
        let serie = match Serie::abrir(&puerto) {
            Ok(s) => s,
            Err(e) => {
                out.push(falla(e));
                continue;
            }
        };
        let mut modem = match Modem::iniciar(Box::new(serie) as Box<dyn Enlace>) {
            Ok(m) => m,
            Err(e) => {
                out.push(falla(e));
                continue;
            }
        };
        let id = match modem.identidad() {
            Ok(i) if !i.imei.is_empty() => i,
            Ok(_) => {
                out.push(falla("no entregó IMEI".into()));
                continue;
            }
            Err(e) => {
                out.push(falla(e.detalle));
                continue;
            }
        };
        if !conocidos.insert(id.imei.clone()) {
            out.push(Sonda::Duplicado { puerto });
            continue;
        }
        match modem.leer_sim() {
            Ok(sim) => out.push(Sonda::Nuevo { puerto, canal: Box::new(CanalModem::nuevo(modem, id)), sim }),
            Err(e) => {
                conocidos.remove(&id.imei);
                out.push(falla(e.detalle));
            }
        }
    }
    out
}
