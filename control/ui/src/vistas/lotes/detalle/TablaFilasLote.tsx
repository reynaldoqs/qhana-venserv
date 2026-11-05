import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, esCancelado, FalloApi, mensajeError } from "../../../api/cliente";
import { useAlReconectar, useEvento } from "../../../api/eventos";
import type { Dispositivo, EstadoFilaLote, EventoLote, FilaLote, Lote } from "../../../api/tipos";
import { Aviso, Boton, Campo, Segmentado, Tarjeta, cx } from "../../../componentes/base";
import { Search } from "../../../componentes/iconos";
import { sinPrefijo, useRetrasado } from "../../../util/datos";
import { EstadoFila, GRUPOS_FILA, bs, conteoGrupos, nombreChip, type FiltroFilas } from "../comunes";

const PAGINA = 200;

const COLOR_ENVIO: Record<string, string> = {
  ok: "bg-ok",
  simulado: "bg-info",
  incierto: "bg-error",
  fallido: "bg-error",
  error: "bg-error",
  abortado: "bg-aviso",
  en_curso: "bg-info animate-pulse",
  pendiente: "bg-borde",
};

const TEXTO_ENVIO: Record<string, string> = {
  ok: "ok",
  simulado: "simulado",
  incierto: "incierto",
  fallido: "fallido",
  abortado: "abortado",
  en_curso: "en curso",
  pendiente: "pendiente",
  sin_capacidad: "para mañana",
};

function Envios({ f, dispositivos }: { f: FilaLote; dispositivos: Dispositivo[] }) {
  if (f.envios.length === 0) return <span className="text-tenue">—</span>;
  return (
    <ul className="flex flex-col gap-1">
      {f.envios.map((e, i) => (
        <li key={i} className="flex items-center gap-2 text-xs" title={e.detalle ?? e.estado}>
          <span className={cx("size-2 rounded-full shrink-0", COLOR_ENVIO[e.estado] ?? "bg-borde")} aria-hidden />
          <span className="font-mono tabular-nums w-8">{e.monto}</span>
          <span className="font-mono text-tenue">{sinPrefijo(e.numero_origen) ?? nombreChip(dispositivos, e.dispositivo, e.slot)}</span>
          <span className="text-tenue">
            {TEXTO_ENVIO[e.estado] ?? e.estado}
            {(e.intentos ?? 1) > 1 && ` · ${e.intentos} intentos`}
          </span>
        </li>
      ))}
    </ul>
  );
}

/** Aplica un evento `fila`/`envio` a la fila cargada, si está. Devuelve null si no se pudo (hay que releer). */
function parchar(filas: FilaLote[], ev: EventoLote): FilaLote[] | null {
  const i = filas.findIndex((f) => f.fila === ev.fila);
  if (i < 0) return filas; // no está en lo que se ve: nada que hacer
  const f = filas[i];
  let nueva: FilaLote;
  if (ev.fase === "fila" && ev.estado) {
    nueva = { ...f, estado: ev.estado as EstadoFilaLote };
  } else if (ev.fase === "envio" && ev.estado) {
    // El envío se reconoce por el monto entre los que aún no terminaron; si es ambiguo, se relee.
    const candidatos = f.envios
      .map((e, j) => [e, j] as const)
      .filter(([e]) => e.monto === ev.monto && (e.estado === "pendiente" || e.estado === "en_curso" || e.estado === ev.estado));
    if (candidatos.length === 0) return null;
    const j = (candidatos.find(([e]) => e.estado === "en_curso") ?? candidatos[0])[1];
    const envios = f.envios.map((e, k) =>
      k === j ? { ...e, estado: ev.estado!, dispositivo: ev.dispositivo_id ?? e.dispositivo, slot: ev.slot ?? e.slot, ts_ms: ev.ts_ms } : e,
    );
    nueva = { ...f, envios, estado: f.estado === "pendiente" && ev.estado === "en_curso" ? "en_curso" : f.estado };
  } else return filas;
  const out = filas.slice();
  out[i] = nueva;
  return out;
}

/**
 * Filas del lote por páginas (`/api/lotes/{id}/filas`), con filtro por estado y búsqueda.
 * Los eventos `fila`/`envio` parchan las filas cargadas; mientras corre, se relee lo cargado cada 5 s.
 * Con un servidor viejo (sin esa ruta) filtra en el navegador las filas que trae `GET /api/lotes/{id}`.
 */
export function TablaFilasLote({ lote, dispositivos }: { lote: Lote; dispositivos: Dispositivo[] }) {
  const [filtro, setFiltro] = useState<FiltroFilas>("todas");
  const [buscar, setBuscar] = useState("");
  const q = useRetrasado(buscar.trim(), 300);
  const [filas, setFilas] = useState<FilaLote[]>([]);
  const [siguiente, setSiguiente] = useState<number | null>(null);
  const [total, setTotal] = useState(0);
  const [cargando, setCargando] = useState(true);
  const [masCargando, setMasCargando] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [viejo, setViejo] = useState(false);
  const ctl = useRef<AbortController | null>(null);
  const cargadas = useRef(0);
  cargadas.current = filas.length;
  const actuales = useRef(filas);
  actuales.current = filas;

  const estados = filtro === "todas" ? undefined : GRUPOS_FILA[filtro];

  /** Lee desde el principio; `cuantas` permite releer todo lo ya cargado (refresco) en vez de solo la primera página. */
  const cargar = useCallback(
    async (cuantas = PAGINA) => {
      ctl.current?.abort();
      const c = new AbortController();
      ctl.current = c;
      try {
        const r = await api.filasLote(lote.id, { estado: estados, q: q || undefined, limite: Math.min(1000, Math.max(PAGINA, cuantas)) }, c.signal);
        if (c.signal.aborted) return;
        setFilas(r.filas);
        setSiguiente(r.siguiente);
        setTotal(r.total);
        setError(null);
        setViejo(false);
      } catch (e) {
        if (c.signal.aborted || esCancelado(e)) return;
        if (e instanceof FalloApi && e.http === 404 && lote.filas) setViejo(true);
        else setError(mensajeError(e));
      } finally {
        if (!c.signal.aborted) setCargando(false);
      }
    },
    [lote.id, estados?.join(","), q, !!lote.filas], // eslint-disable-line react-hooks/exhaustive-deps
  );

  useEffect(() => {
    setCargando(true);
    cargar();
    return () => ctl.current?.abort();
  }, [cargar]);

  const clave = `${estados?.join(",")}|${q}`;
  const claveRef = useRef(clave);
  claveRef.current = clave;
  const masAntes = async () => {
    if (siguiente == null) return;
    setMasCargando(true);
    const pedida = clave;
    try {
      const r = await api.filasLote(lote.id, { estado: estados, q: q || undefined, despues: siguiente, limite: PAGINA });
      if (pedida !== claveRef.current) return; // cambió el filtro mientras tanto
      setFilas((l) => [...l, ...r.filas.filter((f) => !l.some((x) => x.fila === f.fila))]);
      setSiguiente(r.siguiente);
      setTotal(r.total);
    } catch (e) {
      setError(mensajeError(e));
    } finally {
      setMasCargando(false);
    }
  };

  // Mientras corre, relee lo que ya se ve cada 5 s (por si se perdió algún evento).
  useEffect(() => {
    if (lote.estado !== "en_curso" || viejo) return;
    const t = window.setInterval(() => cargar(cargadas.current), 5000);
    return () => window.clearInterval(t);
  }, [lote.estado, cargar, viejo]);

  // Al terminar, pausar o cambiar el lote (reintentar, verificar…), se relee todo lo cargado.
  const firma = `${lote.estado}|${lote.progreso.pendientes}|${lote.progreso.fallidas}|${lote.progreso.inciertas}`;
  const primera = useRef(true);
  useEffect(() => {
    if (primera.current) {
      primera.current = false;
      return;
    }
    if (lote.estado !== "en_curso") cargar(cargadas.current);
  }, [firma]); // eslint-disable-line react-hooks/exhaustive-deps

  useEvento((e) => {
    if (e.tipo !== "lote") return;
    const ev = e as unknown as EventoLote;
    if (Number(ev.lote_id) !== lote.id || ev.fila == null || (ev.fase !== "fila" && ev.fase !== "envio")) return;
    const p = parchar(actuales.current, ev);
    if (p === null) cargar(cargadas.current);
    else if (p !== actuales.current) {
      actuales.current = p;
      setFilas(p);
    }
  });
  useAlReconectar(() => cargar(cargadas.current));

  // Servidor viejo: filtra en el navegador.
  const locales = useMemo(() => {
    if (!viejo || !lote.filas) return null;
    const t = q.toLowerCase();
    return lote.filas.filter(
      (f) =>
        (!estados || estados.includes(f.estado)) &&
        (!t || (f.numero ?? "").includes(t) || (f.referencia ?? "").toLowerCase().includes(t) || String(f.fila) === t),
    );
  }, [viejo, lote.filas, estados?.join(","), q]); // eslint-disable-line react-hooks/exhaustive-deps

  const lista = locales ?? filas;
  const totalLista = locales ? locales.length : total;
  const n = conteoGrupos(lote.progreso);
  const opciones: { id: FiltroFilas; texto: string; n: number }[] = [
    { id: "todas", texto: "Todas", n: n.todas },
    { id: "pendientes", texto: "Pendientes", n: n.pendientes },
    { id: "hechas", texto: lote.simular ? "Simuladas" : "Hechas", n: n.hechas },
    { id: "problemas", texto: "Con problema", n: n.problemas },
    { id: "mañana", texto: "Para mañana", n: n.mañana },
    { id: "omitidas", texto: "Omitidas", n: n.omitidas },
  ];

  return (
    <Tarjeta className="overflow-hidden">
      <div className="flex flex-wrap items-center justify-between gap-2 p-2 border-b border-borde">
        <Segmentado
          etiqueta="Filtrar filas"
          controla={`filas-${lote.id}`}
          valor={filtro}
          onCambio={setFiltro}
          opciones={opciones.filter((o) => o.id === "todas" || o.id === filtro || o.n > 0)}
          className="border-0 bg-transparent p-0"
        />
        <div className="relative w-full sm:w-64">
          <Search className="size-4 absolute left-3 top-1/2 -translate-y-1/2 text-tenue pointer-events-none" />
          <Campo
            type="search"
            aria-label="Buscar por número o referencia"
            placeholder="Número o referencia"
            value={buscar}
            onChange={(e) => setBuscar(e.target.value)}
            className="pl-9 h-9"
          />
        </div>
      </div>
      {error && (
        <div className="p-3">
          <Aviso>No pude leer las filas: {error}</Aviso>
        </div>
      )}
      <div id={`filas-${lote.id}`} role="tabpanel" className="overflow-auto max-h-[70vh]" aria-busy={cargando}>
        <table className="w-full text-sm">
          <thead className="sticky top-0 z-10 bg-panel">
            <tr className="text-left text-xs text-tenue border-b border-borde">
              <th scope="col" className="font-medium px-4 py-2.5 w-14">Fila</th>
              <th scope="col" className="font-medium px-3 py-2.5">Número</th>
              <th scope="col" className="font-medium px-3 py-2.5">Operador</th>
              <th scope="col" className="font-medium px-3 py-2.5 text-right">Monto</th>
              <th scope="col" className="font-medium px-3 py-2.5 text-right">Enviado</th>
              <th scope="col" className="font-medium px-3 py-2.5">Envíos</th>
              <th scope="col" className="font-medium px-3 py-2.5">Estado</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-borde">
            {lista.map((f) => (
              <tr key={f.fila} className={cx(f.estado === "en_curso" && "bg-info-suave", f.estado === "incierta" && "bg-error-suave/50")}>
                <td className="px-4 py-2.5 text-tenue tabular-nums align-top">{f.fila}</td>
                <td className="px-3 py-2.5 font-mono align-top">
                  {f.numero ?? "—"}
                  {f.referencia && <div className="text-xs text-tenue font-sans">{f.referencia}</div>}
                </td>
                <td className="px-3 py-2.5 align-top">{f.operador ?? "—"}</td>
                <td className="px-3 py-2.5 text-right tabular-nums align-top">{bs(f.monto)}</td>
                <td className="px-3 py-2.5 text-right tabular-nums align-top">{bs(f.enviado_bs)}</td>
                <td className="px-3 py-2.5 align-top">
                  <Envios f={f} dispositivos={dispositivos} />
                </td>
                <td className="px-3 py-2.5 align-top">
                  <EstadoFila estado={f.estado} />
                  {f.detalle && <div className="text-xs text-tenue mt-1 max-w-72">{f.detalle}</div>}
                </td>
              </tr>
            ))}
            {!cargando && lista.length === 0 && (
              <tr>
                <td colSpan={7} className="px-4 py-10 text-center text-tenue">
                  {q ? `Ninguna fila coincide con «${q}»` : "Nada en esta vista"}
                </td>
              </tr>
            )}
            {cargando && lista.length === 0 && (
              <tr>
                <td colSpan={7} className="px-4 py-10 text-center text-tenue">
                  Cargando filas…
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
      {lista.length > 0 && (
        <div className="flex flex-wrap items-center justify-between gap-3 px-4 py-3 border-t border-borde text-xs text-tenue">
          <span className="tabular-nums">
            {lista.length === totalLista ? `${totalLista} filas` : `Mostrando ${lista.length} de ${totalLista} filas`}
          </span>
          {!locales && siguiente != null && (
            <Boton variante="secundario" className="h-8 px-4" cargando={masCargando} onClick={masAntes}>
              Cargar {Math.min(PAGINA, Math.max(0, totalLista - lista.length)) || PAGINA} más
            </Boton>
          )}
        </div>
      )}
    </Tarjeta>
  );
}
