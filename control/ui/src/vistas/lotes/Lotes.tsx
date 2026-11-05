import { useCallback, useEffect, useRef, useState } from "react";
import { Layers, Plus, Search } from "../../componentes/iconos";
import { api, esCancelado, mensajeError } from "../../api/cliente";
import { useAlReconectar, useEvento } from "../../api/eventos";
import type { ConteosLotes, Dispositivo, EventoLote, FiltroLotes, Lote } from "../../api/tipos";
import { Aviso, Boton, Campo, Esqueleto, Insignia, Punto, Segmentado, Tarjeta, Titulo, Vacio, cx } from "../../componentes/base";
import { fechaHora, useAgrupar, usePreferencia, useRetrasado } from "../../util/datos";
import { BarraProgreso, EstadoLoteInsignia, LeyendaProgreso, bs, filasBien, procesadas } from "./comunes";
import { NuevoLote } from "./NuevoLote";
import { DetalleLote } from "./DetalleLote";

const PESTANAS = ["todos", "activos", "problemas", "terminados", "simulaciones", "archivados"] as const;
type Pestana = (typeof PESTANAS)[number];

const FILTRO: Record<Pestana, FiltroLotes> = {
  todos: {},
  activos: { estado: ["listo", "en_curso", "pausado"] },
  problemas: { problemas: true },
  terminados: { estado: ["terminado", "cancelado"] },
  simulaciones: { simular: true },
  archivados: { archivados: true },
};

const PAGINA = 50;

/** Para un servidor viejo que no filtra (responde todos los lotes): el mismo filtro, en el navegador. */
function cumple(l: Lote, f: FiltroLotes, q: string): boolean {
  const p = l.progreso;
  if (f.estado && !f.estado.includes(l.estado)) return false;
  if (f.problemas && p.fallidas + p.inciertas + p.parcial === 0) return false;
  if (f.simular !== undefined && l.simular !== f.simular) return false;
  if ((f.archivados === true) !== !!l.archivado && f.archivados !== "todos") return false;
  if (q) {
    const t = q.replace(/^#/, "");
    if (String(l.id) !== t && !l.nombre.toLowerCase().includes(q.toLowerCase())) return false;
  }
  return true;
}
/** Fases que cambian el estado del lote (o lo crean): ahí se relee la lista. El resto solo mueve el progreso. */
const FASES_RECARGA = new Set(["inicio", "fin", "pausa"]);

function FilaLote({ l, abrir, i }: { l: Lote; abrir: (id: number) => void; i: number }) {
  const p = l.progreso;
  const corriendo = l.estado === "en_curso";
  return (
    <li style={{ "--i": Math.min(i, 8) } as React.CSSProperties}>
      <button
        onClick={() => abrir(l.id)}
        className={cx(
          "w-full text-left rounded-3xl bg-panel border p-5 transition-[transform,border-color,box-shadow] duration-300 ease-salida hover:-translate-y-0.5 hover:shadow-elevada grid gap-4 md:grid-cols-[1fr_minmax(220px,320px)_auto] md:items-center",
          corriendo ? "border-info/40 hover:border-info/60" : l.estado === "pausado" ? "border-aviso/40 hover:border-aviso/60" : "border-borde/60 hover:border-tenue/40",
        )}
      >
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-2">
            {corriendo && <Punto tono="info" pulso />}
            <span className="font-semibold truncate">{l.nombre}</span>
            {l.simular ? <Insignia tono="info">Simulación</Insignia> : <Insignia tono="aviso">Real</Insignia>}
            {l.archivado && <Insignia>Archivado</Insignia>}
            {l.verificacion && (
              <Insignia tono={l.verificacion.confirmado ? "ok" : "error"}>{l.verificacion.confirmado ? "Verificado" : "Revisar saldos"}</Insignia>
            )}
          </div>
          <p className="text-xs text-tenue mt-1">
            #{l.id} · {fechaHora(l.creado_ms)} · {p.filas} filas · {bs(p.total_bs)}
            {l.motivo_pausa && <span className="text-aviso"> · {l.motivo_pausa}</span>}
          </p>
        </div>
        <div className="space-y-2">
          <BarraProgreso p={p} />
          <LeyendaProgreso p={p} />
        </div>
        <div className="flex md:flex-col items-center md:items-end justify-between gap-2">
          <p className="text-right leading-none" title={`${procesadas(p)} de ${p.filas} filas procesadas`}>
            <span className="text-2xl font-extrabold tabular-nums tracking-tight">{filasBien(p)}</span>
            <span className="text-sm font-medium text-tenue tabular-nums"> / {p.filas}</span>
            <span className="block text-[11px] text-tenue mt-1">{l.simular ? "filas simuladas" : "filas OK"}</span>
          </p>
          <EstadoLoteInsignia estado={l.estado} />
        </div>
      </button>
    </li>
  );
}

function ListaLotes({ nuevo, abrir }: { nuevo: () => void; abrir: (id: number) => void }) {
  const [pestana, setPestana] = usePreferencia<Pestana>("qhana.lotes.pestana", "todos", PESTANAS);
  const [buscar, setBuscar] = useState("");
  const q = useRetrasado(buscar.trim(), 300);
  const [lotes, setLotes] = useState<Lote[]>([]);
  const [enMarcha, setEnMarcha] = useState<Lote[]>([]);
  const [siguiente, setSiguiente] = useState<number | null>(null);
  const [total, setTotal] = useState(0);
  const [conteos, setConteos] = useState<ConteosLotes | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [cargando, setCargando] = useState(true);
  const [masCargando, setMasCargando] = useState(false);
  const ctl = useRef<AbortController | null>(null);
  const cuantos = useRef(PAGINA);
  cuantos.current = Math.max(PAGINA, lotes.length);

  /** Relee la primera página (o todo lo cargado, al refrescar) y, aparte, los lotes en curso o en pausa para fijarlos arriba. */
  const cargar = useCallback(
    async (limite = PAGINA) => {
      ctl.current?.abort();
      const c = new AbortController();
      ctl.current = c;
      try {
        const [pagina, marcha] = await Promise.all([
          api.lotes({ ...FILTRO[pestana], q: q || undefined, limite: Math.min(200, limite) }, c.signal),
          q ? null : api.lotes({ estado: ["en_curso", "pausado"], limite: 50 }, c.signal).catch(() => null),
        ]);
        if (c.signal.aborted) return;
        if (!pagina.conteos) {
          pagina.lotes = pagina.lotes.filter((l) => cumple(l, FILTRO[pestana], q));
          pagina.total = pagina.lotes.length;
        }
        setLotes(pagina.lotes);
        setSiguiente(pagina.siguiente);
        setTotal(pagina.total);
        if (pagina.conteos) setConteos(pagina.conteos);
        // Servidor viejo (sin filtros): la "página" trae todos; se filtran aquí los en marcha.
        setEnMarcha(marcha ? marcha.lotes.filter((l) => l.estado === "en_curso" || l.estado === "pausado") : []);
        setError(null);
      } catch (e) {
        if (c.signal.aborted || esCancelado(e)) return;
        setError(mensajeError(e));
      } finally {
        if (!c.signal.aborted) setCargando(false);
      }
    },
    [pestana, q],
  );

  useEffect(() => {
    setCargando(true);
    cargar();
    return () => ctl.current?.abort();
  }, [cargar]);

  const masAntes = async () => {
    if (siguiente == null) return;
    setMasCargando(true);
    const pedido = `${pestana}|${q}`;
    try {
      const r = await api.lotes({ ...FILTRO[pestana], q: q || undefined, antes: siguiente, limite: PAGINA });
      if (pedido !== `${pestanaRef.current}|${qRef.current}`) return;
      setLotes((l) => [...l, ...r.lotes.filter((x) => !l.some((y) => y.id === x.id))]);
      setSiguiente(r.siguiente);
      setTotal(r.total);
    } catch (e) {
      setError(mensajeError(e));
    } finally {
      setMasCargando(false);
    }
  };
  const pestanaRef = useRef(pestana);
  pestanaRef.current = pestana;
  const qRef = useRef(q);
  qRef.current = q;

  // Cada evento trae el progreso: se parcha el lote en su lugar. Solo inicio/fin/pausa cambian el estado y releen.
  const recargar = useAgrupar(() => cargar(cuantos.current), 400);
  useEvento((e) => {
    if (e.tipo !== "lote") return;
    const ev = e as unknown as EventoLote;
    const id = Number(ev.lote_id);
    if (ev.progreso) {
      const parche = (l: Lote) => (l.id === id ? { ...l, progreso: ev.progreso! } : l);
      setLotes((ls) => ls.map(parche));
      setEnMarcha((ls) => ls.map(parche));
    }
    if (FASES_RECARGA.has(ev.fase) || !ev.progreso) recargar();
  });
  useAlReconectar(() => cargar(cuantos.current));

  const fijados = new Set(enMarcha.map((l) => l.id));
  const mostrarMarcha = enMarcha.length > 0 && pestana !== "archivados" && pestana !== "terminados";
  const resto = mostrarMarcha ? lotes.filter((l) => !fijados.has(l.id)) : lotes;
  const c = conteos;
  const opciones: { id: Pestana; texto: string; n: number | null }[] = [
    { id: "todos", texto: "Todos", n: c ? c.activos + c.terminados : null },
    { id: "activos", texto: "Activos", n: c?.activos ?? null },
    { id: "problemas", texto: "Con problema", n: c?.problemas ?? null },
    { id: "terminados", texto: "Terminados", n: c?.terminados ?? null },
    { id: "simulaciones", texto: "Simulaciones", n: c?.simulaciones ?? null },
    { id: "archivados", texto: "Archivados", n: c?.archivados ?? null },
  ];
  const vacioTotal = !cargando && !q && pestana === "todos" && lotes.length === 0;

  return (
    <div>
      <Titulo
        titulo="Lotes"
        sub="Operaciones en lote desde un CSV: se validan, se reparten entre los chips y se reportan."
        acciones={
          <Boton variante="primario" icono={<Plus className="size-4" />} onClick={nuevo}>
            Nuevo lote
          </Boton>
        }
      />
      {error && (
        <div className="mb-4">
          <Aviso>{error}</Aviso>
        </div>
      )}

      {mostrarMarcha && (
        <section aria-labelledby="lotes-en-marcha" className="mb-7">
          <h2 id="lotes-en-marcha" className="text-sm font-semibold mb-3">
            En marcha <span className="text-tenue font-medium tabular-nums">{enMarcha.length}</span>
          </h2>
          <ul className="space-y-3">
            {enMarcha.map((l, i) => (
              <FilaLote key={l.id} l={l} i={i} abrir={abrir} />
            ))}
          </ul>
        </section>
      )}

      <div className="flex flex-wrap items-center justify-between gap-3 mb-4">
        <Segmentado etiqueta="Filtrar lotes" controla="lista-lotes" valor={pestana} onCambio={setPestana} opciones={opciones} />
        <div className="relative w-full sm:w-72">
          <Search className="size-4 absolute left-3 top-1/2 -translate-y-1/2 text-tenue pointer-events-none" />
          <Campo
            type="search"
            aria-label="Buscar lotes por nombre o número"
            placeholder="Nombre o #número"
            value={buscar}
            onChange={(e) => setBuscar(e.target.value)}
            className="pl-9"
          />
        </div>
      </div>

      <div id="lista-lotes" role="tabpanel" aria-busy={cargando}>
        {cargando && lotes.length === 0 ? (
          <div className="space-y-3">
            <Esqueleto className="h-24 rounded-3xl" />
            <Esqueleto className="h-24 rounded-3xl" />
          </div>
        ) : vacioTotal ? (
          <Tarjeta>
            <Vacio
              icono={<Layers className="size-5" />}
              titulo="Todavía no hay lotes"
              texto="Sube un CSV con número, monto y operador. Puedes simularlo primero sin mover saldo."
            />
          </Tarjeta>
        ) : resto.length === 0 ? (
          <Tarjeta>
            <Vacio
              icono={<Layers className="size-5" />}
              titulo={q ? `Ningún lote coincide con «${q}»` : mostrarMarcha && lotes.length > 0 ? "Todos están en marcha (arriba)" : "Nada en este filtro"}
            />
          </Tarjeta>
        ) : (
          <>
            <ul className="space-y-3 escalonar">
              {resto.map((l, i) => (
                <FilaLote key={l.id} l={l} i={i} abrir={abrir} />
              ))}
            </ul>
            <div className="mt-4 flex flex-wrap items-center justify-between gap-3 text-xs text-tenue">
              <span className="tabular-nums">
                {lotes.length >= total ? `${total} lote${total === 1 ? "" : "s"}` : `Mostrando ${lotes.length} de ${total} lotes`}
              </span>
              {siguiente != null && (
                <Boton variante="secundario" className="h-9" cargando={masCargando} onClick={masAntes}>
                  Cargar más
                </Boton>
              )}
            </div>
          </>
        )}
        {!vacioTotal && (
          <button
            onClick={nuevo}
            className="mt-4 w-full h-16 rounded-3xl border-2 border-dashed border-borde text-sm font-medium text-tenue hover:text-texto hover:border-tenue/60 hover:bg-panel transition-colors inline-flex items-center justify-center gap-2"
          >
            <Plus className="size-4" /> Nuevo lote
          </button>
        )}
      </div>
    </div>
  );
}

/** Subrutas: #lotes, #lotes/nuevo, #lotes/12 */
export function Lotes({ ruta, ir, dispositivos }: { ruta: string; ir: (r: string) => void; dispositivos: Dispositivo[] }) {
  if (ruta === "nuevo") return <NuevoLote dispositivos={dispositivos} volver={() => ir("")} abrirLote={(id) => ir(String(id))} />;
  const id = Number(ruta);
  if (ruta && Number.isFinite(id)) return <DetalleLote key={id} id={id} dispositivos={dispositivos} volver={() => ir("")} />;
  return <ListaLotes nuevo={() => ir("nuevo")} abrir={(id) => ir(String(id))} />;
}
