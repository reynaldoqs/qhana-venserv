import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, esCancelado, esControlViejo } from "../../../api/cliente";
import { useAlReconectar, useEvento } from "../../../api/eventos";
import type { Carril, Carriles, Dispositivo, EstadoCarril, EventoLote, Lote } from "../../../api/tipos";
import { Insignia, Punto, Tarjeta, cx } from "../../../componentes/base";
import { Smartphone } from "../../../componentes/iconos";
import { etiquetaTelefono, nombreSim, sinPrefijo, useAgrupar } from "../../../util/datos";
import { bs, duracionCorta, reloj } from "../comunes";

const ESTADO: Record<EstadoCarril, { tono: "ok" | "aviso" | "error" | "info" | "neutro"; texto: string }> = {
  enviando: { tono: "info", texto: "Enviando" },
  esperando: { tono: "neutro", texto: "Esperando turno" },
  libre: { tono: "ok", texto: "Libre" },
  sin_capacidad: { tono: "aviso", texto: "Sin capacidad" },
  desconectado: { tono: "error", texto: "Desconectado" },
  excluido: { tono: "neutro", texto: "Excluido" },
};

const clave = (dispositivo: string, slot: number) => `${dispositivo}|${slot}`;

/**
 * Los carriles del lote al día: carga, vuelve a pedirlos cada 3 s mientras corre y, entre medio, aplica cada evento
 * `lote` `fase: envio` a su carril (quién empezó a enviar, quién terminó). `null` si el control no tiene la ruta.
 */
function useCarriles(lote: Lote) {
  const [datos, setDatos] = useState<Carriles | null>(null);
  const [viejo, setViejo] = useState(false);
  const turno = useRef(0);
  const cargar = useCallback(async () => {
    const mio = ++turno.current;
    try {
      const c = await api.carrilesLote(lote.id);
      if (mio === turno.current) setDatos({ carriles: c.carriles ?? [], pendientes: c.pendientes ?? 0, eta_ms: c.eta_ms ?? null });
    } catch (e) {
      if (mio !== turno.current || esCancelado(e)) return;
      if (esControlViejo(e)) setViejo(true);
      // Otros errores: se queda con lo último que tenía (el próximo intento lo arregla).
    }
  }, [lote.id]);

  const corriendo = lote.estado === "en_curso";
  useEffect(() => {
    cargar();
  }, [cargar, lote.estado]);
  useEffect(() => {
    if (!corriendo || viejo) return;
    const t = window.setInterval(cargar, 3000);
    return () => window.clearInterval(t);
  }, [corriendo, viejo, cargar]);

  const pronto = useAgrupar(cargar, 500);
  useEvento((e) => {
    if (e.tipo !== "lote") return;
    const ev = e as unknown as EventoLote;
    if (Number(ev.lote_id) !== lote.id) return;
    if (ev.fase === "envio" && ev.dispositivo_id != null && ev.slot != null) {
      const k = clave(ev.dispositivo_id, ev.slot);
      turno.current++; // una carga en vuelo traería algo más viejo que esto
      setDatos((d) => {
        if (!d) return d;
        return {
          ...d,
          carriles: d.carriles.map((c) => (clave(c.dispositivo, c.slot) === k ? aplicarEnvio(c, ev) : c)),
        };
      });
    }
    pronto();
  });
  useAlReconectar(cargar);

  return { datos, viejo };
}

/** Lo que cambia en un carril con un evento `envio`: arranca (`en_curso`) o termina con su resultado. */
function aplicarEnvio(c: Carril, ev: EventoLote): Carril {
  const monto = ev.monto ?? 0;
  switch (ev.estado) {
    case "en_curso":
      return {
        ...c,
        estado: "enviando",
        actual: { fila: ev.fila ?? 0, numero: ev.numero ?? null, monto, desde_ms: ev.ts_ms || Date.now() },
      };
    case "ok":
      return { ...c, estado: "libre", actual: null, ok: c.ok + 1, enviado_bs: Math.round((c.enviado_bs + monto) * 100) / 100 };
    case "simulado": // una simulación no mueve dinero: cuenta, pero no suma Bs
      return { ...c, estado: "libre", actual: null, ok: c.ok + 1 };
    case "fallido":
      return { ...c, estado: "libre", actual: null, fallidos: c.fallidos + 1 };
    case "incierto":
      return { ...c, estado: "libre", actual: null, inciertos: c.inciertos + 1 };
    default:
      return c;
  }
}

/** Reloj de pared que avanza cada segundo, solo mientras `activo`. */
function useAhora(activo: boolean) {
  const [ahora, setAhora] = useState(() => Date.now());
  useEffect(() => {
    if (!activo) return;
    setAhora(Date.now());
    const t = window.setInterval(() => setAhora(Date.now()), 1000);
    return () => window.clearInterval(t);
  }, [activo]);
  return ahora;
}

/**
 * Cuándo empezó el envío en curso de cada carril. Usa `desde_ms` del control si es creíble; si no (0, en el futuro por
 * desfase de reloj, o muy viejo), la hora en que la interfaz lo vio empezar.
 */
function useInicios(carriles: Carril[]) {
  const vistos = useRef(new Map<string, { fila: number; ms: number }>());
  const ahora = Date.now();
  const out = new Map<string, number>();
  for (const c of carriles) {
    const k = clave(c.dispositivo, c.slot);
    if (c.estado !== "enviando" || !c.actual) {
      vistos.current.delete(k);
      continue;
    }
    let v = vistos.current.get(k);
    if (!v || v.fila !== c.actual.fila) {
      v = { fila: c.actual.fila, ms: ahora };
      vistos.current.set(k, v);
    }
    const d = c.actual.desde_ms ?? 0;
    out.set(k, d > 0 && d <= ahora + 2000 && ahora - d < 15 * 60_000 ? Math.min(d, ahora) : v.ms);
  }
  return out;
}

function Linea({
  c,
  inicio,
  ahora,
  corriendo,
  simular,
}: {
  c: Carril;
  inicio: number | undefined;
  ahora: number;
  corriendo: boolean;
  simular: boolean;
}) {
  const e = ESTADO[c.estado] ?? { tono: "neutro" as const, texto: c.estado };
  const enviando = c.estado === "enviando" && c.actual;
  const apagado = c.estado === "excluido" || c.estado === "desconectado";
  return (
    <li
      className={cx(
        "rounded-2xl px-3.5 py-3 transition-colors duration-300",
        enviando ? "bg-info-suave/60" : "bg-panel-2/50",
        apagado && "opacity-60",
      )}
    >
      <div className="grid gap-x-4 gap-y-2 md:grid-cols-[18rem_1fr_auto] md:items-center">
        {/* Quién */}
        <div className="min-w-0 flex flex-wrap items-center gap-x-2 gap-y-1">
          <span className="font-mono text-sm">{sinPrefijo(c.numero) ?? nombreSim(c.slot)}</span>
          <span className="text-[11px] text-tenue">{nombreSim(c.slot)}</span>
          {c.operador && <Insignia className="h-5 px-2 text-[11px]">{c.operador}</Insignia>}
          {c.tipo === "dealer" && (
            <Insignia tono="info" className="h-5 px-2 text-[11px]">
              Dealer
            </Insignia>
          )}
          {c.receta && <span className="w-full text-[11px] font-mono text-tenue truncate">{c.receta}</span>}
        </div>

        {/* Qué hace ahora */}
        <div className="min-w-0 flex items-center gap-2.5 text-sm" aria-live={enviando ? "off" : undefined}>
          <Punto tono={e.tono} pulso={!!enviando && corriendo} />
          {enviando ? (
            <span className="min-w-0 flex flex-wrap items-baseline gap-x-2">
              <span className="font-medium text-info">Fila {c.actual!.fila}</span>
              <span className="font-mono">→ {sinPrefijo(c.actual!.numero) ?? "—"}</span>
              <span className="tabular-nums">{bs(c.actual!.monto)}</span>
              {inicio != null && <span className="tabular-nums text-tenue">{reloj(ahora - inicio)}</span>}
            </span>
          ) : (
            <span className="min-w-0">
              <span className={cx(e.tono === "aviso" && "text-aviso", e.tono === "error" && "text-error", e.tono === "neutro" && "text-tenue")}>
                {e.texto}
              </span>
              {c.motivo && <span className="text-tenue"> · {c.motivo}</span>}
            </span>
          )}
        </div>

        {/* Cuentas */}
        <dl className="flex flex-wrap gap-x-4 gap-y-1 text-xs tabular-nums md:justify-end">
          <div className="flex gap-1">
            <dt className="text-tenue">{simular ? "simulados" : "OK"}</dt>
            <dd className={cx("font-semibold", c.ok > 0 && "text-ok")}>{c.ok}</dd>
          </div>
          {c.fallidos > 0 && (
            <div className="flex gap-1">
              <dt className="text-tenue">fallidos</dt>
              <dd className="font-semibold text-error">{c.fallidos}</dd>
            </div>
          )}
          {c.inciertos > 0 && (
            <div className="flex gap-1">
              <dt className="text-tenue">inciertos</dt>
              <dd className="font-semibold text-error">{c.inciertos}</dd>
            </div>
          )}
          {!simular && (
            <div className="flex gap-1">
              <dt className="sr-only">Enviado</dt>
              <dd className="font-semibold">{bs(c.enviado_bs)}</dd>
            </div>
          )}
          {c.cupo_restante != null ? (
            <div className="flex gap-1">
              <dt className="text-tenue">cupo</dt>
              <dd>{c.cupo_restante}</dd>
            </div>
          ) : (
            c.cupo_sin_limite && (
              <div className="flex gap-1">
                <dt className="text-tenue">cupo</dt>
                <dd>sin límite</dd>
              </div>
            )
          )}
          {c.saldo && (
            <div className="flex gap-1">
              <dt className="text-tenue">{c.saldo.tipo === "dealer" ? "saldo dealer" : "saldo"}</dt>
              <dd>{bs(c.saldo.bs)}</dd>
            </div>
          )}
          {c.ms_promedio != null && (
            <div className="flex gap-1" title="Tiempo promedio por envío en este lote">
              <dt className="sr-only">Promedio</dt>
              <dd className="text-tenue">~{duracionCorta(c.ms_promedio)}/envío</dd>
            </div>
          )}
        </dl>
      </div>
    </li>
  );
}

/**
 * "Carriles por teléfono": un carril por chip, agrupados por teléfono. Mientras el lote corre muestra quién envía qué
 * (con cronómetro); detenido, sirve de resumen por chip. Cada teléfono hace un envío a la vez.
 */
export function CarrilesLote({ lote, dispositivos }: { lote: Lote; dispositivos: Dispositivo[] }) {
  const { datos, viejo } = useCarriles(lote);
  const corriendo = lote.estado === "en_curso";
  const carriles = datos?.carriles ?? [];
  const hayEnviando = corriendo && carriles.some((c) => c.estado === "enviando");
  const ahora = useAhora(hayEnviando);
  const inicios = useInicios(carriles);

  const telefonos = useMemo(() => {
    const m = new Map<string, Carril[]>();
    for (const c of carriles) m.set(c.dispositivo, [...(m.get(c.dispositivo) ?? []), c]);
    return [...m.entries()].map(([id, lista]) => [id, lista.sort((a, b) => a.slot - b.slot)] as const);
  }, [carriles]);

  if (viejo || !datos || carriles.length === 0) return null;

  const activos = new Set(carriles.filter((c) => c.estado === "enviando" || c.estado === "esperando" || c.estado === "libre").map((c) => c.dispositivo)).size;

  return (
    <Tarjeta className="p-4 sm:p-5 mb-5">
      <div className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1 mb-3.5">
        <h2 className="font-semibold">{corriendo ? "Carriles por teléfono" : "Por chip"}</h2>
        <p className="text-xs text-tenue tabular-nums" aria-live="polite">
          {corriendo ? (
            <>
              {datos.pendientes} pendiente{datos.pendientes === 1 ? "" : "s"}
              {datos.eta_ms != null && datos.eta_ms > 0 && (
                <>
                  {" · "}
                  <span className="text-texto">termina en ~{duracionCorta(datos.eta_ms)}</span>
                </>
              )}
              {activos > 1 && ` · ${activos} teléfonos en paralelo`}
            </>
          ) : (
            `${carriles.length} chip${carriles.length === 1 ? "" : "s"} en ${telefonos.length} teléfono${telefonos.length === 1 ? "" : "s"}`
          )}
        </p>
      </div>
      <div className="space-y-4">
        {telefonos.map(([id, lista]) => {
          const d = dispositivos.find((x) => x.id === id);
          // El listado de teléfonos lleva el final del serial (distingue dos teléfonos del mismo modelo).
          const nombre = d ? etiquetaTelefono(d) : lista[0]?.telefono || id.slice(0, 8);
          return (
            <section key={id} aria-label={`Teléfono ${nombre}`}>
              <p className="mb-1.5 px-1 text-xs text-tenue flex flex-wrap items-center gap-x-1.5">
                <Smartphone className="size-3.5" />
                <span className="font-medium text-texto/90">{nombre}</span>
                {lista.length > 1 && (
                  <span className="opacity-75 max-sm:basis-full max-sm:pl-5">
                    <span className="max-sm:hidden">· </span>
                    {lista.length} chips que se turnan, un envío a la vez
                  </span>
                )}
              </p>
              <ul className={cx("space-y-1.5", lista.length > 1 && "border-l-2 border-borde/70 pl-2")}>
                {lista.map((c) => (
                  <Linea
                    key={clave(c.dispositivo, c.slot)}
                    c={c}
                    inicio={inicios.get(clave(c.dispositivo, c.slot))}
                    ahora={Math.max(ahora, Date.now() - 500)}
                    corriendo={corriendo}
                    simular={lote.simular}
                  />
                ))}
              </ul>
            </section>
          );
        })}
      </div>
    </Tarjeta>
  );
}
