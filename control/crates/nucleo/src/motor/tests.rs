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

// ───────────── recarga dealer Tigo (*555#) ─────────────

const TIGO_DEALER: &str = include_str!("../../../../recetas/tigo-dealer-recargar.yaml");

/// Canal que responde una pantalla fija por cada envío, en orden.
struct Guion {
    pantallas: Mutex<Vec<&'static str>>,
    envios: Mutex<Vec<String>>,
}

impl Guion {
    fn new(pantallas: &[&'static str]) -> Self {
        Self { pantallas: Mutex::new(pantallas.iter().rev().copied().collect()), envios: Mutex::new(vec![]) }
    }
    fn siguiente(&self, enviado: &str) -> Result<PasoUssd, ErrorCanal> {
        self.envios.lock().unwrap().push(enviado.to_string());
        let mut p = self.pantallas.lock().unwrap();
        let texto = p.pop().ok_or_else(ErrorCanal::timeout)?;
        Ok(PasoUssd { sesion: "g".into(), texto: texto.into(), abierta: !p.is_empty(), ms: 5 })
    }
}

#[async_trait]
impl CanalUssd for Guion {
    async fn iniciar(&self, _slot: u32, codigo: &str, _m: ModoUssd, _t: Duration) -> Result<PasoUssd, ErrorCanal> {
        self.siguiente(codigo)
    }
    async fn responder(&self, _s: &str, texto: &str, _t: Duration) -> Result<PasoUssd, ErrorCanal> {
        self.siguiente(texto)
    }
    async fn cancelar(&self, _s: &str) -> Result<(), ErrorCanal> {
        Ok(())
    }
}

// Pantallas reales de *555# (3-nov-2026).
const MENU_555: &str = "Tigo Money\n1- Transferencias\n2- Paquetigos\n3- Recarga Tigo\n4- Pagar Facturas Tigo\n5- Pagar Servicios\n6- Olvide mi PIN\n7- Ubicar Punto Tigo Money\n8- Consultar saldo\n9- Otros";
const RECARGA_TIGO: &str = "Recarga Tigo\n1- Recarga mi numero\n2- Recarga a otro numero";
const MONTO_555: &str = "Escribe el monto en Bs (desde 0.20 ctvos.):";
const NUMERO_555: &str = "Ingresa el numero Tigo al que quieres realizar la compra de recarga:";
const CONFIRMA_555: &str = "Deseas comprar recarga por Bs 1.50  para el 78815232. Introduce tu PIN para confirmar:";

fn guion_dealer(final_: &'static str) -> Guion {
    guion_dealer_con(CONFIRMA_555, final_)
}

fn guion_dealer_con(confirma: &'static str, final_: &'static str) -> Guion {
    Guion::new(&[MENU_555, RECARGA_TIGO, MONTO_555, NUMERO_555, confirma, final_])
}

fn params_dealer() -> BTreeMap<String, String> {
    p(&[("numero", "78815232"), ("monto", "1.50"), ("pin", "4321")])
}

#[tokio::test]
async fn dealer_recarga_sigue_los_pasos_y_tapa_el_pin() {
    let r = Receta::desde_yaml(TIGO_DEALER).unwrap();
    assert!(r.resumen().mueve_dinero);
    let c = guion_dealer("Recarga Tigo por Bs 1.50. Tu saldo total Bs 9.56. Factura: mifactura.tigo.net.bo ID: 1.");
    let res = ejecutar(&r, &c, 0, &params_dealer(), false).await;
    assert_eq!(res.estado, EstadoResultado::Ok, "{res:?}");
    assert_eq!(res.salida["saldo_dealer"], "9.56");
    assert_eq!(res.salida["monto_bs"], "1.50");
    assert_eq!(*c.envios.lock().unwrap(), ["*555#", "3", "2", "1.50", "78815232", "4321"]);
    let enviados: Vec<_> = res.pasos.iter().filter_map(|x| x.enviado.clone()).collect();
    assert_eq!(enviados.last().unwrap(), crate::receta::TAPADO);
    assert!(!format!("{res:?}").contains("4321"), "el PIN no debe salir en el resultado");
}

#[tokio::test]
async fn dealer_simular_no_envia_el_pin() {
    let r = Receta::desde_yaml(TIGO_DEALER).unwrap();
    let c = guion_dealer("Recarga Tigo por Bs 1.50.");
    let res = ejecutar(&r, &c, 0, &params_dealer(), true).await;
    assert_eq!(res.estado, EstadoResultado::Simulado, "{res:?}");
    assert!(!c.envios.lock().unwrap().contains(&"4321".to_string()));
}

#[tokio::test]
async fn dealer_sin_pedido_de_pin_aborta_antes() {
    let r = Receta::desde_yaml(TIGO_DEALER).unwrap();
    let c = guion_dealer_con("Opcion invalida", "x");
    let res = ejecutar(&r, &c, 0, &params_dealer(), false).await;
    assert_eq!(res.estado, EstadoResultado::Abortado, "{res:?}");
    assert!(!c.envios.lock().unwrap().contains(&"4321".to_string()));
}

#[tokio::test]
async fn dealer_confirmacion_con_otro_monto_o_numero_no_envia_el_pin() {
    let r = Receta::desde_yaml(TIGO_DEALER).unwrap();
    for confirma in [
        "Deseas comprar recarga por Bs 15.00  para el 78815232. Introduce tu PIN para confirmar:",
        "Deseas comprar recarga por Bs 1.50  para el 78815230. Introduce tu PIN para confirmar:",
    ] {
        let c = guion_dealer_con(confirma, "x");
        let res = ejecutar(&r, &c, 0, &params_dealer(), false).await;
        assert_eq!(res.estado, EstadoResultado::Abortado, "{confirma}: {res:?}");
        assert!(!c.envios.lock().unwrap().contains(&"4321".to_string()));
    }
    // Monto entero: la pantalla lo muestra con .00.
    let c = guion_dealer_con("Deseas comprar recarga por Bs 2.00  para el 78815232. Introduce tu PIN para confirmar:", "Recarga Tigo por Bs 2.00.");
    let res = ejecutar(&r, &c, 0, &p(&[("numero", "78815232"), ("monto", "2"), ("pin", "4321")]), false).await;
    assert_eq!(res.estado, EstadoResultado::Ok, "{res:?}");
    // …y con monto 2, una confirmación de Bs 20.00 no pasa.
    let c = guion_dealer_con("Deseas comprar recarga por Bs 20.00  para el 78815232. Introduce tu PIN para confirmar:", "x");
    let res = ejecutar(&r, &c, 0, &p(&[("numero", "78815232"), ("monto", "2"), ("pin", "4321")]), false).await;
    assert_eq!(res.estado, EstadoResultado::Abortado, "{res:?}");
}

#[tokio::test]
async fn dealer_pin_incorrecto_o_respuesta_rara_es_incierto() {
    let r = Receta::desde_yaml(TIGO_DEALER).unwrap();
    for final_ in ["PIN incorrecto", "Gracias"] {
        let res = ejecutar(&r, &guion_dealer(final_), 0, &params_dealer(), false).await;
        assert_eq!(res.estado, EstadoResultado::Incierto, "{final_}: {res:?}");
        assert!(!res.detalle.unwrap_or_default().contains("4321"));
    }
}

const TIGO_DEALER_SALDO: &str = include_str!("../../../../recetas/tigo-dealer-saldo.yaml");

#[tokio::test]
async fn dealer_saldo_lee_el_saldo_y_tapa_el_pin() {
    let r = Receta::desde_yaml(TIGO_DEALER_SALDO).unwrap();
    assert!(!r.resumen().mueve_dinero, "es solo consulta");
    let mut l = LineaSimulada::tigo("70000101", 1.0);
    l.pin_dealer = Some("4321".into());
    l.saldo_dealer = 37.5;
    let c = CanalFalso::new(vec![l]);
    let res = ejecutar(&r, &c, 0, &p(&[("pin", "4321")]), false).await;
    assert_eq!(res.estado, EstadoResultado::Ok, "{res:?}");
    assert_eq!(res.salida["saldo_dealer"], "37.50", "no el 9 de «9- Otros» del menú");
    assert_eq!(*c.envios.lock().unwrap(), ["*555#", "8", "4321"]);
    assert!(!format!("{res:?}").contains("4321"));

    let res = ejecutar(&r, &c, 0, &p(&[("pin", "1111")]), false).await;
    assert_eq!(res.estado, EstadoResultado::Abortado, "PIN incorrecto: {res:?}");
}

// ───────────────────────────── montos: formato base y valores calculados ─────────────────────────────

const ENTEL_DEALER: &str = include_str!("../../../../recetas/entel-dealer-recargar.yaml");

fn entel_dealer() -> CanalFalso {
    let mut l = LineaSimulada::entel("71227468", 0.0);
    l.dealer_entel = true;
    l.saldo_dealer = 10.0;
    CanalFalso::new(vec![l])
}

#[tokio::test]
async fn entel_dealer_marca_050_para_50_centavos() {
    let r = Receta::desde_yaml(ENTEL_DEALER).unwrap();
    // Llegue como llegue, Bs 0.50 se marca *133*N*050#.
    for monto in ["0.50", "0,5", "Bs 0.50", ".50"] {
        let c = entel_dealer();
        let res = ejecutar(&r, &c, 0, &p(&[("numero", "71234567"), ("monto", monto)]), false).await;
        assert_eq!(res.estado, EstadoResultado::Ok, "{monto}: {res:?}");
        assert_eq!(c.envios.lock().unwrap()[0], "*133*71234567*050#", "{monto}");
        assert_eq!(res.pasos[0].enviado.as_deref(), Some("*133*71234567*050#"));
        assert!((c.linea(0).saldo_dealer - 9.5).abs() < 1e-9);
        assert_eq!(res.salida["saldo_transferencia"], "9.50");
    }
}

#[tokio::test]
async fn entel_dealer_marca_bs_enteros_tal_cual() {
    let r = Receta::desde_yaml(ENTEL_DEALER).unwrap();
    // *133*N*1# es Bs 1 (no 100): los enteros van en Bs.
    for (monto, codigo, queda) in [("1", "1", 9.0), ("5", "5", 5.0), ("Bs 10,00", "10", 0.0)] {
        let c = entel_dealer();
        let res = ejecutar(&r, &c, 0, &p(&[("numero", "71234567"), ("monto", monto)]), false).await;
        assert_eq!(res.estado, EstadoResultado::Ok, "{monto}: {res:?}");
        assert_eq!(c.envios.lock().unwrap()[0], format!("*133*71234567*{codigo}#"), "{monto}");
        assert!((c.linea(0).saldo_dealer - queda).abs() < 1e-9, "{monto}");
    }
}

#[tokio::test]
async fn entel_dealer_no_acepta_otras_fracciones() {
    let r = Receta::desde_yaml(ENTEL_DEALER).unwrap();
    // Solo 0.50 tiene fracción: 1.50, 0.25 o 0.05 no se marcan.
    for monto in ["1.50", "0.25", "0.05"] {
        let c = entel_dealer();
        let res = ejecutar(&r, &c, 0, &p(&[("numero", "71234567"), ("monto", monto)]), false).await;
        assert_eq!(res.estado, EstadoResultado::Error, "{monto}: {res:?}");
        assert!(c.envios.lock().unwrap().is_empty(), "{monto}: no marca nada");
    }
}

#[tokio::test]
async fn entel_dealer_verifica_el_monto_que_muestra_la_confirmacion() {
    let r = Receta::desde_yaml(ENTEL_DEALER).unwrap();
    // El operador muestra otro monto: se cancela sin confirmar.
    let y = ENTEL_DEALER.replace("{monto_mixto}", "1{monto_mixto}");
    let r2 = Receta::desde_yaml(&y).unwrap();
    let c = entel_dealer();
    let res = ejecutar(&r2, &c, 0, &p(&[("numero", "71234567"), ("monto", "0.50")]), false).await;
    assert_eq!(res.estado, EstadoResultado::Abortado, "{res:?}");
    assert!(res.detalle.unwrap().contains("no coincide"));
    assert!((c.linea(0).saldo_dealer - 10.0).abs() < 1e-9, "no se movió nada");
    // Simulación: llega a la confirmación y responde No.
    let c = entel_dealer();
    let res = ejecutar(&r, &c, 0, &p(&[("numero", "71234567"), ("monto", "0.50")]), true).await;
    assert_eq!(res.estado, EstadoResultado::Simulado, "{res:?}");
    assert_eq!(*c.envios.lock().unwrap(), vec!["*133*71234567*050#", "2"]);
    assert!((c.linea(0).saldo_dealer - 10.0).abs() < 1e-9);
}

#[tokio::test]
async fn monto_entero_con_centavos_no_corre() {
    let y = "nombre: x\nparametros:\n  - { nombre: monto }\npasos:\n  - marcar: '*1*{monto_entero}#'\n";
    let r = Receta::desde_yaml(y).unwrap();
    let c = tigo(14.0);
    let res = ejecutar(&r, &c, 0, &p(&[("monto", "1.50")]), false).await;
    assert_eq!(res.estado, EstadoResultado::Error);
    assert!(res.detalle.unwrap().contains(crate::monto::SOLO_ENTEROS));
    assert!(c.envios.lock().unwrap().is_empty());
    let res = ejecutar(&r, &c, 0, &p(&[("monto", "5.00")]), false).await;
    assert_eq!(c.envios.lock().unwrap()[0], "*1*5#", "{res:?}");
}

#[test]
fn parametros_canonicos_y_valores_calculados() {
    let r = Receta::desde_yaml(TIGO_DEALER).unwrap();
    let v = validar_parametros(&r, &p(&[("numero", "69869869"), ("monto", "1,5"), ("pin", "1234")])).unwrap();
    assert_eq!(v["monto"], "1.50", "el patrón de tigo-dealer-recargar se aplica al monto canónico");
    assert!(!v.contains_key("monto_cent"), "los calculados no van al historial");
    let c = con_calculados(&r, &v);
    assert_eq!((c["monto_cent"].as_str(), c["monto_cent3"].as_str()), ("150", "150"));
    assert!(!c.contains_key("monto_entero"));
    assert_eq!(validar_parametros(&r, &p(&[("numero", "69869869"), ("monto", "2.00"), ("pin", "1234")])).unwrap()["monto"], "2");
    // Las opciones enteras de siempre siguen igual.
    let t = Receta::desde_yaml(TRANSFERIR).unwrap();
    assert_eq!(validar_parametros(&t, &p(&[("numero", "69869869"), ("monto", "10.00")])).unwrap()["monto"], "10");
    assert!(validar_parametros(&t, &p(&[("numero", "69869869"), ("monto", "1.50")])).is_err());
    // Un valor calculado no se puede mandar como parámetro.
    assert!(validar_parametros(&t, &p(&[("numero", "69869869"), ("monto", "1"), ("monto_cent", "100")])).is_err());
}
