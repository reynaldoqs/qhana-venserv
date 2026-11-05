import { useEffect, useMemo, useState } from "react";
import { Activity, BookOpen, Cable, Hash, Layers, MessageSquare, PlugZap, Unplug, RefreshCw } from "../componentes/iconos";
import { api, FalloApi, mensajeError } from "../api/cliente";
import { useAlReconectar, useEvento } from "../api/eventos";
import type { Dispositivo, Evento, EventoLote } from "../api/tipos";
import { Aviso, Boton, Esqueleto, Segmentado, Tarjeta, Titulo, Vacio, cx } from "../componentes/base";
import { etiquetaTelefono } from "../util/datos";

const MAX = 300;

const ACCIONES_USSD: Record<string, string> = { iniciar: "Marcó", responder: "Respondió", cancelar: "Canceló" };

function describir(e: Evento): { icono: typeof Activity; titulo: string; detalle?: string; tono?: string } {
  const s = (k: string) => (e[k] == null ? undefined : String(e[k]));
  const disp = e.dispositivo as Dispositivo | undefined;
  const quien = disp ? etiquetaTelefono(disp) : s("dispositivo_id");
  const err = e.error as { codigo?: string; detalle?: string } | undefined | null;
  switch (e.tipo) {
    case "dispositivo_conectado":
      return { icono: PlugZap, titulo: "Teléfono conectado", detalle: quien, tono: "text-ok" };
    case "dispositivo_desconectado":
      return { icono: Unplug, titulo: "Teléfono desconectado", detalle: quien, tono: "text-aviso" };
    case "dispositivo_actualizado":
      return { icono: RefreshCw, titulo: "Teléfono actualizado", detalle: quien };
    case "chip_cambiado": {
      const num = (k: string) => (e[k] as { numero?: string | null } | null | undefined)?.numero ?? "sin número";
      return {
        icono: RefreshCw,
        titulo: `Chip cambiado · SIM ${Number(e.slot) + 1}`,
        detalle: `${e.anterior ? num("anterior") : "ranura vacía"} → ${num("nuevo")}${e.regreso ? " (ya conocido)" : ""}`,
        tono: "text-aviso",
      };
    }
    case "chip_tipo":
      return {
        icono: RefreshCw,
        titulo: `Chip ${s("tipo") === "dealer" ? "marcado como dealer" : "marcado como normal"} · SIM ${Number(e.slot) + 1}`,
        detalle: quien,
      };
    case "ussd": {
      const accion = ACCIONES_USSD[s("accion") ?? ""] ?? s("accion") ?? "USSD";
      const sim = e.slot != null ? ` · SIM ${Number(e.slot) + 1}` : "";
      return {
        icono: Hash,
        titulo: `${accion} ${s("enviado") ?? ""}${sim}`.trim(),
        detalle: err ? mensajeError(new FalloApi(err.codigo ?? "desconocido", err.detalle ?? "")) : s("texto"),
        tono: err ? "text-error" : undefined,
      };
    }
    case "sms": {
      const sms = (e.sms ?? {}) as { de?: string; texto?: string };
      return { icono: MessageSquare, titulo: `SMS de ${sms.de ?? "?"}`, detalle: sms.texto };
    }
    case "adb":
      return { icono: Cable, titulo: `ADB: ${s("accion") ?? s("estado") ?? ""}`, detalle: s("serial") };
    case "receta": {
      const r = e.resultado as { estado?: string; detalle?: string | null } | undefined;
      if (s("fase") === "inicio")
        return { icono: BookOpen, titulo: `Receta ${s("receta") ?? ""} iniciada${e.simular ? " (simulación)" : ""}` };
      return {
        icono: BookOpen,
        titulo: `Receta terminada: ${r?.estado ?? "?"}`,
        detalle: r?.detalle ?? undefined,
        tono: r?.estado === "incierto" || r?.estado === "error" ? "text-error" : r?.estado === "ok" ? "text-ok" : undefined,
      };
    }
    case "lote": {
      const l = e as unknown as EventoLote;
      const p = l.progreso;
      const avance = p ? `${p.filas - p.pendientes}/${p.filas} filas procesadas · ${p.ok + p.simulado} OK` : undefined;
      switch (l.fase) {
        case "inicio":
          return { icono: Layers, titulo: `Lote #${l.lote_id} iniciado`, detalle: avance, tono: "text-ok" };
        case "fin":
          return {
            icono: Layers,
            titulo: `Lote #${l.lote_id} terminado`,
            detalle: avance,
            tono: p && p.fallidas + p.inciertas + p.parcial > 0 ? "text-aviso" : "text-ok",
          };
        case "pausa":
          return { icono: Layers, titulo: `Lote #${l.lote_id} en pausa`, detalle: l.motivo ?? avance, tono: "text-aviso" };
        case "alerta":
          return {
            icono: Layers,
            titulo: `Lote #${l.lote_id}: envío incierto${l.fila != null ? ` (fila ${l.fila})` : ""}`,
            detalle: l.motivo ?? "Revisa el saldo del chip antes de reintentar.",
            tono: "text-error",
          };
        default:
          return { icono: Layers, titulo: `Lote #${l.lote_id} avanzando`, detalle: avance };
      }
    }
    default:
      return { icono: Activity, titulo: e.tipo };
  }
}

/** Clave estable de un evento (no depende de su posición): así las filas no se remontan con cada evento nuevo. */
function claveDe(e: Evento): string {
  const t = JSON.stringify(e);
  let h = 0;
  for (let i = 0; i < t.length; i++) h = (Math.imul(31, h) + t.charCodeAt(i)) | 0;
  return `${e.ts_ms}-${e.tipo}-${(h >>> 0).toString(36)}`;
}

/** Une dos listas de eventos sin repetir (el historial y lo que llegó en vivo pueden traer el mismo), de más nuevo a más viejo. */
function unir(a: Evento[], b: Evento[]): Evento[] {
  const vistos = new Set<string>();
  const out: Evento[] = [];
  for (const e of [...a, ...b]) {
    const k = claveDe(e);
    if (vistos.has(k) || e.tipo === "perdidos") continue;
    vistos.add(k);
    out.push(e);
  }
  return out.sort((x, y) => y.ts_ms - x.ts_ms).slice(0, MAX);
}

/** Avance de un lote (envíos y filas): muchos por minuto, se juntan en una sola fila. */
const esAvanceLote = (e: Evento) => e.tipo === "lote" && (e.fase === "envio" || e.fase === "fila");

type Filtro = "todo" | "lotes" | "sms" | "recetas" | "ussd" | "sistema";

const CATEGORIA: Record<string, Exclude<Filtro, "todo">> = {
  lote: "lotes",
  sms: "sms",
  receta: "recetas",
  ussd: "ussd",
};
const categoriaDe = (e: Evento): Exclude<Filtro, "todo"> => CATEGORIA[e.tipo] ?? "sistema";

const TILE: Record<string, string> = {
  "text-ok": "bg-ok-suave text-ok",
  "text-aviso": "bg-aviso-suave text-aviso",
  "text-error": "bg-error-suave text-error",
};

interface Fila {
  e: Evento;
  /** Cuántos eventos seguidos del mismo tipo se juntaron en esta fila. */
  n: number;
  hasta: number;
  /** La del evento más viejo del grupo: no cambia cuando se suman eventos nuevos arriba. */
  clave: string;
}

/**
 * Junta lo repetitivo: las "Teléfono actualizado" seguidas del mismo teléfono (el 80% del ruido) y el avance
 * (envío/fila) seguido del mismo lote, que se muestra con el progreso más reciente.
 */
function colapsar(eventos: Evento[]): Fila[] {
  const out: Fila[] = [];
  const id = (x: Evento) => (x.dispositivo as Dispositivo | undefined)?.id ?? x.dispositivo_id;
  for (const e of eventos) {
    const ant = out[out.length - 1];
    const mismoTelefono = ant && e.tipo === "dispositivo_actualizado" && ant.e.tipo === "dispositivo_actualizado" && id(ant.e) === id(e);
    const mismoLote = ant && esAvanceLote(e) && esAvanceLote(ant.e) && ant.e.lote_id === e.lote_id;
    if (mismoTelefono || mismoLote) {
      ant.n += 1;
      ant.hasta = e.ts_ms;
      ant.clave = claveDe(e);
    } else out.push({ e, n: 1, hasta: e.ts_ms, clave: claveDe(e) });
  }
  return out;
}

function etiquetaDia(ms: number): string {
  const d = new Date(ms);
  const hoy = new Date();
  const ayer = new Date();
  ayer.setDate(hoy.getDate() - 1);
  if (d.toDateString() === hoy.toDateString()) return "Hoy";
  if (d.toDateString() === ayer.toDateString()) return "Ayer";
  return d.toLocaleDateString("es-BO", { weekday: "long", day: "numeric", month: "long" });
}

const horaCorta = (ms: number) => new Date(ms).toLocaleTimeString("es-BO", { hour: "2-digit", minute: "2-digit" });

export function Actividad() {
  const [eventos, setEventos] = useState<Evento[]>([]);
  const [cargando, setCargando] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [filtro, setFiltro] = useState<Filtro>("todo");

  const cargar = () => {
    setCargando(true);
    api
      .historial(150)
      .then((h) => {
        setEventos((l) => unir(l, h));
        setError(null);
      })
      .catch((e) => setError(mensajeError(e)))
      .finally(() => setCargando(false));
  };
  useEffect(cargar, []);
  // Tras un corte (o `perdidos`) faltan eventos: se relee el historial.
  useAlReconectar(cargar);

  useEvento((e) => {
    if (e.tipo !== "perdidos") setEventos((l) => unir([e], l));
  });

  const cuenta = (f: Filtro) => (f === "todo" ? eventos.length : eventos.filter((e) => categoriaDe(e) === f).length);
  const filas = useMemo(
    () => colapsar(filtro === "todo" ? eventos : eventos.filter((e) => categoriaDe(e) === filtro)),
    [eventos, filtro],
  );

  return (
    <div>
      <Titulo
        titulo="Actividad"
        sub="Todo lo que pasa en el sistema, en vivo."
        acciones={
          <Boton variante="fantasma" icono={<RefreshCw className="size-4" />} onClick={cargar} cargando={cargando}>
            Recargar historial
          </Boton>
        }
      />
      {error && (
        <div className="mb-4">
          <Aviso>No pude leer el historial: {error}</Aviso>
        </div>
      )}
      <div className="mb-5">
        <Segmentado
          etiqueta="Filtrar actividad"
          valor={filtro}
          onCambio={setFiltro}
          opciones={[
            { id: "todo", texto: "Todo", n: cuenta("todo") },
            { id: "lotes", texto: "Lotes", n: cuenta("lotes") },
            { id: "sms", texto: "SMS", n: cuenta("sms") },
            { id: "recetas", texto: "Recetas", n: cuenta("recetas") },
            { id: "ussd", texto: "USSD", n: cuenta("ussd") },
            { id: "sistema", texto: "Sistema", n: cuenta("sistema") },
          ]}
        />
      </div>

      {cargando && eventos.length === 0 ? (
        <div className="space-y-2" aria-busy>
          {[0, 1, 2, 3, 4, 5].map((i) => (
            <Esqueleto key={i} className="h-16 rounded-2xl" />
          ))}
        </div>
      ) : filas.length === 0 ? (
        <Tarjeta>
          <Vacio icono={<Activity className="size-5" />} titulo={eventos.length === 0 ? "Sin actividad todavía" : "Nada en este filtro"} />
        </Tarjeta>
      ) : (
        <Tarjeta className="overflow-hidden">
          <ul>
            {filas.map(({ e, n, hasta, clave }, i) => {
              const d = describir(e);
              const Icono = d.icono;
              const dia = etiquetaDia(e.ts_ms);
              const nuevoDia = i === 0 || etiquetaDia(filas[i - 1].e.ts_ms) !== dia;
              return (
                <li key={clave}>
                  {nuevoDia && (
                    <p className="sticky top-0 z-10 bg-panel/95 backdrop-blur px-5 py-2 text-[11px] font-semibold uppercase tracking-[0.14em] text-tenue border-b border-borde capitalize">
                      {dia}
                    </p>
                  )}
                  <div className="flex gap-3.5 px-5 py-3 border-b border-borde/60 last:border-b-0 aparecer">
                    <span className={cx("size-9 shrink-0 rounded-xl grid place-items-center", TILE[d.tono ?? ""] ?? "bg-panel-2 text-tenue")}>
                      <Icono className="size-[18px]" />
                    </span>
                    <div className="min-w-0 flex-1">
                      <div className="flex items-baseline justify-between gap-3">
                        <p className="text-sm font-medium truncate">
                          {d.titulo}
                          {n > 1 && (
                            <span className="ml-2 text-xs font-semibold text-tenue tabular-nums">
                              ×{n}
                              {esAvanceLote(e) && " eventos"}
                            </span>
                          )}
                        </p>
                        <time className="text-xs text-tenue tabular-nums shrink-0">
                          {n > 1 ? `${horaCorta(hasta)} – ${horaCorta(e.ts_ms)}` : horaCorta(e.ts_ms)}
                        </time>
                      </div>
                      {d.detalle && (
                        <p className="text-xs text-tenue font-mono mt-0.5 whitespace-pre-wrap break-words line-clamp-3">{d.detalle}</p>
                      )}
                    </div>
                  </div>
                </li>
              );
            })}
          </ul>
        </Tarjeta>
      )}
    </div>
  );
}
