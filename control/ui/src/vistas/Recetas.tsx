import { useEffect, useMemo, useState } from "react";
import { BookOpen, CheckCircle2, CircleAlert, CircleDashed, FlaskConical, Play, TriangleAlert } from "../componentes/iconos";
import { api, FalloApi } from "../api/cliente";
import type { Dispositivo, EstadoResultado, Receta, Resultado } from "../api/tipos";
import { Aviso, Boton, Campo, Insignia, Interruptor, Selector, Tarjeta, Titulo, Vacio, cx } from "../componentes/base";
import { etiquetaOperador, nombreSim, operadorDe, segundos, useRecetas } from "../util/datos";
import { simsConChip } from "../componentes/dispositivo";

const estadoUi: Record<EstadoResultado, { tono: "ok" | "aviso" | "error" | "info"; texto: string; icono: typeof CheckCircle2 }> = {
  ok: { tono: "ok", texto: "Completada", icono: CheckCircle2 },
  simulado: { tono: "info", texto: "Simulada", icono: FlaskConical },
  abortado: { tono: "aviso", texto: "Abortada sin mover nada", icono: CircleDashed },
  incierto: { tono: "error", texto: "Incierta: revisar saldo", icono: TriangleAlert },
  error: { tono: "error", texto: "Error", icono: CircleAlert },
};

const FILTROS = ["Todas", "Tigo", "Entel", "Viva"] as const;
type Filtro = (typeof FILTROS)[number];
const deFiltro = (r: Receta, f: Filtro) => f === "Todas" || r.operador.toUpperCase().includes(f.toUpperCase());

const colorTono = { ok: "text-ok", aviso: "text-aviso", error: "text-error", info: "text-info" } as const;

function validar(r: Receta, valores: Record<string, string>): string | null {
  for (const p of r.parametros) {
    const v = (valores[p.nombre] ?? "").trim();
    if (!v) return `Falta ${p.nombre}`;
    if (p.opciones?.length && !p.opciones.includes(v)) return `${p.nombre} debe ser uno de: ${p.opciones.join(", ")}`;
    if (p.patron && !new RegExp(p.patron).test(v)) return `${p.nombre} no tiene el formato esperado`;
  }
  return null;
}

function VistaResultado({ r }: { r: Resultado }) {
  const e = estadoUi[r.estado] ?? estadoUi.error;
  const Icono = e.icono;
  const total = r.pasos.reduce((s, p) => s + (p.ms ?? 0), 0);
  return (
    <Tarjeta className="p-5 aparecer">
      <div className="flex items-center justify-between gap-3">
        <div className="flex items-center gap-2.5">
          <Icono className={cx("size-5", colorTono[e.tono])} />
          <span className="font-semibold">{e.texto}</span>
        </div>
        <span className="text-xs text-tenue tabular-nums">{segundos(total)}</span>
      </div>
      {r.detalle && <p className="mt-2 text-sm text-tenue">{r.detalle}</p>}

      {Object.keys(r.salida).length > 0 && (
        <dl className="mt-4 grid grid-cols-[auto_1fr] gap-x-6 gap-y-1.5 text-sm">
          {Object.entries(r.salida).map(([k, v]) => (
            <div key={k} className="contents">
              <dt className="text-tenue">{k.replace(/_/g, " ")}</dt>
              <dd className="font-mono font-medium">{v}</dd>
            </div>
          ))}
        </dl>
      )}

      <ol className="mt-5 relative border-l border-borde ml-1.5 space-y-4">
        {r.pasos.map((p, i) => (
          <li key={i} className="pl-5 relative">
            <span className="absolute -left-[5px] top-1.5 size-2.5 rounded-full bg-panel border-2 border-tenue" />
            <div className="flex items-center gap-2 text-xs">
              <span className="font-semibold uppercase tracking-wide text-tenue">{p.accion}</span>
              {p.enviado && <span className="font-mono rounded bg-panel-2 text-texto px-1.5 py-0.5">{p.enviado}</span>}
              {p.ms != null && <span className="text-tenue tabular-nums">{segundos(p.ms)}</span>}
            </div>
            {p.texto && (
              <pre className="mt-1.5 rounded-xl bg-panel-2 px-3 py-2 text-[12.5px] leading-relaxed whitespace-pre-wrap font-mono">
                {p.texto}
              </pre>
            )}
          </li>
        ))}
      </ol>
    </Tarjeta>
  );
}

export function Recetas({ dispositivos }: { dispositivos: Dispositivo[] }) {
  const { lista, error } = useRecetas();
  const conectados = dispositivos.filter((d) => d.conectado);
  const [filtro, setFiltro] = useState<Filtro>("Todas");
  const visibles = useMemo(() => lista.filter((r) => deFiltro(r, filtro)), [lista, filtro]);
  const [nombre, setNombre] = useState("");
  const [dispId, setDispId] = useState("");
  const [slot, setSlot] = useState(0);
  const [valores, setValores] = useState<Record<string, string>>({});
  const [simular, setSimular] = useState(true);
  const [confirmando, setConfirmando] = useState(false);
  const [corriendo, setCorriendo] = useState(false);
  const [resultado, setResultado] = useState<Resultado | null>(null);
  const [fallo, setFallo] = useState<string | null>(null);

  const receta = useMemo(() => visibles.find((r) => r.nombre === nombre), [visibles, nombre]);
  const disp = conectados.find((d) => d.id === dispId);

  useEffect(() => {
    // Si la receta elegida queda fuera del filtro, pasa a la primera visible.
    if (!visibles.some((r) => r.nombre === nombre)) setNombre(visibles[0]?.nombre ?? "");
  }, [visibles, nombre]);
  useEffect(() => {
    if (!disp && conectados[0]) setDispId(conectados[0].id);
  }, [disp, conectados]);
  useEffect(() => {
    setValores({});
    setResultado(null);
    setFallo(null);
    setConfirmando(false);
  }, [nombre]);
  useEffect(() => setConfirmando(false), [simular, dispId, slot, valores]);
  // Al cambiar de receta o teléfono, elige solo el primer chip del operador de la receta.
  useEffect(() => {
    if (!receta || !disp) return;
    const s = disp.sims.find((x) => operadorDe(x) === receta.operador.toUpperCase());
    if (s) setSlot(s.slot);
  }, [receta?.nombre, disp?.id]); // eslint-disable-line react-hooks/exhaustive-deps

  // Si el operador del chip no coincide con el de la receta, se avisa (no se bloquea: el nombre de red puede variar).
  const sim = disp?.sims.find((s) => s.slot === slot);
  const operadorDistinto = receta && sim && operadorDe(sim) !== receta.operador.toUpperCase();
  const invalido = receta ? validar(receta, valores) : "Elige una receta";
  // Las recetas de solo consulta se ejecutan directo; simular solo tiene sentido si mueven saldo.
  const mueve = receta?.mueve_dinero ?? true;
  const simulando = mueve && simular;

  async function ejecutar() {
    if (!receta || !disp || invalido) return;
    if (mueve && !simular && !confirmando) {
      setConfirmando(true);
      return;
    }
    setConfirmando(false);
    setCorriendo(true);
    setResultado(null);
    setFallo(null);
    try {
      setResultado(await api.ejecutarReceta(receta.nombre, disp.id, slot, valores, simulando));
    } catch (e) {
      const f = e as FalloApi;
      setFallo(`${f.codigo}: ${f.message}`);
    } finally {
      setCorriendo(false);
    }
  }

  return (
    <div>
      <Titulo titulo="Recetas" sub="Flujos USSD guardados por operador. Simula primero: llega hasta la confirmación y responde No." />

      {error && <Aviso>No pude leer las recetas: {error}</Aviso>}

      {lista.length === 0 && !error ? (
        <Tarjeta>
          <Vacio icono={<BookOpen className="size-5" />} titulo="Sin recetas" texto="Agrega archivos YAML en control/recetas." />
        </Tarjeta>
      ) : (
        <div className="grid gap-4 lg:grid-cols-[360px_1fr]">
          <div className="space-y-3">
            <div role="tablist" className="grid grid-cols-4 gap-1 rounded-xl border border-borde bg-panel p-1">
              {FILTROS.map((f) => {
                const n = lista.filter((r) => deFiltro(r, f)).length;
                return (
                  <button
                    key={f}
                    role="tab"
                    aria-selected={filtro === f}
                    onClick={() => setFiltro(f)}
                    className={cx(
                      "rounded-xl px-2 py-1.5 text-sm font-medium transition-colors",
                      filtro === f ? "bg-panel-2 text-texto" : "text-tenue hover:bg-panel-2",
                    )}
                  >
                    {f} <span className="text-xs tabular-nums opacity-70">{n}</span>
                  </button>
                );
              })}
            </div>

            <Tarjeta className="p-1.5">
              {visibles.length === 0 && (
                <p className="px-3 py-6 text-center text-sm text-tenue">Sin recetas de {filtro} todavía.</p>
              )}
              {visibles.map((r) => (
                <button
                  key={r.nombre}
                  onClick={() => setNombre(r.nombre)}
                  className={cx(
                    "w-full text-left rounded-xl px-3 py-2.5 transition-colors",
                    r.nombre === nombre ? "bg-panel-2" : "hover:bg-panel-2",
                  )}
                >
                  <div className="flex items-center justify-between gap-2">
                    <span className={cx("font-medium text-sm", r.nombre === nombre && "text-texto")}>{r.titulo}</span>
                    <span className="flex gap-1">
                      {r.mueve_dinero && <Insignia tono="aviso">Mueve saldo</Insignia>}
                      <Insignia>{r.operador}</Insignia>
                    </span>
                  </div>
                  {r.descripcion && <p className="text-xs text-tenue mt-0.5 line-clamp-2">{r.descripcion}</p>}
                </button>
              ))}
            </Tarjeta>

            {receta && (
              <Tarjeta className="p-4 space-y-3.5">
                <Selector etiqueta="Teléfono" value={dispId} onChange={(e) => setDispId(e.target.value)}>
                  {conectados.length === 0 && <option value="">Sin teléfonos en línea</option>}
                  {conectados.map((d) => (
                    <option key={d.id} value={d.id}>
                      {d.modelo} · {d.adb_serial?.slice(-6) ?? d.id.slice(0, 6)}
                    </option>
                  ))}
                </Selector>
                {disp && (
                  <Selector etiqueta="Chip" value={slot} onChange={(e) => setSlot(Number(e.target.value))}>
                    {simsConChip(disp).map((s) => (
                      <option key={s.slot} value={s.slot}>
                        {nombreSim(s.slot)} · {etiquetaOperador(s)} {s.numero ? `· ${s.numero}` : ""}
                      </option>
                    ))}
                  </Selector>
                )}

                {receta.parametros.map((p) =>
                  p.opciones?.length ? (
                    <Selector
                      key={p.nombre}
                      etiqueta={p.nombre}
                      value={valores[p.nombre] ?? ""}
                      onChange={(e) => setValores((v) => ({ ...v, [p.nombre]: e.target.value }))}
                    >
                      <option value="">Elegir…</option>
                      {p.opciones.map((o) => (
                        <option key={o}>{o}</option>
                      ))}
                    </Selector>
                  ) : (
                    <Campo
                      key={p.nombre}
                      etiqueta={p.nombre}
                      className="font-mono"
                      value={valores[p.nombre] ?? ""}
                      onChange={(e) => setValores((v) => ({ ...v, [p.nombre]: e.target.value }))}
                    />
                  ),
                )}

                {mueve && (
                  <div className="pt-1">
                    <Interruptor
                      activo={simular}
                      onCambio={setSimular}
                      etiqueta={
                        <span>
                          Simular <span className="text-tenue">(no confirma)</span>
                        </span>
                      }
                    />
                  </div>
                )}

                {operadorDistinto && (
                  <Aviso tono="aviso">
                    El chip es de {sim && etiquetaOperador(sim)} y la receta es para {receta.operador}.
                  </Aviso>
                )}
                {mueve && !simular && (
                  <Aviso tono="aviso">Ejecución real: puede mover saldo. El paso de confirmación nunca se reintenta.</Aviso>
                )}

                <Boton
                  variante={mueve && !simular && confirmando ? "peligro" : "primario"}
                  className="w-full"
                  onClick={ejecutar}
                  cargando={corriendo}
                  disabled={!!invalido || !disp}
                  icono={simulando ? <FlaskConical className="size-4" /> : <Play className="size-4" />}
                  title={invalido ?? undefined}
                >
                  {simulando ? "Simular" : confirmando ? "Confirmar ejecución real" : "Ejecutar"}
                </Boton>
                {invalido && receta.parametros.length > 0 && Object.keys(valores).length > 0 && (
                  <p className="text-xs text-tenue">{invalido}</p>
                )}
              </Tarjeta>
            )}
          </div>

          <div>
            {fallo && <Aviso>{fallo}</Aviso>}
            {resultado ? (
              <VistaResultado r={resultado} />
            ) : (
              !fallo && (
                <Tarjeta>
                  <Vacio
                    icono={<Play className="size-5" />}
                    titulo={corriendo ? "Ejecutando…" : "El resultado aparece aquí"}
                    texto="Cada paso muestra lo que se envió, lo que respondió el operador y cuánto tardó."
                  />
                </Tarjeta>
              )
            )}
          </div>
        </div>
      )}
    </div>
  );
}
