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
    TigoDealerMenu,
    TigoDealerSaldoPin,
    TigoDealerRecargas,
    TigoDealerMonto,
    TigoDealerNumero { monto: String },
    TigoDealerPin { monto: String, numero: String },
    /// Confirmación de una recarga Entel dealer (`*133*número*monto#`).
    EntelDealerConfirmar { numero: String, centavos: u64 },
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
    /// Chip Tigo dealer: PIN que pide `*555#`. `None` = chip normal (no tiene `*555#`).
    pub pin_dealer: Option<String>,
    /// Saldo de dealer que gastan las recargas `*555#` (Tigo) o `*133*número*monto#` (Entel).
    pub saldo_dealer: f64,
    /// Chip Entel dealer: acepta `*133*número*monto#` (Bs enteros o `050`). Sin esto, Entel responde que no es dealer.
    pub dealer_entel: bool,
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
            pin_dealer: None,
            saldo_dealer: 0.0,
            dealer_entel: false,
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
            pin_dealer: None,
            saldo_dealer: 0.0,
            dealer_entel: false,
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
            // Tigo Money de un chip dealer. Pantallas medidas el 3-nov-2026 (chip 77716151); la de PIN errado no.
            (Operador::Tigo, "*555#") if self.pin_dealer.is_some() => self.abrir(
                Paso::TigoDealerMenu,
                "Tigo Money\n1- Transferencias\n2- Paquetigos\n3- Recarga Tigo\n4- Pagar Facturas Tigo\n5- Pagar Servicios\n6- Olvide mi PIN\n7- Ubicar Punto Tigo Money\n8- Consultar saldo\n9- Otros".into(),
            ),
            (Operador::Entel, "*105#") => {
                let t = self.texto_saldo();
                self.cerrar(t)
            }
            // Recarga Entel dealer: `*133*71234567*050#` (Bs 0.50) o `*133*71234567*5#` (Bs 5). Las pantallas siguen la receta
            // `entel-dealer-recargar` (medida el 30-oct-2026); el texto exacto de "no es dealer" no se midió.
            (Operador::Entel, c) if c.starts_with("*133*") && c.ends_with('#') && c.matches('*').count() == 3 => {
                let partes: Vec<&str> = c[5..c.len() - 1].split('*').collect();
                let (numero, cent) = (partes[0], partes.get(1).copied().unwrap_or(""));
                if !self.dealer_entel {
                    return self.cerrar("El numero de dealer no existe");
                }
                let numero_ok = numero.len() == 8 && numero.chars().all(|x| x.is_ascii_digit());
                // Bs enteros (`1` = Bs 1); la única fracción es `050` (Bs 0.50, medido el 30-oct-2026).
                let centavos = match cent {
                    _ if !numero_ok || cent.is_empty() || !cent.chars().all(|x| x.is_ascii_digit()) => None,
                    "050" => Some(50),
                    _ if cent.starts_with('0') => None,
                    _ => cent.parse::<u64>().ok().map(|b| b * 100),
                };
                let Some(centavos) = centavos else {
                    return self.cerrar("Datos invalidos");
                };
                let t = format!("Esta seguro de recargar Bs. {}.{:02} al telefono {numero}\n1 Si\n2 No", centavos / 100, centavos % 100);
                self.abrir(Paso::EntelDealerConfirmar { numero: numero.to_string(), centavos }, t)
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
            Paso::TigoDealerMenu => match r {
                "8" => self.abrir(Paso::TigoDealerSaldoPin, "Ingresa tu PIN para confirmar:".into()),
                "3" => self.abrir(Paso::TigoDealerRecargas, "Recarga Tigo\n1- Recarga mi numero\n2- Recarga a otro numero".into()),
                _ => self.cerrar("Opcion invalida"),
            },
            Paso::TigoDealerSaldoPin => {
                if self.pin_dealer.as_deref() != Some(r) {
                    return Some(self.cerrar("PIN incorrecto"));
                }
                self.cerrar(format!(
                    "03/11/26 11:07\nTu saldo total Tigo Money es de: Bs {:.2}\nSaldo Principal: Bs {:.2}",
                    self.saldo_dealer, self.saldo_dealer
                ))
            }
            Paso::TigoDealerRecargas => match r {
                "2" => self.abrir(Paso::TigoDealerMonto, "Escribe el monto en Bs (desde 0.20 ctvos.):".into()),
                _ => self.cerrar("Opcion invalida"),
            },
            Paso::TigoDealerMonto => match r.parse::<f64>() {
                Ok(m) if m >= 0.2 => self.abrir(
                    Paso::TigoDealerNumero { monto: r.to_string() },
                    "Ingresa el numero Tigo al que quieres realizar la compra de recarga:".into(),
                ),
                _ => self.cerrar("Monto invalido"),
            },
            Paso::TigoDealerNumero { monto } => {
                if !(r.len() == 8 && r.chars().all(|c| c.is_ascii_digit())) {
                    return Some(self.cerrar("Numero invalido"));
                }
                let m: f64 = monto.parse().unwrap_or(0.0);
                let t = format!("Deseas comprar recarga por Bs {m:.2}  para el {r}. Introduce tu PIN para confirmar:");
                self.abrir(Paso::TigoDealerPin { monto, numero: r.to_string() }, t)
            }
            Paso::TigoDealerPin { monto, numero } => {
                if self.pin_dealer.as_deref() != Some(r) {
                    return Some(self.cerrar("PIN incorrecto"));
                }
                let m: f64 = monto.parse().unwrap_or(0.0);
                if m > self.saldo_dealer + 1e-9 {
                    return Some(self.cerrar("Saldo insuficiente"));
                }
                self.saldo_dealer = ((self.saldo_dealer - m) * 100.0).round() / 100.0;
                self.movimientos.push(format!("Recarga {monto} Bs a {numero}"));
                self.cerrar(format!(
                    "Recarga Tigo por Bs {m:.2}. Tu saldo total Bs {:.2}. Factura: mifactura.tigo.net.bo PAGA CON TIGO MONEY EN LA APP MI TIGO! ID: 2790003884.",
                    self.saldo_dealer
                ))
            }
            Paso::EntelDealerConfirmar { numero, centavos } => match r {
                "1" => {
                    let m = centavos as f64 / 100.0;
                    if m > self.saldo_dealer + 1e-9 {
                        return Some(self.cerrar("Saldo insuficiente para transferencia"));
                    }
                    self.saldo_dealer = ((self.saldo_dealer - m) * 100.0).round() / 100.0;
                    self.movimientos.push(format!("Recarga {m:.2} Bs a {numero}"));
                    self.cerrar(format!(
                        "Se ha transferido Bs. {m:.2} a la linea {numero}. Su saldo actual para transferencia es de Bs. {:.2}",
                        self.saldo_dealer
                    ))
                }
                "2" => self.cerrar("Operacion cancelada"),
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
    fn recarga_dealer_tigo() {
        let mut l = LineaSimulada::tigo("69759675", 1.0);
        assert!(l.marcar("*555#").texto.contains("incorrecto"), "un chip normal no tiene *555#");
        l.pin_dealer = Some("4321".into());
        l.saldo_dealer = 10.0;
        assert!(l.marcar("*555#").texto.contains("3- Recarga Tigo"));
        l.responder("3").unwrap();
        l.responder("2").unwrap();
        l.responder("1.50").unwrap();
        assert!(l.responder("78815232").unwrap().texto.contains("PIN"));
        let f = l.responder("4321").unwrap();
        assert!(f.texto.starts_with("Recarga Tigo por Bs 1.50. Tu saldo total Bs 8.50.") && !f.abierta, "{f:?}");
        assert!((l.saldo_dealer - 8.5).abs() < 1e-9);
        l.marcar("*555#");
        assert!(l.responder("8").unwrap().texto.contains("PIN"));
        assert!(l.responder("4321").unwrap().texto.contains("Tu saldo total Tigo Money es de: Bs 8.50"));
    }

    #[test]
    fn recarga_dealer_entel() {
        let mut l = LineaSimulada::entel("71227468", 0.0);
        assert!(l.marcar("*133*71234567*050#").texto.contains("dealer no existe"), "un chip normal no es dealer");
        l.dealer_entel = true;
        l.saldo_dealer = 10.0;
        let c = l.marcar("*133*71234567*050#");
        assert!(c.abierta && c.texto.starts_with("Esta seguro de recargar Bs. 0.50 al telefono 71234567"), "{c:?}");
        let f = l.responder("1").unwrap();
        assert!(f.texto.starts_with("Se ha transferido Bs. 0.50 a la linea 71234567.") && !f.abierta, "{f:?}");
        assert!((l.saldo_dealer - 9.5).abs() < 1e-9);
        // Montos enteros en Bs: `1` es Bs 1 (no 1 centavo).
        assert!(l.marcar("*133*71234567*1#").texto.starts_with("Esta seguro de recargar Bs. 1.00 al telefono"));
        assert!(l.responder("1").unwrap().texto.starts_with("Se ha transferido Bs. 1.00"));
        assert!((l.saldo_dealer - 8.5).abs() < 1e-9);
        assert!(l.marcar("*133*71234567*025#").texto.contains("invalidos"), "solo 050 es fracción");
    }

    #[test]
    fn limite_diario() {
        let mut l = LineaSimulada::tigo("1", 100.0);
        l.transferencias_hoy = 5;
        assert!(l.marcar("*66#1#").texto.contains("maximo numero de transferecias diarias"));
    }
}
