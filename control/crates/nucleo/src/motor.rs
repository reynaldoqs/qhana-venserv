//! Motor de recetas: ejecuta una [`Receta`] sobre cualquier [`CanalUssd`].
//!
//! Reglas de seguridad (contrato):
//! - Antes de mover dinero, cualquier sorpresa aborta (`abortado`): no se movió nada.
//! - `confirmar` nunca se reintenta. Sin respuesta clara después de confirmar → `incierto`.
//! - En `simular`, `confirmar` elige la opción "No".
//! - Al terminar, siempre se cancela la sesión si quedó abierta.

use crate::canal::CanalUssd;
use crate::menu;
use crate::receta::{sustituir, Accion, Receta};
use protocolo::{ahora_ms, EstadoResultado, PasoResultado, Resultado};
use regex::Regex;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

pub const TIMEOUT_POR_DEFECTO_MS: u64 = 30_000;

/// Valida los parámetros contra la receta. Devuelve los valores limpios.
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
        let v = parametros
            .get(&p.nombre)
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .ok_or_else(|| format!("falta el parámetro '{}'", p.nombre))?;
        if let Some(pat) = &p.patron {
            let re = Regex::new(&format!("^(?:{pat})$")).map_err(|e| e.to_string())?;
            if !re.is_match(&v) {
                return Err(format!("'{}' = '{v}' no cumple el patrón {pat}", p.nombre));
            }
        }
        if let Some(ops) = &p.opciones {
            if !ops.iter().any(|o| o == &v) {
                return Err(format!("'{}' = '{v}' no es una opción válida ({})", p.nombre, ops.join(", ")));
            }
        }
        limpios.insert(p.nombre.clone(), v);
    }
    Ok(limpios)
}

fn coincide(patron: &str, texto: &str) -> Result<bool, String> {
    Regex::new(&format!("(?is){patron}"))
        .map(|re| re.is_match(texto))
        .map_err(|e| format!("regex inválida '{patron}': {e}"))
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
        (Ok(()), Ok(params)) => correr(receta, canal, slot, &params, simular, &mut st).await,
    };

    // Siempre cerrar la sesión si quedó abierta.
    if st.abierta {
        if let Some(s) = &st.sesion {
            if let Err(e) = canal.cancelar(s).await {
                tracing::warn!("no pude cancelar la sesión {s}: {e}");
            }
        }
    }

    let params_salida = validar_parametros(receta, parametros).unwrap_or_default();
    let mut salida = BTreeMap::new();
    for (k, r) in &receta.salida {
        if let Ok(re) = Regex::new(&sustituir(r, &params_salida, true)) {
            if let Some(v) = st
                .textos
                .iter()
                .find_map(|t| re.captures(t).and_then(|c| c.get(1)).map(|m| m.as_str().to_string()))
            {
                salida.insert(k.clone(), v);
            }
        }
    }

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
                return st.cortar(format!("paso {n} ({nombre} {enviado:?}): {e}"));
            }
        };
        st.sesion = Some(r.sesion.clone());
        st.abierta = r.abierta;
        st.texto = r.texto.clone();
        st.textos.push(r.texto.clone());
        st.pasos.push(PasoResultado {
            accion: nombre.into(),
            enviado: Some(enviado),
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
                Ok(false) if matches!(accion, Accion::Confirmar(_)) => {
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
