//! Motor de recetas: ejecuta una [`Receta`] sobre cualquier [`CanalUssd`].
//!
//! Reglas de seguridad (contrato):
//! - Antes de mover dinero, cualquier sorpresa aborta (`abortado`): no se movió nada.
//! - `confirmar` nunca se reintenta. Sin respuesta clara después de confirmar → `incierto`.
//! - En `simular`, `confirmar` elige la opción "No".
//! - Al terminar, siempre se cancela la sesión si quedó abierta.

use crate::canal::CanalUssd;
use crate::menu;
use crate::monto;
use crate::receta::{sustituir, tapar, Accion, Receta};
use protocolo::{ahora_ms, EstadoResultado, PasoResultado, Resultado};
use regex::Regex;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

pub const TIMEOUT_POR_DEFECTO_MS: u64 = 30_000;

/// Valida los parámetros contra la receta. Devuelve los valores limpios (sin los valores calculados: esos los
/// agrega [`con_calculados`]). El `monto` se normaliza a Bs canónico (`0,5` → `0.50`) **antes** de mirar su
/// `patron` y sus `opciones` (que también están en Bs).
pub fn validar_parametros(
    receta: &Receta,
    parametros: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, String> {
    let mut limpios = BTreeMap::new();
    for k in parametros.keys() {
        if !receta.parametros.iter().any(|p| &p.nombre == k) {
            return Err(format!("parámetro desconocido '{k}'"));
        }
    }
    for p in &receta.parametros {
        let mut v = parametros
            .get(&p.nombre)
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .ok_or_else(|| format!("falta el parámetro '{}'", p.nombre))?;
        let es_monto = p.nombre == "monto";
        let centavos = if es_monto {
            let c = monto::centavos(&v).map_err(|e| format!("'monto': {e}"))?;
            v = monto::texto(c);
            Some(c)
        } else {
            None
        };
        if let Some(pat) = &p.patron {
            let re = Regex::new(&format!("^(?:{pat})$")).map_err(|e| e.to_string())?;
            if !re.is_match(&v) {
                return Err(format!("'{}' = '{v}' no cumple el patrón {pat}", p.nombre));
            }
        }
        if let Some(ops) = &p.opciones {
            let vale = match centavos {
                // Las opciones de `monto` están en Bs: "0.50" y "0.5" son la misma.
                Some(c) => ops.iter().any(|o| monto::centavos(o).ok() == Some(c)),
                None => ops.iter().any(|o| o == &v),
            };
            if !vale {
                return Err(format!("'{}' = '{v}' no es una opción válida ({})", p.nombre, ops.join(", ")));
            }
        }
        if let Some(e) = centavos.and_then(|c| monto::rechazo(c, |n| receta.usa_parametro(n))) {
            return Err(e);
        }
        limpios.insert(p.nombre.clone(), v);
    }
    Ok(limpios)
}

/// Agrega los valores calculados del `monto` (`monto_cent`, `monto_cent3`, `monto_entero`, `monto_mixto`) si la receta declara
/// `monto` y su valor es un monto válido. Llamar con parámetros ya validados.
pub fn con_calculados(receta: &Receta, params: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mut p = params.clone();
    if receta.parametros.iter().any(|q| q.nombre == "monto") {
        if let Some(c) = params.get("monto").and_then(|m| monto::centavos(m).ok()) {
            for (k, v) in monto::calculados(c) {
                p.insert(k.to_string(), v);
            }
        }
    }
    p
}

/// Para probar un paso: normaliza el `monto` (si la receta lo declara y vino) y agrega sus valores calculados,
/// sin exigir que cumpla `patron`/`opciones` (así se puede probar qué pasa con un monto que no está en el menú).
pub fn preparar_para_probar(receta: &Receta, params: &BTreeMap<String, String>) -> Result<BTreeMap<String, String>, String> {
    let mut p = params.clone();
    if receta.parametros.iter().any(|q| q.nombre == "monto") {
        if let Some(m) = params.get("monto") {
            let c = monto::centavos(m).map_err(|e| format!("'monto': {e}"))?;
            if let Some(e) = monto::rechazo(c, |n| receta.usa_parametro(n)) {
                return Err(e);
            }
            p.insert("monto".into(), monto::texto(c));
        }
    }
    Ok(con_calculados(receta, &p))
}

/// ¿`texto` coincide con `patron`? Sin importar mayúsculas y `.` cruza líneas (regla de esperar/error_si/exito/verificar).
pub fn coincide(patron: &str, texto: &str) -> Result<bool, String> {
    Regex::new(&format!("(?is){patron}"))
        .map(|re| re.is_match(texto))
        .map_err(|e| format!("regex inválida '{patron}': {e}"))
}

/// `salida` de la receta: cada regex sobre los textos recibidos, en orden; vale el primer grupo que aparece.
pub fn extraer_salida(receta: &Receta, params: &BTreeMap<String, String>, textos: &[String]) -> BTreeMap<String, String> {
    let mut salida = BTreeMap::new();
    for (k, r) in &receta.salida {
        if let Ok(re) = Regex::new(&sustituir(r, params, true)) {
            if let Some(v) =
                textos.iter().find_map(|t| re.captures(t).and_then(|c| c.get(1)).map(|m| m.as_str().to_string()))
            {
                salida.insert(k.clone(), v);
            }
        }
    }
    salida
}

enum Fin {
    Ok,
    Simulado(String),
    Abortado(String),
    Incierto(String),
    Error(String),
}

struct Estado {
    sesion: Option<String>,
    abierta: bool,
    texto: String,
    pasos: Vec<PasoResultado>,
    textos: Vec<String>,
    /// Ya se envió la confirmación: desde aquí nada es "abortado", todo es "incierto".
    confirmado: bool,
}

impl Estado {
    fn cortar(&self, motivo: String) -> Fin {
        if self.confirmado {
            Fin::Incierto(format!("{motivo} (después de confirmar: revisa el saldo)"))
        } else {
            Fin::Abortado(motivo)
        }
    }
    fn opciones(&self) -> String {
        let o: Vec<String> = menu::opciones(&self.texto).into_iter().map(|o| o.texto).collect();
        if o.is_empty() { "ninguna".into() } else { o.join(" | ") }
    }
}

/// Ejecuta `receta` por el chip `slot` del canal. Nunca entra en pánico: todo termina en un `Resultado`.
pub async fn ejecutar(
    receta: &Receta,
    canal: &dyn CanalUssd,
    slot: u32,
    parametros: &BTreeMap<String, String>,
    simular: bool,
) -> Resultado {
    let inicio_ms = ahora_ms();
    let t0 = Instant::now();
    let mut st = Estado {
        sesion: None,
        abierta: false,
        texto: String::new(),
        pasos: vec![],
        textos: vec![],
        confirmado: false,
    };

    let fin = match (receta.validar(), validar_parametros(receta, parametros)) {
        (Err(e), _) => Fin::Error(format!("receta inválida: {e}")),
        (_, Err(e)) => Fin::Error(e),
        (Ok(()), Ok(params)) => correr(receta, canal, slot, &con_calculados(receta, &params), simular, &mut st).await,
    };

    // Siempre cerrar la sesión si quedó abierta.
    if st.abierta {
        if let Some(s) = &st.sesion {
            if let Err(e) = canal.cancelar(s).await {
                tracing::warn!("no pude cancelar la sesión {s}: {e}");
            }
        }
    }

    let params_salida = validar_parametros(receta, parametros).map(|p| con_calculados(receta, &p)).unwrap_or_default();
    let salida = extraer_salida(receta, &params_salida, &st.textos);

    let (estado, detalle) = match fin {
        Fin::Ok => (EstadoResultado::Ok, None),
        Fin::Simulado(d) => (EstadoResultado::Simulado, Some(d)),
        Fin::Abortado(d) => (EstadoResultado::Abortado, Some(d)),
        Fin::Incierto(d) => (EstadoResultado::Incierto, Some(d)),
        Fin::Error(d) => (EstadoResultado::Error, Some(d)),
    };
    Resultado {
        ok: matches!(estado, EstadoResultado::Ok | EstadoResultado::Simulado),
        estado,
        detalle,
        pasos: st.pasos,
        salida,
        receta: receta.nombre.clone(),
        dispositivo: None,
        slot,
        simular,
        inicio_ms,
        duracion_ms: t0.elapsed().as_millis() as u64,
    }
}

async fn correr(
    receta: &Receta,
    canal: &dyn CanalUssd,
    slot: u32,
    params: &BTreeMap<String, String>,
    simular: bool,
    st: &mut Estado,
) -> Fin {
    let tiene_exito = receta.pasos.iter().any(|p| p.exito.is_some());
    let secretos = receta.secretos(params);
    let mut exito_visto = false;

    for (i, paso) in receta.pasos.iter().enumerate() {
        let n = i + 1;
        let accion = match paso.accion() {
            Ok(a) => a,
            Err(e) => return Fin::Error(format!("paso {n}: {e}")),
        };
        let nombre = accion.nombre();
        let timeout = Duration::from_millis(paso.timeout_ms.unwrap_or(TIMEOUT_POR_DEFECTO_MS));

        // Acciones que necesitan una sesión abierta.
        if matches!(accion, Accion::Enviar(_) | Accion::Elegir(_) | Accion::Confirmar(_))
            && (st.sesion.is_none() || !st.abierta)
        {
            return st.cortar(format!(
                "paso {n} ({nombre}): la sesión ya está cerrada; el operador respondió: {:?}",
                st.texto
            ));
        }

        // Un 'marcar' posterior al primero exige que la sesión anterior ya haya cerrado.
        if i > 0 && matches!(accion, Accion::Marcar(_)) && st.abierta {
            return st.cortar(format!("paso {n} (marcar): hay una sesión abierta; la receta debe cerrarla antes de marcar de nuevo"));
        }

        // Qué se envía en este paso.
        let enviado: String = match accion {
            Accion::Marcar(c) => sustituir(c, params, false),
            Accion::Enviar(t) => sustituir(t, params, false),
            Accion::Elegir(re) => {
                let pat = sustituir(re, params, true);
                match menu::elegir(&st.texto, &pat) {
                    Ok(Some(num)) => num,
                    Ok(None) => {
                        return st.cortar(format!(
                            "paso {n}: no encontré la opción /{pat}/ en el menú (opciones: {})",
                            st.opciones()
                        ))
                    }
                    Err(e) => return Fin::Error(format!("paso {n}: regex inválida: {e}")),
                }
            }
            Accion::Verificar(re) => {
                let pat = sustituir(re, params, true);
                match coincide(&pat, &st.texto) {
                    Ok(true) => {
                        st.pasos.push(protocolo::PasoResultado {
                            accion: nombre.into(),
                            enviado: None,
                            texto: st.texto.clone(),
                            abierta: st.abierta,
                            ms: 0,
                        });
                        continue;
                    }
                    Ok(false) => {
                        return st.cortar(format!(
                            "paso {n}: la pantalla no coincide con /{pat}/: {:?}",
                            st.texto
                        ))
                    }
                    Err(e) => return Fin::Error(format!("paso {n}: {e}")),
                }
            }
            Accion::Confirmar(re) => {
                let pat = sustituir(re, params, true);
                // Aun en simulación, la opción de confirmar debe existir: así la simulación
                // detecta cambios del operador.
                let si = match menu::elegir(&st.texto, &pat) {
                    Ok(Some(num)) => num,
                    Ok(None) => {
                        return st.cortar(format!(
                            "paso {n}: no encontré la opción de confirmar /{pat}/ (opciones: {})",
                            st.opciones()
                        ))
                    }
                    Err(e) => return Fin::Error(format!("paso {n}: regex inválida: {e}")),
                };
                if simular {
                    let sesion = st.sesion.clone().unwrap_or_default();
                    return match menu::elegir(&st.texto, "No") {
                        Ok(Some(no)) => {
                            match canal.responder(&sesion, &no, timeout).await {
                                Ok(r) => {
                                    st.pasos.push(PasoResultado {
                                        accion: "confirmar".into(),
                                        enviado: Some(no),
                                        texto: r.texto.clone(),
                                        abierta: r.abierta,
                                        ms: r.ms,
                                    });
                                    st.abierta = r.abierta;
                                    st.texto = r.texto.clone();
                                    st.textos.push(r.texto);
                                }
                                Err(e) => tracing::warn!("simulación: no pude responder No: {e}"),
                            }
                            Fin::Simulado("simulación: se llegó a la confirmación y se respondió No".into())
                        }
                        _ => Fin::Simulado(
                            "simulación: se llegó a la confirmación; no había opción 'No', se canceló la sesión".into(),
                        ),
                    };
                }
                st.confirmado = true;
                si
            }
        };

        // Un 'enviar' marcado `mueve_dinero` (el PIN) es el punto sin retorno, como `confirmar`.
        if paso.mueve_dinero {
            if simular {
                return Fin::Simulado(format!(
                    "simulación: se llegó al paso {n}, que mueve dinero; no se envió. Última pantalla: {:?}",
                    st.texto.trim()
                ));
            }
            st.confirmado = true;
        }
        let mostrado = tapar(&enviado, &secretos);

        let respuesta = match accion {
            Accion::Marcar(_) => canal.iniciar(slot, &enviado, receta.modo, timeout).await,
            _ => {
                let sesion = st.sesion.clone().unwrap_or_default();
                canal.responder(&sesion, &enviado, timeout).await
            }
        };

        let r = match respuesta {
            Ok(r) => r,
            Err(e) => {
                if matches!(accion, Accion::Marcar(_)) {
                    st.abierta = false;
                }
                return st.cortar(format!("paso {n} ({nombre} {mostrado:?}): {e}"));
            }
        };
        st.sesion = Some(r.sesion.clone());
        st.abierta = r.abierta;
        st.texto = r.texto.clone();
        st.textos.push(r.texto.clone());
        st.pasos.push(PasoResultado {
            accion: nombre.into(),
            enviado: Some(mostrado),
            texto: r.texto,
            abierta: r.abierta,
            ms: r.ms,
        });

        if let Some(re) = &paso.error_si {
            match coincide(&sustituir(re, params, true), &st.texto) {
                Ok(true) => return st.cortar(format!("el operador respondió: {}", st.texto.trim())),
                Ok(false) => {}
                Err(e) => return Fin::Error(format!("paso {n}: {e}")),
            }
        }
        if let Some(re) = &paso.esperar {
            let pat = sustituir(re, params, true);
            match coincide(&pat, &st.texto) {
                Ok(true) => {}
                Ok(false) => {
                    return st.cortar(format!(
                        "paso {n}: respuesta inesperada (esperaba /{pat}/): {:?}",
                        st.texto
                    ))
                }
                Err(e) => return Fin::Error(format!("paso {n}: {e}")),
            }
        }
        if let Some(re) = &paso.exito {
            let pat = sustituir(re, params, true);
            match coincide(&pat, &st.texto) {
                Ok(true) => {
                    exito_visto = true;
                    break;
                }
                Ok(false) if matches!(accion, Accion::Confirmar(_)) || paso.mueve_dinero => {
                    return Fin::Incierto(format!(
                        "se confirmó pero la respuesta no coincide con el éxito esperado /{pat}/: {:?}. Revisa el saldo.",
                        st.texto
                    ))
                }
                Ok(false) => {}
                Err(e) => return Fin::Error(format!("paso {n}: {e}")),
            }
        }
    }

    if exito_visto || !tiene_exito {
        Fin::Ok
    } else if st.confirmado {
        Fin::Incierto("se confirmó pero no apareció la respuesta de éxito. Revisa el saldo.".into())
    } else {
        Fin::Abortado("la receta terminó sin la respuesta de éxito esperada".into())
    }
}

#[cfg(test)]
mod tests;
