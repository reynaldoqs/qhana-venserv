use super::*;
use crate::canal::{CanalUssd, ErrorCanal};
use crate::simulacion::LineaSimulada;
use async_trait::async_trait;
use protocolo::{codigos, ModoUssd, PasoUssd};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

const TRANSFERIR: &str = include_str!("../../../../recetas/tigo-transferir.yaml");
const SALDO: &str = include_str!("../../../../recetas/tigo-saldo.yaml");
const MOVIMIENTOS: &str = include_str!("../../../../recetas/tigo-movimientos.yaml");
const ENTEL: &str = include_str!("../../../../recetas/entel-transferencias.yaml");

#[derive(Default, Clone, Copy)]
enum Falla {
    #[default]
    Ninguna,
    /// Aplica la transferencia pero la respuesta al "Si" nunca llega.
    TimeoutTrasConfirmar,
    /// El operador muestra otro número en la confirmación.
    NumeroCambiado,
}

/// Canal falso: un teléfono con chips simulados.
struct CanalFalso {
    lineas: Mutex<Vec<LineaSimulada>>,
    sesion: Mutex<Option<(String, usize)>>,
    contador: AtomicUsize,
    cancelaciones: AtomicUsize,
    envios: Mutex<Vec<String>>,
    ultima: Mutex<String>,
    falla: Falla,
}

impl CanalFalso {
    fn new(lineas: Vec<LineaSimulada>) -> Self {
        Self {
            lineas: Mutex::new(lineas),
            sesion: Mutex::new(None),
            contador: AtomicUsize::new(0),
            cancelaciones: AtomicUsize::new(0),
            envios: Mutex::new(vec![]),
            ultima: Mutex::new(String::new()),
            falla: Falla::Ninguna,
        }
    }
    fn con_falla(mut self, f: Falla) -> Self {
        self.falla = f;
        self
    }
    fn linea(&self, i: usize) -> LineaSimulada {
        self.lineas.lock().unwrap()[i].clone()
    }
    fn cancelaciones(&self) -> usize {
        self.cancelaciones.load(Ordering::SeqCst)
    }
    fn alterar(&self, mut texto: String) -> String {
        if let Falla::NumeroCambiado = self.falla {
            if texto.contains("Confirma") {
                texto = texto.replace("a 69869869", "a 69869860");
            }
        }
        *self.ultima.lock().unwrap() = texto.clone();
        texto
    }
}

#[async_trait]
impl CanalUssd for CanalFalso {
    async fn iniciar(&self, slot: u32, codigo: &str, modo: ModoUssd, _t: Duration) -> Result<PasoUssd, ErrorCanal> {
        self.envios.lock().unwrap().push(codigo.to_string());
        let mut lineas = self.lineas.lock().unwrap();
        let l = lineas
            .get_mut(slot as usize)
            .ok_or_else(|| ErrorCanal::nuevo(codigos::SIM_INVALIDA, "no hay chip"))?;
        let p = l.marcar(codigo);
        let abierta = p.abierta && modo == ModoUssd::Pantalla;
        if !abierta {
            l.cancelar();
        }
        let id = format!("s{}", self.contador.fetch_add(1, Ordering::SeqCst));
        *self.sesion.lock().unwrap() = if abierta { Some((id.clone(), slot as usize)) } else { None };
        Ok(PasoUssd { sesion: id, texto: self.alterar(p.texto), abierta, ms: 5 })
    }

    async fn responder(&self, sesion: &str, texto: &str, _t: Duration) -> Result<PasoUssd, ErrorCanal> {
        self.envios.lock().unwrap().push(texto.to_string());
        let slot = match &*self.sesion.lock().unwrap() {
            Some((s, slot)) if s == sesion => *slot,
            _ => return Err(ErrorCanal::nuevo(codigos::SESION_INEXISTENTE, sesion)),
        };
        let era_confirmacion = self.ultima.lock().unwrap().contains("Confirma");
        let p = self.lineas.lock().unwrap()[slot]
            .responder(texto)
            .ok_or_else(|| ErrorCanal::nuevo(codigos::SESION_INEXISTENTE, sesion))?;
        if !p.abierta {
            *self.sesion.lock().unwrap() = None;
        }
        if era_confirmacion && texto == "1" {
            if let Falla::TimeoutTrasConfirmar = self.falla {
                return Err(ErrorCanal::timeout());
            }
        }
        Ok(PasoUssd { sesion: sesion.into(), texto: self.alterar(p.texto), abierta: p.abierta, ms: 5 })
    }

    async fn cancelar(&self, sesion: &str) -> Result<(), ErrorCanal> {
        self.cancelaciones.fetch_add(1, Ordering::SeqCst);
        let mut s = self.sesion.lock().unwrap();
        if let Some((id, slot)) = &*s {
            if id == sesion {
                self.lineas.lock().unwrap()[*slot].cancelar();
                *s = None;
            }
        }
        Ok(())
    }
}

fn p(pares: &[(&str, &str)]) -> BTreeMap<String, String> {
    pares.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

fn tigo(saldo: f64) -> CanalFalso {
    CanalFalso::new(vec![LineaSimulada::tigo("78815232", saldo), LineaSimulada::tigo("69759675", 3.0)])
}

#[tokio::test]
async fn saldo_por_pantalla() {
    let r = Receta::desde_yaml(SALDO).unwrap();
    let c = tigo(14.0);
    let res = ejecutar(&r, &c, 0, &p(&[]), false).await;
    assert_eq!(res.estado, EstadoResultado::Ok, "{res:?}");
    assert_eq!(res.salida["saldo_principal"], "14");
    assert_eq!(res.salida["linea"], "78815232");
    assert_eq!(res.salida["saldo_total"], "24");
    // Tigo deja la sesión abierta incluso para el saldo: el motor debe cerrarla al terminar.
    assert_eq!(c.cancelaciones(), 1, "modo pantalla: la sesión se cancela al final");
}

#[tokio::test]
async fn transferencia_real() {
    let r = Receta::desde_yaml(TRANSFERIR).unwrap();
    let c = tigo(14.0);
    let res = ejecutar(&r, &c, 0, &p(&[("numero", "69869869"), ("monto", "10")]), false).await;
    assert_eq!(res.estado, EstadoResultado::Ok, "{res:?}");
    assert!(res.ok);
    assert_eq!(res.salida["costo"], "0.20");
    assert_eq!(*c.envios.lock().unwrap(), vec!["*66#1#", "1", "69869869", "4", "1"]);
    assert!((c.linea(0).saldo_principal - 3.8).abs() < 1e-9);
    assert_eq!(c.linea(0).transferencias_hoy, 1);
    assert_eq!(res.pasos.len(), 6); // 5 envíos + verificar
    assert_eq!(c.cancelaciones(), 0, "la sesión la cerró el operador");
}

#[tokio::test]
async fn simulacion_responde_no_y_no_mueve_dinero() {
    let r = Receta::desde_yaml(TRANSFERIR).unwrap();
    let c = tigo(14.0);
    let res = ejecutar(&r, &c, 0, &p(&[("numero", "69869869"), ("monto", "1")]), true).await;
    assert_eq!(res.estado, EstadoResultado::Simulado, "{res:?}");
    assert!(res.ok);
    assert_eq!(c.envios.lock().unwrap().last().unwrap(), "2");
    assert_eq!(res.pasos.last().unwrap().texto, "Transferencia cancelada");
    assert!((c.linea(0).saldo_principal - 14.0).abs() < 1e-9);
    assert_eq!(c.linea(0).transferencias_hoy, 0);
}

#[tokio::test]
async fn limite_diario_aborta_y_cancela() {
    let r = Receta::desde_yaml(TRANSFERIR).unwrap();
    let c = tigo(14.0);
    c.lineas.lock().unwrap()[0].transferencias_hoy = 5;
    let res = ejecutar(&r, &c, 0, &p(&[("numero", "69869869"), ("monto", "1")]), false).await;
    assert_eq!(res.estado, EstadoResultado::Abortado, "{res:?}");
    assert!(res.detalle.unwrap().contains("maximo numero"));
    assert_eq!(c.cancelaciones(), 1, "la sesión quedó abierta: hay que cancelarla");
    assert!(!c.linea(0).sesion_abierta());
}

#[tokio::test]
async fn monto_que_el_saldo_no_alcanza() {
    let r = Receta::desde_yaml(TRANSFERIR).unwrap();
    let c = tigo(8.0); // 10 + 0.20 no alcanza: el menú no ofrece 10 Bs
    let res = ejecutar(&r, &c, 0, &p(&[("numero", "69869869"), ("monto", "10")]), false).await;
    assert_eq!(res.estado, EstadoResultado::Abortado, "{res:?}");
    assert!(res.detalle.as_deref().unwrap().contains("no encontré la opción"));
    assert_eq!(c.cancelaciones(), 1);
    assert!((c.linea(0).saldo_principal - 8.0).abs() < 1e-9);
}

#[tokio::test]
async fn parametros_invalidos_no_marcan_nada() {
    let r = Receta::desde_yaml(TRANSFERIR).unwrap();
    let c = tigo(14.0);
    for params in [
        p(&[("numero", "123"), ("monto", "1")]),
        p(&[("numero", "69869869"), ("monto", "3")]),
        p(&[("numero", "69869869")]),
        p(&[("numero", "69869869"), ("monto", "1"), ("extra", "x")]),
    ] {
        let res = ejecutar(&r, &c, 0, &params, false).await;
        assert_eq!(res.estado, EstadoResultado::Error, "{params:?} → {res:?}");
    }
    assert!(c.envios.lock().unwrap().is_empty());
}

#[tokio::test]
async fn timeout_tras_confirmar_es_incierto() {
    let r = Receta::desde_yaml(TRANSFERIR).unwrap();
    let c = tigo(14.0).con_falla(Falla::TimeoutTrasConfirmar);
    let res = ejecutar(&r, &c, 0, &p(&[("numero", "69869869"), ("monto", "1")]), false).await;
    assert_eq!(res.estado, EstadoResultado::Incierto, "{res:?}");
    assert!(!res.ok);
    // El "Si" se envió una sola vez: nunca se reintenta.
    let envios = c.envios.lock().unwrap().clone();
    assert_eq!(envios.iter().filter(|e| *e == "1").count(), 3); // menú, monto 1 Bs, Si
}

#[tokio::test]
async fn numero_distinto_en_confirmacion_aborta_sin_confirmar() {
    let r = Receta::desde_yaml(TRANSFERIR).unwrap();
    let c = tigo(14.0).con_falla(Falla::NumeroCambiado);
    let res = ejecutar(&r, &c, 0, &p(&[("numero", "69869869"), ("monto", "1")]), false).await;
    assert_eq!(res.estado, EstadoResultado::Abortado, "{res:?}");
    assert!(res.detalle.unwrap().contains("no coincide"));
    assert_eq!(c.cancelaciones(), 1);
    assert_eq!(c.linea(0).transferencias_hoy, 0);
}

#[tokio::test]
async fn movimientos_tigo() {
    let r = Receta::desde_yaml(MOVIMIENTOS).unwrap();
    let c = tigo(14.0);
    let res = ejecutar(&r, &c, 0, &p(&[]), false).await;
    assert_eq!(res.estado, EstadoResultado::Ok, "{res:?}");
    assert!(res.pasos.last().unwrap().texto.starts_with("Ultimos movimientos"));
}

#[tokio::test]
async fn entel_formato_sin_separador() {
    let r = Receta::desde_yaml(ENTEL).unwrap();
    let c = CanalFalso::new(vec![LineaSimulada::entel("71227468", 0.0)]);
    let res = ejecutar(&r, &c, 0, &p(&[]), false).await;
    assert_eq!(res.estado, EstadoResultado::Ok, "{res:?}");
    assert_eq!(*c.envios.lock().unwrap(), vec!["*133#", "3", "1"]);
    assert_eq!(res.salida["ultima"], "59163819601 Bs 1.00 03/09/2026 22:32:12");
}

#[tokio::test]
async fn chip_inexistente_aborta() {
    let r = Receta::desde_yaml(SALDO).unwrap();
    let c = tigo(14.0);
    let res = ejecutar(&r, &c, 7, &p(&[]), false).await;
    assert_eq!(res.estado, EstadoResultado::Abortado);
    assert!(res.detalle.unwrap().contains("sim_invalida"));
}

const DOS_MARCAR: &str = r#"
nombre: dos-marcar
titulo: Dos consultas
operador: TIGO
modo: pantalla
accion: saldo
pasos:
  - marcar: "*105#"
  - marcar: "*#66#"
"#;

#[tokio::test]
async fn segundo_marcar_con_la_sesion_abierta_es_error() {
    // La receta es válida (un segundo 'marcar' vale cuando la sesión cerró), pero Tigo deja la sesión abierta:
    // el motor no marca encima de una sesión viva y la cierra.
    let r = Receta::desde_yaml(DOS_MARCAR).expect("dos 'marcar' son válidos");
    let c = tigo(14.0);
    let res = ejecutar(&r, &c, 0, &p(&[]), false).await;
    assert_ne!(res.estado, EstadoResultado::Ok, "{res:?}");
    assert!(res.detalle.as_deref().unwrap_or("").contains("sesión abierta"), "{res:?}");
    assert_eq!(c.cancelaciones(), 1, "la sesión que quedó abierta se cierra");
}
