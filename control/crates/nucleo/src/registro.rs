use crate::canal::CanalUssd;
use protocolo::{ahora_ms, Dispositivo, EstadoDispositivo, Hola, Sim};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

struct Entrada {
    info: Dispositivo,
    canal: Option<Arc<dyn CanalUssd>>,
    /// Identifica la conexión actual: una conexión vieja que se cierra tarde no borra a la nueva.
    generacion: u64,
}

/// Dispositivos conocidos (conectados o no) y el canal USSD de los conectados.
#[derive(Default)]
pub struct Registro {
    entradas: RwLock<HashMap<String, Entrada>>,
    /// android_id → serial ADB (lo llena el gestor ADB, aunque el agente aún no se haya conectado).
    adb: RwLock<HashMap<String, String>>,
    generaciones: std::sync::atomic::AtomicU64,
}

impl Registro {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registra la conexión de un agente. Devuelve la generación de esta conexión.
    pub fn conectar(&self, hola: &Hola, canal: Arc<dyn CanalUssd>) -> u64 {
        let generacion = self
            .generaciones
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1;
        let adb_serial = self.adb.read().unwrap().get(&hola.dispositivo_id).cloned();
        let info = Dispositivo {
            id: hola.dispositivo_id.clone(),
            conectado: true,
            adb_serial,
            fabricante: hola.fabricante.clone(),
            modelo: hola.modelo.clone(),
            android: hola.android.clone(),
            app_version: hola.app_version.clone(),
            sims: hola.sims.clone(),
            estado: None,
            ultima_vez_ms: ahora_ms(),
            ocupado: false,
            capacidades: hola.capacidades.clone(),
        };
        let mut e = self.entradas.write().unwrap();
        let estado_previo = e.get(&hola.dispositivo_id).and_then(|x| x.info.estado.clone());
        let mut info = info;
        info.estado = estado_previo;
        e.insert(hola.dispositivo_id.clone(), Entrada { info, canal: Some(canal), generacion });
        generacion
    }

    /// Marca el dispositivo como desconectado si `generacion` sigue siendo la conexión vigente.
    /// Devuelve `true` si efectivamente se desconectó.
    pub fn desconectar(&self, id: &str, generacion: u64) -> bool {
        let mut e = self.entradas.write().unwrap();
        if let Some(x) = e.get_mut(id) {
            if x.generacion == generacion {
                x.canal = None;
                x.info.conectado = false;
                x.info.ocupado = false;
                x.info.ultima_vez_ms = ahora_ms();
                return true;
            }
        }
        false
    }

    pub fn canal(&self, id: &str) -> Option<Arc<dyn CanalUssd>> {
        self.entradas.read().unwrap().get(id).and_then(|x| x.canal.clone())
    }

    pub fn obtener(&self, id: &str) -> Option<Dispositivo> {
        self.entradas.read().unwrap().get(id).map(|x| x.info.clone())
    }

    pub fn listar(&self) -> Vec<Dispositivo> {
        let mut v: Vec<_> = self.entradas.read().unwrap().values().map(|x| x.info.clone()).collect();
        v.sort_by(|a, b| b.conectado.cmp(&a.conectado).then(a.id.cmp(&b.id)));
        v
    }

    pub fn conectados(&self) -> usize {
        self.entradas.read().unwrap().values().filter(|x| x.info.conectado).count()
    }

    fn modificar(&self, id: &str, f: impl FnOnce(&mut Dispositivo)) -> Option<Dispositivo> {
        let mut e = self.entradas.write().unwrap();
        let x = e.get_mut(id)?;
        f(&mut x.info);
        x.info.ultima_vez_ms = ahora_ms();
        Some(x.info.clone())
    }

    pub fn actualizar_sims(&self, id: &str, sims: Vec<Sim>) -> Option<Dispositivo> {
        self.modificar(id, |d| d.sims = sims)
    }

    pub fn actualizar_estado(&self, id: &str, estado: EstadoDispositivo) -> Option<Dispositivo> {
        self.modificar(id, |d| d.estado = Some(estado))
    }

    pub fn tocar(&self, id: &str) {
        let mut e = self.entradas.write().unwrap();
        if let Some(x) = e.get_mut(id) {
            x.info.ultima_vez_ms = ahora_ms();
        }
    }

    pub fn marcar_ocupado(&self, id: &str, ocupado: bool) -> Option<Dispositivo> {
        self.modificar(id, |d| d.ocupado = ocupado)
    }

    /// Cruza serial ADB ↔ android_id. Devuelve el dispositivo si ya estaba registrado.
    pub fn vincular_adb(&self, dispositivo_id: &str, serial: &str) -> Option<Dispositivo> {
        self.adb.write().unwrap().insert(dispositivo_id.to_string(), serial.to_string());
        let mut e = self.entradas.write().unwrap();
        let x = e.get_mut(dispositivo_id)?;
        if x.info.adb_serial.as_deref() == Some(serial) {
            return None;
        }
        x.info.adb_serial = Some(serial.to_string());
        Some(x.info.clone())
    }

    /// Olvida el vínculo de un serial que desapareció de ADB.
    pub fn desvincular_adb(&self, serial: &str) -> Option<Dispositivo> {
        let id = {
            let mut adb = self.adb.write().unwrap();
            let id = adb.iter().find(|(_, s)| s.as_str() == serial).map(|(i, _)| i.clone())?;
            adb.remove(&id);
            id
        };
        self.modificar(&id, |d| d.adb_serial = None)
    }
}
