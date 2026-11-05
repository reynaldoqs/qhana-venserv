import { useEffect, useRef, useState } from "react";
import type { Evento } from "./tipos";
import { conToken, normalizarViejo, sesion } from "./cliente";

export type EstadoConexion = "conectando" | "conectado" | "desconectado";

type Oyente = (e: Evento) => void;

/** Si el reloj avanza esto de más entre dos latidos, el PC estuvo dormido. */
const SALTO_SUENO_MS = 15_000;
const LATIDO_MS = 5_000;
/** Un WebSocket que no abre en este tiempo se descarta y se reintenta. */
const ESPERA_ABRIR_MS = 8_000;

/**
 * Una sola conexión WebSocket a /api/eventos para toda la app, con reconexión.
 * Los componentes se suscriben con `useEvento` o leen el estado con `useConexion`.
 *
 * Al dormir el PC el socket puede quedar "abierto" pero muerto (no llega `onclose`), así que al despertar
 * (salto del reloj), al volver a la pestaña o al volver la red se reconecta a la fuerza. Tras cada
 * reconexión se avisa a `useAlReconectar`: los eventos de mientras se perdieron y hay que releer todo. Lo mismo con
 * `{tipo: "perdidos"}`: el control descartó eventos de esta conexión porque iba atrasada.
 */
class BusEventos {
  private ws: WebSocket | null = null;
  private oyentes = new Set<Oyente>();
  private oyentesEstado = new Set<(s: EstadoConexion) => void>();
  private oyentesReconexion = new Set<() => void>();
  private espera = 1000;
  private reintento: number | null = null;
  private yaAbrio = false;
  estado: EstadoConexion = "conectando";

  private iniciado = false;

  iniciar() {
    if (this.iniciado) return;
    this.iniciado = true;
    this.vigilar();
    this.conectar();
    // Con un token nuevo se reabre la conexión (la anterior pudo ser rechazada).
    let ultimo = sesion.estado().token;
    sesion.suscribir(() => {
      const t = sesion.estado().token;
      if (t !== ultimo && t) this.reconectar();
      ultimo = t;
    });
  }

  /** Cierra la conexión actual (viva o muerta) y abre otra ya. */
  reconectar() {
    if (this.reintento !== null) window.clearTimeout(this.reintento);
    this.reintento = null;
    this.soltar();
    this.espera = 1000;
    this.conectar();
  }

  private soltar() {
    const ws = this.ws;
    this.ws = null;
    if (!ws) return;
    ws.onopen = ws.onmessage = ws.onclose = ws.onerror = null;
    try {
      ws.close();
    } catch {
      /* ya cerrado */
    }
  }

  private vigilar() {
    let ultimo = Date.now();
    window.setInterval(() => {
      const ahora = Date.now();
      if (ahora - ultimo > LATIDO_MS + SALTO_SUENO_MS) this.reconectar();
      ultimo = ahora;
    }, LATIDO_MS);
    document.addEventListener("visibilitychange", () => {
      if (document.visibilityState === "visible" && this.estado !== "conectado") this.reconectar();
    });
    window.addEventListener("online", () => this.reconectar());
  }

  private conectar() {
    this.reintento = null;
    const proto = location.protocol === "https:" ? "wss" : "ws";
    const ws = new WebSocket(conToken(`${proto}://${location.host}/api/eventos`));
    this.ws = ws;
    this.cambiar("conectando");
    const tope = window.setTimeout(() => {
      if (this.ws === ws && ws.readyState === WebSocket.CONNECTING) this.caida();
    }, ESPERA_ABRIR_MS);
    ws.onopen = () => {
      window.clearTimeout(tope);
      this.espera = 1000;
      this.cambiar("conectado");
      if (this.yaAbrio) this.oyentesReconexion.forEach((o) => o());
      this.yaAbrio = true;
    };
    ws.onmessage = (m) => {
      try {
        const e = normalizarViejo(JSON.parse(m.data)) as Evento;
        this.oyentes.forEach((o) => o(e));
        if (e.tipo === "perdidos") this.oyentesReconexion.forEach((o) => o());
      } catch {
        /* mensaje no JSON: se ignora */
      }
    };
    ws.onclose = () => {
      window.clearTimeout(tope);
      this.caida();
    };
  }

  private caida() {
    this.soltar();
    this.cambiar("desconectado");
    this.reintento = window.setTimeout(() => this.conectar(), this.espera);
    this.espera = Math.min(this.espera * 2, 15000);
  }

  suscribirReconexion(o: () => void) {
    this.oyentesReconexion.add(o);
    return () => this.oyentesReconexion.delete(o);
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

/** Llama a `fn` cada vez que la conexión de eventos vuelve tras cortarse o el control avisa `perdidos` (hay que releer el estado). */
export function useAlReconectar(fn: () => void) {
  const ref = useRef(fn);
  ref.current = fn;
  useEffect(() => {
    bus.iniciar();
    const quitar = bus.suscribirReconexion(() => ref.current());
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
