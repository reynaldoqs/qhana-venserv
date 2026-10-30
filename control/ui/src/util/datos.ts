import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "../api/cliente";
import { useEvento } from "../api/eventos";
import type { Dispositivo, Receta, Sim } from "../api/tipos";

/** Lista de dispositivos: carga al inicio, se refresca con eventos y cada 10 s por si se pierde alguno. */
export function useDispositivos() {
  const [lista, setLista] = useState<Dispositivo[]>([]);
  const [cargando, setCargando] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const pendiente = useRef<number | null>(null);

  const cargar = useCallback(async () => {
    try {
      const d = await api.dispositivos();
      d.sort((a, b) => Number(b.conectado) - Number(a.conectado) || a.modelo.localeCompare(b.modelo));
      setLista(d);
      setError(null);
    } catch (e) {
      setError((e as Error).message);
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
  useEvento((e) => {
    if (!e.tipo.startsWith("dispositivo_") && e.tipo !== "adb") return;
    if (pendiente.current) window.clearTimeout(pendiente.current);
    pendiente.current = window.setTimeout(cargar, 250);
  });

  return { lista, cargando, error, recargar: cargar };
}

export function useRecetas() {
  const [lista, setLista] = useState<Receta[]>([]);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    api.recetas().then(setLista, (e) => setError((e as Error).message));
  }, []);
  return { lista, error };
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

export const nombreSim = (slot: number) => `SIM ${slot + 1}`;

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
