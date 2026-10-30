//! Operadores simulados con las pantallas reales medidas el 26–28 oct 2026 (Tigo y Entel, Bolivia).
//! Sirven para tests del motor y para el agente simulado: desarrollar la interfaz sin gastar saldo.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operador {
    Tigo,
    Entel,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pantalla {
    pub texto: String,
    pub abierta: bool,
}

impl Pantalla {
    fn abierta(t: impl Into<String>) -> Self {
        Self { texto: t.into(), abierta: true }
    }
    fn cerrada(t: impl Into<String>) -> Self {
        Self { texto: t.into(), abierta: false }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Paso {
    /// Pantalla final que deja el campo de respuesta (como *105# en Tigo): cualquier respuesta cierra.
    Final,
    TigoMenu,
    TigoMenuLimite,
    TigoNumero,
    TigoMonto { numero: String, opciones: Vec<u32> },
    TigoConfirmar { numero: String, monto: u32 },
    EntelMenu,
    EntelConsulta,
}

/// Una línea (chip) simulada.
#[derive(Debug, Clone)]
pub struct LineaSimulada {
    pub operador: Operador,
    pub linea: String,
    pub saldo_principal: f64,
    pub doble_carga: f64,
    pub transferencias_hoy: u32,
    pub limite_diario: u32,
    /// "1.00 Bs a 69869869", la más reciente al final.
    pub movimientos: Vec<String>,
    /// Texto de SMS que el operador mandaría tras la última operación (lo consume el agente simulado).
    pub sms_pendiente: Option<String>,
    sesion: Option<Paso>,
}

pub const COMISION_TIGO: f64 = 0.20;
const MONTOS_TIGO: [u32; 5] = [1, 2, 5, 10, 20];

fn bs(x: f64) -> String {
    if (x - x.round()).abs() < 1e-9 {
        format!("{}", x.round() as i64)
    } else {
        format!("{x:.2}")
    }
}

impl LineaSimulada {
    pub fn tigo(linea: &str, saldo_principal: f64) -> Self {
        Self {
            operador: Operador::Tigo,
            linea: linea.into(),
            saldo_principal,
            doble_carga: 10.0,
            transferencias_hoy: 0,
            limite_diario: 5,
            movimientos: vec![],
            sms_pendiente: None,
            sesion: None,
        }
    }

    pub fn entel(linea: &str, saldo_principal: f64) -> Self {
        Self {
            operador: Operador::Entel,
            linea: linea.into(),
            saldo_principal,
            doble_carga: 0.0,
            transferencias_hoy: 0,
            limite_diario: 5,
            movimientos: vec!["59163819601 Bs 1.00 03/09/2026 22:32:12.".into()],
            sms_pendiente: None,
            sesion: None,
        }
    }

    pub fn sesion_abierta(&self) -> bool {
        self.sesion.is_some()
    }

    pub fn cancelar(&mut self) {
        self.sesion = None;
    }

    fn abrir(&mut self, paso: Paso, texto: String) -> Pantalla {
        self.sesion = Some(paso);
        Pantalla::abierta(texto)
    }

    fn cerrar(&mut self, texto: impl Into<String>) -> Pantalla {
        self.sesion = None;
        Pantalla::cerrada(texto)
    }

    pub fn texto_saldo(&self) -> String {
        match self.operador {
            Operador::Tigo => format!(
                "Linea:{}\nSaldo Total: {} Bs vence el 25/12/2026 23:59:59\nSaldo Principal: {} Bs vence el 25/12/2026 23:59:59\nSaldo Doble Carga: {} Bs vence el 25/12/2026 00:00:00",
                self.linea,
                bs(self.saldo_principal + self.doble_carga),
                bs(self.saldo_principal),
                bs(self.doble_carga)
            ),
            Operador::Entel => "Saldo:\nBeneficios Combo:\nMIN 253.2 Exp 31/12\nSMS 200 Exp:31/10\nMB 4G 5189.6 Exp 31/12\nMarque *105*1# para mas detalles".into(),
        }
    }

    /// Marca un código (inicia sesión). Reemplaza cualquier sesión anterior.
    pub fn marcar(&mut self, codigo: &str) -> Pantalla {
        self.sesion = None;
        match (self.operador, codigo.trim()) {
            (Operador::Tigo, "*105#") => {
                let t = self.texto_saldo();
                self.abrir(Paso::Final, t)
            }
            (Operador::Tigo, "*66#1#") => {
                if self.transferencias_hoy >= self.limite_diario {
                    self.abrir(
                        Paso::TigoMenuLimite,
                        "Bienvenido al servicio de transferencias\nUsted alcanzo el maximo numero de transferecias diarias\n\n1.-Ultimos movimientos\n".into(),
                    )
                } else {
                    self.abrir(
                        Paso::TigoMenu,
                        "Bienvenido al servicio de transferencias\n\n1.-Transferir por unica vez\n2.-Ultimos movimientos\n".into(),
                    )
                }
            }
            (Operador::Entel, "*105#") => {
                let t = self.texto_saldo();
                self.cerrar(t)
            }
            (Operador::Entel, "*133#") => self.abrir(
                Paso::EntelMenu,
                "Escoja una opcion\n1 Saldo para transferencia\n2 Saldo Pre-pago\n3 Consulta de transferencia".into(),
            ),
            _ => self.cerrar("Codigo USSD incorrecto"),
        }
    }

    fn texto_movimientos(&self) -> String {
        match self.operador {
            Operador::Tigo => {
                let mut t = "Ultimos movimientos\n".to_string();
                for (i, m) in self.movimientos.iter().rev().take(5).enumerate() {
                    t.push_str(&format!("{}.-{}\n", i + 1, m));
                }
                if self.movimientos.is_empty() {
                    t.push_str("Sin movimientos\n");
                }
                t
            }
            Operador::Entel => self.movimientos.last().cloned().unwrap_or_else(|| "Sin transferencias.".into()),
        }
    }

    /// Contesta en la sesión abierta.
    pub fn responder(&mut self, texto: &str) -> Option<Pantalla> {
        let paso = self.sesion.take()?;
        let r = texto.trim();
        Some(match paso {
            Paso::Final => self.cerrar(""),
            Paso::TigoMenu => match r {
                "1" => self.abrir(Paso::TigoNumero, "Ingrese numero de destino\n".into()),
                "2" => {
                    let t = self.texto_movimientos();
                    self.cerrar(t)
                }
                _ => self.cerrar("Opcion invalida"),
            },
            Paso::TigoMenuLimite => match r {
                "1" => {
                    let t = self.texto_movimientos();
                    self.cerrar(t)
                }
                _ => self.cerrar("Opcion invalida"),
            },
            Paso::TigoNumero => {
                let valido = r.len() == 8
                    && r.chars().all(|c| c.is_ascii_digit())
                    && (r.starts_with('6') || r.starts_with('7'));
                if !valido {
                    return Some(self.cerrar("Numero de destino invalido"));
                }
                let opciones: Vec<u32> = MONTOS_TIGO
                    .iter()
                    .copied()
                    .filter(|m| self.saldo_principal + 1e-9 >= *m as f64 + COMISION_TIGO)
                    .collect();
                if opciones.is_empty() {
                    return Some(self.cerrar("Saldo insuficiente para transferir"));
                }
                let mut t = "Ingrese monto\n".to_string();
                for (i, m) in opciones.iter().enumerate() {
                    t.push_str(&format!("{}.-{} Bs\n", i + 1, m));
                }
                self.abrir(Paso::TigoMonto { numero: r.to_string(), opciones }, t)
            }
            Paso::TigoMonto { numero, opciones } => {
                let idx = r.parse::<usize>().ok().filter(|i| *i >= 1 && *i <= opciones.len());
                match idx {
                    Some(i) => {
                        let monto = opciones[i - 1];
                        let t = format!(
                            "Confirma transferir por unica vez {monto}.00 Bs a {numero} con un costo de 0.20?\n1.-Si\n2.-No\n"
                        );
                        self.abrir(Paso::TigoConfirmar { numero, monto }, t)
                    }
                    None => self.cerrar("Opcion invalida"),
                }
            }
            Paso::TigoConfirmar { numero, monto } => match r {
                "1" => {
                    self.saldo_principal -= monto as f64 + COMISION_TIGO;
                    self.saldo_principal = (self.saldo_principal * 100.0).round() / 100.0;
                    self.transferencias_hoy += 1;
                    self.movimientos.push(format!("{monto}.00 Bs a {numero}"));
                    self.sms_pendiente =
                        Some(format!("Tigo: Transferiste {monto}.00 Bs a {numero}. Saldo principal: {} Bs", bs(self.saldo_principal)));
                    self.cerrar(format!("Le ha transferido {monto}.00 a {numero}"))
                }
                "2" => self.cerrar("Transferencia cancelada"),
                _ => self.cerrar("Opcion invalida"),
            },
            Paso::EntelMenu => match r {
                "1" => self.cerrar(format!("Su saldo para transferencia es de {} Bs.", bs(self.saldo_principal))),
                "2" => self.cerrar(format!("Su saldo pre-pago es de {} Bs.", bs(self.saldo_principal))),
                "3" => self.abrir(
                    Paso::EntelConsulta,
                    "Escoja una opcion\n1 Ultimas transferencias \n2 Total de transferencias".into(),
                ),
                _ => self.cerrar("Opcion invalida"),
            },
            Paso::EntelConsulta => match r {
                "1" => {
                    let t = self.texto_movimientos();
                    self.cerrar(t)
                }
                "2" => self.cerrar("El total de transferencias es de 0.00 Bs."),
                _ => self.cerrar("Opcion invalida"),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flujo_tigo_completo() {
        let mut l = LineaSimulada::tigo("69759675", 14.0);
        assert!(l.marcar("*66#1#").texto.contains("Transferir por unica vez"));
        assert!(l.responder("1").unwrap().texto.contains("numero de destino"));
        let m = l.responder("69869869").unwrap();
        assert!(m.texto.contains("4.-10 Bs"));
        assert!(!m.texto.contains("20 Bs")); // 14 < 20.20
        let c = l.responder("1").unwrap();
        assert!(c.texto.contains("1.00 Bs a 69869869 con un costo de 0.20"));
        let f = l.responder("1").unwrap();
        assert_eq!(f, Pantalla { texto: "Le ha transferido 1.00 a 69869869".into(), abierta: false });
        assert!((l.saldo_principal - 12.8).abs() < 1e-9);
    }

    #[test]
    fn limite_diario() {
        let mut l = LineaSimulada::tigo("1", 100.0);
        l.transferencias_hoy = 5;
        assert!(l.marcar("*66#1#").texto.contains("maximo numero de transferecias diarias"));
    }
}
