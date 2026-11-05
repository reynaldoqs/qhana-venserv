import { useCallback, useEffect, useRef, useState } from "react";
import { api, mensajeError } from "../api/cliente";
import { useAlReconectar, useEvento } from "../api/eventos";
import type { Dispositivo, ResumenReceta, Sim } from "../api/tipos";

/** Lista de dispositivos: carga al inicio, se refresca con eventos y cada 10 s por si se pierde alguno. */
export function useDispositivos() {
  const [lista, setLista] = useState<Dispositivo[]>([]);
  const [cargando, setCargando] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const cargar = useCallback(async () => {
    try {
      const d = await api.dispositivos();
      d.sort((a, b) => Number(b.conectado) - Number(a.conectado) || a.modelo.localeCompare(b.modelo));
      setLista(d);
      setError(null);
    } catch (e) {
      setError(mensajeError(e));
    } finally {
      setCargando(false);
    }
  }, []);

  useEffect(() => {
    cargar();
    const t = setInterval(cargar, 10_000);
    return () => clearInterval(t);
  }, [cargar]);

  // Varios eventos seguidos → una sola recarga.
  const agrupada = useAgrupar(cargar, 250);
  useEvento((e) => {
    if (e.tipo.startsWith("dispositivo_") || e.tipo === "adb") agrupada();
  });
  // Los eventos de mientras estuvo cortada la conexión se perdieron.
  useAlReconectar(cargar);

  return { lista, cargando, error, recargar: cargar };
}

/** Lista de recetas: se relee cuando una cambia (`receta_cambiada`), cuando termina una ejecución (`uso`) y al reconectar. */
export function useRecetas() {
  const [lista, setLista] = useState<ResumenReceta[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [cargando, setCargando] = useState(true);
  const cargar = useCallback(async () => {
    try {
      setLista(await api.recetas());
      setError(null);
    } catch (e) {
      setError(mensajeError(e));
    } finally {
      setCargando(false);
    }
  }, []);
  useEffect(() => {
    cargar();
  }, [cargar]);
  const agrupada = useAgrupar(cargar, 400);
  useEvento((e) => {
    if (e.tipo === "receta_cambiada") agrupada();
    // Una ejecución terminada cambia `uso` y `sin_probar`.
    else if (e.tipo === "receta" && e.fase === "fin") agrupada();
  });
  useAlReconectar(cargar);
  return { lista, error, cargando, recargar: cargar };
}

export interface OpcionMenu {
  numero: string;
  texto: string;
}

/** Mismo criterio que el motor: `1.-Texto`, `1. Texto`, `1) Texto`, `1 Texto`. */
export function opcionesMenu(texto: string): OpcionMenu[] {
  const out: OpcionMenu[] = [];
  for (const linea of texto.split(/\r?\n/)) {
    const m = linea.match(/^\s*(\d{1,2})\s*(?:\.-|\.|\)|-|\s)\s*(.+?)\s*$/);
    if (m && !/^\d+$/.test(m[2])) out.push({ numero: m[1], texto: m[2] });
  }
  return out;
}

export const hora = (ms: number) =>
  new Date(ms).toLocaleTimeString("es-BO", { hour: "2-digit", minute: "2-digit", second: "2-digit" });

export const fechaHora = (ms: number) =>
  new Date(ms).toLocaleString("es-BO", {
    day: "2-digit",
    month: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });

export const segundos = (ms: number) => `${(ms / 1000).toFixed(1)} s`;

export function hace(ms: number): string {
  if (!ms) return "nunca";
  const s = Math.max(0, Math.round((Date.now() - ms) / 1000));
  if (s < 10) return "ahora";
  if (s < 60) return `hace ${s} s`;
  if (s < 3600) return `hace ${Math.round(s / 60)} min`;
  return `hace ${Math.round(s / 3600)} h`;
}

/** Como `hace`, pero sigue con días ("hace 3 d") para fechas viejas. */
export function haceLargo(ms: number | null | undefined): string {
  if (!ms) return "nunca";
  const s = Math.max(0, Math.round((Date.now() - ms) / 1000));
  if (s < 2 * 86400) return hace(ms);
  return `hace ${Math.round(s / 86400)} d`;
}

export const nombreSim = (slot: number) => `SIM ${slot + 1}`;

/** Número local sin el prefijo de Bolivia: "+59178815232" → "78815232". */
export const sinPrefijo = (n: string | null | undefined) => (n == null ? n : n.replace(/^\+?591/, "")) as string | null;

/** "Infinix X678B · 012456": el modelo más el final del serial USB (o del id) para distinguir teléfonos iguales. */
export const etiquetaTelefono = (d: Pick<Dispositivo, "modelo" | "adb_serial" | "id">) =>
  `${d.modelo} · ${d.adb_serial?.slice(-6) ?? d.id.slice(0, 6)}`;

/**
 * Devuelve una función que junta llamadas seguidas en una sola, `ms` después de la última.
 * Para recargar ante ráfagas de eventos sin pedir lo mismo diez veces.
 */
export function useAgrupar(fn: () => void, ms: number) {
  const ref = useRef(fn);
  ref.current = fn;
  const t = useRef<number | null>(null);
  useEffect(
    () => () => {
      if (t.current) window.clearTimeout(t.current);
    },
    [],
  );
  return useCallback(() => {
    if (t.current) window.clearTimeout(t.current);
    t.current = window.setTimeout(() => {
      t.current = null;
      ref.current();
    }, ms);
  }, [ms]);
}

/** El valor, pero solo después de `ms` sin cambios (para búsquedas mientras se escribe). */
export function useRetrasado<T>(valor: T, ms: number): T {
  const [v, setV] = useState(valor);
  useEffect(() => {
    const t = window.setTimeout(() => setV(valor), ms);
    return () => window.clearTimeout(t);
  }, [valor, ms]);
  return v;
}

/** Lee y guarda una preferencia en el navegador; si no hay almacenamiento, vale solo mientras dura la página. */
export function usePreferencia<T extends string>(clave: string, inicial: T, validas?: readonly T[]): [T, (v: T) => void] {
  const [v, setV] = useState<T>(() => {
    try {
      const x = localStorage.getItem(clave) as T | null;
      return x != null && (!validas || validas.includes(x)) ? x : inicial;
    } catch {
      return inicial;
    }
  });
  const fijar = useCallback(
    (x: T) => {
      setV(x);
      try {
        localStorage.setItem(clave, x);
      } catch {
        /* sin almacenamiento */
      }
    },
    [clave],
  );
  return [v, fijar];
}

const sinAcentos = (t: string) => t.normalize("NFD").replace(/[\u0300-\u036f]/g, "");

/**
 * Operador de un chip: TIGO, ENTEL o VIVA. Sale del nombre de red y, si no lo reconoce, del código de red
 * (Bolivia: 73601 Viva, 73602 Entel, 73603 Tigo). Espejo de `operador_de_sim` en control/crates/lotes/src/normalizar.rs.
 */
export function operadorDe(sim: Pick<Sim, "operador" | "mcc_mnc">): string {
  const t = sinAcentos(sim.operador).toUpperCase();
  for (const [clave, op] of [["TIGO", "TIGO"], ["ENTEL", "ENTEL"], ["VIVA", "VIVA"], ["NUEVATEL", "VIVA"]]) {
    if (t.includes(clave)) return op;
  }
  const porRed: Record<string, string> = { "73601": "VIVA", "73602": "ENTEL", "73603": "TIGO" };
  return porRed[sim.mcc_mnc.trim()] ?? sim.operador.trim().toUpperCase();
}

/** "ENTEL (FutbolSinLimites)" cuando la marca no lleva el nombre del operador; "TIGO" si ya lo dice. */
export function etiquetaOperador(sim: Pick<Sim, "operador" | "mcc_mnc">): string {
  const op = operadorDe(sim);
  const marca = sim.operador.trim();
  if (!marca) return op || "—";
  return sinAcentos(marca).toUpperCase().includes(op) ? op : `${op} (${marca})`;
}
