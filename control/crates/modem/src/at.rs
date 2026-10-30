//! Conversación AT con el módem (bloqueante; se llama desde `spawn_blocking`).
//! Equivale a la clase `Modem` de `modem/tigo.py`, que es la referencia de comportamiento.

use crate::enlace::Enlace;
use crate::gsm7;
use nucleo::ErrorCanal;
use protocolo::{codigos, Sim};
use regex::Regex;
use std::sync::OnceLock;
use std::thread::sleep;
use std::time::{Duration, Instant};

const PAUSA: Duration = Duration::from_millis(50);

/// Cómo se manda el USSD al módem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Formato {
    /// Huawei: código y respuestas empaquetados en GSM de 7 bits, en hex.
    Gsm7Hex,
    /// Quectel, SIMCom y otros: texto plano con `AT+CSCS="GSM"`. Sin probar con hardware.
    Texto,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Identidad {
    pub imei: String,
    pub fabricante: String,
    pub modelo: String,
}

/// Respuesta `+CUSD`. `estado`: 0 sesión terminada, 1 espera respuesta, 2 cancelada, 4 no soportado, 5 sin respuesta.
#[derive(Debug, Clone, PartialEq)]
pub struct RespUssd {
    pub estado: u8,
    pub texto: String,
}

pub struct Modem<E: Enlace> {
    enlace: E,
    pub formato: Formato,
}

fn re_fin() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\r\n(OK|ERROR|\+CME ERROR:[^\r]*|\+CMS ERROR:[^\r]*)\r\n").unwrap())
}
fn re_cusd_hex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r#"\+CUSD:\s*(\d)(?:,"([0-9A-Fa-f]*)",\s*(\d+))?\s*\r?\n"#).unwrap())
}
fn re_cusd_texto() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r#"(?s)\+CUSD:\s*(\d)(?:,"(.*?)",\s*(\d+))?\s*\r?\n"#).unwrap())
}
fn re_error() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\+CME ERROR:[^\r\n]*|\r\nERROR\r\n").unwrap())
}

/// Líneas con contenido: sin eco del comando, sin `OK` y sin vacías.
pub fn lineas_utiles(resp: &str) -> Vec<&str> {
    resp.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && *l != "OK" && !l.starts_with("AT"))
        .collect()
}

fn io_a_canal(e: std::io::Error) -> ErrorCanal {
    tracing::warn!("error de E/S con el módem: {e}");
    ErrorCanal::desconectado()
}

impl<E: Enlace> Modem<E> {
    /// Abre la conversación: `ATE0` (sin eco) y errores verbosos. `Err` si el puerto no responde AT.
    pub fn iniciar(enlace: E) -> Result<Self, String> {
        let mut m = Self { enlace, formato: Formato::Gsm7Hex };
        let r = m.at("ATE0", Duration::from_millis(1500)).map_err(|e| e.detalle)?;
        if !r.contains("OK") {
            return Err("no responde a AT".into());
        }
        let _ = m.at("AT+CMEE=2", Duration::from_secs(1));
        Ok(m)
    }

    fn vaciar(&mut self) -> Result<(), ErrorCanal> {
        self.enlace.leer().map(|_| ()).map_err(io_a_canal)
    }

    /// Manda un comando y espera `OK`/`ERROR`. Devuelve todo lo recibido.
    pub fn at(&mut self, cmd: &str, timeout: Duration) -> Result<String, ErrorCanal> {
        self.vaciar()?;
        self.enlace.escribir(format!("{cmd}\r").as_bytes()).map_err(io_a_canal)?;
        let t0 = Instant::now();
        let mut buf = String::new();
        while t0.elapsed() < timeout {
            buf.push_str(&String::from_utf8_lossy(&self.enlace.leer().map_err(io_a_canal)?));
            if re_fin().is_match(&buf) {
                break;
            }
            sleep(PAUSA);
        }
        Ok(buf)
    }

    pub fn identidad(&mut self) -> Result<Identidad, ErrorCanal> {
        let t = Duration::from_secs(2);
        let imei = {
            let r = self.at("AT+CGSN", t)?;
            lineas_utiles(&r)
                .into_iter()
                .find(|l| l.len() >= 14 && l.chars().all(|c| c.is_ascii_digit()))
                .unwrap_or("")
                .to_string()
        };
        let primera = |m: &mut Self, cmd: &str| -> Result<String, ErrorCanal> {
            Ok(lineas_utiles(&m.at(cmd, t)?).first().copied().unwrap_or("").trim_start_matches("+CGMI:").trim().to_string())
        };
        let fabricante = primera(self, "AT+CGMI")?;
        let modelo = primera(self, "AT+CGMM")?;
        self.formato = if fabricante.to_lowercase().contains("huawei") { Formato::Gsm7Hex } else { Formato::Texto };
        if self.formato == Formato::Texto {
            let _ = self.at("AT+CSCS=\"GSM\"", t);
        }
        Ok(Identidad { imei, fabricante, modelo })
    }

    /// Lee el estado del chip. Un módem sin chip o con `SIM failure` devuelve `estado: "ausente"`.
    pub fn leer_sim(&mut self) -> Result<Sim, ErrorCanal> {
        let t = Duration::from_secs(2);
        let mut sim = Sim {
            slot: 0,
            sub_id: 0,
            operador: String::new(),
            mcc_mnc: String::new(),
            iccid: None,
            numero: None,
            estado: "desconocido".into(),
            voz: None,
            red: None,
        };
        let pin = self.at("AT+CPIN?", t)?;
        sim.estado = if pin.contains("READY") {
            "listo"
        } else if pin.contains("SIM PIN") {
            "pin"
        } else if pin.contains("SIM PUK") {
            "puk"
        } else if pin.contains("CME ERROR") {
            "ausente"
        } else {
            "desconocido"
        }
        .into();
        if sim.estado != "listo" {
            return Ok(sim);
        }

        for cmd in ["AT^ICCID?", "AT+QCCID", "AT+CCID"] {
            let r = self.at(cmd, t)?;
            if let Some(c) = Regex::new(r#"CCID:\s*"?([0-9A-Fa-f]{18,22})"#).unwrap().captures(&r) {
                sim.iccid = Some(c[1].to_string());
                break;
            }
        }
        let re_cops = Regex::new(r#"\+COPS:\s*\d+(?:,(\d+),"([^"]*)"(?:,(\d+))?)?"#).unwrap();
        let _ = self.at("AT+COPS=3,2", t)?;
        if let Some(c) = re_cops.captures(&self.at("AT+COPS?", t)?) {
            sim.mcc_mnc = c.get(2).map(|m| m.as_str().to_string()).unwrap_or_default();
            sim.red = c.get(3).and_then(|m| red_desde_act(m.as_str()));
        }
        let _ = self.at("AT+COPS=3,0", t)?;
        if let Some(c) = re_cops.captures(&self.at("AT+COPS?", t)?) {
            sim.operador = c.get(2).map(|m| m.as_str().trim().to_uppercase()).unwrap_or_default();
        }
        if sim.operador.is_empty() {
            sim.operador = operador_por_mcc_mnc(&sim.mcc_mnc).into();
        }
        let creg = self.at("AT+CREG?", t)?;
        sim.voz = Regex::new(r"\+CREG:\s*(?:\d+,)?(\d+)").unwrap().captures(&creg).map(|c| {
            match &c[1] {
                "1" | "5" => "en_servicio",
                "0" | "2" | "3" => "sin_servicio",
                _ => "sin_servicio",
            }
            .to_string()
        });
        let cnum = self.at("AT+CNUM", t)?;
        sim.numero = Regex::new(r#"\+CNUM:[^,]*,"([^"]*)""#)
            .unwrap()
            .captures(&cnum)
            .map(|c| c[1].trim_start_matches('+').trim_start_matches("591").to_string())
            .filter(|n| !n.is_empty());
        Ok(sim)
    }

    /// Manda un USSD (o la respuesta a un menú abierto) y espera el `+CUSD`.
    pub fn ussd(&mut self, texto: &str, timeout: Duration) -> Result<RespUssd, ErrorCanal> {
        self.vaciar()?;
        let cmd = match self.formato {
            Formato::Gsm7Hex => format!("AT+CUSD=1,\"{}\",15\r", gsm7::empaquetar(texto)),
            Formato::Texto => format!("AT+CUSD=1,\"{texto}\",15\r"),
        };
        self.enlace.escribir(cmd.as_bytes()).map_err(io_a_canal)?;
        let t0 = Instant::now();
        let mut buf = String::new();
        let re = match self.formato {
            Formato::Gsm7Hex => re_cusd_hex(),
            Formato::Texto => re_cusd_texto(),
        };
        while t0.elapsed() < timeout {
            buf.push_str(&String::from_utf8_lossy(&self.enlace.leer().map_err(io_a_canal)?));
            if let Some(c) = re.captures(&buf) {
                let estado: u8 = c[1].parse().unwrap_or(0);
                let dcs: u32 = c.get(3).and_then(|d| d.as_str().parse().ok()).unwrap_or(15);
                let crudo = c.get(2).map(|m| m.as_str()).unwrap_or("");
                let texto = match self.formato {
                    Formato::Gsm7Hex => gsm7::decodificar(crudo, dcs),
                    Formato::Texto if dcs == 72 => gsm7::decodificar(crudo, dcs),
                    Formato::Texto => crudo.to_string(),
                };
                return Ok(RespUssd { estado, texto: texto.trim().to_string() });
            }
            if let Some(e) = re_error().find(&buf) {
                return Err(ErrorCanal::nuevo(codigos::RED, e.as_str().trim()));
            }
            sleep(PAUSA);
        }
        Err(ErrorCanal::timeout())
    }

    /// Cierra la sesión USSD abierta en el módem.
    pub fn cancelar(&mut self) -> Result<(), ErrorCanal> {
        self.at("AT+CUSD=2", Duration::from_secs(2)).map(|_| ())
    }
}

fn red_desde_act(act: &str) -> Option<String> {
    Some(
        match act {
            "0" | "1" | "3" => "2G",
            "2" | "4" | "5" | "6" => "3G",
            "7" => "LTE",
            _ => return None,
        }
        .into(),
    )
}

/// Respaldo cuando `AT+COPS` no trae el nombre. MCC 736 = Bolivia.
fn operador_por_mcc_mnc(mcc_mnc: &str) -> &'static str {
    match mcc_mnc {
        "73601" => "VIVA",
        "73602" => "ENTEL",
        "73603" => "TIGO",
        _ => "",
    }
}

#[cfg(test)]
pub(crate) mod falso {
    use super::*;
    use std::collections::VecDeque;
    use std::io;

    /// Módem de mentira: por cada comando recibido entrega las respuestas programadas.
    /// `guion`: (comienzo del comando, texto que "llega" después, en orden).
    pub struct Falso {
        pub guion: Vec<(String, Vec<String>)>,
        pub enviados: Vec<String>,
        pub pendiente: VecDeque<u8>,
        pub roto: bool,
    }

    impl Falso {
        pub fn nuevo(guion: &[(&str, &[&str])]) -> Self {
            Self {
                guion: guion.iter().map(|(c, r)| (c.to_string(), r.iter().map(|s| s.to_string()).collect())).collect(),
                enviados: vec![],
                pendiente: VecDeque::new(),
                roto: false,
            }
        }
    }

    impl Enlace for Falso {
        fn escribir(&mut self, d: &[u8]) -> io::Result<()> {
            if self.roto {
                return Err(io::Error::other("USB desconectado"));
            }
            let cmd = String::from_utf8_lossy(d).trim().to_string();
            self.enviados.push(cmd.clone());
            if let Some((_, rs)) = self.guion.iter_mut().find(|(c, r)| cmd.starts_with(c.as_str()) && !r.is_empty()) {
                self.pendiente.extend(rs.remove(0).bytes());
            } else {
                self.pendiente.extend(b"\r\nOK\r\n");
            }
            Ok(())
        }
        fn leer(&mut self) -> io::Result<Vec<u8>> {
            if self.roto {
                return Err(io::Error::other("USB desconectado"));
            }
            Ok(self.pendiente.drain(..).collect())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::falso::Falso;
    use super::*;

    fn rapido() -> Duration {
        Duration::from_secs(2)
    }

    #[test]
    fn no_responde_a_at() {
        struct Mudo;
        impl Enlace for Mudo {
            fn escribir(&mut self, _: &[u8]) -> std::io::Result<()> {
                Ok(())
            }
            fn leer(&mut self) -> std::io::Result<Vec<u8>> {
                Ok(vec![])
            }
        }
        assert!(Modem::iniciar(Mudo).is_err());
    }

    #[test]
    fn identidad_huawei() {
        let f = Falso::nuevo(&[
            ("AT+CGSN", &["\r\n862567020947870\r\n\r\nOK\r\n"]),
            ("AT+CGMI", &["\r\nhuawei\r\n\r\nOK\r\n"]),
            ("AT+CGMM", &["\r\nE303\r\n\r\nOK\r\n"]),
        ]);
        let mut m = Modem::iniciar(f).unwrap();
        let id = m.identidad().unwrap();
        assert_eq!(id, Identidad { imei: "862567020947870".into(), fabricante: "huawei".into(), modelo: "E303".into() });
        assert_eq!(m.formato, Formato::Gsm7Hex);
    }

    #[test]
    fn ussd_huawei_manda_empaquetado_y_lee_menu() {
        // "1.-Saldo" empaquetado como lo devolvería el módem
        let hex = gsm7::empaquetar("1.-Saldo");
        let respuesta = format!("\r\nOK\r\n\r\n+CUSD: 1,\"{hex}\",15\r\n");
        let f = Falso::nuevo(&[("AT+CUSD=1", &[&respuesta])]);
        let mut m = Modem::iniciar(f).unwrap();
        let r = m.ussd("*105#", rapido()).unwrap();
        assert_eq!(r, RespUssd { estado: 1, texto: "1.-Saldo".into() });
        assert!(m.enlace.enviados.iter().any(|c| c == "AT+CUSD=1,\"AA18AC3602\",15"), "{:?}", m.enlace.enviados);
    }

    #[test]
    fn ussd_texto_plano() {
        let f = Falso::nuevo(&[
            ("AT+CGMI", &["\r\nQuectel\r\n\r\nOK\r\n"]),
            ("AT+CUSD=1", &["\r\nOK\r\n\r\n+CUSD: 0,\"Saldo: Bs 10.00\nvence hoy\",15\r\n"]),
        ]);
        let mut m = Modem::iniciar(f).unwrap();
        m.identidad().unwrap();
        assert_eq!(m.formato, Formato::Texto);
        let r = m.ussd("*105#", rapido()).unwrap();
        assert_eq!(r.estado, 0);
        assert_eq!(r.texto, "Saldo: Bs 10.00\nvence hoy");
        assert!(m.enlace.enviados.iter().any(|c| c == "AT+CUSD=1,\"*105#\",15"));
    }

    #[test]
    fn ussd_error_del_modem_es_red() {
        let f = Falso::nuevo(&[("AT+CUSD=1", &["\r\n+CME ERROR: UNKNOWN\r\n"])]);
        let mut m = Modem::iniciar(f).unwrap();
        let e = m.ussd("*105#", rapido()).unwrap_err();
        assert_eq!(e.codigo, codigos::RED);
        assert!(e.detalle.contains("UNKNOWN"));
    }

    #[test]
    fn ussd_sin_respuesta_es_timeout() {
        let f = Falso::nuevo(&[("AT+CUSD=1", &["\r\nOK\r\n"])]);
        let mut m = Modem::iniciar(f).unwrap();
        let e = m.ussd("*105#", Duration::from_millis(300)).unwrap_err();
        assert_eq!(e.codigo, codigos::TIMEOUT);
    }

    #[test]
    fn puerto_roto_es_desconectado() {
        let f = Falso::nuevo(&[]);
        let mut m = Modem::iniciar(f).unwrap();
        m.enlace.roto = true;
        let e = m.ussd("*105#", rapido()).unwrap_err();
        assert_eq!(e.codigo, codigos::DESCONECTADO);
    }

    #[test]
    fn sim_tigo_lista() {
        let f = Falso::nuevo(&[
            ("AT+CPIN?", &["\r\n+CPIN: READY\r\n\r\nOK\r\n"]),
            ("AT^ICCID?", &["\r\n^ICCID: \"8959103000797642548\"\r\n\r\nOK\r\n"]),
            ("AT+COPS?", &[
                "\r\n+COPS: 0,2,\"73603\",2\r\n\r\nOK\r\n",
                "\r\n+COPS: 0,0,\"Tigo\",2\r\n\r\nOK\r\n",
            ]),
            ("AT+CREG?", &["\r\n+CREG: 0,1\r\n\r\nOK\r\n"]),
            ("AT+CNUM", &["\r\n+CNUM: \"\",\"+59169759675\",145\r\n\r\nOK\r\n"]),
        ]);
        let mut m = Modem::iniciar(f).unwrap();
        let s = m.leer_sim().unwrap();
        assert_eq!(s.estado, "listo");
        assert_eq!(s.iccid.as_deref(), Some("8959103000797642548"));
        assert_eq!(s.mcc_mnc, "73603");
        assert_eq!(s.operador, "TIGO");
        assert_eq!(s.red.as_deref(), Some("3G"));
        assert_eq!(s.voz.as_deref(), Some("en_servicio"));
        assert_eq!(s.numero.as_deref(), Some("69759675"));
    }

    #[test]
    fn sim_con_falla_es_ausente() {
        let f = Falso::nuevo(&[("AT+CPIN?", &["\r\n+CME ERROR: SIM failure\r\n"])]);
        let mut m = Modem::iniciar(f).unwrap();
        let s = m.leer_sim().unwrap();
        assert_eq!(s.estado, "ausente");
        assert_eq!(s.iccid, None);
    }
}
