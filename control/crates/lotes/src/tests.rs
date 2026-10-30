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

const RECETAS: [&str; 4] = [
    include_str!("../../../recetas/tigo-transferir.yaml"),
    include_str!("../../../recetas/tigo-saldo.yaml"),
    include_str!("../../../recetas/tigo-movimientos.yaml"),
    include_str!("../../../recetas/entel-transferencias.yaml"),
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
    demora: Duration,
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
            demora,
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
        if self.sesion.lock().unwrap().is_some() {
            return Err(ErrorCanal::nuevo(codigos::OCUPADO, "ya hay una sesión USSD abierta"));
        }
        let mut ls = self.lineas.lock().unwrap();
        let l = ls.get_mut(slot as usize).ok_or_else(|| ErrorCanal::nuevo(codigos::SIM_INVALIDA, "no hay chip"))?;
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
        let sims = t.lineas.lock().unwrap().iter().enumerate().map(|(i, l)| sim(i as u32, &l.linea)).collect();
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
    let v = b.lotes.validar(csv, false).await.unwrap();
    let f = |linea: u32| v.filas.iter().find(|x| x.fila == linea).unwrap().clone();

    let f2 = f(2);
    assert_eq!(f2.numero.as_deref(), Some("69869869"));
    assert_eq!(f2.monto, Some(12));
    assert_eq!(f2.envios, vec![10, 2]);
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
    assert_eq!(f7.envios, vec![20; 7] .into_iter().chain([10]).collect::<Vec<_>>());
    assert!(f(8).errores.iter().any(|e| e.contains("operador desconocido")));

    let r = &v.resumen;
    assert_eq!(r.filas, 7);
    assert_eq!(r.invalidas, 4);
    assert_eq!(r.validas, 3);
    assert_eq!(r.total_bs, 12 + 1 + 150);
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
async fn csv_sin_columnas_es_error() {
    let b = banco(&[]);
    let e = b.lotes.validar("numero,monto\n69869869,5\n", false).await.unwrap_err();
    assert_eq!((e.http, e.codigo), (400, codigos::PARAMETRO));
    assert!(e.detalle.contains("operador"));
}

#[tokio::test]
async fn validar_consulta_saldos() {
    let t = Telefono::new(vec![LineaSimulada::tigo("78815232", 14.0), LineaSimulada::tigo("69759675", 3.0)]);
    let b = banco(&[("tel1", t)]);
    let v = b.lotes.validar("numero,monto,operador\n69869869,5,TIGO\n", true).await.unwrap();
    let saldos: Vec<_> = v.chips.iter().map(|c| c.saldo).collect();
    assert_eq!(saldos, vec![Some(14.0), Some(3.0)]);
    assert_eq!(v.filas[0].plan[0].slot, 0);
    // Un dato fresco no se vuelve a consultar: la consulta siguiente no cambia la hora.
    let t0 = v.chips[0].saldo_ms;
    let v2 = b.lotes.validar("numero,monto,operador\n69869869,5,TIGO\n", true).await.unwrap();
    assert_eq!(v2.chips[0].saldo_ms, t0);
}

// ───────────────────────────── ejecución ─────────────────────────────

#[tokio::test]
async fn lote_real_completo() {
    let t = Telefono::new(vec![LineaSimulada::tigo("78815232", 30.0), LineaSimulada::tigo("69759675", 3.0)]);
    let b = banco(&[("tel1", t.clone())]);
    b.lotes.ctx.fijar_saldo("tel1", 0, 30.0, None, None);
    b.lotes.ctx.fijar_saldo("tel1", 1, 3.0, None, None);
    let lote = b.lotes.crear("prueba", "numero,monto,operador,referencia\n69869869,12,TIGO,A\n70000000,1,TIGO,B\n123,5,TIGO,C\n", false).unwrap();
    assert_eq!(lote.estado, EstadoLote::Listo);
    assert_eq!(lote.progreso.omitidas, 1);
    b.lotes.iniciar(lote.id).unwrap();
    let fin = esperar_fin(&b.lotes, lote.id).await;
    assert_eq!(fin.estado, EstadoLote::Terminado, "{fin:?}");
    let fs = filas(&fin);
    assert_eq!(fs[0].estado, EstadoFila::Ok);
    assert_eq!(fs[0].enviado_bs, 12);
    assert_eq!(fs[1].estado, EstadoFila::Ok);
    assert_eq!(fs[2].estado, EstadoFila::Omitida);
    assert_eq!(fin.progreso.enviado_bs, 13);
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
    let lote = b.lotes.crear("", "69869869;12;TIGO\n70000000;5;TIGO\n", true).unwrap();
    assert!(lote.simular);
    b.lotes.iniciar(lote.id).unwrap();
    let fin = esperar_fin(&b.lotes, lote.id).await;
    assert_eq!(fin.estado, EstadoLote::Terminado);
    assert!(filas(&fin).iter().all(|f| f.estado == EstadoFila::Simulado));
    assert_eq!(fin.progreso.simulado, 2);
    assert_eq!(fin.progreso.enviado_bs, 0);
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
    let lote = b.lotes.crear("limite", csv, false).unwrap();
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
    let lote = b.lotes.crear("saldo", "numero,monto,operador\n69869869,10,TIGO\n", false).unwrap();
    b.lotes.iniciar(lote.id).unwrap();
    let fin = esperar_fin(&b.lotes, lote.id).await;
    assert_eq!(filas(&fin)[0].estado, EstadoFila::Ok, "{fin:?}");
    assert_eq!(t.linea(1).transferencias_hoy, 1);
    assert_eq!(b.lotes.ctx.saldo("tel1", 0).map(|s| s.0), Some(0.0));
}

#[tokio::test]
async fn incierto_pausa_el_lote_y_se_puede_reanudar() {
    let t = Telefono::new(vec![LineaSimulada::tigo("78815232", 100.0)]);
    t.timeout_tras_confirmar.store(true, Ordering::SeqCst);
    let b = banco(&[("tel1", t.clone())]);
    let lote = b.lotes.crear("incierto", "numero,monto,operador\n69869869,5,TIGO\n70000000,5,TIGO\n", false).unwrap();
    b.lotes.iniciar(lote.id).unwrap();
    let fin = esperar_fin(&b.lotes, lote.id).await;
    assert_eq!(fin.estado, EstadoLote::Pausado, "{fin:?}");
    assert!(fin.motivo_pausa.as_deref().unwrap().contains("incierto"));
    let fs = filas(&fin);
    assert_eq!(fs[0].estado, EstadoFila::Incierta);
    assert_eq!(fs[1].estado, EstadoFila::Pendiente);
    // La transferencia se aplicó en el operador pero el envío no se repitió.
    assert_eq!(t.linea(0).transferencias_hoy, 1);

    // Revisado el saldo, se reanuda: sigue con lo pendiente y nunca reintenta el incierto.
    t.timeout_tras_confirmar.store(false, Ordering::SeqCst);
    b.lotes.iniciar(lote.id).unwrap();
    let fin = esperar_fin(&b.lotes, lote.id).await;
    assert_eq!(fin.estado, EstadoLote::Terminado);
    let fs = filas(&fin);
    assert_eq!((fs[0].estado, fs[1].estado), (EstadoFila::Incierta, EstadoFila::Ok));
    assert_eq!(t.linea(0).transferencias_hoy, 2);
}

#[tokio::test]
async fn cancelar_termina_lo_que_esta_en_vuelo() {
    let t = Telefono::con_demora(vec![LineaSimulada::tigo("78815232", 100.0)], Duration::from_millis(15));
    let b = banco(&[("tel1", t.clone())]);
    let csv = "numero,monto,operador\n69869869,1,TIGO\n69869869,1,TIGO\n69869869,1,TIGO\n";
    let lote = b.lotes.crear("cancelar", csv, false).unwrap();
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
    let lote = b.lotes.crear("pausa", csv, false).unwrap();
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
    let lote = b.lotes.crear("sin chips", "numero,monto,operador\n69869869,5,TIGO\n", false).unwrap();
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
    let lote = b.lotes.crear("corte", "numero,monto,operador\n69869869,5,TIGO\n70000000,1,TIGO\n", false).unwrap();
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
    let a = b.lotes.crear("a", "69869869;1;TIGO\n", false).unwrap();
    let c = b.lotes.crear("c", "69869869;1;TIGO\n", false).unwrap();
    b.lotes.iniciar(a.id).unwrap();
    assert_eq!(b.lotes.iniciar(a.id).unwrap_err().codigo, "en_curso");
    assert_eq!(b.lotes.iniciar(c.id).unwrap_err().codigo, "otro_lote");
    esperar_fin(&b.lotes, a.id).await;
    let lista = b.lotes.listar().unwrap();
    assert_eq!(lista.len(), 2);
    assert!(lista.iter().all(|l| l.filas.is_none()));
}

#[tokio::test]
async fn reparte_en_paralelo_entre_telefonos() {
    let demora = Duration::from_millis(40);
    let t1 = Telefono::con_demora(vec![LineaSimulada::tigo("78815232", 50.0)], demora);
    let t2 = Telefono::con_demora(vec![LineaSimulada::tigo("69759675", 50.0)], demora);
    let b = banco(&[("tel1", t1.clone()), ("tel2", t2.clone())]);
    let csv = "numero,monto,operador\n69869869,1,TIGO\n69869869,1,TIGO\n69869869,1,TIGO\n69869869,1,TIGO\n";
    let lote = b.lotes.crear("paralelo", csv, false).unwrap();
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
    let lote = b.lotes.crear("fallo", "numero,monto,operador\n69869869,5,TIGO\n70000000,5,TIGO\n", false).unwrap();
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
    let re = b.lotes.reintentar_fallidas(lote.id).unwrap();
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
    assert_eq!(b.lotes.reintentar_fallidas(lote.id).unwrap_err().codigo, "nada_que_reintentar");
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
async fn reintentar_fallidas_no_toca_inciertos_ni_lotes_en_curso() {
    let t = Telefono::con_demora(vec![LineaSimulada::tigo("78815232", 100.0)], Duration::from_millis(15));
    t.timeout_tras_confirmar.store(true, Ordering::SeqCst);
    let b = banco(&[("tel1", t.clone())]);
    let lote = b.lotes.crear("incierto", "numero,monto,operador\n69869869,5,TIGO\n70000000,5,TIGO\n", false).unwrap();
    b.lotes.iniciar(lote.id).unwrap();
    assert_eq!(b.lotes.reintentar_fallidas(lote.id).unwrap_err().codigo, "en_curso");
    let fin = esperar_fin(&b.lotes, lote.id).await;
    assert_eq!(fin.estado, EstadoLote::Pausado);
    assert!(!fin.revision.completo);
    assert!(fin.revision.motivos.iter().any(|m| m == "1 fila incierta"), "{:?}", fin.revision);
    assert_eq!(b.lotes.reintentar_fallidas(lote.id).unwrap_err().codigo, "hay_inciertos");
    assert_eq!(b.lotes.reintentar_fallidas(999).unwrap_err().codigo, "sin_lote");
    b.lotes.cancelar(lote.id).unwrap();
    assert_eq!(b.lotes.reintentar_fallidas(lote.id).unwrap_err().codigo, "cancelado");
}

#[tokio::test]
async fn verificar_cuadra_y_detecta_diferencias() {
    let t = Telefono::new(vec![LineaSimulada::tigo("78815232", 50.0)]);
    let b = banco(&[("tel1", t.clone())]);
    let lote = b.lotes.crear("cuadre", "numero,monto,operador\n69869869,12,TIGO\n", false).unwrap();
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
    assert_eq!(b.lotes.listar().unwrap()[0].verificacion, Some(v));

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
    let lote = b.lotes.crear("sim", "numero,monto,operador\n69869869,5,TIGO\n", true).unwrap();
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
    let lote = b.lotes.crear("uno", "numero,monto,operador\n69869869,5,TIGO\n", false).unwrap();
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

    let v = b.lotes.validar("numero,monto,operador\n69869869,5,TIGO\n", false).await.unwrap();
    assert_eq!(v.filas[0].plan[0].slot, 1, "el chip nuevo no recibe envíos");

    // Con los dos cambiados no queda con qué enviar, y el motivo lo dice.
    poner_chips(&b, "tel1", vec![sim_con(0, "70000001", None), sim_con(1, "70000002", None)]);
    let v = b.lotes.validar("numero,monto,operador\n69869869,5,TIGO\n", false).await.unwrap();
    assert!(v.filas[0].plan.is_empty(), "{:?}", v.filas[0]);
    assert!(v.filas[0].avisos.iter().chain(&v.filas[0].errores).any(|m| m.contains("saldo verificado")), "{:?}", v.filas[0]);

    // Consultando los saldos, entran.
    let v = b.lotes.validar("numero,monto,operador\n69869869,5,TIGO\n", true).await.unwrap();
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

// ───────────── chips PDA ─────────────

/// Receta PDA de transferir: la misma pantalla de Tigo, con su propio nombre y un cupo de 2 por día.
fn poner_receta_pda(b: &Banco) {
    let yaml = include_str!("../../../recetas/tigo-transferir.yaml")
        .replace("nombre: tigo-transferir", "nombre: tigo-pda-transferir\nchip_tipo: pda")
        .replace("por_dia: 5", "por_dia: 2");
    let r = Receta::desde_yaml(&yaml).unwrap();
    b.lotes.ctx.recetas.write().unwrap().insert(r.nombre.clone(), r);
}

fn telefono_pda() -> Arc<Telefono> {
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
    let b = banco(&[("tel1", telefono_pda())]);
    assert_eq!(chip_de(&b, 0).tipo, "normal");
    assert_eq!(b.lotes.ctx.fijar_tipo("tel1", 0, "pda").unwrap(), "normal");
    assert_eq!(chip_de(&b, 0).tipo, "pda");
    assert_eq!(chip_de(&b, 1).tipo, "normal");
    assert!(b.lotes.ctx.fijar_tipo("tel1", 0, "raro").is_err());
    assert!(b.lotes.ctx.fijar_tipo("tel1", 7, "pda").is_err(), "no hay chip en esa ranura");
}

#[tokio::test]
async fn el_tipo_sigue_al_chip_no_a_la_ranura() {
    let b = banco(&[("tel1", telefono_pda())]);
    b.lotes.ctx.fijar_tipo("tel1", 0, "pda").unwrap();
    // El chip A (78815232) pasa a la ranura 1 y en la 0 entra uno nuevo.
    poner_chips(&b, "tel1", vec![sim_con(0, "70000001", None), sim(1, "78815232")]);
    assert_eq!(chip_de(&b, 0).tipo, "normal", "el chip nuevo no hereda el tipo de la ranura");
    assert_eq!(chip_de(&b, 1).tipo, "pda", "el chip A sigue siendo PDA en su nueva ranura");
}

#[tokio::test]
async fn chip_pda_usa_la_receta_pda_con_su_propio_cupo() {
    let b = banco(&[("tel1", telefono_pda())]);
    poner_receta_pda(&b);
    b.lotes.ctx.fijar_tipo("tel1", 0, "pda").unwrap();
    assert_eq!(chip_de(&b, 0).cupo_hoy, 2, "cupo de la receta PDA");
    assert_eq!(chip_de(&b, 1).cupo_hoy, 5, "el chip normal sigue con la de la compañía");

    // La validación solo usa la receta de la compañía.
    b.lotes.ctx.fijar_saldo("tel1", 0, 40.0, None, None);
    b.lotes.ctx.fijar_saldo("tel1", 1, 10.0, None, None);
    let v = b.lotes.validar("numero,monto,operador\n69869869,5,TIGO\n", false).await.unwrap();
    assert_eq!(v.filas[0].receta.as_deref(), Some("tigo-transferir"));

    // El envío sale por el chip de más saldo (el PDA) y corre su receta.
    let lote = b.lotes.crear("pda", "numero,monto,operador\n69869869,5,TIGO\n", false).unwrap();
    b.lotes.iniciar(lote.id).unwrap();
    let fin = esperar_fin(&b.lotes, lote.id).await;
    assert_eq!(filas(&fin)[0].estado, EstadoFila::Ok, "{fin:?}");
    assert_eq!(recetas_ejecutadas(&b), vec!["tigo-pda-transferir"]);
    let c = chip_de(&b, 0);
    assert_eq!((c.enviados_hoy, c.cupo_hoy), (1, 1));
}

#[tokio::test]
async fn chip_pda_sin_receta_pda_usa_la_de_la_compania() {
    let b = banco(&[("tel1", telefono_pda())]);
    b.lotes.ctx.fijar_tipo("tel1", 0, "pda").unwrap();
    assert_eq!(chip_de(&b, 0).cupo_hoy, 5);
    enviar_por_ranura_0(&b).await;
    assert_eq!(recetas_ejecutadas(&b), vec!["tigo-transferir"]);
}

#[tokio::test]
async fn saldo_de_un_chip_pda_usa_su_receta_de_saldo_si_existe() {
    let b = banco(&[("tel1", telefono_pda())]);
    b.lotes.ctx.fijar_tipo("tel1", 0, "pda").unwrap();
    b.lotes.ctx.consultar_saldos(None, None).await;
    assert_eq!(recetas_ejecutadas(&b), vec!["tigo-saldo", "tigo-saldo"], "sin receta de saldo PDA, usa la de la compañía");

    let yaml = include_str!("../../../recetas/tigo-saldo.yaml").replace("nombre: tigo-saldo", "nombre: tigo-pda-saldo\nchip_tipo: pda");
    let r = Receta::desde_yaml(&yaml).unwrap();
    b.lotes.ctx.recetas.write().unwrap().insert(r.nombre.clone(), r);
    b.lotes.ctx.consultar_saldos(None, None).await;
    let ult: Vec<String> = recetas_ejecutadas(&b).into_iter().skip(2).collect();
    assert_eq!(ult, vec!["tigo-pda-saldo", "tigo-saldo"], "el PDA usa la suya; el normal, la de la compañía");
}

#[tokio::test]
async fn receta_efectiva_respeta_el_tipo_del_chip() {
    let b = banco(&[("tel1", telefono_pda())]);
    poner_receta_pda(&b);
    let ctx = &b.lotes.ctx;
    let compania = ctx.recetas.read().unwrap()["tigo-transferir"].clone();
    let pda = ctx.recetas.read().unwrap()["tigo-pda-transferir"].clone();
    assert_eq!(ctx.receta_efectiva(&compania, "pda").unwrap().nombre, "tigo-pda-transferir");
    assert_eq!(ctx.receta_efectiva(&compania, "normal").unwrap().nombre, "tigo-transferir");
    assert_eq!(ctx.receta_efectiva(&pda, "pda").unwrap().nombre, "tigo-pda-transferir");
    assert!(ctx.receta_efectiva(&pda, "normal").is_err(), "una receta PDA no corre en un chip normal");
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
    assert_eq!(p.fijar_tipo(1, "pda").unwrap(), "normal");
    assert_eq!(p.tipos().unwrap().get(&1).map(String::as_str), Some("pda"));
}
