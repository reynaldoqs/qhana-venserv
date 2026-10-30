import { useEffect, useRef, useState } from "react";
import type { Evento } from "./tipos";

export type EstadoConexion = "conectando" | "conectado" | "desconectado";

type Oyente = (e: Evento) => void;

/**
 * Una sola conexión WebSocket a /api/eventos para toda la app, con reconexión.
 * Los componentes se suscriben con `useEvento` o leen el estado con `useConexion`.
 */
class BusEventos {
  private ws: WebSocket | null = null;
  private oyentes = new Set<Oyente>();
  private oyentesEstado = new Set<(s: EstadoConexion) => void>();
  private espera = 1000;
  estado: EstadoConexion = "conectando";

  iniciar() {
    if (this.ws) return;
    this.conectar();
  }

  private conectar() {
    const proto = location.protocol === "https:" ? "wss" : "ws";
    const ws = new WebSocket(`${proto}://${location.host}/api/eventos`);
    this.ws = ws;
    this.cambiar("conectando");
    ws.onopen = () => {
      this.espera = 1000;
      this.cambiar("conectado");
    };
    ws.onmessage = (m) => {
      try {
        const e = JSON.parse(m.data) as Evento;
        this.oyentes.forEach((o) => o(e));
      } catch {
        /* mensaje no JSON: se ignora */
      }
    };
    ws.onclose = () => {
      this.ws = null;
      this.cambiar("desconectado");
      setTimeout(() => this.conectar(), this.espera);
      this.espera = Math.min(this.espera * 2, 15000);
    };
  }

  private cambiar(s: EstadoConexion) {
    this.estado = s;
    this.oyentesEstado.forEach((o) => o(s));
  }

  suscribir(o: Oyente) {
    this.oyentes.add(o);
    return () => this.oyentes.delete(o);
  }

  suscribirEstado(o: (s: EstadoConexion) => void) {
    this.oyentesEstado.add(o);
    return () => this.oyentesEstado.delete(o);
  }
}

export const bus = new BusEventos();

/** Llama a `fn` con cada evento (la referencia más reciente, sin re-suscribir). */
export function useEvento(fn: (e: Evento) => void) {
  const ref = useRef(fn);
  ref.current = fn;
  useEffect(() => {
    bus.iniciar();
    const quitar = bus.suscribir((e) => ref.current(e));
    return () => {
      quitar();
    };
  }, []);
}

export function useConexion(): EstadoConexion {
  const [s, setS] = useState<EstadoConexion>(bus.estado);
  useEffect(() => {
    bus.iniciar();
    const quitar = bus.suscribirEstado(setS);
    return () => {
      quitar();
    };
  }, []);
  return s;
}
