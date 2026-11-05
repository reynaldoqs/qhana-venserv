import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ArrowLeft, Check, Copy, Key, MessageSquare, RefreshCw, Search } from "../componentes/iconos";
import { api, esCancelado, mensajeError } from "../api/cliente";
import { useAlReconectar, useEvento } from "../api/eventos";
import type { Dispositivo, Sms } from "../api/tipos";
import { Aviso, Boton, Campo, Esqueleto, Insignia, Segmentado, Selector, Tarjeta, Titulo, Vacio, cx, useToast } from "../componentes/base";
import { simsConChip } from "../componentes/dispositivo";
import { etiquetaOperador, etiquetaTelefono, fechaHora, hace, nombreSim } from "../util/datos";
import { agrupar, claseDe, codigoDe } from "../util/sms";

type Filtro = "principales" | "codigos" | "promos" | "todos";

/** Cuántos SMS se piden por vez; "Cargar más" agranda el límite (el agente lista del más nuevo al más viejo). */
const PAGINA = 150;

function BotonCopiar({ codigo }: { codigo: string }) {
  const [hecho, setHecho] = useState(false);
  const avisar = useToast();
  return (
    <button
      onClick={async () => {
        try {
          await navigator.clipboard.writeText(codigo);
          setHecho(true);
          avisar("Código copiado", "ok");
          window.setTimeout(() => setHecho(false), 1800);
        } catch {
          avisar("El navegador no dejó copiar", "error");
        }
      }}
      className="inline-flex items-center gap-2 h-9 pl-3.5 pr-3 rounded-full bg-black/25 hover:bg-black/40 transition-colors"
      aria-label={`Copiar código ${codigo}`}
    >
      <Key className="size-4 text-tenue" />
      <span className="font-mono font-semibold tracking-[0.2em] text-base">{codigo}</span>
      {hecho ? <Check className="size-4 text-ok" /> : <Copy className="size-4 text-tenue" />}
    </button>
  );
}

function Burbuja({ m }: { m: Sms }) {
  const codigo = codigoDe(m.texto);
  return (
    <div className="max-w-[85%] rounded-3xl rounded-tl-lg bg-panel-2 px-4 py-3 aparecer">
      {codigo && (
        <div className="mb-2.5">
          <BotonCopiar codigo={codigo} />
        </div>
      )}
      <p className="text-sm whitespace-pre-wrap break-words">{m.texto}</p>
      <div className="mt-2 flex items-center gap-2 text-[11px] text-tenue">
        <time className="tabular-nums">{fechaHora(m.fecha_ms)}</time>
        {m.slot != null && <span>· {nombreSim(m.slot)}</span>}
        {!m.leido && <Insignia tono="info">Nuevo</Insignia>}
      </div>
    </div>
  );
}

export function Mensajes({ dispositivos, inicial }: { dispositivos: Dispositivo[]; inicial?: string }) {
  const conectados = dispositivos.filter((d) => d.conectado);
  const [dispId, setDispId] = useState(inicial ?? "");
  const [sms, setSms] = useState<Sms[]>([]);
  const [cargando, setCargando] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [filtro, setFiltro] = useState<Filtro>("principales");
  const [sel, setSel] = useState<string | null>(null);
  const [verHilo, setVerHilo] = useState(false); // solo cuenta en pantallas chicas
  const disp = conectados.find((d) => d.id === dispId);
  const hilo = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!disp && conectados[0]) setDispId(conectados[0].id);
  }, [disp, conectados]);

  const [slot, setSlot] = useState<number | null>(null);
  const [limite, setLimite] = useState(PAGINA);
  const [hayMas, setHayMas] = useState(false);
  const [buscar, setBuscar] = useState("");
  const ctl = useRef<AbortController | null>(null);

  const cargar = useCallback(async () => {
    if (!dispId) return;
    ctl.current?.abort();
    const c = new AbortController();
    ctl.current = c;
    setCargando(true);
    try {
      const l = await api.sms(dispId, { limite, slot }, c.signal);
      if (c.signal.aborted) return;
      setSms(l);
      setHayMas(l.length >= limite);
      setError(null);
    } catch (e) {
      if (c.signal.aborted || esCancelado(e)) return;
      setError(mensajeError(e));
    } finally {
      if (!c.signal.aborted) setCargando(false);
    }
  }, [dispId, slot, limite]);

  // Otro teléfono o SIM: se empieza de cero.
  useEffect(() => {
    setSms([]);
    setSel(null);
    setLimite(PAGINA);
  }, [dispId, slot]);
  useEffect(() => {
    cargar();
    return () => ctl.current?.abort();
  }, [cargar]);
  useAlReconectar(cargar);

  // Un SMS nuevo de este teléfono (y de la SIM elegida) aparece sin recargar.
  useEvento((e) => {
    if (e.tipo !== "sms" || e.dispositivo_id !== dispId) return;
    const nuevo = e.sms as Sms | undefined;
    if (nuevo && (slot == null || nuevo.slot === slot)) setSms((l) => [nuevo, ...l.filter((x) => x.id !== nuevo.id)]);
  });

  const cuentas = useMemo(
    () => ({
      principales: sms.filter((m) => claseDe(m) !== "promo").length,
      codigos: sms.filter((m) => claseDe(m) === "codigo").length,
      promos: sms.filter((m) => claseDe(m) === "promo").length,
      todos: sms.length,
    }),
    [sms],
  );
  const t = buscar.trim().toLowerCase();
  const visibles = useMemo(
    () =>
      sms.filter(
        (m) =>
          (filtro === "todos" ? true : filtro === "principales" ? claseDe(m) !== "promo" : filtro === "codigos" ? claseDe(m) === "codigo" : claseDe(m) === "promo") &&
          (!t || m.de.toLowerCase().includes(t) || m.texto.toLowerCase().includes(t)),
      ),
    [sms, filtro, t],
  );
  const conversaciones = useMemo(() => agrupar(visibles), [visibles]);
  const actual = conversaciones.find((c) => c.de === sel) ?? conversaciones[0];

  useEffect(() => {
    hilo.current?.scrollTo({ top: hilo.current.scrollHeight });
  }, [actual?.de, actual?.mensajes.length]);

  const moverFoco = (e: React.KeyboardEvent<HTMLUListElement>) => {
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    const botones = [...e.currentTarget.querySelectorAll<HTMLButtonElement>("button")];
    const i = botones.indexOf(document.activeElement as HTMLButtonElement);
    const destino = botones[e.key === "ArrowDown" ? Math.min(i + 1, botones.length - 1) : Math.max(i - 1, 0)];
    if (destino) {
      e.preventDefault();
      destino.focus();
    }
  };

  return (
    <div>
      <Titulo
        titulo="SMS"
        sub="Mensajes recibidos en los chips de cada teléfono, agrupados por remitente."
        acciones={
          <>
            <Selector value={dispId} onChange={(e) => setDispId(e.target.value)} className="w-64" aria-label="Teléfono">
              {conectados.length === 0 && <option value="">Sin teléfonos en línea</option>}
              {conectados.map((d) => (
                <option key={d.id} value={d.id}>
                  {etiquetaTelefono(d)}
                </option>
              ))}
            </Selector>
            <Boton variante="fantasma" icono={<RefreshCw className="size-4" />} onClick={cargar} cargando={cargando}>
              Actualizar
            </Boton>
          </>
        }
      />
      {error && (
        <div className="mb-4">
          <Aviso>{error}</Aviso>
        </div>
      )}

      <div className="mb-5 flex flex-wrap items-center gap-3">
        <Segmentado
          etiqueta="Filtrar mensajes"
          valor={filtro}
          onCambio={(f) => {
            setFiltro(f);
            setSel(null);
          }}
          opciones={[
            { id: "principales", texto: "Principales", n: cuentas.principales },
            { id: "codigos", texto: "Códigos", n: cuentas.codigos },
            { id: "promos", texto: "Promos", n: cuentas.promos },
            { id: "todos", texto: "Todos", n: cuentas.todos },
          ]}
        />
        {disp && simsConChip(disp).length > 1 && (
          <Selector
            aria-label="SIM"
            className="w-48"
            value={slot ?? ""}
            onChange={(e) => setSlot(e.target.value === "" ? null : Number(e.target.value))}
          >
            <option value="">Todas las SIM</option>
            {simsConChip(disp).map((s) => (
              <option key={s.slot} value={s.slot}>
                {nombreSim(s.slot)} · {etiquetaOperador(s)}
              </option>
            ))}
          </Selector>
        )}
        <div className="relative w-full sm:w-64 sm:ml-auto">
          <Search className="size-4 absolute left-3 top-1/2 -translate-y-1/2 text-tenue pointer-events-none" />
          <Campo
            type="search"
            aria-label="Buscar por remitente o texto"
            placeholder="Remitente o texto"
            value={buscar}
            onChange={(e) => setBuscar(e.target.value)}
            className="pl-9"
          />
        </div>
      </div>

      {cargando && sms.length === 0 ? (
        <div className="grid lg:grid-cols-[340px_1fr] gap-4" aria-busy>
          <div className="space-y-2">
            {[0, 1, 2, 3, 4].map((i) => (
              <Esqueleto key={i} className="h-[72px] rounded-2xl" />
            ))}
          </div>
          <Esqueleto className="h-96 rounded-3xl hidden lg:block" />
        </div>
      ) : conversaciones.length === 0 ? (
        <Tarjeta>
          <Vacio
            icono={<MessageSquare className="size-5" />}
            titulo={sms.length === 0 ? "Sin mensajes" : t ? `Nada coincide con «${buscar.trim()}»` : "Nada en este filtro"}
            texto={sms.length === 0 || t ? undefined : "Prueba con “Todos” para ver también la publicidad y los códigos."}
          />
        </Tarjeta>
      ) : (
        <div className="grid lg:grid-cols-[340px_1fr] gap-4 lg:h-[calc(100vh-24rem)] lg:min-h-[420px]">
          <Tarjeta className={cx("overflow-y-auto p-2", verHilo && "hidden lg:block")}>
            <ul onKeyDown={moverFoco} className="space-y-1">
              {conversaciones.map((c) => {
                const activa = actual?.de === c.de;
                return (
                  <li key={c.de}>
                    <button
                      onClick={() => {
                        setSel(c.de);
                        setVerHilo(true);
                      }}
                      aria-current={activa ? "true" : undefined}
                      className={cx(
                        "w-full text-left flex gap-3 rounded-2xl px-3 py-3 transition-colors",
                        activa ? "bg-panel-2" : "hover:bg-panel-2/60",
                      )}
                    >
                      <span className="size-10 shrink-0 rounded-xl bg-black/25 grid place-items-center text-xs font-bold text-tenue font-mono">
                        {c.de.replace(/[^\p{L}\p{N}]/gu, "").slice(0, 3).toUpperCase() || "?"}
                      </span>
                      <span className="min-w-0 flex-1">
                        <span className="flex items-baseline justify-between gap-2">
                          <span className="font-semibold font-mono truncate">{c.de}</span>
                          <time className="text-[11px] text-tenue shrink-0">{hace(c.ultimo.fecha_ms)}</time>
                        </span>
                        <span className="flex items-center justify-between gap-2 mt-0.5">
                          <span className="text-xs text-tenue truncate">{c.ultimo.texto}</span>
                          {c.sinLeer > 0 && (
                            <span className="shrink-0 min-w-5 h-5 px-1.5 rounded-full bg-info text-sobre-acento text-[11px] font-bold grid place-items-center tabular-nums">
                              {c.sinLeer}
                            </span>
                          )}
                        </span>
                      </span>
                    </button>
                  </li>
                );
              })}
            </ul>
            {hayMas && (
              <div className="p-2">
                <Boton variante="secundario" className="w-full h-9" cargando={cargando} onClick={() => setLimite((l) => l + PAGINA)}>
                  Cargar mensajes más viejos
                </Boton>
              </div>
            )}
          </Tarjeta>

          {actual && (
            <Tarjeta className={cx("flex flex-col overflow-hidden min-h-0", !verHilo && "hidden lg:flex")}>
              <div className="flex items-center gap-3 px-5 h-16 border-b border-borde shrink-0">
                <button
                  onClick={() => setVerHilo(false)}
                  aria-label="Volver a la lista"
                  className="lg:hidden size-9 -ml-2 rounded-full grid place-items-center text-tenue hover:text-texto"
                >
                  <ArrowLeft className="size-5" />
                </button>
                <div className="min-w-0">
                  <p className="font-semibold font-mono truncate">{actual.de}</p>
                  <p className="text-xs text-tenue">
                    {actual.mensajes.length} mensaje{actual.mensajes.length === 1 ? "" : "s"}
                    {actual.sinLeer > 0 && ` · ${actual.sinLeer} sin leer`}
                  </p>
                </div>
              </div>
              <div ref={hilo} className="flex-1 overflow-y-auto p-5 space-y-3 max-h-[70vh] lg:max-h-none">
                {actual.mensajes.map((m) => (
                  <Burbuja key={m.id} m={m} />
                ))}
              </div>
            </Tarjeta>
          )}
        </div>
      )}
    </div>
  );
}
