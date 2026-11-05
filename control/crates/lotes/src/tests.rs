//! Pruebas de punta a punta del servicio de lotes con teléfonos falsos (pantallas reales de Tigo).

use super::*;
use async_trait::async_trait;
use nucleo::simulacion::LineaSimulada;
use nucleo::{Bus, CanalUssd, ErrorCanal, Receta, Registro};
use protocolo::{codigos, Hola, ModoUssd, PasoUssd, Sim};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, RwLock};
use std::time::Duration;

const RECETAS: [&str; 7] = [
    include_str!("../../../recetas/tigo-transferir.yaml"),
    include_str!("../../../recetas/tigo-saldo.yaml"),
    include_str!("../../../recetas/tigo-movimientos.yaml"),
    include_str!("../../../recetas/entel-transferencias.yaml"),
    include_str!("../../../recetas/tigo-dealer-recargar.yaml"),
    include_str!("../../../recetas/tigo-dealer-saldo.yaml"),
    include_str!("../../../recetas/entel-dealer-recargar.yaml"),
];

/// Teléfono falso con chips simulados.
struct Telefono {
    lineas: Mutex<Vec<LineaSimulada>>,
    sesion: Mutex<Option<(String, usize)>>,
    n: AtomicUsize,
    /// La respuesta al "Si" nunca llega (pero la transferencia se aplica).
    timeout_tras_confirmar: AtomicBool,
    /// Mientras esté puesto, contestar este número de destino falla por timeout (antes de confirmar: no
    /// se mueve dinero) y el envío termina `fallido` tras sus reintentos.
    fallar_numero: Mutex<Option<String>>,
    /// La próxima marcación entra en pánico (una sola vez).
    explotar: AtomicBool,
    /// Toda marcación responde `ocupado` (p. ej. una ventana USSD que mandó la red).
    siempre_ocupado: AtomicBool,
    demora: Duration,
    /// Códigos marcados, en orden (para revisar lo que realmente se marcó).
    marcados: Mutex<Vec<String>>,
}

impl Telefono {
    fn new(lineas: Vec<LineaSimulada>) -> Arc<Self> {
        Self::con_demora(lineas, Duration::ZERO)
    }
    fn con_demora(lineas: Vec<LineaSimulada>, demora: Duration) -> Arc<Self> {
        Arc::new(Self {
            lineas: Mutex::new(lineas),
            sesion: Mutex::new(None),
            n: AtomicUsize::new(0),
            timeout_tras_confirmar: AtomicBool::new(false),
            fallar_numero: Mutex::new(None),
            explotar: AtomicBool::new(false),
            siempre_ocupado: AtomicBool::new(false),
            demora,
            marcados: Mutex::new(vec![]),
        })
    }
    fn linea(&self, i: usize) -> LineaSimulada {
        self.lineas.lock().unwrap()[i].clone()
    }
}

#[async_trait]
impl CanalUssd for Telefono {
    async fn iniciar(&self, slot: u32, codigo: &str, modo: ModoUssd, _t: Duration) -> Result<PasoUssd, ErrorCanal> {
        tokio::time::sleep(self.demora).await;
        if self.explotar.swap(false, Ordering::SeqCst) {
            panic!("el canal explotó");
        }
        if self.siempre_ocupado.load(Ordering::SeqCst) || self.sesion.lock().unwrap().is_some() {
            return Err(ErrorCanal::nuevo(codigos::OCUPADO, "ya hay una sesión USSD abierta"));
        }
        let mut ls = self.lineas.lock().unwrap();
        let l = ls.get_mut(slot as usize).ok_or_else(|| ErrorCanal::nuevo(codigos::SIM_INVALIDA, "no hay chip"))?;
        self.marcados.lock().unwrap().push(codigo.to_string());
        let p = l.marcar(codigo);
        let abierta = p.abierta && modo == ModoUssd::Pantalla;
        let id = format!("s{}", self.n.fetch_add(1, Ordering::SeqCst));
        if abierta {
            *self.sesion.lock().unwrap() = Some((id.clone(), slot as usize));
        } else {
            l.cancelar();
        }
        Ok(PasoUssd { sesion: id, texto: p.texto, abierta, ms: 1 })
    }

    async fn responder(&self, sesion: &str, texto: &str, _t: Duration) -> Result<PasoUssd, ErrorCanal> {
        tokio::time::sleep(self.demora).await;
        if self.fallar_numero.lock().unwrap().as_deref() == Some(texto) {
            return Err(ErrorCanal::timeout());
        }
        let slot = match &*self.sesion.lock().unwrap() {
            Some((s, slot)) if s == sesion => *slot,
            _ => return Err(ErrorCanal::nuevo(codigos::SESION_INEXISTENTE, sesion)),
        };
        let (p, era_confirmar) = {
            let mut ls = self.lineas.lock().unwrap();
            let era = ls[slot].sesion_abierta();
            let antes_saldo = ls[slot].saldo_principal;
            let p = ls[slot].responder(texto).ok_or_else(|| ErrorCanal::nuevo(codigos::SESION_INEXISTENTE, sesion))?;
            (p, era && ls[slot].saldo_principal < antes_saldo)
        };
        if !p.abierta {
            *self.sesion.lock().unwrap() = None;
        }
        if era_confirmar && self.timeout_tras_confirmar.load(Ordering::SeqCst) {
            return Err(ErrorCanal::timeout());
        }
        Ok(PasoUssd { sesion: sesion.into(), texto: p.texto, abierta: p.abierta, ms: 1 })
    }

    async fn cancelar(&self, sesion: &str) -> Result<(), ErrorCanal> {
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

fn sim(slot: u32, numero: &str) -> Sim {
    Sim {
        slot,
        sub_id: slot as i64 + 1,
        operador: "TIGO".into(),
        mcc_mnc: "73603".into(),
        iccid: None,
        numero: Some(numero.into()),
        estado: "listo".into(),
        voz: Some("en_servicio".into()),
        red: Some("LTE".into()),
    }
}

struct Banco {
    lotes: Arc<Lotes>,
    registro: Arc<Registro>,
}

fn recetas() -> contexto::Recetas {
    let mut m = BTreeMap::new();
    for y in RECETAS {
        let r = Receta::desde_yaml(y).unwrap();
        m.insert(r.nombre.clone(), r);
    }
    Arc::new(RwLock::new(m))
}

fn esperas_rapidas() -> Esperas {
    Esperas {
        reintento: Duration::from_millis(20),
        ocupado: Duration::from_millis(20),
        tic: Duration::from_millis(10),
        max_espera_ocupado: Duration::from_millis(500),
    }
}

fn banco_con(almacen: Arc<almacen::Almacen>, telefonos: &[(&str, Arc<Telefono>)]) -> Banco {
    let registro = Arc::new(Registro::new());
    for (id, t) in telefonos {
        let sims = t
            .lineas
            .lock()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(i, l)| match l.operador {
                nucleo::simulacion::Operador::Tigo => sim(i as u32, &l.linea),
                nucleo::simulacion::Operador::Entel => {
                    Sim { operador: "ENTEL".into(), mcc_mnc: "73602".into(), ..sim(i as u32, &l.linea) }
                }
            })
            .collect();
        let hola = Hola {
            protocolo: 1,
            token: String::new(),
            dispositivo_id: id.to_string(),
            fabricante: "TEST".into(),
            modelo: format!("Tel {id}"),
            android: "14".into(),
            sdk: 34,
            app_version: "0".into(),
            sims,
            capacidades: vec![],
            adb_serial: None,
        };
        registro.conectar(&hola, t.clone());
    }
    let ctx = Contexto::new(registro.clone(), Bus::default(), almacen, recetas()).unwrap();
    Banco { lotes: Lotes::new(ctx, esperas_rapidas()).unwrap(), registro }
}

fn banco(telefonos: &[(&str, Arc<Telefono>)]) -> Banco {
    banco_con(Arc::new(almacen::Almacen::en_memoria().unwrap()), telefonos)
}

async fn esperar_fin(l: &Arc<Lotes>, id: i64) -> Lote {
    for _ in 0..1_000 {
        if l.en_curso().is_empty() {
            let lote = l.obtener(id).unwrap();
            if lote.estado != EstadoLote::EnCurso {
                return lote;
            }
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("el lote {id} no terminó");
}

fn todos() -> FiltroLotes {
    FiltroLotes { limite: 50, archivados: Some(false), ..Default::default() }
}

fn filas(l: &Lote) -> Vec<FilaLote> {
    l.filas.clone().unwrap()
}

// ───────────────────────────── validación ─────────────────────────────

#[tokio::test]
async fn validacion_completa() {
    let t = Telefono::new(vec![LineaSimulada::tigo("78815232", 30.0), LineaSimulada::tigo("69759675", 3.0)]);
    let b = banco(&[("tel1", t)]);
    // Saldos conocidos para un plan determinista.
    b.lotes.ctx.fijar_saldo("tel1", 0, 30.0, None, None);
    b.lotes.ctx.fijar_saldo("tel1", 1, 3.0, None, None);
    let csv = "\u{feff}Teléfono;Monto (Bs);Operadora;Acción;Ref\n\
               +591 6986-9869;12,00;tigo;;Pedido 1\n\
               69869869;Bs 1;TIGO;transferir;Pedido 2\n\
               71227468;5;ENTEL;;Pedido 3\n\
               1234;5;TIGO;;Pedido 4\n\
               78815232;12,50;TIGO;;Pedido 5\n\
               78815232;150;TIGO;;Pedido 6\n\
               78815233;7;claro;;Pedido 7\n";
    let v = b.lotes.validar(csv, false, None).await.unwrap();
    let f = |linea: u32| v.filas.iter().find(|x| x.fila == linea).unwrap().clone();

    let f2 = f(2);
    assert_eq!(f2.numero.as_deref(), Some("69869869"));
    assert_eq!(f2.monto, Some(12.0));
    assert_eq!(f2.envios, vec![10.0, 2.0]);
    assert_eq!(f2.receta.as_deref(), Some("tigo-transferir"));
    assert_eq!(f2.estado, EstadoValidacion::Aviso, "{f2:?}"); // número repetido con la fila 3
    assert!(f2.avisos.iter().any(|a| a.contains("repetido")));
    // Plan: los dos envíos al chip con más saldo (30).
    assert!(f2.plan.iter().all(|p| p.dispositivo == "tel1" && p.slot == 0), "{:?}", f2.plan);

    assert!(f(4).errores.iter().any(|e| e.contains("no hay receta de transferir para ENTEL")));
    assert!(f(5).errores.iter().any(|e| e.contains("8 dígitos")));
    assert!(f(6).errores.iter().any(|e| e.contains("centavos")));
    let f7 = f(7);
    assert!(f7.avisos.iter().any(|a| a.contains("monto alto")));
    assert_eq!(f7.envios, vec![20.0; 7].into_iter().chain([10.0]).collect::<Vec<_>>());
    assert!(f(8).errores.iter().any(|e| e.contains("operador desconocido")));

    let r = &v.resumen;
    assert_eq!(r.filas, 7);
    assert_eq!(r.invalidas, 4);
    assert_eq!(r.validas, 3);
    assert_eq!(r.total_bs, 12.0 + 1.0 + 150.0);
    assert_eq!(r.envios, 2 + 1 + 8);
    assert!((r.comision_total - 11.0 * 0.2).abs() < 1e-9);
    // Cupo: 5 + 5 = 10 envíos para 11 → al menos 1 sin capacidad; y el saldo de 33 no alcanza para 163.
    assert!(r.sin_capacidad >= 1);
    assert_eq!(r.planificados + r.sin_capacidad, r.envios);
    let tigo = &r.operadores[0];
    assert_eq!((tigo.operador.as_str(), tigo.chips, tigo.cupo_hoy), ("TIGO", 2, 10));
    assert!((tigo.saldo_conocido - 33.0).abs() < 1e-9);
    assert!(v.chips.iter().all(|c| c.cupo_hoy == 5));
}

#[tokio::test]
async fn aviso_de_lote_por_receta_sin_probar() {
    let t = Telefono::new(vec![LineaSimulada::tigo("78815232", 30.0)]);
    let b = banco(&[("tel1", t)]);
    b.lotes.ctx.fijar_saldo("tel1", 0, 30.0, None, None);
    let csv = "numero,monto,operador\n69869869,5,TIGO\n";
    // Sin versión (recetas de prueba): no se avisa.
    let v = b.lotes.validar(csv, false, None).await.unwrap();
    assert!(v.resumen.avisos.is_empty(), "{:?}", v.resumen.avisos);

    // Con versión y nunca probada: aviso de lote (no de fila).
    let receta = {
        let mut m = b.lotes.ctx.recetas.write().unwrap();
        let r = m.get_mut("tigo-transferir").unwrap();
        r.version = Some("v-editada".into());
        r.clone()
    };
    let v = b.lotes.validar(csv, false, None).await.unwrap();
    assert_eq!(v.resumen.avisos, vec!["la receta tigo-transferir cambió y no se ha probado".to_string()]);
    assert_eq!(v.filas[0].estado, EstadoValidacion::Valida, "el aviso no cambia la fila");

    // Una ejecución simulada de esa versión la deja probada.
    let params: BTreeMap<String, String> =
        [("numero", "69869869"), ("monto", "5")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    let (r, _) = b.lotes.ctx.ejecutar_receta(&receta, "tel1", 0, &params, true).await.unwrap();
    assert_eq!(r.estado, protocolo::EstadoResultado::Simulado, "{r:?}");
    assert!(b.lotes.ctx.almacen.version_probada(&["tigo-transferir"], "v-editada").unwrap());
    let v = b.lotes.validar(csv, false, None).await.unwrap();
    assert!(v.resumen.avisos.is_empty(), "{:?}", v.resumen.avisos);
}

#[tokio::test]
async fn csv_sin_columnas_es_error() {
    let b = banco(&[]);
    let e = b.lotes.validar("numero,monto\n69869869,5\n", false, None).await.unwrap_err();
    assert_eq!((e.http, e.codigo), (400, codigos::PARAMETRO));
    assert!(e.detalle.contains("operador"));
}

#[tokio::test]
async fn validar_consulta_saldos() {
    let t = Telefono::new(vec![LineaSimulada::tigo("78815232", 14.0), LineaSimulada::tigo("69759675", 3.0)]);
    let b = banco(&[("tel1", t)]);
    let v = b.lotes.validar("numero,monto,operador\n69869869,5,TIGO\n", true, None).await.unwrap();
    let saldos: Vec<_> = v.chips.iter().map(|c| c.saldo).collect();
    assert_eq!(saldos, vec![Some(14.0), Some(3.0)]);
    assert_eq!(v.filas[0].plan[0].slot, 0);
    // Un dato fresco no se vuelve a consultar: la consulta siguiente no cambia la hora.
    let t0 = v.chips[0].saldo_ms;
    let v2 = b.lotes.validar("numero,monto,operador\n69869869,5,TIGO\n", true, None).await.unwrap();
    assert_eq!(v2.chips[0].saldo_ms, t0);
}

// ───────────────────────────── ejecución ─────────────────────────────

#[tokio::test]
async fn lote_real_completo() {
    let t = Telefono::new(vec![LineaSimulada::tigo("78815232", 30.0), LineaSimulada::tigo("69759675", 3.0)]);
    let b = banco(&[("tel1", t.clone())]);
    b.lotes.ctx.fijar_saldo("tel1", 0, 30.0, None, None);
    b.lotes.ctx.fijar_saldo("tel1", 1, 3.0, None, None);
    let lote = b.lotes.crear("prueba", "numero,monto,operador,referencia\n69869869,12,TIGO,A\n70000000,1,TIGO,B\n123,5,TIGO,C\n", false, None, false).unwrap();
    assert_eq!(lote.estado, EstadoLote::Listo);
    assert_eq!(lote.progreso.omitidas, 1);
    b.lotes.iniciar(lote.id).unwrap();
    let fin = esperar_fin(&b.lotes, lote.id).await;
    assert_eq!(fin.estado, EstadoLote::Terminado, "{fin:?}");
    let fs = filas(&fin);
    assert_eq!(fs[0].estado, EstadoFila::Ok);
    assert_eq!(fs[0].enviado_bs, 12.0);
    assert_eq!(fs[1].estado, EstadoFila::Ok);
    assert_eq!(fs[2].estado, EstadoFila::Omitida);
    assert_eq!(fin.progreso.enviado_bs, 13.0);
    assert!((fin.progreso.comision_bs - 0.6).abs() < 1e-9);
    // Saldo real del chip: 30 - 13 - 0.6 (la 1 Bs pudo ir al chip 2, que tenía 3).
    let gastado = (30.0 - t.linea(0).saldo_principal) + (3.0 - t.linea(1).saldo_principal);
    assert!((gastado - 13.6).abs() < 1e-6, "gastado {gastado}");
    // El saldo conocido se ajustó y el cupo bajó.
    let chips = b.lotes.ctx.chips();
    let usados: u32 = chips.iter().map(|c| c.enviados_hoy).sum();
    assert_eq!(usados, 3);
    assert!((chips.iter().filter_map(|c| c.saldo).sum::<f64>() - (33.0 - 13.6)).abs() < 1e-6);
    // CSV de resultado.
    let (nombre, csv) = b.lotes.resultado_csv(lote.id, ',').unwrap();
    assert!(nombre.ends_with(".csv"));
    assert!(csv.starts_with('\u{feff}'));
    let lineas: Vec<&str> = csv.trim_start_matches('\u{feff}').lines().collect();
    assert!(lineas[0].starts_with("fila,numero,operador,accion,monto,referencia,estado"));
    assert!(lineas[1].starts_with("2,69869869,TIGO,transferir,12,A,ok,12,0.40,2,2,"), "{}", lineas[1]);
    assert!(lineas[3].contains(",omitida,"));
    // No se puede volver a iniciar: no queda nada pendiente.
    assert_eq!(b.lotes.iniciar(lote.id).unwrap_err().codigo, "nada_pendiente");
}

#[tokio::test]
async fn lote_simulado_no_mueve_saldo() {
    let t = Telefono::new(vec![LineaSimulada::tigo("78815232", 30.0)]);
    let b = banco(&[("tel1", t.clone())]);
    let lote = b.lotes.crear("", "69869869;12;TIGO\n70000000;5;TIGO\n", true, None, false).unwrap();
    assert!(lote.simular);
    b.lotes.iniciar(lote.id).unwrap();
    let fin = esperar_fin(&b.lotes, lote.id).await;
    assert_eq!(fin.estado, EstadoLote::Terminado);
    assert!(filas(&fin).iter().all(|f| f.estado == EstadoFila::Simulado));
    assert_eq!(fin.progreso.simulado, 2);
    assert_eq!(fin.progreso.enviado_bs, 0.0);
    assert_eq!(t.linea(0).saldo_principal, 30.0);
    assert_eq!(t.linea(0).transferencias_hoy, 0);
    assert_eq!(b.lotes.ctx.chips()[0].enviados_hoy, 0);
}

#[tokio::test]
async fn limite_diario_reasigna_a_otro_chip() {
    // El chip 1 solo permite 2 transferencias (el operador lo dice a la tercera); la receta cree que 5.
    let mut a = LineaSimulada::tigo("78815232", 100.0);
    a.limite_diario = 2;
    let t = Telefono::new(vec![a, LineaSimulada::tigo("69759675", 50.0)]);
    let b = banco(&[("tel1", t.clone())]);
    b.lotes.ctx.fijar_saldo("tel1", 0, 100.0, None, None);
    b.lotes.ctx.fijar_saldo("tel1", 1, 50.0, None, None);
    let csv = "numero,monto,operador\n69869869,1,TIGO\n69869869,1,TIGO\n69869869,1,TIGO\n69869869,1,TIGO\n";
    let lote = b.lotes.crear("limite", csv, false, None, false).unwrap();
    b.lotes.iniciar(lote.id).unwrap();
    let fin = esperar_fin(&b.lotes, lote.id).await;
    assert_eq!(fin.estado, EstadoLote::Terminado, "{fin:?}");
    assert_eq!(fin.progreso.ok, 4, "{fin:?}");
    assert_eq!(t.linea(0).transferencias_hoy, 2);
    assert_eq!(t.linea(1).transferencias_hoy, 2);
    let chips = b.lotes.ctx.chips();
    assert!(chips[0].agotado_hoy && chips[0].cupo_hoy == 0);
    assert!(!chips[1].agotado_hoy);
}

#[tokio::test]
async fn saldo_desconocido_insuficiente_reasigna() {
    // Un saldo conocido y reciente (que ya no es cierto) hace que el plan elija el chip pobre; el menú no
    // ofrece el monto → se reasigna. (Sin dato, el inicio del lote consultaría los saldos reales.)
    let t = Telefono::new(vec![LineaSimulada::tigo("78815232", 2.0), LineaSimulada::tigo("69759675", 40.0)]);
    let b = banco(&[("tel1", t.clone())]);
    b.lotes.ctx.fijar_saldo("tel1", 0, 50.0, None, None);
    b.lotes.ctx.fijar_saldo("tel1", 1, 40.0, None, None);
    let lote = b.lotes.crear("saldo", "numero,monto,operador\n69869869,10,TIGO\n", false, None, false).unwrap();
    b.lotes.iniciar(lote.id).unwrap();
    let fin = esperar_fin(&b.lotes, lote.id).await;
    assert_eq!(filas(&fin)[0].estado, EstadoFila::Ok, "{fin:?}");
    assert_eq!(t.linea(1).transferencias_hoy, 1);
    assert_eq!(b.lotes.ctx.saldo("tel1", 0).map(|s| s.0), Some(0.0));
}

#[tokio::test]
async fn incierto_no_detiene_el_lote_ni_se_reintenta_solo() {
    let t = Telefono::new(vec![LineaSimulada::tigo("78815232", 100.0)]);
    t.timeout_tras_confirmar.store(true, Ordering::SeqCst);
    let b = banco(&[("tel1", t.clone())]);
    let lote = b.lotes.crear("incierto", "numero,monto,operador\n69869869,5,TIGO\n70000000,5,TIGO\n", false, None, false).unwrap();
    b.lotes.iniciar(lote.id).unwrap();
    let fin = esperar_fin(&b.lotes, lote.id).await;
    // Sigue con las demás filas y termina; ninguna se repitió sola.
    assert_eq!(fin.estado, EstadoLote::Terminado, "{fin:?}");
    assert_eq!(fin.motivo_pausa, None);
    let fs = filas(&fin);
    assert_eq!((fs[0].estado, fs[1].estado), (EstadoFila::Incierta, EstadoFila::Incierta), "{fs:?}");
    assert_eq!(t.linea(0).transferencias_hoy, 2);
    assert!(fin.revision.motivos.iter().any(|m| m == "2 filas inciertas"), "{:?}", fin.revision);
    // Iniciar de nuevo no los toca: no hay nada pendiente.
    assert_eq!(b.lotes.iniciar(lote.id).unwrap_err().codigo, "nada_pendiente");
}

#[tokio::test]
async fn incierto_en_medio_sigue_con_las_demas() {
    // Solo la fila 2 queda incierta: las filas 3 y 4 se envían igual.
    let t = Telefono::new(vec![LineaSimulada::tigo("78815232", 100.0)]);
    let b = banco(&[("tel1", t.clone())]);
    let csv = "numero,monto,operador\n69869869,5,TIGO\n70000000,5,TIGO\n70000001,5,TIGO\n";
    let lote = b.lotes.crear("medio", csv, false, None, false).unwrap();
    t.timeout_tras_confirmar.store(true, Ordering::SeqCst);
    // Al primer envío incierto se apaga la falla, para que el resto salga bien.
    let t2 = t.clone();
    let id = lote.id;
    let lotes = b.lotes.clone();
    tokio::spawn(async move {
        loop {
            if let Ok(l) = lotes.obtener(id) {
                if l.progreso.inciertas > 0 {
                    t2.timeout_tras_confirmar.store(false, Ordering::SeqCst);
                    return;
                }
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    });
    b.lotes.iniciar(lote.id).unwrap();
    let fin = esperar_fin(&b.lotes, lote.id).await;
    assert_eq!(fin.estado, EstadoLote::Terminado, "{fin:?}");
    let fs = filas(&fin);
    assert_eq!(fs[0].estado, EstadoFila::Incierta, "{fs:?}");
    assert_eq!(fin.progreso.inciertas, 1, "{fs:?}");
    assert_eq!(fin.progreso.ok, 2, "{fs:?}");
}

#[tokio::test]
async fn cancelar_termina_lo_que_esta_en_vuelo() {
    let t = Telefono::con_demora(vec![LineaSimulada::tigo("78815232", 100.0)], Duration::from_millis(15));
    let b = banco(&[("tel1", t.clone())]);
    let csv = "numero,monto,operador\n69869869,1,TIGO\n69869869,1,TIGO\n69869869,1,TIGO\n";
    let lote = b.lotes.crear("cancelar", csv, false, None, false).unwrap();
    b.lotes.iniciar(lote.id).unwrap();
    tokio::time::sleep(Duration::from_millis(30)).await;
    b.lotes.cancelar(lote.id).unwrap();
    let fin = esperar_fin(&b.lotes, lote.id).await;
    assert_eq!(fin.estado, EstadoLote::Cancelado);
    let fs = filas(&fin);
    // El primero terminó (no se corta a medias); el resto quedó pendiente.
    assert_eq!(fs[0].estado, EstadoFila::Ok);
    assert!(fs[1..].iter().all(|f| f.estado == EstadoFila::Pendiente), "{fs:?}");
    assert_eq!(t.linea(0).transferencias_hoy, 1);
    assert_eq!(b.lotes.iniciar(lote.id).unwrap_err().codigo, "cancelado");
}

#[tokio::test]
async fn pausar_y_reanudar() {
    let t = Telefono::con_demora(vec![LineaSimulada::tigo("78815232", 100.0)], Duration::from_millis(15));
    let b = banco(&[("tel1", t.clone())]);
    let csv = "numero,monto,operador\n69869869,1,TIGO\n69869869,1,TIGO\n69869869,1,TIGO\n";
    let lote = b.lotes.crear("pausa", csv, false, None, false).unwrap();
    b.lotes.iniciar(lote.id).unwrap();
    tokio::time::sleep(Duration::from_millis(30)).await;
    b.lotes.pausar(lote.id).unwrap();
    let fin = esperar_fin(&b.lotes, lote.id).await;
    assert_eq!(fin.estado, EstadoLote::Pausado);
    assert_eq!(fin.motivo_pausa.as_deref(), Some("pausado por el usuario"));
    assert!(fin.progreso.pendientes >= 1);
    b.lotes.iniciar(lote.id).unwrap();
    let fin = esperar_fin(&b.lotes, lote.id).await;
    assert_eq!(fin.estado, EstadoLote::Terminado);
    assert_eq!(fin.progreso.ok, 3);
    assert_eq!(t.linea(0).transferencias_hoy, 3);
}

#[tokio::test]
async fn sin_chips_queda_sin_capacidad_y_se_retoma() {
    let almacen = Arc::new(almacen::Almacen::en_memoria().unwrap());
    let b = banco_con(almacen.clone(), &[]);
    let lote = b.lotes.crear("sin chips", "numero,monto,operador\n69869869,5,TIGO\n", false, None, false).unwrap();
    b.lotes.iniciar(lote.id).unwrap();
    let fin = esperar_fin(&b.lotes, lote.id).await;
    assert_eq!(fin.estado, EstadoLote::Terminado);
    assert_eq!(filas(&fin)[0].estado, EstadoFila::SinCapacidad);
    assert!(filas(&fin)[0].detalle.as_deref().unwrap().contains("no hay chips de TIGO"));

    // Otro día (o con un teléfono conectado) se vuelve a iniciar y retoma lo pendiente.
    let t = Telefono::new(vec![LineaSimulada::tigo("78815232", 20.0)]);
    let b2 = banco_con(almacen, &[("tel1", t.clone())]);
    b2.lotes.iniciar(lote.id).unwrap();
    let fin = esperar_fin(&b2.lotes, lote.id).await;
    assert_eq!(filas(&fin)[0].estado, EstadoFila::Ok);
    assert_eq!(t.linea(0).transferencias_hoy, 1);
    drop(b.registro);
}

#[tokio::test]
async fn al_arrancar_lo_en_curso_queda_incierto() {
    let almacen = Arc::new(almacen::Almacen::en_memoria().unwrap());
    let b = banco_con(almacen.clone(), &[]);
    let lote = b.lotes.crear("corte", "numero,monto,operador\n69869869,5,TIGO\n70000000,1,TIGO\n", false, None, false).unwrap();
    // Simula un corte con el primer envío en curso.
    let mut e = filas(&lote)[0].envios[0].clone();
    e.estado = EstadoEnvio::EnCurso;
    b.lotes.ctx.pers.guardar_envio(&e).unwrap();
    b.lotes.ctx.pers.estado_lote(lote.id, EstadoLote::EnCurso, None).unwrap();
    // Reinicio del control.
    let b2 = banco_con(almacen, &[]);
    let l = b2.lotes.obtener(lote.id).unwrap();
    assert_eq!(l.estado, EstadoLote::Pausado);
    assert!(l.motivo_pausa.as_deref().unwrap().contains("inciertos"));
    let fs = filas(&l);
    assert_eq!(fs[0].estado, EstadoFila::Incierta);
    assert_eq!(fs[1].estado, EstadoFila::Pendiente);
}

#[tokio::test]
async fn un_solo_lote_a_la_vez() {
    let t = Telefono::con_demora(vec![LineaSimulada::tigo("78815232", 100.0)], Duration::from_millis(20));
    let b = banco(&[("tel1", t)]);
    let a = b.lotes.crear("a", "69869869;1;TIGO\n", false, None, false).unwrap();
    let c = b.lotes.crear("c", "69869869;1;TIGO\n", false, None, true).unwrap();
    b.lotes.iniciar(a.id).unwrap();
    assert_eq!(b.lotes.iniciar(a.id).unwrap_err().codigo, "en_curso");
    assert_eq!(b.lotes.iniciar(c.id).unwrap_err().codigo, "otro_lote");
    esperar_fin(&b.lotes, a.id).await;
    let lista = b.lotes.listar(&todos()).unwrap();
    assert_eq!(lista.lotes.len(), 2);
    assert!(lista.lotes.iter().all(|l| l.filas.is_none()));
}

#[tokio::test]
async fn reparte_en_paralelo_entre_telefonos() {
    let demora = Duration::from_millis(40);
    let t1 = Telefono::con_demora(vec![LineaSimulada::tigo("78815232", 50.0)], demora);
    let t2 = Telefono::con_demora(vec![LineaSimulada::tigo("69759675", 50.0)], demora);
    let b = banco(&[("tel1", t1.clone()), ("tel2", t2.clone())]);
    let csv = "numero,monto,operador\n69869869,1,TIGO\n69869869,1,TIGO\n69869869,1,TIGO\n69869869,1,TIGO\n";
    let lote = b.lotes.crear("paralelo", csv, false, None, false).unwrap();
    let t0 = std::time::Instant::now();
    b.lotes.iniciar(lote.id).unwrap();
    let fin = esperar_fin(&b.lotes, lote.id).await;
    assert_eq!(fin.progreso.ok, 4, "{fin:?}");
    // Los dos teléfonos trabajaron.
    assert_eq!(t1.linea(0).transferencias_hoy + t2.linea(0).transferencias_hoy, 4);
    assert!(t1.linea(0).transferencias_hoy >= 1 && t2.linea(0).transferencias_hoy >= 1);
    // 4 envíos × 5 pasos × 40 ms en serie serían ≥ 800 ms; en paralelo, cerca de la mitad (~400 ms).
    // Margen amplio para que una máquina cargada no dé un falso fallo.
    assert!(t0.elapsed() < Duration::from_millis(680), "tardó {:?}", t0.elapsed());
}

// ───────────────────────────── reintentar fallidas / revisión / verificar ─────────────────────────────

#[tokio::test]
async fn reintentar_fallidas_solo_repite_lo_fallido() {
    // La fila 2 falla por timeout (1 intento + 2 reintentos → `fallido`); la fila 3 sale bien.
    let t = Telefono::new(vec![LineaSimulada::tigo("78815232", 100.0)]);
    *t.fallar_numero.lock().unwrap() = Some("69869869".into());
    let b = banco(&[("tel1", t.clone())]);
    b.lotes.ctx.fijar_saldo("tel1", 0, 100.0, None, None);
    let lote = b.lotes.crear("fallo", "numero,monto,operador\n69869869,5,TIGO\n70000000,5,TIGO\n", false, None, false).unwrap();
    assert!(!lote.revision.completo);
    b.lotes.iniciar(lote.id).unwrap();
    let fin = esperar_fin(&b.lotes, lote.id).await;
    assert_eq!(fin.estado, EstadoLote::Terminado, "{fin:?}");
    let fs = filas(&fin);
    assert_eq!((fs[0].estado, fs[1].estado), (EstadoFila::Fallida, EstadoFila::Ok), "{fs:?}");
    assert_eq!(t.linea(0).transferencias_hoy, 1);
    // `terminado` no es éxito.
    assert!(!fin.revision.completo);
    assert_eq!(fin.revision.motivos, vec!["1 fila fallida".to_string()]);
    // Terminado ya no reintenta solo: iniciar dice que no hay nada pendiente.
    assert_eq!(b.lotes.iniciar(lote.id).unwrap_err().codigo, "nada_pendiente");

    // Una verificación guardada se borra cuando el lote cambia.
    b.lotes.verificar(lote.id).await.unwrap();
    assert!(b.lotes.obtener(lote.id).unwrap().verificacion.is_some());
    let re = b.lotes.reintentar_fallidas(lote.id, false).unwrap();
    assert!(re.verificacion.is_none());
    assert_eq!(re.estado, EstadoLote::Listo);
    let fs = filas(&re);
    assert_eq!((fs[0].estado, fs[1].estado), (EstadoFila::Pendiente, EstadoFila::Ok));
    assert_eq!(fs[0].detalle, None);

    *t.fallar_numero.lock().unwrap() = None;
    b.lotes.iniciar(lote.id).unwrap();
    let fin = esperar_fin(&b.lotes, lote.id).await;
    let fs = filas(&fin);
    assert_eq!((fs[0].estado, fs[1].estado), (EstadoFila::Ok, EstadoFila::Ok), "{fs:?}");
    // La fila 3, que ya estaba ok, no se repitió: 2 transferencias en total, no 3.
    assert_eq!(t.linea(0).transferencias_hoy, 2);
    assert!(fin.revision.completo, "{:?}", fin.revision);
    assert_eq!(b.lotes.reintentar_fallidas(lote.id, false).unwrap_err().codigo, "nada_que_reintentar");
}

#[test]
fn revision_explica_por_que_no_esta_completo() {
    let p = Progreso { filas: 9, ok: 3, parcial: 1, fallidas: 2, sin_capacidad: 3, ..Default::default() };
    let r = Revision::de(EstadoLote::Terminado, &p);
    assert!(!r.completo);
    assert_eq!(r.motivos, vec!["1 fila parcial", "2 filas fallidas", "3 filas para mañana"]);
    let p = Progreso { filas: 2, ok: 2, ..Default::default() };
    assert!(Revision::de(EstadoLote::Terminado, &p).completo);
    // Pausado o cancelado nunca es completo, aunque todo lo hecho esté ok.
    assert_eq!(Revision::de(EstadoLote::Pausado, &p).motivos, vec!["el lote está pausado"]);
    assert!(!Revision::de(EstadoLote::Cancelado, &p).completo);
}

#[tokio::test]
async fn reintentar_inciertas_solo_si_se_pide() {
    let t = Telefono::con_demora(vec![LineaSimulada::tigo("78815232", 100.0)], Duration::from_millis(15));
    t.timeout_tras_confirmar.store(true, Ordering::SeqCst);
    let b = banco(&[("tel1", t.clone())]);
    let lote = b.lotes.crear("incierto", "numero,monto,operador\n69869869,5,TIGO\n", false, None, false).unwrap();
    b.lotes.iniciar(lote.id).unwrap();
    assert_eq!(b.lotes.reintentar_fallidas(lote.id, true).unwrap_err().codigo, "en_curso");
    let fin = esperar_fin(&b.lotes, lote.id).await;
    assert_eq!(fin.estado, EstadoLote::Terminado);
    assert!(!fin.revision.completo);
    assert!(fin.revision.motivos.iter().any(|m| m == "1 fila incierta"), "{:?}", fin.revision);
    // Sin pedirlo, un incierto no se toca.
    assert_eq!(b.lotes.reintentar_fallidas(lote.id, false).unwrap_err().codigo, "nada_que_reintentar");
    assert_eq!(filas(&b.lotes.obtener(lote.id).unwrap())[0].estado, EstadoFila::Incierta);
    // Pedido por el operador (revisó el saldo): vuelve a pendiente y se envía otra vez.
    let re = b.lotes.reintentar_fallidas(lote.id, true).unwrap();
    assert_eq!(re.estado, EstadoLote::Listo);
    assert_eq!(filas(&re)[0].estado, EstadoFila::Pendiente);
    t.timeout_tras_confirmar.store(false, Ordering::SeqCst);
    b.lotes.iniciar(lote.id).unwrap();
    let fin = esperar_fin(&b.lotes, lote.id).await;
    assert_eq!(filas(&fin)[0].estado, EstadoFila::Ok);
    assert_eq!(t.linea(0).transferencias_hoy, 2);

    assert_eq!(b.lotes.reintentar_fallidas(999, false).unwrap_err().codigo, "sin_lote");
    let otro = b.lotes.crear("otro", "numero,monto,operador\n69869869,5,TIGO\n", false, None, true).unwrap();
    b.lotes.cancelar(otro.id).unwrap();
    assert_eq!(b.lotes.reintentar_fallidas(otro.id, true).unwrap_err().codigo, "cancelado");
}

#[tokio::test]
async fn verificar_cuadra_y_detecta_diferencias() {
    let t = Telefono::new(vec![LineaSimulada::tigo("78815232", 50.0)]);
    let b = banco(&[("tel1", t.clone())]);
    let lote = b.lotes.crear("cuadre", "numero,monto,operador\n69869869,12,TIGO\n", false, None, false).unwrap();
    assert_eq!(b.lotes.verificar(lote.id).await.unwrap_err().codigo, "sin_base");
    b.lotes.iniciar(lote.id).unwrap();
    let fin = esperar_fin(&b.lotes, lote.id).await;
    assert!(fin.revision.completo);

    // 12 Bs en dos envíos (10 + 2) con 0.20 de comisión cada uno = 12.40.
    let v = b.lotes.verificar(lote.id).await.unwrap();
    assert_eq!(v.chips.len(), 1, "{v:?}");
    let c = &v.chips[0];
    assert_eq!(c.estado, EstadoCuadre::Cuadra, "{v:?}");
    assert_eq!((c.saldo_inicial, c.envios_ok), (Some(50.0), 2));
    assert_eq!((c.descontado_esperado, c.descontado_real), (12.4, Some(12.4)));
    assert!(v.cuadra && v.completo && v.confirmado, "{v:?}");
    // Queda guardada en el lote: sobrevive a recargar y a otro servicio sobre la misma base.
    assert_eq!(b.lotes.obtener(lote.id).unwrap().verificacion, Some(v.clone()));
    assert_eq!(b.lotes.listar(&todos()).unwrap().lotes[0].verificacion, Some(v));

    // Alguien gastó 3 Bs por fuera: ya no cuadra, aunque el lote esté completo.
    t.lineas.lock().unwrap()[0].saldo_principal -= 3.0;
    let v = b.lotes.verificar(lote.id).await.unwrap();
    assert_eq!(v.chips[0].estado, EstadoCuadre::Diferencia, "{v:?}");
    assert_eq!(v.chips[0].diferencia_bs, Some(3.0));
    assert!(!v.cuadra && v.completo && !v.confirmado);
    // La nueva verificación reemplaza a la anterior.
    assert_eq!(b.lotes.obtener(lote.id).unwrap().verificacion, Some(v));
}

#[tokio::test]
async fn verificar_no_aplica_a_simulaciones() {
    let t = Telefono::new(vec![LineaSimulada::tigo("78815232", 50.0)]);
    let b = banco(&[("tel1", t)]);
    let lote = b.lotes.crear("sim", "numero,monto,operador\n69869869,5,TIGO\n", true, None, false).unwrap();
    b.lotes.iniciar(lote.id).unwrap();
    let fin = esperar_fin(&b.lotes, lote.id).await;
    assert!(fin.revision.completo);
    assert_eq!(b.lotes.verificar(lote.id).await.unwrap_err().codigo, "simulado");
}

// ───────────────────────────── cambio de chips ─────────────────────────────

fn sim_con(slot: u32, numero: &str, iccid: Option<&str>) -> Sim {
    Sim { iccid: iccid.map(String::from), ..sim(slot, numero) }
}

fn poner_chips(b: &Banco, id: &str, sims: Vec<Sim>) {
    b.registro.actualizar_sims(id, sims).unwrap();
}

fn chip_de(b: &Banco, slot: u32) -> ChipInfo {
    b.lotes.ctx.chips().into_iter().find(|c| c.slot == slot).unwrap()
}

/// Lote de un envío que sale por el chip de la ranura 0 (el de más saldo).
async fn enviar_por_ranura_0(b: &Banco) {
    b.lotes.ctx.fijar_saldo("tel1", 0, 40.0, None, None);
    b.lotes.ctx.fijar_saldo("tel1", 1, 10.0, None, None);
    let lote = b.lotes.crear("uno", "numero,monto,operador\n69869869,5,TIGO\n", false, None, false).unwrap();
    b.lotes.iniciar(lote.id).unwrap();
    let fin = esperar_fin(&b.lotes, lote.id).await;
    assert_eq!(filas(&fin)[0].estado, EstadoFila::Ok, "{fin:?}");
}

#[tokio::test]
async fn chip_nuevo_en_la_ranura_no_hereda_saldo_cupo_ni_agotado() {
    let t = Telefono::new(vec![LineaSimulada::tigo("78815232", 40.0), LineaSimulada::tigo("69759675", 10.0)]);
    let b = banco(&[("tel1", t)]);
    let mut eventos = b.lotes.ctx.bus.suscribir();
    enviar_por_ranura_0(&b).await;
    b.lotes.ctx.agotar_hoy("tel1", 0);
    let antes = chip_de(&b, 0);
    assert_eq!((antes.enviados_hoy, antes.agotado_hoy, antes.sin_verificar), (1, true, false));
    assert!(antes.saldo.is_some());

    poner_chips(&b, "tel1", vec![sim_con(0, "70000001", None), sim(1, "69759675")]);
    let nuevo = chip_de(&b, 0);
    assert_eq!(nuevo.numero.as_deref(), Some("70000001"));
    assert_eq!((nuevo.saldo, nuevo.enviados_hoy, nuevo.agotado_hoy, nuevo.sin_verificar), (None, 0, false, true));
    assert!(nuevo.cupo_hoy > 0);
    // La otra ranura sigue igual.
    assert!(chip_de(&b, 1).saldo.is_some());

    let mut cambios = vec![];
    while let Ok(e) = eventos.try_recv() {
        if e.tipo == "chip_cambiado" {
            cambios.push(e);
        }
    }
    assert_eq!(cambios.len(), 1, "{cambios:?}");
    assert_eq!(cambios[0].datos["slot"], 0);
    assert_eq!(cambios[0].datos["anterior"]["numero"], "78815232");
    assert_eq!(cambios[0].datos["nuevo"]["numero"], "70000001");
    assert_eq!(cambios[0].datos["regreso"], false);
}

#[tokio::test]
async fn cupo_y_agotado_siguen_al_chip_aunque_cambie_de_ranura() {
    let t = Telefono::new(vec![LineaSimulada::tigo("78815232", 40.0), LineaSimulada::tigo("69759675", 10.0)]);
    let b = banco(&[("tel1", t)]);
    enviar_por_ranura_0(&b).await;
    b.lotes.ctx.agotar_hoy("tel1", 0);

    // El chip A (78815232) pasa a la ranura 1; en la ranura 0 entra uno nuevo.
    poner_chips(&b, "tel1", vec![sim_con(0, "70000001", None), sim(1, "78815232")]);
    let (r0, r1) = (chip_de(&b, 0), chip_de(&b, 1));
    assert_eq!((r0.enviados_hoy, r0.agotado_hoy), (0, false));
    assert_eq!((r1.enviados_hoy, r1.agotado_hoy), (1, true), "el chip A conserva su cupo gastado y su agotado");
    assert_eq!(r1.cupo_hoy, 0);
    assert_eq!(r1.saldo, None, "el saldo no se hereda: hay que consultarlo en la ranura nueva");
}

#[tokio::test]
async fn iccid_distingue_chips_y_un_dato_que_falta_no_es_un_cambio() {
    let t = Telefono::new(vec![LineaSimulada::tigo("78815232", 40.0)]);
    let b = banco(&[("tel1", t)]);
    b.lotes.ctx.fijar_saldo("tel1", 0, 40.0, None, None);

    // Android entrega ahora el ICCID del mismo chip, y después deja de entregar el número: sigue siendo el mismo.
    poner_chips(&b, "tel1", vec![sim_con(0, "78815232", Some("8959103000797642548"))]);
    assert!(!chip_de(&b, 0).sin_verificar);
    assert!(chip_de(&b, 0).saldo.is_some());
    let mut sin_numero = sim_con(0, "78815232", Some("8959103000797642548"));
    sin_numero.numero = None;
    poner_chips(&b, "tel1", vec![sin_numero]);
    assert!(chip_de(&b, 0).saldo.is_some(), "perder el número no es un cambio de chip");

    // Mismo número pero otro ICCID (la operadora reasignó el número): es otro chip.
    poner_chips(&b, "tel1", vec![sim_con(0, "78815232", Some("8959103000000000001"))]);
    let c = chip_de(&b, 0);
    assert_eq!((c.saldo, c.sin_verificar), (None, true));
}

#[tokio::test]
async fn quitar_y_volver_a_poner_el_chip_pide_saldo_de_nuevo() {
    let t = Telefono::new(vec![LineaSimulada::tigo("78815232", 40.0)]);
    let b = banco(&[("tel1", t)]);
    b.lotes.ctx.fijar_saldo("tel1", 0, 40.0, None, None);
    let mut vacia = sim(0, "78815232");
    vacia.estado = "ausente".into();
    poner_chips(&b, "tel1", vec![vacia]);
    assert!(b.lotes.ctx.chips().is_empty());
    poner_chips(&b, "tel1", vec![sim(0, "78815232")]);
    let c = chip_de(&b, 0);
    assert_eq!((c.saldo, c.sin_verificar), (None, true));
}

#[tokio::test]
async fn chip_sin_verificar_no_entra_al_reparto_hasta_consultar_su_saldo() {
    let t = Telefono::new(vec![LineaSimulada::tigo("78815232", 40.0), LineaSimulada::tigo("69759675", 40.0)]);
    let b = banco(&[("tel1", t)]);
    b.lotes.ctx.fijar_saldo("tel1", 0, 40.0, None, None);
    b.lotes.ctx.fijar_saldo("tel1", 1, 40.0, None, None);
    poner_chips(&b, "tel1", vec![sim_con(0, "70000001", None), sim(1, "69759675")]);

    let v = b.lotes.validar("numero,monto,operador\n69869869,5,TIGO\n", false, None).await.unwrap();
    assert_eq!(v.filas[0].plan[0].slot, 1, "el chip nuevo no recibe envíos");

    // Con los dos cambiados no queda con qué enviar, y el motivo lo dice.
    poner_chips(&b, "tel1", vec![sim_con(0, "70000001", None), sim_con(1, "70000002", None)]);
    let v = b.lotes.validar("numero,monto,operador\n69869869,5,TIGO\n", false, None).await.unwrap();
    assert!(v.filas[0].plan.is_empty(), "{:?}", v.filas[0]);
    assert!(v.filas[0].avisos.iter().chain(&v.filas[0].errores).any(|m| m.contains("saldo verificado")), "{:?}", v.filas[0]);

    // Consultando los saldos, entran.
    let v = b.lotes.validar("numero,monto,operador\n69869869,5,TIGO\n", true, None).await.unwrap();
    assert_eq!(v.filas[0].plan.len(), 1, "{:?}", v.filas[0]);
    assert!(!chip_de(&b, 0).sin_verificar);
}

#[tokio::test]
async fn el_vigilante_consulta_solo_el_saldo_del_chip_nuevo() {
    let t = Telefono::new(vec![LineaSimulada::tigo("78815232", 25.0), LineaSimulada::tigo("69759675", 7.0)]);
    let b = banco(&[("tel1", t)]);
    // La ranura 1 tiene un saldo viejo que no debe volver a consultarse.
    b.lotes.ctx.fijar_saldo("tel1", 0, 1.0, None, None);
    b.lotes.ctx.fijar_saldo("tel1", 1, 99.0, None, None);
    b.lotes.ctx.vigilar_chips();
    poner_chips(&b, "tel1", vec![sim_con(0, "70000001", None), sim(1, "69759675")]);
    b.lotes.ctx.bus.emitir("dispositivo_actualizado", serde_json::json!({}));

    for _ in 0..200 {
        if b.lotes.ctx.saldo("tel1", 0).map(|s| s.0) == Some(25.0) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(b.lotes.ctx.saldo("tel1", 0).map(|s| s.0), Some(25.0));
    assert!(!chip_de(&b, 0).sin_verificar);
    assert_eq!(b.lotes.ctx.saldo("tel1", 1).map(|s| s.0), Some(99.0), "no toca los chips que no cambiaron");
}

// ───────────── chips dealer ─────────────

/// Receta dealer de transferir: la misma pantalla de Tigo, con su propio nombre y un cupo de 2 por día.
fn poner_receta_dealer(b: &Banco) {
    let yaml = include_str!("../../../recetas/tigo-transferir.yaml")
        .replace("nombre: tigo-transferir", "nombre: tigo-dealer-transferir\nchip_tipo: dealer")
        .replace("por_dia: 5", "por_dia: 2");
    let r = Receta::desde_yaml(&yaml).unwrap();
    b.lotes.ctx.recetas.write().unwrap().insert(r.nombre.clone(), r);
}

fn telefono_dealer() -> Arc<Telefono> {
    Telefono::new(vec![LineaSimulada::tigo("78815232", 40.0), LineaSimulada::tigo("69759675", 10.0)])
}

fn recetas_ejecutadas(b: &Banco) -> Vec<String> {
    let mut v: Vec<String> = b
        .lotes
        .ctx
        .almacen
        .ejecuciones(50)
        .unwrap()
        .iter()
        .map(|e| e["receta"].as_str().unwrap().to_string())
        .collect();
    v.reverse();
    v
}

#[tokio::test]
async fn chip_nuevo_es_normal_y_el_tipo_se_marca_por_ranura() {
    let b = banco(&[("tel1", telefono_dealer())]);
    assert_eq!(chip_de(&b, 0).tipo, "normal");
    assert_eq!(b.lotes.ctx.fijar_tipo("tel1", 0, "dealer").unwrap(), "normal");
    assert_eq!(chip_de(&b, 0).tipo, "dealer");
    assert_eq!(chip_de(&b, 1).tipo, "normal");
    assert!(b.lotes.ctx.fijar_tipo("tel1", 0, "raro").is_err());
    assert!(b.lotes.ctx.fijar_tipo("tel1", 7, "dealer").is_err(), "no hay chip en esa ranura");
}

#[tokio::test]
async fn el_tipo_sigue_al_chip_no_a_la_ranura() {
    let b = banco(&[("tel1", telefono_dealer())]);
    b.lotes.ctx.fijar_tipo("tel1", 0, "dealer").unwrap();
    // El chip A (78815232) pasa a la ranura 1 y en la 0 entra uno nuevo.
    poner_chips(&b, "tel1", vec![sim_con(0, "70000001", None), sim(1, "78815232")]);
    assert_eq!(chip_de(&b, 0).tipo, "normal", "el chip nuevo no hereda el tipo de la ranura");
    assert_eq!(chip_de(&b, 1).tipo, "dealer", "el chip A sigue siendo dealer en su nueva ranura");
}

#[tokio::test]
async fn chip_dealer_usa_la_receta_dealer_con_su_propio_cupo() {
    let b = banco(&[("tel1", telefono_dealer())]);
    poner_receta_dealer(&b);
    b.lotes.ctx.fijar_tipo("tel1", 0, "dealer").unwrap();
    assert_eq!(chip_de(&b, 0).cupo_hoy, 2, "cupo de la receta dealer");
    assert_eq!(chip_de(&b, 1).cupo_hoy, 5, "el chip normal sigue con la de la compañía");

    // La validación solo usa la receta de la compañía.
    b.lotes.ctx.fijar_saldo("tel1", 0, 40.0, None, None);
    b.lotes.ctx.fijar_saldo("tel1", 1, 10.0, None, None);
    let v = b.lotes.validar("numero,monto,operador\n69869869,5,TIGO\n", false, None).await.unwrap();
    assert_eq!(v.filas[0].receta.as_deref(), Some("tigo-transferir"));

    // El envío sale por el chip de más saldo (el dealer) y corre su receta.
    let lote = b.lotes.crear("dealer", "numero,monto,operador\n69869869,5,TIGO\n", false, None, false).unwrap();
    b.lotes.iniciar(lote.id).unwrap();
    let fin = esperar_fin(&b.lotes, lote.id).await;
    assert_eq!(filas(&fin)[0].estado, EstadoFila::Ok, "{fin:?}");
    assert_eq!(recetas_ejecutadas(&b), vec!["tigo-dealer-transferir"]);
    let c = chip_de(&b, 0);
    assert_eq!((c.enviados_hoy, c.cupo_hoy), (1, 1));
}

#[tokio::test]
async fn chip_dealer_sin_receta_dealer_usa_la_de_la_compania() {
    let b = banco(&[("tel1", telefono_dealer())]);
    b.lotes.ctx.fijar_tipo("tel1", 0, "dealer").unwrap();
    assert_eq!(chip_de(&b, 0).cupo_hoy, 5);
    enviar_por_ranura_0(&b).await;
    assert_eq!(recetas_ejecutadas(&b), vec!["tigo-transferir"]);
}

#[tokio::test]
async fn saldo_de_un_chip_dealer_usa_su_receta_de_saldo_si_existe() {
    let b = banco(&[("tel1", telefono_dealer())]);
    b.lotes.ctx.fijar_tipo("tel1", 0, "dealer").unwrap();
    b.lotes.ctx.consultar_saldos(None, None, false).await;
    assert_eq!(recetas_ejecutadas(&b), vec!["tigo-saldo", "tigo-saldo"], "sin receta de saldo dealer, usa la de la compañía");

    let yaml = include_str!("../../../recetas/tigo-saldo.yaml").replace("nombre: tigo-saldo", "nombre: tigo-dealer-saldo\nchip_tipo: dealer");
    let r = Receta::desde_yaml(&yaml).unwrap();
    b.lotes.ctx.recetas.write().unwrap().insert(r.nombre.clone(), r);
    b.lotes.ctx.consultar_saldos(None, None, false).await;
    let ult: Vec<String> = recetas_ejecutadas(&b).into_iter().skip(2).collect();
    assert_eq!(ult, vec!["tigo-dealer-saldo", "tigo-saldo"], "el dealer usa la suya; el normal, la de la compañía");
}

#[tokio::test]
async fn receta_efectiva_respeta_el_tipo_del_chip() {
    let b = banco(&[("tel1", telefono_dealer())]);
    poner_receta_dealer(&b);
    let ctx = &b.lotes.ctx;
    let compania = ctx.recetas.read().unwrap()["tigo-transferir"].clone();
    let dealer = ctx.recetas.read().unwrap()["tigo-dealer-transferir"].clone();
    assert_eq!(ctx.receta_efectiva(&compania, "dealer").unwrap().nombre, "tigo-dealer-transferir");
    assert_eq!(ctx.receta_efectiva(&compania, "normal").unwrap().nombre, "tigo-transferir");
    assert_eq!(ctx.receta_efectiva(&dealer, "dealer").unwrap().nombre, "tigo-dealer-transferir");
    assert!(ctx.receta_efectiva(&dealer, "normal").is_err(), "una receta dealer no corre en un chip normal");
}

#[test]
fn una_base_anterior_sin_columna_tipo_se_migra() {
    let a = Arc::new(almacen::Almacen::en_memoria().unwrap());
    a.con(|c| {
        c.execute_batch("CREATE TABLE chips (id INTEGER PRIMARY KEY AUTOINCREMENT, iccid TEXT, numero TEXT, visto_ms INTEGER NOT NULL);
                         INSERT INTO chips (iccid, numero, visto_ms) VALUES ('1', '78815232', 0);")
    })
    .unwrap();
    let p = persistencia::Persistencia::new(a).unwrap();
    assert_eq!(p.tipos().unwrap().get(&1).map(String::as_str), Some("normal"));
    assert_eq!(p.fijar_tipo(1, "dealer").unwrap(), "normal");
    assert_eq!(p.tipos().unwrap().get(&1).map(String::as_str), Some("dealer"));
}

#[tokio::test]
async fn pin_del_chip_se_guarda_sigue_al_chip_y_no_sale_en_el_historial() {
    let b = banco(&[("tel1", telefono_dealer())]);
    let r = Receta::desde_yaml(include_str!("../../../recetas/tigo-dealer-recargar.yaml")).unwrap();
    let pedido: BTreeMap<String, String> =
        [("numero", "69869869"), ("monto", "1.50")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    let sin_pin = b.lotes.ctx.ejecutar_receta(&r, "tel1", 0, &pedido, true).await.unwrap_err();
    assert_eq!(sin_pin.codigo, codigos::PARAMETRO);

    assert!(b.lotes.ctx.fijar_pin("tel1", 0, Some("12a4")).is_err());
    b.lotes.ctx.fijar_pin("tel1", 0, Some("4321")).unwrap();
    assert!(chip_de(&b, 0).tiene_pin);
    assert!(!chip_de(&b, 1).tiene_pin);

    b.lotes.ctx.ejecutar_receta(&r, "tel1", 0, &pedido, true).await.unwrap();
    let guardada = &b.lotes.ctx.almacen.ejecuciones(1).unwrap()[0];
    assert_eq!(guardada["parametros"]["pin"], nucleo::receta::TAPADO);
    assert!(!guardada.to_string().contains("4321"));

    // El chip cambia de ranura: el PIN va con él.
    poner_chips(&b, "tel1", vec![sim_con(0, "70000001", None), sim(1, "78815232")]);
    assert!(!chip_de(&b, 0).tiene_pin);
    assert!(chip_de(&b, 1).tiene_pin);
    b.lotes.ctx.fijar_pin("tel1", 1, None).unwrap();
    assert!(!chip_de(&b, 1).tiene_pin);
}

// ───────────────────────────── lote de un chip ─────────────────────────────

/// Teléfono con el chip 0 Tigo dealer (PIN 4321, 10 Bs de dealer) y el chip 1 normal.
fn banco_dealer() -> (Banco, Arc<Telefono>) {
    let mut dealer = LineaSimulada::tigo("78815232", 40.0);
    dealer.pin_dealer = Some("4321".into());
    dealer.saldo_dealer = 10.0;
    let t = Telefono::new(vec![dealer, LineaSimulada::tigo("69759675", 10.0)]);
    let b = banco(&[("tel1", t.clone())]);
    b.lotes.ctx.fijar_tipo("tel1", 0, "dealer").unwrap();
    b.lotes.ctx.fijar_pin("tel1", 0, Some("4321")).unwrap();
    (b, t)
}

fn origen(slot: u32) -> OrigenLote {
    OrigenLote::un_chip("tel1", slot, "tigo-dealer-recargar")
}

const RECARGAS: &str = "numero,monto\n69869869,1.50\n70000001,2\n";

#[tokio::test]
async fn lote_de_un_chip_recarga_con_numero_y_monto() {
    let (b, t) = banco_dealer();
    let v = b.lotes.validar(RECARGAS, false, Some(&origen(0))).await.unwrap();
    assert_eq!(v.resumen.invalidas, 0, "{v:#?}");
    assert_eq!(v.resumen.total_bs, 3.5);
    let o = v.origen.as_ref().unwrap();
    assert_eq!((o.operador.as_deref(), o.numero.as_deref()), (Some("TIGO"), Some("78815232")));
    assert!(v.filas.iter().all(|f| f.accion == "recargar" && f.receta.as_deref() == Some("tigo-dealer-recargar")));
    assert!(v.filas.iter().all(|f| f.plan.iter().all(|p| p.slot == 0)), "todo sale del chip elegido");

    let lote = b.lotes.crear("recargas", RECARGAS, false, Some(&origen(0)), false).unwrap();
    assert_eq!(lote.origen.as_ref().unwrap().receta.as_deref(), Some("tigo-dealer-recargar"));
    b.lotes.iniciar(lote.id).unwrap();
    let fin = esperar_fin(&b.lotes, lote.id).await;
    assert!(filas(&fin).iter().all(|f| f.estado == EstadoFila::Ok), "{fin:#?}");
    assert_eq!(fin.progreso.enviado_bs, 3.5);
    assert!(filas(&fin).iter().flat_map(|f| f.envios.iter()).all(|e| e.slot == Some(0)));
    assert!((t.linea(0).saldo_dealer - 6.5).abs() < 1e-9);
    assert_eq!(t.linea(0).movimientos, ["Recarga 1.50 Bs a 69869869", "Recarga 2 Bs a 70000001"]);
    // Cuadra con el saldo dealer (10 al iniciar → 6.50), no con el principal.
    let v = b.lotes.verificar(lote.id).await.unwrap();
    assert_eq!(v.saldo, "dealer");
    let c = &v.chips[0];
    assert_eq!((c.saldo_inicial, c.saldo_actual, c.descontado_esperado), (Some(10.0), Some(6.5), 3.5), "{v:#?}");
    assert!(v.cuadra && v.confirmado, "{v:#?}");
    assert_eq!(t.linea(0).saldo_principal, 40.0);
}

#[tokio::test]
async fn verificar_lote_dealer_detecta_diferencia_y_sin_pin_no_aplica() {
    let (b, t) = banco_dealer();
    let lote = b.lotes.crear("r", RECARGAS, false, Some(&origen(0)), false).unwrap();
    b.lotes.iniciar(lote.id).unwrap();
    esperar_fin(&b.lotes, lote.id).await;
    // Alguien gastó 1 Bs de dealer por fuera.
    t.lineas.lock().unwrap()[0].saldo_dealer -= 1.0;
    let v = b.lotes.verificar(lote.id).await.unwrap();
    assert_eq!(v.chips[0].estado, EstadoCuadre::Diferencia);
    assert_eq!(v.chips[0].diferencia_bs, Some(1.0));

    // Un lote que se inició sin poder leer el saldo dealer (sin PIN) no tiene con qué cuadrar.
    let (b, _) = banco_dealer();
    let lote = b.lotes.crear("r", RECARGAS, false, Some(&origen(0)), false).unwrap();
    b.lotes.ctx.fijar_pin("tel1", 0, None).unwrap();
    b.lotes.iniciar(lote.id).unwrap();
    esperar_fin(&b.lotes, lote.id).await;
    assert_eq!(b.lotes.verificar(lote.id).await.unwrap_err().codigo, "no_aplica");
}

#[tokio::test]
async fn lote_de_un_chip_simulado_no_envia_el_pin() {
    let (b, t) = banco_dealer();
    let lote = b.lotes.crear("sim", RECARGAS, true, Some(&origen(0)), false).unwrap();
    b.lotes.iniciar(lote.id).unwrap();
    let fin = esperar_fin(&b.lotes, lote.id).await;
    assert!(filas(&fin).iter().all(|f| f.estado == EstadoFila::Simulado), "{fin:#?}");
    assert_eq!(t.linea(0).saldo_dealer, 10.0);
}

#[tokio::test]
async fn lote_de_un_chip_rechaza_chip_o_filas_que_no_sirven() {
    let (b, _) = banco_dealer();
    // Chip normal con receta dealer.
    let e = b.lotes.validar(RECARGAS, false, Some(&origen(1))).await.unwrap_err();
    assert!(e.detalle.contains("dealer"), "{e:?}");
    // Sin PIN guardado.
    b.lotes.ctx.fijar_pin("tel1", 0, None).unwrap();
    let e = b.lotes.validar(RECARGAS, false, Some(&origen(0))).await.unwrap_err();
    assert!(e.detalle.contains("PIN"), "{e:?}");
    b.lotes.ctx.fijar_pin("tel1", 0, Some("4321")).unwrap();
    // Chip que no existe.
    assert!(b.lotes.validar(RECARGAS, false, Some(&origen(7))).await.is_err());
    // Filas: otro operador, monto con 3 decimales, número corto.
    let csv = "numero,monto,operador\n69869869,1,ENTEL\n69869869,1.555,\n123,1,\n";
    let v = b.lotes.validar(csv, false, Some(&origen(0))).await.unwrap();
    let errores: Vec<String> = v.filas.iter().map(|f| f.errores.join("; ")).collect();
    assert!(errores[0].contains("ENTEL"), "{errores:?}");
    assert!(errores[1].contains("monto"), "{errores:?}");
    assert!(errores[2].contains("8 dígitos"), "{errores:?}");
}

#[tokio::test]
async fn lote_por_operador_sigue_igual_con_centavos() {
    let b = banco(&[("tel1", telefono_dealer())]);
    let v = b.lotes.validar("numero,monto,operador\n69869869,1.50,TIGO\n", false, None).await.unwrap();
    assert!(v.filas[0].errores[0].contains("centavos"), "{:?}", v.filas[0].errores);
}

#[tokio::test]
async fn consultar_saldos_lee_el_dealer_solo_a_pedido_y_con_pin() {
    let (b, t) = banco_dealer();
    // Consulta automática (lotes, cambio de chip): nunca manda el PIN.
    b.lotes.ctx.consultar_saldos(None, None, false).await;
    assert_eq!(chip_de(&b, 0).saldo_dealer, None);
    assert_eq!(chip_de(&b, 0).saldo, Some(40.0), "el principal sí");

    // El botón del Panel: principal y dealer.
    b.lotes.ctx.consultar_saldos(None, None, true).await;
    let c = chip_de(&b, 0);
    assert_eq!(c.saldo_dealer, Some(10.0));
    assert!(c.saldo_dealer_ms.is_some());
    assert_eq!(chip_de(&b, 1).saldo_dealer, None, "chip normal: no tiene saldo dealer");
    assert_eq!(recetas_ejecutadas(&b).iter().filter(|r| *r == "tigo-dealer-saldo").count(), 1);

    // Sin PIN guardado se salta (no adivina ni pide nada).
    t.lineas.lock().unwrap()[0].saldo_dealer = 3.0;
    b.lotes.ctx.fijar_pin("tel1", 0, None).unwrap();
    b.lotes.ctx.consultar_saldos(None, None, true).await;
    assert_eq!(chip_de(&b, 0).saldo_dealer, Some(10.0), "queda el último conocido");
}

// ───────────────────────────── seguridad del ejecutor ─────────────────────────────

#[tokio::test]
async fn lote_real_igual_a_otro_es_duplicado_salvo_forzar() {
    let b = banco(&[]);
    let csv = "numero,monto,operador\n69869869,5,TIGO\n";
    let a = b.lotes.crear("Pedidos 29-oct", csv, false, None, false).unwrap();
    // Mismo CSV con espacios al final y líneas vacías: igual.
    let e = b.lotes.crear("otro", "numero,monto,operador  \r\n\r\n69869869,5,TIGO\r\n", false, None, false).unwrap_err();
    assert_eq!((e.http, e.codigo), (409, "duplicado"));
    assert!(e.detalle.starts_with(&format!("Igual al lote #{} «Pedidos 29-oct», creado ", a.id)), "{}", e.detalle);
    // Las simulaciones no se revisan; forzar lo crea igual.
    b.lotes.crear("sim", csv, true, None, false).unwrap();
    b.lotes.crear("sim2", csv, true, None, false).unwrap();
    let f = b.lotes.crear("forzado", csv, false, None, true).unwrap();
    // Un lote cancelado no cuenta (pero el forzado sigue ahí).
    b.lotes.cancelar(a.id).unwrap();
    assert_eq!(b.lotes.crear("x", csv, false, None, false).unwrap_err().codigo, "duplicado");
    b.lotes.cancelar(f.id).unwrap();
    b.lotes.crear("x", csv, false, None, false).unwrap();
}

/// Marca a mano un envío y su fila (como si hubiera corrido) y guarda el progreso.
fn forzar_envio(b: &Banco, lote: &Lote, fila: usize, estado: EstadoEnvio) {
    let mut f = filas(lote)[fila].clone();
    f.envios[0].estado = estado;
    b.lotes.ctx.pers.guardar_envio(&f.envios[0]).unwrap();
    f.estado = estado_fila(&f.envios);
    b.lotes.ctx.pers.guardar_fila(&f).unwrap();
    b.lotes.ctx.pers.refrescar_progreso(lote.id).unwrap();
}

#[tokio::test]
async fn lista_paginada_con_filtros_y_conteos_sin_leer_filas() {
    let b = banco(&[]);
    let mut ids = vec![];
    for i in 0..5 {
        let csv = format!("numero,monto,operador\n6986986{i},5,TIGO\n70000000,1,TIGO\n");
        ids.push(b.lotes.crear(&format!("Pedidos {i}"), &csv, i % 2 == 1, None, false).unwrap().id);
    }
    // ids[0] terminado con una fila fallida (problema); ids[4] cancelado y archivado.
    let l0 = b.lotes.obtener(ids[0]).unwrap();
    forzar_envio(&b, &l0, 0, EstadoEnvio::Fallido);
    b.lotes.ctx.pers.estado_lote(ids[0], EstadoLote::Terminado, None).unwrap();
    b.lotes.cancelar(ids[4]).unwrap();
    assert_eq!(b.lotes.archivar(ids[1], true).unwrap_err().codigo, "no_aplica", "solo terminados o cancelados");
    assert!(b.lotes.archivar(ids[4], true).unwrap().archivado);

    // Páginas de 2, más nuevos primero, sin archivados.
    let f = |x: FiltroLotes| b.lotes.listar(&x).unwrap();
    let p1 = f(FiltroLotes { limite: 2, ..todos() });
    assert_eq!(p1.lotes.iter().map(|l| l.id).collect::<Vec<_>>(), vec![ids[3], ids[2]]);
    assert_eq!((p1.siguiente, p1.total), (Some(ids[2]), 4));
    assert!(p1.lotes.iter().all(|l| l.filas.is_none()));
    // Un lote nuevo no corre el cursor.
    b.lotes.crear("nuevo", "numero,monto,operador\n77777777,5,TIGO\n", true, None, false).unwrap();
    let p2 = f(FiltroLotes { limite: 2, antes: p1.siguiente, ..todos() });
    assert_eq!(p2.lotes.iter().map(|l| l.id).collect::<Vec<_>>(), vec![ids[1], ids[0]]);
    assert_eq!(p2.siguiente, None);

    // El progreso guardado coincide con el calculado desde las filas.
    let guardado = p2.lotes.iter().find(|l| l.id == ids[0]).unwrap();
    let completo = b.lotes.obtener(ids[0]).unwrap();
    assert_eq!(guardado.progreso, completo.progreso);
    assert_eq!(guardado.revision, completo.revision);
    assert_eq!(guardado.progreso.fallidas, 1);

    let solo = |x: FiltroLotes| f(x).lotes.iter().map(|l| l.id).collect::<Vec<_>>();
    assert_eq!(solo(FiltroLotes { problemas: Some(true), ..todos() }), vec![ids[0]]);
    assert_eq!(solo(FiltroLotes { simular: Some(false), ..todos() }), vec![ids[2], ids[0]]);
    assert_eq!(solo(FiltroLotes { estados: vec![EstadoLote::Terminado, EstadoLote::Cancelado], archivados: None, ..todos() }), vec![ids[4], ids[0]]);
    assert_eq!(solo(FiltroLotes { archivados: Some(true), ..todos() }), vec![ids[4]]);
    assert_eq!(solo(FiltroLotes { q: Some("pedidos 3".into()), ..todos() }), vec![ids[3]]);
    assert_eq!(solo(FiltroLotes { q: Some(format!("#{}", ids[2])), ..todos() }), vec![ids[2]]);
    assert_eq!(solo(FiltroLotes { q: Some("100%".into()), ..todos() }), Vec::<i64>::new(), "% es literal");

    let c = f(todos()).conteos;
    assert_eq!(c, ConteosLotes { activos: 4, problemas: 1, terminados: 1, simulaciones: 3, archivados: 1 });
    // conteos no depende del filtro.
    assert_eq!(f(FiltroLotes { simular: Some(true), ..todos() }).conteos, c);
}

#[tokio::test]
async fn filas_por_paginas_con_filtros() {
    let b = banco(&[]);
    let csv = "numero,monto,operador,referencia\n69869861,5,TIGO,A1\n69869862,5,TIGO,B2\n123,5,TIGO,C3\n69869864,5,TIGO,A4\n69869865,5,TIGO,B5\n";
    let l = b.lotes.crear("filas", csv, true, None, false).unwrap();
    let f = |x: FiltroFilas| b.lotes.filas(l.id, &x).unwrap();
    let p1 = f(FiltroFilas { limite: 2, ..Default::default() });
    assert_eq!(p1.filas.iter().map(|x| x.fila).collect::<Vec<_>>(), vec![2, 3]);
    assert_eq!((p1.siguiente, p1.total), (Some(3), 5));
    assert_eq!(p1.filas[0].envios.len(), 1, "con sus envíos");
    let p2 = f(FiltroFilas { limite: 2, despues: p1.siguiente, ..Default::default() });
    assert_eq!(p2.filas.iter().map(|x| x.fila).collect::<Vec<_>>(), vec![4, 5]);
    let p3 = f(FiltroFilas { limite: 2, despues: p2.siguiente, ..Default::default() });
    assert_eq!((p3.filas.len(), p3.siguiente), (1, None));
    let om = f(FiltroFilas { limite: 10, estados: vec![EstadoFila::Omitida], ..Default::default() });
    assert_eq!((om.total, om.filas[0].fila), (1, 4));
    let q = f(FiltroFilas { limite: 10, q: Some("b".into()), ..Default::default() });
    assert_eq!(q.filas.iter().map(|x| x.referencia.clone().unwrap()).collect::<Vec<_>>(), vec!["B2", "B5"]);
    assert_eq!(f(FiltroFilas { limite: 10, q: Some("69869864".into()), ..Default::default() }).total, 1);
    assert_eq!(b.lotes.filas(999, &FiltroFilas { limite: 1, ..Default::default() }).unwrap_err().codigo, "sin_lote");
    let sin = b.lotes.obtener_sin_filas(l.id).unwrap();
    assert!(sin.filas.is_none());
    assert_eq!(sin.progreso, b.lotes.obtener(l.id).unwrap().progreso);
}

#[tokio::test]
async fn al_arrancar_todo_envio_en_curso_queda_incierto_en_cualquier_lote() {
    let almacen = Arc::new(almacen::Almacen::en_memoria().unwrap());
    let b = banco_con(almacen.clone(), &[]);
    let lote = b.lotes.crear("pausado", "numero,monto,operador\n69869869,5,TIGO\n70000000,1,TIGO\n", false, None, false).unwrap();
    let mut e = filas(&lote)[0].envios[0].clone();
    e.estado = EstadoEnvio::EnCurso;
    b.lotes.ctx.pers.guardar_envio(&e).unwrap();
    // El lote no estaba en curso (p. ej. su estado no llegó a guardarse).
    b.lotes.ctx.pers.estado_lote(lote.id, EstadoLote::Pausado, Some("pausado por el usuario")).unwrap();
    let b2 = banco_con(almacen, &[]);
    let l = b2.lotes.obtener(lote.id).unwrap();
    assert_eq!(l.estado, EstadoLote::Pausado);
    assert_eq!(filas(&l)[0].envios[0].estado, EstadoEnvio::Incierto);
    assert_eq!(b2.lotes.obtener_sin_filas(lote.id).unwrap().progreso.inciertas, 1, "progreso guardado al día");
}

#[tokio::test]
async fn si_la_base_no_guarda_el_en_curso_no_se_despacha_y_se_pausa() {
    let t = Telefono::new(vec![LineaSimulada::tigo("78815232", 100.0)]);
    let b = banco(&[("tel1", t.clone())]);
    let lote = b.lotes.crear("sin base", "numero,monto,operador\n69869869,5,TIGO\n", false, None, false).unwrap();
    b.lotes
        .ctx
        .almacen
        .con(|c| {
            c.execute_batch(
                "CREATE TRIGGER no_en_curso BEFORE UPDATE ON lote_envios WHEN NEW.estado = 'en_curso'
                 BEGIN SELECT RAISE(ABORT, 'disco lleno'); END;",
            )
        })
        .unwrap();
    b.lotes.iniciar(lote.id).unwrap();
    let fin = esperar_fin(&b.lotes, lote.id).await;
    assert_eq!(fin.estado, EstadoLote::Pausado);
    assert!(fin.motivo_pausa.as_deref().unwrap().starts_with("no se pudo guardar en la base: "), "{:?}", fin.motivo_pausa);
    assert!(fin.motivo_pausa.as_deref().unwrap().contains("disco lleno"));
    assert_eq!(filas(&fin)[0].envios[0].estado, EstadoEnvio::Pendiente);
    assert_eq!(t.linea(0).transferencias_hoy, 0, "no se marcó nada");
    assert!(!b.registro.obtener("tel1").unwrap().ocupado, "el teléfono quedó libre");
}

#[tokio::test]
async fn si_la_tarea_de_un_envio_se_cae_queda_incierto_y_libera_el_telefono() {
    let t = Telefono::new(vec![LineaSimulada::tigo("78815232", 100.0)]);
    let b = banco(&[("tel1", t.clone())]);
    // Saldo fresco: iniciar no lo consulta (si no, la consulta se llevaría el pánico).
    b.lotes.ctx.fijar_saldo("tel1", 0, 100.0, None, None);
    let mut rx = b.lotes.ctx.bus.suscribir();
    t.explotar.store(true, Ordering::SeqCst);
    let lote = b.lotes.crear("pánico", "numero,monto,operador\n69869869,5,TIGO\n70000000,1,TIGO\n", false, None, false).unwrap();
    b.lotes.iniciar(lote.id).unwrap();
    let fin = esperar_fin(&b.lotes, lote.id).await;
    let fs = filas(&fin);
    assert_eq!(fs[0].estado, EstadoFila::Incierta);
    assert!(fs[0].envios[0].detalle.as_deref().unwrap().contains("se cayó"));
    assert_eq!(fs[1].estado, EstadoFila::Ok, "el lote siguió con lo demás");
    assert!(!b.registro.obtener("tel1").unwrap().ocupado);
    let mut alerta = false;
    while let Ok(e) = rx.try_recv() {
        alerta |= e.tipo == "lote" && e.datos["fase"] == "alerta";
    }
    assert!(alerta, "se avisó");
    // La ejecución que se cayó no quedó `en_curso` en el historial.
    let ej = b.lotes.ctx.almacen.ejecuciones(10).unwrap();
    assert!(ej.iter().all(|e| e["estado"] != "en_curso"), "{ej:?}");
    assert!(ej.iter().any(|e| e["estado"] == "incierto"));
}

#[tokio::test]
async fn telefono_siempre_ocupado_termina_fallido_tras_10_reintentos() {
    let t = Telefono::new(vec![LineaSimulada::tigo("78815232", 100.0)]);
    t.siempre_ocupado.store(true, Ordering::SeqCst);
    let b = banco(&[("tel1", t.clone())]);
    let lote = b.lotes.crear("ocupado", "numero,monto,operador\n69869869,5,TIGO\n", false, None, false).unwrap();
    b.lotes.iniciar(lote.id).unwrap();
    let fin = esperar_fin(&b.lotes, lote.id).await;
    let e = &filas(&fin)[0].envios[0];
    assert_eq!(e.estado, EstadoEnvio::Fallido);
    assert!(e.detalle.as_deref().unwrap().starts_with("el teléfono siguió ocupado"), "{:?}", e.detalle);
    assert_eq!(e.intentos, ejecutor::MAX_OCUPADO + 1);
    assert_eq!(t.linea(0).transferencias_hoy, 0);
}

#[tokio::test]
async fn recargar_recetas_a_mitad_no_cambia_el_lote() {
    let t = Telefono::con_demora(vec![LineaSimulada::tigo("78815232", 100.0)], Duration::from_millis(15));
    let b = banco(&[("tel1", t.clone())]);
    let lote = b.lotes.crear("copia", "numero,monto,operador\n69869869,5,TIGO\n70000000,1,TIGO\n69869869,2,TIGO\n", false, None, false).unwrap();
    b.lotes.iniciar(lote.id).unwrap();
    tokio::time::sleep(Duration::from_millis(30)).await;
    // Se "recarga" la carpeta y la receta desaparece: el lote sigue con su copia.
    b.lotes.ctx.recetas.write().unwrap().remove("tigo-transferir");
    let fin = esperar_fin(&b.lotes, lote.id).await;
    assert_eq!(fin.progreso.ok, 3, "{:?}", fin.revision);
}

#[tokio::test]
async fn un_telefono_reservado_no_lo_toma_nadie_mas() {
    let t = Telefono::con_demora(vec![LineaSimulada::tigo("78815232", 100.0)], Duration::from_millis(60));
    let b = banco(&[("tel1", t.clone())]);
    let ctx = b.lotes.ctx.clone();
    let r = ctx.reservar("tel1").unwrap();
    assert!(ctx.reservar("tel1").is_none());
    let receta = ctx.recetas.read().unwrap()["tigo-saldo"].clone();
    let e = ctx.ejecutar_receta(&receta, "tel1", 0, &BTreeMap::new(), false).await.unwrap_err();
    assert_eq!(e.codigo, codigos::OCUPADO);
    drop(r);
    assert!(!ctx.ocupado("tel1"));
    // La ejecución queda en el historial antes de marcar (en curso) y se completa al terminar.
    let c2 = ctx.clone();
    let tarea = tokio::spawn(async move { c2.ejecutar_receta(&receta, "tel1", 0, &BTreeMap::new(), false).await });
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert!(ctx.ocupado("tel1"));
    assert_eq!(ctx.almacen.ejecuciones(1).unwrap()[0]["estado"], "en_curso");
    let (res, id) = tarea.await.unwrap().unwrap();
    assert_eq!(res.estado, protocolo::EstadoResultado::Ok);
    let ej = ctx.almacen.ejecuciones(1).unwrap();
    assert_eq!((ej[0]["id"].as_i64(), ej[0]["estado"].as_str()), (id, Some("ok")));
    assert!(!ctx.ocupado("tel1"));
}

#[tokio::test]
async fn una_base_vieja_sin_progreso_se_completa_una_vez() {
    let a = Arc::new(almacen::Almacen::en_memoria().unwrap());
    let b = banco_con(a.clone(), &[]);
    let l = b.lotes.crear("viejo", "numero,monto,operador\n69869869,12,TIGO\n1,5,TIGO\n", true, None, false).unwrap();
    a.con(|c| c.execute("UPDATE lotes SET progreso = NULL", [])).unwrap();
    let b2 = banco_con(a, &[]);
    let p = b2.lotes.obtener_sin_filas(l.id).unwrap().progreso;
    assert_eq!((p.filas, p.omitidas, p.pendientes, p.total_bs), (1, 1, 1, 12.0));
}


// ───────────────────────────── montos con centavos, recargas dealer y chips elegidos ─────────────────────────────

/// tel1: [Tigo dealer 78815232 (PIN 4321, dealer 10 Bs), Tigo normal 69759675]
/// tel2: [Entel dealer 71227468 (dealer 5 Bs, sin PIN: su receta no lo pide), Tigo normal 77700001]
fn banco_mixto(demora: Duration) -> (Banco, Arc<Telefono>, Arc<Telefono>) {
    let mut tigo_dealer = LineaSimulada::tigo("78815232", 40.0);
    tigo_dealer.pin_dealer = Some("4321".into());
    tigo_dealer.saldo_dealer = 10.0;
    let mut entel_dealer = LineaSimulada::entel("71227468", 0.0);
    entel_dealer.dealer_entel = true;
    entel_dealer.saldo_dealer = 5.0;
    let t1 = Telefono::con_demora(vec![tigo_dealer, LineaSimulada::tigo("69759675", 10.0)], demora);
    let t2 = Telefono::con_demora(vec![entel_dealer, LineaSimulada::tigo("77700001", 10.0)], demora);
    let b = banco(&[("tel1", t1.clone()), ("tel2", t2.clone())]);
    b.lotes.ctx.fijar_tipo("tel1", 0, "dealer").unwrap();
    b.lotes.ctx.fijar_pin("tel1", 0, Some("4321")).unwrap();
    b.lotes.ctx.fijar_tipo("tel2", 0, "dealer").unwrap();
    for (d, sl, saldo) in [("tel1", 0, 40.0), ("tel1", 1, 10.0), ("tel2", 1, 10.0)] {
        b.lotes.ctx.fijar_saldo(d, sl, saldo, None, None);
    }
    (b, t1, t2)
}

const RECARGAS_MIXTAS: &str = "numero,monto,operador,accion,referencia\n\
                               69869869,1.50,TIGO,recargar,A\n\
                               71234567,0.50,ENTEL,recargar,B\n\
                               71234568,\"2,00\",ENTEL,recargar,C\n";

#[tokio::test]
async fn validacion_entel_dealer_bs_enteros_o_0_50() {
    let (b, _, _) = banco_mixto(Duration::ZERO);
    let csv = "numero,monto,operador,accion\n71234567,0.5,ENTEL,recargar\n71234568,100,ENTEL,recargar\n71234569,1.50,ENTEL,recargar\n71234560,0.25,ENTEL,recargar\n";
    let v = b.lotes.validar(csv, false, None).await.unwrap();
    assert_eq!(v.filas[0].envios, vec![0.5], "{:?}", v.filas[0]);
    assert_eq!(v.filas[1].envios, vec![100.0], "Bs 100 es un solo envío, no 200 de 0.50");
    assert!(v.filas[2].errores[0].contains("1.50 Bs no es válido"), "{:?}", v.filas[2].errores);
    assert!(!v.filas[3].errores.is_empty());
    assert_eq!(v.resumen.total_bs, 100.5);
    // Las opciones enteras de siempre: igual que antes.
    let v = b.lotes.validar("numero,monto,operador\n69869869,12,TIGO\n69869860,1.50,TIGO\n", false, None).await.unwrap();
    assert_eq!(v.filas[0].envios, vec![10.0, 2.0]);
    assert!(v.filas[1].errores[0].contains("centavos"), "{:?}", v.filas[1].errores);
}

#[tokio::test]
async fn por_operador_recargar_va_solo_a_los_chips_dealer() {
    let (b, t1, t2) = banco_mixto(Duration::ZERO);
    let v = b.lotes.validar(RECARGAS_MIXTAS, false, None).await.unwrap();
    assert_eq!(v.resumen.invalidas, 0, "{:#?}", v.filas);
    let recetas: Vec<_> = v.filas.iter().map(|f| f.receta.clone().unwrap()).collect();
    assert_eq!(recetas, ["tigo-dealer-recargar", "entel-dealer-recargar", "entel-dealer-recargar"]);
    let planes: Vec<(String, u32)> = v.filas.iter().flat_map(|f| f.plan.iter().map(|p| (p.dispositivo.clone(), p.slot))).collect();
    assert_eq!(planes.len(), 3, "un envío por fila: {planes:?}");
    assert!(planes.iter().all(|(d, sl)| *sl == 0 && (d == "tel1" || d == "tel2")), "solo chips dealer: {planes:?}");

    let lote = b.lotes.crear("recargas", RECARGAS_MIXTAS, false, None, false).unwrap();
    b.lotes.iniciar(lote.id).unwrap();
    let fin = esperar_fin(&b.lotes, lote.id).await;
    assert!(filas(&fin).iter().all(|f| f.estado == EstadoFila::Ok), "{:#?}", filas(&fin));
    assert_eq!(fin.progreso.enviado_bs, 4.0);
    // Entel: Bs 0.50 va como 050 y los enteros en Bs (2 → *2#, no *200#).
    let entel: Vec<String> = t2.marcados.lock().unwrap().clone();
    assert_eq!(entel, ["*133*71234567*050#", "*133*71234568*2#"]);
    assert!((t2.linea(0).saldo_dealer - 2.5).abs() < 1e-9);
    assert!((t1.linea(0).saldo_dealer - 8.5).abs() < 1e-9);
    // Los chips normales no se tocaron (ni su saldo principal).
    assert_eq!((t1.linea(1).saldo_principal, t2.linea(1).saldo_principal), (10.0, 10.0));
    assert!(t1.linea(1).movimientos.is_empty() && t2.linea(1).movimientos.is_empty());

    // Verificar: por chip y tipo. El Tigo dealer cuadra con su saldo dealer; el Entel no tiene receta de saldo dealer.
    let v = b.lotes.verificar(lote.id).await.unwrap();
    assert_eq!(v.saldo, "dealer", "{v:#?}");
    let tigo = v.chips.iter().find(|c| c.dispositivo == "tel1").unwrap();
    assert_eq!((tigo.saldo.as_str(), tigo.estado, tigo.saldo_inicial, tigo.saldo_actual), ("dealer", EstadoCuadre::Cuadra, Some(10.0), Some(8.5)));
    let entel = v.chips.iter().find(|c| c.dispositivo == "tel2").unwrap();
    assert_eq!((entel.saldo.as_str(), entel.estado, entel.descontado_esperado), ("dealer", EstadoCuadre::SinDato, 2.5));
    assert!(v.avisos.iter().any(|a| a.contains("no hay receta de saldo dealer para ENTEL")), "{:?}", v.avisos);
    assert!(!v.cuadra);
}

#[tokio::test]
async fn por_operador_un_chip_dealer_sin_pin_queda_fuera_del_plan() {
    let (b, _, _) = banco_mixto(Duration::ZERO);
    b.lotes.ctx.fijar_pin("tel1", 0, None).unwrap();
    let v = b.lotes.validar(RECARGAS_MIXTAS, false, None).await.unwrap();
    let tigo = &v.filas[0];
    assert_eq!((tigo.sin_capacidad, tigo.plan.len()), (1, 0));
    assert!(tigo.motivo_sin_capacidad.as_deref().unwrap().contains("sin PIN guardado"), "{tigo:?}");
    // Entel no pide PIN: sigue entrando.
    assert!(v.filas[1..].iter().all(|f| f.sin_capacidad == 0));

    // Los carriles lo muestran excluido, con el motivo.
    let lote = b.lotes.crear("r", RECARGAS_MIXTAS, true, None, false).unwrap();
    let c = b.lotes.carriles(lote.id).unwrap();
    let tel1 = c.carriles.iter().find(|x| x.dispositivo == "tel1" && x.slot == 0).unwrap();
    assert_eq!(tel1.estado, "excluido");
    assert!(tel1.motivo.as_deref().unwrap().contains("sin PIN guardado"));
    // Un chip normal de Tigo no puede recargar (solo hay receta dealer): también fuera.
    let normal = c.carriles.iter().find(|x| x.dispositivo == "tel1" && x.slot == 1).unwrap();
    assert_eq!(normal.estado, "excluido", "{normal:?}");
}

/// Recargas por operador: el resumen y los chips muestran el cupo de la receta de recargar (no el de transferir).
#[tokio::test]
async fn por_operador_recargar_resume_el_cupo_de_su_receta() {
    let (b, _, _) = banco_mixto(Duration::ZERO);
    // Tigo dealer con límite diario (3); Entel dealer tal cual (sin `por_dia`).
    let yaml = include_str!("../../../recetas/tigo-dealer-recargar.yaml").replace("chip_tipo: dealer\n", "chip_tipo: dealer\nlimites:\n  por_dia: 3\n");
    let r = Receta::desde_yaml(&yaml).unwrap();
    assert_eq!(r.limites.as_ref().and_then(|l| l.por_dia), Some(3));
    b.lotes.ctx.recetas.write().unwrap().insert(r.nombre.clone(), r);

    let v = b.lotes.validar(RECARGAS_MIXTAS, false, None).await.unwrap();
    assert_eq!((v.resumen.planificados, v.resumen.sin_capacidad), (3, 0));
    let ops: Vec<(&str, &str)> = v.resumen.operadores.iter().map(|o| (o.operador.as_str(), o.accion.as_str())).collect();
    assert_eq!(ops, [("ENTEL", "recargar"), ("TIGO", "recargar")]);
    let entel = &v.resumen.operadores[0];
    assert_eq!((entel.chips, entel.cupo_hoy, entel.cupo_sin_limite, entel.envios), (1, 0, true, 2));
    let tigo = &v.resumen.operadores[1];
    assert_eq!((tigo.chips, tigo.cupo_hoy, tigo.cupo_sin_limite, tigo.envios), (1, 3, false, 1));
    // El saldo principal no cuenta para recargas.
    assert!(v.resumen.operadores.iter().all(|o| o.saldo_conocido == 0.0 && o.saldo_desconocido_chips == 0));
    assert!((tigo.necesario_bs - 1.5).abs() < 1e-9);

    let chip = |d: &str, sl: u32| v.chips.iter().filter(|c| c.dispositivo == d && c.slot == sl).collect::<Vec<_>>();
    let e = chip("tel2", 0);
    assert_eq!(e.len(), 1);
    assert_eq!((e[0].accion.as_deref(), e[0].sin_limite, e[0].cupo_hoy, e[0].saldo), (Some("recargar"), true, 0, None));
    let t = chip("tel1", 0);
    assert_eq!((t[0].accion.as_deref(), t[0].sin_limite, t[0].cupo_hoy, t[0].enviados_hoy), (Some("recargar"), false, 3, 0));
    // Los chips normales no recargan: aparecen una vez, sin acción, con los números de transferir.
    let n = chip("tel1", 1);
    assert_eq!((n.len(), n[0].accion.as_deref(), n[0].cupo_hoy), (1, None, 5));
    // Nunca el cupo "infinito" interno.
    let json = serde_json::to_string(&v).unwrap();
    assert!(!json.contains("1000"), "{json}");

    // Lote mixto: una entrada por (operador, acción); la de transferir sigue como siempre.
    let mixto = "numero,monto,operador,accion\n69869869,1.50,TIGO,recargar\n69869860,5,TIGO,transferir\n";
    let v = b.lotes.validar(mixto, false, None).await.unwrap();
    let ops: Vec<(&str, &str, u32, u32)> =
        v.resumen.operadores.iter().map(|o| (o.operador.as_str(), o.accion.as_str(), o.cupo_hoy, o.envios)).collect();
    // Transferir: los 3 chips Tigo en línea, 5 cada uno (el dealer también usa la de la compañía).
    assert_eq!(ops, [("TIGO", "recargar", 3, 1), ("TIGO", "transferir", 15, 1)]);
    let tr = &v.resumen.operadores[1];
    assert!((tr.saldo_conocido - 60.0).abs() < 1e-9 && !tr.cupo_sin_limite, "{tr:?}");
    assert!((tr.necesario_bs - 5.2).abs() < 1e-9, "{tr:?}");
    let t = chip_acciones(&v, "tel1", 0);
    assert_eq!(t, [(Some("recargar".to_string()), 3), (Some("transferir".to_string()), 5)]);
}

fn chip_acciones(v: &Validacion, d: &str, sl: u32) -> Vec<(Option<String>, u32)> {
    v.chips.iter().filter(|c| c.dispositivo == d && c.slot == sl).map(|c| (c.accion.clone(), c.cupo_hoy)).collect()
}

#[tokio::test]
async fn origen_de_varios_chips_de_varios_operadores() {
    let (b, t1, t2) = banco_mixto(Duration::from_millis(20));
    // El chip normal de Tigo no puede recargar: se excluye con aviso; los dealer sirven.
    let o = OrigenLote::de_chips(&[("tel1", 0), ("tel2", 0), ("tel1", 1)], "recargar");
    let csv = "numero,monto,operador,referencia\n69869869,1.50,TIGO,A\n71234567,0.50,ENTEL,B\n70000001,1,VIVA,C\n69869860,2,,D\n";
    let v = b.lotes.validar(csv, false, Some(&o)).await.unwrap();
    assert_eq!(v.avisos_origen.len(), 1, "{:?}", v.avisos_origen);
    assert!(v.avisos_origen[0].contains("solo para chips dealer"), "{:?}", v.avisos_origen);
    let origen = v.origen.clone().unwrap();
    assert_eq!(origen.accion.as_deref(), Some("recargar"));
    type Fila = (String, u32, Option<String>, Option<String>, Option<String>);
    let chips: Vec<Fila> = origen
        .chips
        .iter()
        .map(|c| (c.dispositivo.clone(), c.slot, c.operador.clone(), c.tipo.clone(), c.receta.clone()))
        .collect();
    assert_eq!(
        chips,
        vec![
            ("tel1".into(), 0, Some("TIGO".into()), Some("dealer".into()), Some("tigo-dealer-recargar".into())),
            ("tel2".into(), 0, Some("ENTEL".into()), Some("dealer".into()), Some("entel-dealer-recargar".into())),
        ]
    );
    assert!(origen.dispositivo.is_none(), "la forma nueva no lleva los campos viejos");
    assert_eq!(v.filas[0].errores, Vec::<String>::new());
    assert!(v.filas[2].errores[0].contains("ninguno de los chips elegidos es VIVA"), "{:?}", v.filas[2].errores);
    assert!(v.filas[3].errores[0].contains("falta el operador"), "{:?}", v.filas[3].errores);
    // Con chips de dos operadores, la columna operador es obligatoria.
    let e = b.lotes.validar("numero,monto\n69869869,1.50\n", false, Some(&o)).await.unwrap_err();
    assert!(e.detalle.contains("operador"), "{e:?}");
    // Sin ningún chip que sirva: 400 con los motivos.
    let e = b.lotes.validar(csv, false, Some(&OrigenLote::de_chips(&[("tel1", 1)], "recargar"))).await.unwrap_err();
    assert_eq!(e.http, 400);
    assert!(e.detalle.contains("ninguno de los chips elegidos sirve"), "{e:?}");

    // Corre solo en los chips elegidos y en paralelo (un envío por teléfono a la vez).
    let csv = "numero,monto,operador\n69869869,1.50,TIGO\n71234567,0.50,ENTEL\n71234567,3,ENTEL\n69869861,0.20,TIGO\n";
    let lote = b.lotes.crear("chips", csv, false, Some(&o), false).unwrap();
    let guardado = lote.origen.clone().unwrap();
    assert_eq!((guardado.chips.len(), guardado.accion.as_deref()), (2, Some("recargar")));
    b.lotes.iniciar(lote.id).unwrap();
    // En vivo: los dos teléfonos trabajan a la vez.
    let mut a_la_vez = false;
    for _ in 0..200 {
        let c = b.lotes.carriles(lote.id).unwrap();
        if c.carriles.iter().filter(|x| x.estado == "enviando").count() == 2 {
            a_la_vez = true;
            let x = c.carriles.iter().find(|x| x.estado == "enviando").unwrap();
            assert!(x.actual.is_some());
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(a_la_vez, "los dos teléfonos deberían enviar en paralelo");
    let fin = esperar_fin(&b.lotes, lote.id).await;
    assert!(filas(&fin).iter().all(|f| f.estado == EstadoFila::Ok), "{:#?}", filas(&fin));
    let usados: Vec<(String, u32)> =
        filas(&fin).iter().flat_map(|f| f.envios.iter()).map(|e| (e.dispositivo.clone().unwrap(), e.slot.unwrap())).collect();
    assert!(usados.iter().all(|(_, sl)| *sl == 0), "{usados:?}");
    assert_eq!(*t2.marcados.lock().unwrap(), ["*133*71234567*050#", "*133*71234567*3#"]);
    assert!((t1.linea(0).saldo_dealer - (10.0 - 1.7)).abs() < 1e-9);

    // Un solo operador entre los elegidos: el CSV puede ser solo numero,monto.
    let solo = OrigenLote::de_chips(&[("tel1", 0)], "recargar");
    let v = b.lotes.validar("numero,monto\n69869869,1.50\n", false, Some(&solo)).await.unwrap();
    assert_eq!((v.resumen.invalidas, v.filas[0].receta.as_deref()), (0, Some("tigo-dealer-recargar")));
}

#[tokio::test]
async fn el_origen_viejo_sigue_valiendo_y_trae_chips() {
    let (b, _) = banco_dealer();
    let v = b.lotes.validar(RECARGAS, false, Some(&origen(0))).await.unwrap();
    let o = v.origen.unwrap();
    assert_eq!((o.dispositivo.as_deref(), o.slot, o.receta.as_deref()), (Some("tel1"), Some(0), Some("tigo-dealer-recargar")));
    assert_eq!(o.chips.len(), 1);
    assert_eq!(o.chips[0].receta.as_deref(), Some("tigo-dealer-recargar"));
    assert_eq!(o.accion.as_deref(), Some("recargar"));
    // Mismo hash de siempre para la forma vieja.
    let viejo = hash_lote(RECARGAS, Some(&origen(0)));
    let mut h = Sha256::new();
    h.update(RECARGAS.trim_end().as_bytes());
    h.update(b"\n--origen--\n");
    h.update(b"tel1|0|tigo-dealer-recargar");
    assert_eq!(viejo, h.finalize().iter().map(|b| format!("{b:02x}")).collect::<String>());
    assert_ne!(viejo, hash_lote(RECARGAS, Some(&OrigenLote::de_chips(&[("tel1", 0)], "recargar"))));
}

#[tokio::test]
async fn verificar_mixto_transferencia_y_recarga() {
    let (b, t1, _) = banco_mixto(Duration::ZERO);
    let csv = "numero,monto,operador,accion\n69869869,5,TIGO,transferir\n69869860,1.50,TIGO,recargar\n";
    let lote = b.lotes.crear("mixto", csv, false, None, false).unwrap();
    b.lotes.iniciar(lote.id).unwrap();
    let fin = esperar_fin(&b.lotes, lote.id).await;
    assert!(filas(&fin).iter().all(|f| f.estado == EstadoFila::Ok), "{:#?}", filas(&fin));
    let v = b.lotes.verificar(lote.id).await.unwrap();
    assert_eq!(v.saldo, "mixto", "{v:#?}");
    let dealer = v.chips.iter().find(|c| c.saldo == "dealer").unwrap();
    assert_eq!((dealer.dispositivo.as_str(), dealer.slot, dealer.estado), ("tel1", 0, EstadoCuadre::Cuadra));
    assert_eq!((dealer.saldo_inicial, dealer.saldo_actual), (Some(10.0), Some(8.5)));
    let principal = v.chips.iter().find(|c| c.saldo == "principal").unwrap();
    assert_eq!(principal.estado, EstadoCuadre::Cuadra, "{v:#?}");
    assert_eq!(principal.descontado_esperado, 5.2);
    assert!(v.cuadra, "{v:#?}");
    assert!((t1.linea(0).saldo_dealer - 8.5).abs() < 1e-9);
}

#[tokio::test]
async fn carriles_con_la_forma_del_contrato() {
    let (b, _, _) = banco_mixto(Duration::ZERO);
    let lote = b.lotes.crear("r", RECARGAS_MIXTAS, false, None, false).unwrap();
    let antes = b.lotes.carriles(lote.id).unwrap();
    assert_eq!((antes.pendientes, antes.eta_ms), (3, None));
    let libre = antes.carriles.iter().find(|c| c.dispositivo == "tel2" && c.slot == 0).unwrap();
    assert_eq!((libre.estado.as_str(), libre.receta.as_deref(), libre.cupo_restante), ("libre", Some("entel-dealer-recargar"), None));
    assert!(libre.cupo_sin_limite, "sin límite, no \"sin dato\"");
    b.lotes.iniciar(lote.id).unwrap();
    esperar_fin(&b.lotes, lote.id).await;
    let c = b.lotes.carriles(lote.id).unwrap();
    let j = serde_json::to_value(&c).unwrap();
    assert_eq!(j["pendientes"], 0);
    assert!(j["eta_ms"].is_null());
    let tel1 = j["carriles"].as_array().unwrap().iter().find(|x| x["dispositivo"] == "tel1" && x["slot"] == 0).unwrap();
    let mut claves: Vec<&str> = tel1.as_object().unwrap().keys().map(String::as_str).collect();
    claves.sort();
    assert_eq!(
        claves,
        [
            "actual", "cupo_restante", "cupo_sin_limite", "dispositivo", "enviado_bs", "estado", "fallidos", "inciertos", "motivo", "ms_promedio", "numero",
            "ok", "operador", "receta", "saldo", "slot", "telefono", "tipo"
        ]
    );
    assert_eq!(tel1["telefono"], "Tel tel1");
    assert_eq!((tel1["operador"].as_str(), tel1["tipo"].as_str(), tel1["receta"].as_str()), (Some("TIGO"), Some("dealer"), Some("tigo-dealer-recargar")));
    assert_eq!((tel1["ok"].as_u64(), tel1["enviado_bs"].as_f64()), (Some(1), Some(1.5)));
    assert_eq!(tel1["saldo"]["tipo"], "dealer");
    assert_eq!(tel1["saldo"]["bs"].as_f64(), Some(8.5));
    assert!(tel1["ms_promedio"].is_u64());
    let tel2 = c.carriles.iter().find(|x| x.dispositivo == "tel2" && x.slot == 0).unwrap();
    assert_eq!((tel2.ok, tel2.enviado_bs, tel2.estado.as_str()), (2, 2.5, "libre"));
    assert_eq!(b.lotes.carriles(999).unwrap_err().http, 404);
}

#[test]
fn un_origen_viejo_guardado_se_migra_y_su_saldo_inicial_es_dealer() {
    let a = Arc::new(almacen::Almacen::en_memoria().unwrap());
    let b = banco_con(a.clone(), &[]);
    let l = b.lotes.crear("viejo", "numero,monto,operador,accion\n69869869,1.50,TIGO,recargar\n", true, None, false).unwrap();
    // Como lo dejaba la versión anterior: origen viejo y saldo inicial sin tipo.
    a.con(|c| {
        c.execute(
            "UPDATE lotes SET origen = '{\"dispositivo\":\"tel1\",\"slot\":0,\"receta\":\"tigo-dealer-recargar\",\"operador\":\"TIGO\",\"numero\":\"78815232\"}'",
            [],
        )?;
        c.execute_batch(
            "DROP TABLE lote_saldo_inicial;
             CREATE TABLE lote_saldo_inicial (lote_id INTEGER NOT NULL, dispositivo TEXT NOT NULL, slot INTEGER NOT NULL,
                saldo REAL NOT NULL, ts_ms INTEGER NOT NULL, PRIMARY KEY (lote_id, dispositivo, slot));",
        )?;
        c.execute("INSERT INTO lote_saldo_inicial VALUES (?1, 'tel1', 0, 10.0, 5)", [l.id])
    })
    .unwrap();
    let b2 = banco_con(a.clone(), &[]);
    let o = b2.lotes.obtener(l.id).unwrap().origen.unwrap();
    assert_eq!((o.dispositivo.as_deref(), o.receta.as_deref(), o.accion.as_deref()), (Some("tel1"), Some("tigo-dealer-recargar"), Some("recargar")));
    assert_eq!(o.chips.len(), 1);
    assert_eq!((o.chips[0].dispositivo.as_str(), o.chips[0].numero.as_deref()), ("tel1", Some("78815232")));
    let base = b2.lotes.ctx.pers.saldos_iniciales(l.id).unwrap();
    assert_eq!(base.len(), 1);
    assert_eq!((base[0].tipo.as_str(), base[0].saldo), ("dealer", 10.0));
    // Y quedó guardado con la forma nueva (una tercera apertura no vuelve a migrar nada).
    let json: String = a.con(|c| c.query_row("SELECT origen FROM lotes WHERE id = ?1", [l.id], |r| r.get(0))).unwrap();
    assert!(json.contains("\"chips\"") && json.contains("\"accion\":\"recargar\""), "{json}");
    banco_con(a, &[]);
}

#[tokio::test]
async fn ejecutar_normaliza_el_monto_y_monto_entero_con_centavos_es_400() {
    let t = Telefono::new(vec![LineaSimulada::tigo("78815232", 30.0)]);
    let b = banco(&[("tel1", t.clone())]);
    let p = |m: &str| -> BTreeMap<String, String> {
        [("numero", "69869869"), ("monto", m)].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    };
    // "5,00" llega como 5: el historial guarda el canónico.
    let r = b.lotes.ctx.recetas.read().unwrap()["tigo-transferir"].clone();
    let (res, _) = b.lotes.ctx.ejecutar_receta(&r, "tel1", 0, &p("5,00"), true).await.unwrap();
    assert_eq!(res.estado, protocolo::EstadoResultado::Simulado, "{res:?}");
    assert_eq!(b.lotes.ctx.almacen.ejecuciones(1).unwrap()[0]["parametros"]["monto"], "5");
    // Un parámetro inválido sigue siendo un resultado `error` (sin marcar nada), como antes.
    let (res, _) = b.lotes.ctx.ejecutar_receta(&r, "tel1", 0, &p("3"), true).await.unwrap();
    assert_eq!(res.estado, protocolo::EstadoResultado::Error);
    // {monto_entero} con centavos: no corre, 400 parametro.
    let y = "nombre: e\noperador: TIGO\nparametros:\n  - { nombre: numero }\n  - { nombre: monto }\npasos:\n  - marcar: '*1*{numero}*{monto_entero}#'\n";
    let r = Receta::desde_yaml(y).unwrap();
    let antes = t.marcados.lock().unwrap().len();
    let e = b.lotes.ctx.ejecutar_receta(&r, "tel1", 0, &p("1.50"), false).await.unwrap_err();
    assert_eq!(e.codigo, codigos::PARAMETRO);
    assert!(e.detalle.contains("solo acepta montos enteros"), "{e:?}");
    assert_eq!(t.marcados.lock().unwrap().len(), antes, "no marcó nada");
}

/// Transferir sin `por_dia`: sin límite en el resumen y en los chips, sin exponer el cupo "infinito" interno.
#[tokio::test]
async fn transferir_sin_por_dia_es_sin_limite() {
    let t = Telefono::new(vec![LineaSimulada::tigo("78815232", 30.0)]);
    let b = banco(&[("tel1", t)]);
    b.lotes.ctx.fijar_saldo("tel1", 0, 30.0, None, None);
    let yaml = include_str!("../../../recetas/tigo-transferir.yaml").replace("  por_dia: 5\n", "");
    let r = Receta::desde_yaml(&yaml).unwrap();
    assert!(r.limites.as_ref().and_then(|l| l.por_dia).is_none());
    b.lotes.ctx.recetas.write().unwrap().insert(r.nombre.clone(), r);
    let v = b.lotes.validar("numero,monto,operador\n69869869,5,TIGO\n", false, None).await.unwrap();
    assert_eq!(v.resumen.planificados, 1);
    let o = &v.resumen.operadores[0];
    assert_eq!((o.accion.as_str(), o.cupo_hoy, o.cupo_sin_limite), ("transferir", 0, true));
    assert_eq!((v.chips[0].sin_limite, v.chips[0].cupo_hoy), (true, 0));
    assert!(!serde_json::to_string(&v).unwrap().contains("1000"));
}

// ───────────────────────────── PDA → dealer (compatibilidad) ─────────────────────────────

#[test]
fn tipo_pda_se_acepta_y_se_guarda_como_dealer() {
    let b = banco(&[("tel1", telefono_dealer())]);
    assert_eq!(b.lotes.ctx.fijar_tipo("tel1", 0, "pda").unwrap(), "normal");
    assert_eq!(chip_de(&b, 0).tipo, "dealer");
    // Volver a marcarlo `dealer` no cambia nada: ya lo era.
    assert_eq!(b.lotes.ctx.fijar_tipo("tel1", 0, " PDA ").unwrap(), "dealer");
    assert!(b.lotes.ctx.fijar_tipo("tel1", 0, "otro").is_err());
    // Un origen pedido (o guardado) con `tipo: pda` se lee como `dealer`.
    let o: OrigenLote = serde_json::from_str(r#"{"chips":[{"dispositivo":"tel1","slot":0,"tipo":"pda"}],"accion":"recargar"}"#).unwrap();
    assert_eq!(o.chips[0].tipo.as_deref(), Some("dealer"));
    let v: Verificacion = serde_json::from_str(
        r#"{"lote_id":1,"verificado_ms":0,"saldo":"pda","completo":true,"motivos":[],"cuadra":true,"confirmado":true,
            "chips":[{"dispositivo":"tel1","slot":0,"numero":null,"saldo":"pda","estado":"cuadra","saldo_inicial":1.0,
            "saldo_actual":1.0,"descontado_esperado":0.0,"descontado_real":0.0,"diferencia_bs":0.0,"envios_ok":0}],"avisos":[]}"#,
    )
    .unwrap();
    assert_eq!((v.saldo.as_str(), v.chips[0].saldo.as_str()), ("dealer", "dealer"));
    // ChipInfo con los campos viejos.
    let c: ChipInfo = serde_json::from_str(
        r#"{"dispositivo":"tel1","slot":0,"operador":"TIGO","numero":null,"saldo":null,"saldo_ms":null,"enviados_hoy":0,
            "cupo_hoy":0,"tipo":"pda","saldo_pda":3.5,"saldo_pda_ms":7}"#,
    )
    .unwrap();
    assert_eq!((c.tipo.as_str(), c.saldo_dealer, c.saldo_dealer_ms), ("dealer", Some(3.5), Some(7)));
    let json = serde_json::to_value(&c).unwrap();
    assert!(json.get("saldo_pda").is_none() && json["saldo_dealer"] == 3.5, "{json}");
}

#[tokio::test]
async fn lote_de_un_chip_con_el_nombre_anterior_de_la_receta() {
    let (b, t) = banco_dealer();
    // Un pedido (o un lote guardado) con el nombre viejo corre la receta renombrada.
    let viejo = OrigenLote::un_chip("tel1", 0, "tigo-pda-recargar");
    let v = b.lotes.validar(RECARGAS, false, Some(&viejo)).await.unwrap();
    assert_eq!(v.resumen.invalidas, 0, "{v:#?}");
    assert!(v.filas.iter().all(|f| f.receta.as_deref() == Some("tigo-dealer-recargar")), "{v:#?}");
    assert_eq!(v.origen.as_ref().unwrap().receta.as_deref(), Some("tigo-dealer-recargar"));
    let lote = b.lotes.crear("recargas", RECARGAS, false, Some(&viejo), false).unwrap();
    // Como quedaba un lote de antes del cambio: filas y origen con el nombre viejo.
    b.lotes.ctx.almacen.con(|c| {
        c.execute("UPDATE lote_filas SET receta = 'tigo-pda-recargar' WHERE lote_id = ?1", [lote.id])?;
        c.execute("UPDATE lotes SET origen = replace(origen, 'tigo-dealer-recargar', 'tigo-pda-recargar') WHERE id = ?1", [lote.id])
    })
    .unwrap();
    let c = b.lotes.carriles(lote.id).unwrap();
    assert_eq!(c.carriles.iter().find(|x| x.slot == 0).unwrap().receta.as_deref(), Some("tigo-dealer-recargar"));
    b.lotes.iniciar(lote.id).unwrap();
    let fin = esperar_fin(&b.lotes, lote.id).await;
    assert!(filas(&fin).iter().all(|f| f.estado == EstadoFila::Ok), "{fin:#?}");
    assert!((t.linea(0).saldo_dealer - 6.5).abs() < 1e-9);
    assert!(recetas_ejecutadas(&b).iter().filter(|r| r.contains("recargar")).all(|r| r == "tigo-dealer-recargar"));
}

#[tokio::test]
async fn el_cupo_de_hoy_cuenta_los_envios_hechos_con_el_nombre_anterior() {
    let (b, _) = banco_dealer();
    let yaml = include_str!("../../../recetas/tigo-dealer-recargar.yaml")
        .replace("chip_tipo: dealer\n", "chip_tipo: dealer\nlimites:\n  por_dia: 3\n");
    let r = Receta::desde_yaml(&yaml).unwrap();
    b.lotes.ctx.recetas.write().unwrap().insert(r.nombre.clone(), r);
    // Dos recargas reales de hoy, guardadas cuando la receta se llamaba `tigo-pda-recargar`.
    let ahora = protocolo::ahora_ms();
    for _ in 0..2 {
        let id = b
            .lotes
            .ctx
            .almacen
            .iniciar_ejecucion(ahora, "tigo-pda-recargar", "tel1", 0, false, &BTreeMap::new(), None)
            .unwrap();
        b.lotes.ctx.almacen.con(|c| c.execute("UPDATE ejecuciones SET estado = 'ok', ok = 1 WHERE id = ?1", [id])).unwrap();
    }
    let v = b.lotes.validar(RECARGAS, false, Some(&origen(0))).await.unwrap();
    let chip = v.chips.iter().find(|c| c.slot == 0 && c.accion.as_deref() == Some("recargar")).unwrap();
    assert_eq!((chip.enviados_hoy, chip.cupo_hoy), (2, 1), "{chip:#?}");
}

#[test]
fn una_base_con_pda_se_migra_a_dealer_sin_perder_el_pin() {
    let a = Arc::new(almacen::Almacen::en_memoria().unwrap());
    // Como la dejaba la versión anterior (antes de "dealer").
    a.con(|c| {
        c.execute_batch(
            r#"CREATE TABLE chips (id INTEGER PRIMARY KEY AUTOINCREMENT, iccid TEXT, numero TEXT, visto_ms INTEGER NOT NULL,
                   tipo TEXT NOT NULL DEFAULT 'normal', pin TEXT, saldo_pda REAL, saldo_pda_ms INTEGER);
               INSERT INTO chips (iccid, numero, visto_ms, tipo, pin, saldo_pda, saldo_pda_ms) VALUES
                   (NULL, '77716151', 1, 'pda', '4321', 12.82, 1793750854610),
                   ('8959', '78815232', 1, 'normal', NULL, NULL, NULL);
               CREATE TABLE lotes (id INTEGER PRIMARY KEY, nombre TEXT NOT NULL, creado_ms INTEGER NOT NULL, estado TEXT NOT NULL,
                   simular INTEGER NOT NULL, motivo_pausa TEXT, origen TEXT, progreso TEXT,
                   problemas INTEGER NOT NULL DEFAULT 0, archivado INTEGER NOT NULL DEFAULT 0, hash TEXT);
               INSERT INTO lotes (id, nombre, creado_ms, estado, simular, origen, progreso) VALUES (6, '1-200', 0, 'terminado', 0,
                   '{"chips":[{"dispositivo":"23cc","slot":0,"operador":"TIGO","numero":"+59177716151","tipo":"pda","receta":"tigo-pda-recargar"}],"accion":"recargar","dispositivo":"23cc","slot":0,"receta":"tigo-pda-recargar"}',
                   '{"filas":0,"ok":0,"simulado":0,"parcial":0,"fallidas":0,"inciertas":0,"pendientes":0,"sin_capacidad":0,"omitidas":0,"enviado_bs":0.0,"comision_bs":0.0,"total_bs":0.0}');
               CREATE TABLE lote_saldo_inicial (lote_id INTEGER NOT NULL, dispositivo TEXT NOT NULL, slot INTEGER NOT NULL,
                   tipo TEXT NOT NULL DEFAULT 'principal', saldo REAL NOT NULL, ts_ms INTEGER NOT NULL,
                   PRIMARY KEY (lote_id, dispositivo, slot, tipo));
               INSERT INTO lote_saldo_inicial VALUES (6, '23cc', 0, 'pda', 61.42, 5), (6, '23cc', 1, 'principal', 3.0, 5);
               CREATE TABLE lote_verificacion (lote_id INTEGER PRIMARY KEY, ts_ms INTEGER NOT NULL, json TEXT NOT NULL);
               INSERT INTO lote_verificacion VALUES (6, 0,
                   '{"lote_id":6,"verificado_ms":0,"saldo":"pda","completo":true,"motivos":[],"cuadra":false,"confirmado":false,"chips":[{"dispositivo":"23cc","slot":0,"numero":"+59177716151","saldo":"pda","estado":"diferencia","saldo_inicial":61.42,"saldo_actual":23.82,"descontado_esperado":40.0,"descontado_real":37.6,"diferencia_bs":-2.4,"envios_ok":200}],"avisos":["ENTEL: no hay receta de saldo PDA para ENTEL"]}');
               INSERT INTO eventos (ts_ms, tipo, datos) VALUES
                   (1, 'chip_tipo', '{"anterior":"normal","dispositivo_id":"23cc","slot":0,"tipo":"pda"}'),
                   (2, 'receta', '{"fase":"inicio","receta":"tigo-pda-recargar"}');"#,
        )
    })
    .unwrap();
    let revisar = |p: &persistencia::Persistencia| {
        // El PIN y el resto del chip, intactos; el tipo y el saldo, con el nombre nuevo.
        assert_eq!(p.pin(1).unwrap().as_deref(), Some("4321"));
        assert_eq!(p.tipos().unwrap().get(&1).map(String::as_str), Some("dealer"));
        assert_eq!(p.tipos().unwrap().get(&2).map(String::as_str), Some("normal"));
        let d = &p.datos_chips().unwrap()[&1];
        assert_eq!((d.tiene_pin, d.saldo_dealer, d.saldo_dealer_ms), (true, Some(12.82), Some(1793750854610)));
        assert_eq!(p.saldos_dealer().unwrap()[&1], (12.82, 1793750854610));
        let numero: String = a.con(|c| c.query_row("SELECT numero FROM chips WHERE id = 1", [], |r| r.get(0))).unwrap();
        assert_eq!(numero, "77716151");
        let columnas: Vec<String> = a
            .con(|c| c.prepare("SELECT name FROM pragma_table_info('chips')")?.query_map([], |r| r.get(0))?.collect())
            .unwrap();
        assert!(!columnas.iter().any(|c| c.contains("pda")), "{columnas:?}");
        // Saldo inicial.
        let base = p.saldos_iniciales(6).unwrap();
        let tipos: Vec<(u32, &str)> = base.iter().map(|b| (b.slot, b.tipo.as_str())).collect();
        assert_eq!(tipos, [(0, "dealer"), (1, "principal")]);
        // JSON del lote: tipo y saldo en `dealer`, el nombre de la receta tal cual (se resuelve por nombres_anteriores).
        let (lote, _) = p.lote(6).unwrap().unwrap();
        let o = lote.origen.unwrap();
        assert_eq!((o.chips[0].tipo.as_deref(), o.chips[0].receta.as_deref()), (Some("dealer"), Some("tigo-pda-recargar")));
        let v = lote.verificacion.unwrap();
        assert_eq!((v.saldo.as_str(), v.chips[0].saldo.as_str()), ("dealer", "dealer"));
        assert_eq!(v.avisos, ["ENTEL: no hay receta de saldo dealer para ENTEL"]);
        let crudo: String = a.con(|c| c.query_row("SELECT origen || json FROM lotes, lote_verificacion", [], |r| r.get(0))).unwrap();
        assert!(!crudo.contains("\"pda\"") && crudo.contains("tigo-pda-recargar"), "{crudo}");
        // Eventos: solo los de tipo de chip.
        let ev: Vec<String> = a
            .con(|c| c.prepare("SELECT datos FROM eventos ORDER BY ts_ms")?.query_map([], |r| r.get(0))?.collect())
            .unwrap();
        assert!(ev[0].contains("\"tipo\":\"dealer\"") && ev[1].contains("tigo-pda-recargar"), "{ev:?}");
    };
    let p = persistencia::Persistencia::new(a.clone()).unwrap();
    revisar(&p);
    // Idempotente: abrir otra vez no cambia nada.
    let p = persistencia::Persistencia::new(a.clone()).unwrap();
    revisar(&p);
}
