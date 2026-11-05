//! Probar un paso de una receta contra una pantalla, sin marcar nada (`POST /api/recetas/probar-paso`).
//! Usa las mismas reglas del motor: [`menu::marcar_opciones`] para elegir/confirmar, [`motor::coincide`] para
//! esperar/error_si/exito/verificar y [`motor::extraer_salida`] para la salida.

use crate::menu;
use crate::motor::{coincide, extraer_salida};
use crate::receta::{sustituir, tapar, Accion, Parametro, Receta};
use regex::Regex;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct OpcionProbada {
    pub numero: String,
    pub texto: String,
    pub coincide: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Prueba {
    pub accion: String,
    /// Lo que el motor contestaría (secretos tapados). `None` si no hay opción que coincida o el paso no envía nada.
    pub enviaria: Option<String>,
    /// Opciones del menú (pantalla de antes) para `elegir`/`confirmar`; vacío en los demás pasos.
    pub opciones: Vec<OpcionProbada>,
    pub esperar: Option<bool>,
    pub error_si: Option<bool>,
    pub exito: Option<bool>,
    pub verificar: Option<bool>,
    pub salida: BTreeMap<String, String>,
    /// `seguiria` · `exito` · `abortaria` · `incierto` (después del paso que mueve dinero).
    pub resultado: String,
    pub motivo: Option<String>,
    /// Parámetros usados, con los que faltaban completados con un ejemplo (secretos tapados).
    pub parametros: BTreeMap<String, String>,
}

/// Valor de ejemplo para un parámetro que no vino: la primera opción, o algo que cumpla su patrón.
pub fn ejemplo(p: &Parametro) -> String {
    if let Some(o) = p.opciones.as_ref().and_then(|o| o.first()) {
        return o.clone();
    }
    let por_nombre = match p.nombre.as_str() {
        "numero" => "70000000",
        "monto" => "10",
        "pin" => "1234",
        _ => "1",
    };
    let Some(pat) = &p.patron else { return por_nombre.to_string() };
    let Ok(re) = Regex::new(&format!("^(?:{pat})$")) else { return por_nombre.to_string() };
    [por_nombre, "70000000", "60000000", "10", "5", "1", "1234", "123456", "0", "a", "abc"]
        .into_iter()
        .find(|c| re.is_match(c))
        .unwrap_or(por_nombre)
        .to_string()
}

/// Los parámetros dados, más un ejemplo para cada parámetro declarado que falte.
pub fn completar_parametros(receta: &Receta, dados: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mut p: BTreeMap<String, String> =
        dados.iter().map(|(k, v)| (k.clone(), v.trim().to_string())).filter(|(_, v)| !v.is_empty()).collect();
    for q in &receta.parametros {
        p.entry(q.nombre.clone()).or_insert_with(|| ejemplo(q));
    }
    p
}

/// Evalúa el paso `n` (1-based). `texto` es la pantalla que llegó **después** del paso; `texto_antes`, la de antes
/// (para elegir/confirmar/verificar; si falta, se usa `texto`).
pub fn probar_paso(
    receta: &Receta,
    n: usize,
    texto: &str,
    texto_antes: Option<&str>,
    dados: &BTreeMap<String, String>,
) -> Result<Prueba, String> {
    let paso = receta
        .pasos
        .get(n.wrapping_sub(1))
        .ok_or_else(|| format!("la receta tiene {} pasos: no existe el paso {n}", receta.pasos.len()))?;
    let accion = paso.accion().map_err(|e| format!("paso {n}: {e}"))?;
    // El monto se normaliza (`0,5` → `0.50`) y se agregan sus valores calculados, como en el motor.
    let params = crate::motor::preparar_para_probar(receta, &completar_parametros(receta, dados))?;
    let secretos = receta.secretos(&params);
    let antes = texto_antes.unwrap_or(texto);
    // Desde el paso que mueve dinero (incluido), cualquier corte es `incierto`, no `abortaria` (como el motor).
    let confirmado = receta.paso_que_mueve_dinero().is_some_and(|m| m < n);
    let cortar = |motivo: String| -> (String, Option<String>) {
        if confirmado {
            ("incierto".into(), Some(format!("{motivo} (después de confirmar: revisa el saldo)")))
        } else {
            ("abortaria".into(), Some(motivo))
        }
    };

    let mut p = Prueba {
        accion: accion.nombre().into(),
        enviaria: None,
        opciones: vec![],
        esperar: None,
        error_si: None,
        exito: None,
        verificar: None,
        salida: BTreeMap::new(),
        resultado: "seguiria".into(),
        motivo: None,
        // Solo los declarados (los valores calculados no se muestran como parámetros).
        parametros: receta.sin_secretos(&params.iter().filter(|(k, _)| !crate::monto::es_reservado(k)).map(|(k, v)| (k.clone(), v.clone())).collect()),
    };
    let mut corte: Option<(String, Option<String>)> = None;

    match accion {
        Accion::Marcar(c) | Accion::Enviar(c) => p.enviaria = Some(tapar(&sustituir(c, &params, false), &secretos)),
        Accion::Elegir(re) | Accion::Confirmar(re) => {
            let pat = sustituir(re, &params, true);
            let marcadas = menu::marcar_opciones(antes, &pat).map_err(|e| format!("paso {n}: regex inválida: {e}"))?;
            p.enviaria = marcadas.iter().find(|(_, si)| *si).map(|(o, _)| o.numero.clone());
            p.opciones = marcadas
                .into_iter()
                .map(|(o, si)| OpcionProbada { numero: o.numero, texto: o.texto, coincide: si })
                .collect();
            if p.enviaria.is_none() {
                let hay: Vec<&str> = p.opciones.iter().map(|o| o.texto.as_str()).collect();
                let hay = if hay.is_empty() { "ninguna".to_string() } else { hay.join(" | ") };
                let motivo = format!("paso {n}: no encontré la opción /{pat}/ en el menú (opciones: {hay})");
                // No se envió nada: aunque este paso sea `confirmar`, solo cuenta un paso anterior que movió dinero.
                corte = Some(if receta.paso_que_mueve_dinero().is_some_and(|m| m + 1 < n) {
                    cortar(motivo)
                } else {
                    ("abortaria".into(), Some(motivo))
                });
            }
        }
        Accion::Verificar(re) => {
            let pat = sustituir(re, &params, true);
            let si = coincide(&pat, antes).map_err(|e| format!("paso {n}: {e}"))?;
            p.verificar = Some(si);
            if !si {
                corte = Some(cortar(format!("paso {n}: la pantalla no coincide con /{pat}/")));
            }
        }
    }

    // `verificar` no envía nada: el motor no mira esperar/error_si/exito en ese paso.
    if !matches!(accion, Accion::Verificar(_)) {
        let evaluar = |r: &Option<String>| -> Result<Option<bool>, String> {
            r.as_deref().map(|r| coincide(&sustituir(r, &params, true), texto)).transpose().map_err(|e| format!("paso {n}: {e}"))
        };
        p.error_si = evaluar(&paso.error_si)?;
        p.esperar = evaluar(&paso.esperar)?;
        p.exito = evaluar(&paso.exito)?;
        let mueve = matches!(accion, Accion::Confirmar(_)) || paso.mueve_dinero;
        if corte.is_none() {
            corte = if p.error_si == Some(true) {
                Some(cortar(format!("el operador respondió: {}", texto.trim())))
            } else if p.esperar == Some(false) {
                Some(cortar(format!(
                    "paso {n}: respuesta inesperada (esperaba /{}/)",
                    sustituir(paso.esperar.as_deref().unwrap_or(""), &params, true)
                )))
            } else if p.exito == Some(true) {
                Some(("exito".into(), None))
            } else if p.exito == Some(false) && mueve {
                Some(("incierto".into(), Some("se confirmó pero la respuesta no coincide con el éxito esperado. Revisa el saldo.".into())))
            } else {
                None
            };
        }
    }

    // Último paso sin `exito` visto: el motor termina igual que aquí.
    if corte.is_none() && n == receta.pasos.len() {
        let tiene_exito = receta.pasos.iter().any(|p| p.exito.is_some());
        corte = Some(if !tiene_exito {
            ("exito".into(), None)
        } else if receta.paso_que_mueve_dinero().is_some_and(|m| m < n) {
            ("incierto".into(), Some("se confirmó pero no apareció la respuesta de éxito. Revisa el saldo.".into()))
        } else {
            ("abortaria".into(), Some("la receta terminó sin la respuesta de éxito esperada".into()))
        });
    }
    if let Some((r, m)) = corte {
        p.resultado = r;
        p.motivo = m;
    }

    let textos: Vec<String> = texto_antes.into_iter().chain(std::iter::once(texto)).map(str::to_string).collect();
    p.salida = extraer_salida(receta, &params, &textos);
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TRANSFERIR: &str = include_str!("../../../recetas/tigo-transferir.yaml");
    const SALDO: &str = include_str!("../../../recetas/tigo-saldo.yaml");

    fn params(xs: &[(&str, &str)]) -> BTreeMap<String, String> {
        xs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    fn paso_de(r: &Receta, que: &str) -> usize {
        r.pasos
            .iter()
            .position(|p| match que {
                "elegir" => p.elegir.is_some(),
                "enviar" => p.enviar.is_some(),
                "verificar" => p.verificar.is_some(),
                "confirmar" => p.confirmar.is_some(),
                _ => p.marcar.is_some(),
            })
            .unwrap()
            + 1
    }

    #[test]
    fn marcar_con_salida_y_sin_exito_termina_bien() {
        let r = Receta::desde_yaml(SALDO).unwrap();
        let t = "Linea: 78815232\nSaldo Total: 10.50 Bs\nSaldo Principal: 10.50 Bs";
        let p = probar_paso(&r, 1, t, None, &BTreeMap::new()).unwrap();
        assert_eq!(p.enviaria.as_deref(), Some("*105#"));
        assert_eq!(p.esperar, Some(true));
        assert_eq!(p.error_si, None);
        assert_eq!(p.salida["saldo_principal"], "10.50");
        assert_eq!(p.resultado, "exito", "único paso y sin 'exito' en la receta");
        // Esperar que no coincide: abortaría.
        let p = probar_paso(&r, 1, "Servicio no disponible", None, &BTreeMap::new()).unwrap();
        assert_eq!((p.esperar, p.resultado.as_str()), (Some(false), "abortaria"));
        assert!(p.motivo.unwrap().contains("esperaba"));
        assert!(probar_paso(&r, 2, "x", None, &BTreeMap::new()).is_err());
    }

    #[test]
    fn elegir_con_parametro_de_ejemplo() {
        let r = Receta::desde_yaml(TRANSFERIR).unwrap();
        let n = r.pasos.iter().position(|p| p.elegir.as_deref().is_some_and(|e| e.contains("{monto}"))).unwrap() + 1;
        let antes = "Elija el monto\n1.-1 Bs\n2.-2 Bs\n3.-5 Bs\n4.-10 Bs\n5.-20 Bs";
        // Sin parámetros: monto toma la primera opción ("1") y numero un ejemplo que cumple el patrón.
        let p = probar_paso(&r, n, "Confirma transferir 1 Bs a 70000000?\n1.-Si\n2.-No", Some(antes), &BTreeMap::new()).unwrap();
        assert_eq!(p.parametros["monto"], "1");
        assert!(Regex::new(r"^[67]\d{7}$").unwrap().is_match(&p.parametros["numero"]));
        assert_eq!(p.enviaria.as_deref(), Some("1"));
        assert_eq!(p.opciones.len(), 5);
        assert!(p.opciones[0].coincide && !p.opciones[3].coincide);
        // Con monto 10: la opción 4.
        let p = probar_paso(&r, n, "Confirma", Some(antes), &params(&[("monto", "10"), ("numero", "69869869")])).unwrap();
        assert_eq!(p.enviaria.as_deref(), Some("4"));
        assert_eq!(p.resultado, "seguiria");
        // Monto que no está en el menú: abortaría con las opciones en el motivo.
        let p = probar_paso(&r, n, "Confirma", Some("1.-1 Bs\n2.-2 Bs"), &params(&[("monto", "50")])).unwrap();
        assert_eq!(p.enviaria, None);
        assert_eq!(p.resultado, "abortaria");
        assert!(p.motivo.unwrap().contains("1 Bs | 2 Bs"));
    }

    #[test]
    fn enviar_verificar_y_confirmar() {
        let r = Receta::desde_yaml(TRANSFERIR).unwrap();
        let ps = params(&[("numero", "69869869"), ("monto", "10")]);
        let p = probar_paso(&r, paso_de(&r, "enviar"), "Elija el monto\n1.-1 Bs", None, &ps).unwrap();
        assert_eq!(p.enviaria.as_deref(), Some("69869869"));
        assert_eq!(p.esperar, Some(true));

        let nv = paso_de(&r, "verificar");
        let bien = "Confirma transferir por unica vez 10.00 Bs a 69869869 con un costo de 0.20?\n1.-Si\n2.-No";
        let p = probar_paso(&r, nv, bien, None, &ps).unwrap();
        assert_eq!((p.verificar, p.enviaria.clone(), p.esperar), (Some(true), None, None));
        assert_eq!(p.resultado, "seguiria");
        let p = probar_paso(&r, nv, &bien.replace("69869869", "69869860"), None, &ps).unwrap();
        assert_eq!((p.verificar, p.resultado.as_str()), (Some(false), "abortaria"));

        let nc = paso_de(&r, "confirmar");
        let p = probar_paso(&r, nc, "Le ha transferido 10.00 a 69869869", Some(bien), &ps).unwrap();
        assert_eq!(p.enviaria.as_deref(), Some("1"), "el número de la opción Si, no la de No");
        assert_eq!(p.exito, Some(true));
        assert_eq!(p.resultado, "exito");
        assert_eq!(p.salida.get("costo").map(String::as_str), Some("0.20"));
        // Después de confirmar, una respuesta que no es el éxito queda incierta, nunca abortada.
        let p = probar_paso(&r, nc, "Error del sistema", Some(bien), &ps).unwrap();
        assert_eq!((p.exito, p.resultado.as_str()), (Some(false), "incierto"));
    }

    #[test]
    fn monto_canonico_y_calculados_al_probar() {
        let r = Receta::desde_yaml(include_str!("../../../recetas/entel-dealer-recargar.yaml")).unwrap();
        // Bs 0.50 va como 050; un entero va en Bs (1 → *1#, no *100#).
        let ps = params(&[("numero", "71234567"), ("monto", ".50")]);
        let p = probar_paso(&r, 1, "Esta seguro de recargar Bs. 0.50 al telefono 71234567", None, &ps).unwrap();
        assert_eq!(p.enviaria.as_deref(), Some("*133*71234567*050#"));
        assert_eq!(p.parametros["monto"], "0.50");
        assert!(!p.parametros.contains_key("monto_mixto"), "los calculados no se listan como parámetros");
        let p = probar_paso(&r, 1, "x", None, &params(&[("numero", "71234567"), ("monto", "1")])).unwrap();
        assert_eq!(p.enviaria.as_deref(), Some("*133*71234567*1#"));
        // "0,5" se normaliza antes de sustituir; el verificar compara el monto mostrado.
        let ps = params(&[("numero", "71234567"), ("monto", "0,5")]);
        let p = probar_paso(&r, 2, "Esta seguro de recargar Bs. 0.50 al telefono 71234567\n1 Si\n2 No", None, &ps).unwrap();
        assert_eq!(p.verificar, Some(true));
        let p = probar_paso(&r, 2, "Esta seguro de recargar Bs. 10.50 al telefono 71234567\n1 Si\n2 No", None, &ps).unwrap();
        assert_eq!((p.verificar, p.resultado.as_str()), (Some(false), "abortaria"));
        // {monto_entero} con centavos: no se prueba (400).
        let y = "nombre: x\nparametros:\n  - { nombre: monto }\npasos:\n  - marcar: '*1*{monto_entero}#'\n";
        let e = probar_paso(&Receta::desde_yaml(y).unwrap(), 1, "x", None, &params(&[("monto", "1.5")])).unwrap_err();
        assert!(e.contains("enteros"), "{e}");
    }

    #[test]
    fn error_si_y_secretos() {
        let y = "nombre: x\nparametros:\n  - { nombre: pin, secreto: true, patron: '\\d{4}' }\npasos:\n  - marcar: '*1#'\n    error_si: 'limite'\n  - enviar: '{pin}'\n    mueve_dinero: true\n    exito: 'listo'\n";
        let r = Receta::desde_yaml(y).unwrap();
        let p = probar_paso(&r, 1, "Superaste el LIMITE diario", None, &BTreeMap::new()).unwrap();
        assert_eq!((p.error_si, p.resultado.as_str()), (Some(true), "abortaria"));
        let p = probar_paso(&r, 2, "listo", None, &params(&[("pin", "4321")])).unwrap();
        assert_eq!(p.enviaria.as_deref(), Some(crate::receta::TAPADO));
        assert_eq!(p.parametros["pin"], crate::receta::TAPADO);
        assert_eq!(p.resultado, "exito");
    }
}
