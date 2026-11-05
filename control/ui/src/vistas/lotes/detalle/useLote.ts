import { useCallback, useEffect, useRef, useState } from "react";
import { api, esCancelado, mensajeError } from "../../../api/cliente";
import { useAlReconectar, useEvento } from "../../../api/eventos";
import type { EventoLote, Lote } from "../../../api/tipos";
import { useAgrupar } from "../../../util/datos";

/** Fases que cambian algo más que el progreso (estado, motivo de pausa, revisión): ahí se relee el lote entero. */
const FASES_RECARGA = new Set(["inicio", "fin", "pausa"]);

/**
 * Un aviso que dejó otra vista para mostrarlo al abrir el lote (p. ej. "se creó pero no arrancó").
 * Vive en el módulo para sobrevivir al cambio de vista.
 */
const avisosPendientes = new Map<number, string>();
export const dejarAviso = (id: number, texto: string) => avisosPendientes.set(id, texto);

/**
 * El lote (sin filas) al día: carga, se parcha con el `progreso` de cada evento `lote`, se relee entero en
 * inicio/fin/pausa, al reconectar o con `perdidos`, y cada 5 s mientras está en curso por si se pierde algo.
 * Ignora respuestas viejas: si llegan dos cargas cruzadas, gana la última pedida.
 */
export function useLote(id: number) {
  const [lote, setLote] = useState<Lote | null>(null);
  const [error, setError] = useState<string | null>(() => {
    const a = avisosPendientes.get(id) ?? null;
    avisosPendientes.delete(id);
    return a;
  });
  const [cargado, setCargado] = useState(false);
  const turno = useRef(0);

  const cargar = useCallback(async () => {
    const mio = ++turno.current;
    try {
      const l = await api.lote(id);
      if (mio !== turno.current) return;
      setLote(l);
    } catch (e) {
      if (mio !== turno.current || esCancelado(e)) return;
      setError(mensajeError(e));
    } finally {
      if (mio === turno.current) setCargado(true);
    }
  }, [id]);

  useEffect(() => {
    cargar();
  }, [cargar]);

  useEffect(() => {
    if (lote?.estado !== "en_curso") return;
    const t = window.setInterval(cargar, 5000);
    return () => window.clearInterval(t);
  }, [lote?.estado, cargar]);

  const recargar = useAgrupar(cargar, 200);
  useEvento((e) => {
    if (e.tipo !== "lote") return;
    const ev = e as unknown as EventoLote;
    if (Number(ev.lote_id) !== id) return;
    if (ev.progreso) {
      // Un evento invalida cualquier carga en vuelo (traería un progreso más viejo).
      turno.current++;
      setLote((l) => (l ? { ...l, progreso: ev.progreso!, ...(ev.fase === "inicio" ? { estado: "en_curso" as const, motivo_pausa: null } : {}) } : l));
    }
    if (FASES_RECARGA.has(ev.fase) || !ev.progreso) recargar();
  });
  useAlReconectar(cargar);

  /** Reemplaza el lote con lo que devolvió una acción, sin perder las filas de un servidor viejo. */
  const fijar = useCallback((l: Lote) => {
    turno.current++;
    setLote((ant) => ({ ...l, filas: l.filas ?? ant?.filas }));
  }, []);

  return { lote, error, setError, cargado, recargar: cargar, fijar };
}
