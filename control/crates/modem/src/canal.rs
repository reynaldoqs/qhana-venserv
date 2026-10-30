//! [`CanalModem`]: un módem como `CanalUssd`, para que el motor de recetas no note la diferencia
//! con un teléfono. Un módem tiene un solo chip (slot 0) y una sesión USSD a la vez.

use crate::at::{Identidad, Modem};
use crate::enlace::Enlace;
use async_trait::async_trait;
use nucleo::{CanalUssd, ErrorCanal};
use protocolo::{codigos, ModoUssd, PasoUssd, Sim};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, TryLockError};
use std::time::{Duration, Instant};

struct Interno {
    modem: Modem<Box<dyn Enlace>>,
    /// Sesión USSD abierta (el módem estaba esperando una respuesta).
    sesion: Option<String>,
}

struct Compartido {
    interno: Mutex<Interno>,
    muerto: AtomicBool,
    contador: AtomicU64,
}

#[derive(Clone)]
pub struct CanalModem {
    c: Arc<Compartido>,
    pub identidad: Identidad,
}

fn ocupado() -> ErrorCanal {
    ErrorCanal::nuevo(codigos::OCUPADO, "el módem está atendiendo otra petición")
}

impl CanalModem {
    pub fn nuevo(modem: Modem<Box<dyn Enlace>>, identidad: Identidad) -> Self {
        Self {
            c: Arc::new(Compartido {
                interno: Mutex::new(Interno { modem, sesion: None }),
                muerto: AtomicBool::new(false),
                contador: AtomicU64::new(0),
            }),
            identidad,
        }
    }

    /// `true` si falló la E/S con el puerto (se desenchufó): el detector lo da de baja.
    pub fn muerto(&self) -> bool {
        self.c.muerto.load(Ordering::Relaxed)
    }

    /// Relee el chip si el módem no está en medio de otra cosa. `None` si está ocupado.
    pub fn leer_sim(&self) -> Option<Result<Sim, ErrorCanal>> {
        let mut g = match self.c.interno.try_lock() {
            Ok(g) => g,
            Err(TryLockError::WouldBlock) => return None,
            Err(TryLockError::Poisoned(p)) => p.into_inner(),
        };
        if g.sesion.is_some() {
            return None;
        }
        Some(self.marcar(g.modem.leer_sim()))
    }

    fn marcar<T>(&self, r: Result<T, ErrorCanal>) -> Result<T, ErrorCanal> {
        if let Err(e) = &r {
            if e.codigo == codigos::DESCONECTADO {
                self.c.muerto.store(true, Ordering::Relaxed);
            }
        }
        r
    }

    /// Ejecuta `f` con el módem, en un hilo de bloqueo. `ocupado` si ya hay otra petición en curso.
    async fn con_modem<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Interno, &Compartido) -> Result<T, ErrorCanal> + Send + 'static,
    ) -> Result<T, ErrorCanal> {
        let this = self.clone();
        tokio::task::spawn_blocking(move || {
            let mut g = match this.c.interno.try_lock() {
                Ok(g) => g,
                Err(TryLockError::WouldBlock) => return Err(ocupado()),
                Err(TryLockError::Poisoned(p)) => p.into_inner(),
            };
            let r = f(&mut g, &this.c);
            this.marcar(r)
        })
        .await
        .map_err(|e| ErrorCanal::nuevo(codigos::DESCONOCIDO, e.to_string()))?
    }
}

/// Traduce el `+CUSD` a un paso y actualiza la sesión. `id` es el id de sesión a usar.
fn paso(i: &mut Interno, id: String, r: crate::at::RespUssd, t0: Instant) -> Result<PasoUssd, ErrorCanal> {
    let ms = t0.elapsed().as_millis() as u64;
    match r.estado {
        0 | 1 => {
            let abierta = r.estado == 1;
            i.sesion = abierta.then(|| id.clone());
            Ok(PasoUssd { sesion: id, texto: r.texto, abierta, ms })
        }
        otro => {
            i.sesion = None;
            let motivo = match otro {
                2 => "sesión cancelada por la red",
                4 => "operación no soportada",
                5 => "la red no respondió",
                _ => "respuesta USSD desconocida",
            };
            Err(ErrorCanal::nuevo(codigos::RED, format!("{motivo} (+CUSD {otro}): {}", r.texto)))
        }
    }
}

#[async_trait]
impl CanalUssd for CanalModem {
    async fn iniciar(&self, slot: u32, codigo: &str, _modo: ModoUssd, timeout: Duration) -> Result<PasoUssd, ErrorCanal> {
        if slot != 0 {
            return Err(ErrorCanal::nuevo(codigos::SIM_INVALIDA, format!("el módem solo tiene el slot 0 (pidieron {slot})")));
        }
        let codigo = codigo.to_string();
        self.con_modem(move |i, c| {
            if i.sesion.take().is_some() {
                i.modem.cancelar()?;
            }
            let id = format!("m{}", c.contador.fetch_add(1, Ordering::Relaxed) + 1);
            let t0 = Instant::now();
            match i.modem.ussd(&codigo, timeout) {
                Ok(r) => paso(i, id, r, t0),
                Err(e) => {
                    if e.codigo == codigos::TIMEOUT {
                        let _ = i.modem.cancelar();
                    }
                    Err(e)
                }
            }
        })
        .await
    }

    async fn responder(&self, sesion: &str, texto: &str, timeout: Duration) -> Result<PasoUssd, ErrorCanal> {
        let (sesion, texto) = (sesion.to_string(), texto.to_string());
        self.con_modem(move |i, _| {
            if i.sesion.as_deref() != Some(sesion.as_str()) {
                return Err(ErrorCanal::nuevo(codigos::SESION_INEXISTENTE, format!("no hay sesión {sesion}")));
            }
            let t0 = Instant::now();
            match i.modem.ussd(&texto, timeout) {
                Ok(r) => paso(i, sesion, r, t0),
                Err(e) => {
                    if e.codigo == codigos::TIMEOUT {
                        let _ = i.modem.cancelar();
                        i.sesion = None;
                    }
                    Err(e)
                }
            }
        })
        .await
    }

    async fn cancelar(&self, sesion: &str) -> Result<(), ErrorCanal> {
        let sesion = sesion.to_string();
        self.con_modem(move |i, _| {
            if i.sesion.as_deref() == Some(sesion.as_str()) {
                i.sesion = None;
                i.modem.cancelar()?;
            }
            Ok(())
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::at::falso::Falso;
    use crate::gsm7;

    fn canal(guion: &[(&str, &[&str])]) -> CanalModem {
        let modem = Modem::iniciar(Box::new(Falso::nuevo(guion)) as Box<dyn Enlace>).unwrap();
        CanalModem::nuevo(modem, Identidad { imei: "1".into(), fabricante: "huawei".into(), modelo: "E303".into() })
    }

    fn cusd(estado: u8, texto: &str) -> String {
        format!("\r\nOK\r\n\r\n+CUSD: {estado},\"{}\",15\r\n", gsm7::empaquetar(texto))
    }

    const T: Duration = Duration::from_secs(2);

    #[tokio::test]
    async fn menu_de_varios_pasos() {
        let (a, b) = (cusd(1, "1.-Transferir"), cusd(0, "Listo"));
        let c = canal(&[("AT+CUSD=1", &[&a, &b])]);
        let p = c.iniciar(0, "*66#", ModoUssd::Pantalla, T).await.unwrap();
        assert!(p.abierta);
        assert_eq!(p.texto, "1.-Transferir");
        let p2 = c.responder(&p.sesion, "1", T).await.unwrap();
        assert!(!p2.abierta);
        assert_eq!(p2.sesion, p.sesion);
        assert_eq!(p2.texto, "Listo");
        // Cerrada: ya no se puede contestar.
        let e = c.responder(&p.sesion, "1", T).await.unwrap_err();
        assert_eq!(e.codigo, codigos::SESION_INEXISTENTE);
    }

    #[tokio::test]
    async fn slot_distinto_de_cero() {
        let c = canal(&[]);
        let e = c.iniciar(1, "*105#", ModoUssd::Api, T).await.unwrap_err();
        assert_eq!(e.codigo, codigos::SIM_INVALIDA);
    }

    #[tokio::test]
    async fn cancelada_por_la_red() {
        let a = cusd(2, "");
        let c = canal(&[("AT+CUSD=1", &[&a])]);
        let e = c.iniciar(0, "*105#", ModoUssd::Api, T).await.unwrap_err();
        assert_eq!(e.codigo, codigos::RED);
    }

    #[tokio::test]
    async fn timeout_cierra_la_sesion() {
        let c = canal(&[("AT+CUSD=1", &["\r\nOK\r\n"])]);
        let e = c.iniciar(0, "*105#", ModoUssd::Api, Duration::from_millis(300)).await.unwrap_err();
        assert_eq!(e.codigo, codigos::TIMEOUT);
        assert!(!c.muerto());
    }

    #[tokio::test]
    async fn puerto_que_se_cae_marca_muerto() {
        use std::sync::atomic::AtomicBool;
        // Responde OK hasta que se "desenchufa" (la bandera), y desde ahí toda E/S falla.
        struct Desenchufable(Arc<AtomicBool>);
        impl Enlace for Desenchufable {
            fn escribir(&mut self, _: &[u8]) -> std::io::Result<()> {
                if self.0.load(Ordering::Relaxed) { Err(std::io::Error::other("USB")) } else { Ok(()) }
            }
            fn leer(&mut self) -> std::io::Result<Vec<u8>> {
                if self.0.load(Ordering::Relaxed) { Err(std::io::Error::other("USB")) } else { Ok(b"\r\nOK\r\n".to_vec()) }
            }
        }
        let cable = Arc::new(AtomicBool::new(false));
        let modem = Modem::iniciar(Box::new(Desenchufable(cable.clone())) as Box<dyn Enlace>).unwrap();
        let c = CanalModem::nuevo(modem, Identidad { imei: "1".into(), fabricante: String::new(), modelo: String::new() });
        assert!(!c.muerto());
        cable.store(true, Ordering::Relaxed);
        let e = c.iniciar(0, "*105#", ModoUssd::Api, T).await.unwrap_err();
        assert_eq!(e.codigo, codigos::DESCONECTADO);
        assert!(c.muerto());
    }
}
