import { useMemo, useRef, useState } from "react";
import { ArrowLeft, Download, FileSpreadsheet, FlaskConical, Play, RotateCw, Upload } from "../../componentes/iconos";
import { api, FalloApi } from "../../api/cliente";
import type { Dispositivo, FilaValidada, Validacion } from "../../api/tipos";
import { Aviso, Boton, Campo, Interruptor, Tarjeta, Titulo, cx } from "../../componentes/base";
import { hace } from "../../util/datos";
import { EstadoValidacion, bs, nombreChip } from "./comunes";

type Filtro = "todas" | "errores" | "avisos" | "validas";

function nombrePorDefecto(archivo: string | null) {
  const base = archivo?.replace(/\.(csv|txt)$/i, "") ?? "Lote";
  const hoy = new Date().toLocaleDateString("es-BO", { day: "2-digit", month: "2-digit" });
  return `${base} · ${hoy}`;
}

function Cifra({ etiqueta, valor, sub, tono }: { etiqueta: string; valor: React.ReactNode; sub?: React.ReactNode; tono?: string }) {
  return (
    <Tarjeta className="px-4 py-3.5">
      <p className="text-xs font-medium text-tenue">{etiqueta}</p>
      <p className={cx("mt-1 text-2xl font-semibold tabular-nums tracking-tight", tono)}>{valor}</p>
      {sub && <p className="text-xs text-tenue mt-0.5">{sub}</p>}
    </Tarjeta>
  );
}

/** Barra "tengo X de lo que necesito". */
function Medidor({ etiqueta, tiene, necesita, formato }: { etiqueta: string; tiene: number; necesita: number; formato: (n: number) => string }) {
  const ok = tiene >= necesita;
  const pct = necesita > 0 ? Math.min(100, (tiene / necesita) * 100) : 100;
  return (
    <div>
      <div className="flex justify-between text-xs mb-1.5">
        <span className="text-tenue">{etiqueta}</span>
        <span className={cx("font-medium tabular-nums", ok ? "text-ok" : "text-aviso")}>
          {formato(tiene)} / {formato(necesita)}
        </span>
      </div>
      <div className="h-1.5 rounded-full bg-panel-2 overflow-hidden">
        <div className={cx("h-full rounded-full", ok ? "bg-ok" : "bg-aviso")} style={{ width: `${pct}%` }} />
      </div>
    </div>
  );
}

function TablaFilas({ filas, dispositivos }: { filas: FilaValidada[]; dispositivos: Dispositivo[] }) {
  const [filtro, setFiltro] = useState<Filtro>(() => (filas.some((f) => f.estado === "invalida") ? "errores" : "todas"));
  const vistas = useMemo(
    () => ({
      todas: filas,
      errores: filas.filter((f) => f.estado === "invalida"),
      avisos: filas.filter((f) => f.estado === "aviso" || f.sin_capacidad > 0),
      validas: filas.filter((f) => f.estado !== "invalida"),
    }),
    [filas],
  );
  const lista = vistas[filtro];
  const etiquetas: [Filtro, string][] = [
    ["todas", "Todas"],
    ["errores", "Con errores"],
    ["avisos", "Avisos"],
    ["validas", "Válidas"],
  ];
  return (
    <Tarjeta className="overflow-hidden">
      <div className="flex gap-1 p-2 border-b border-borde">
        {etiquetas.map(([id, t]) => (
          <button
            key={id}
            onClick={() => setFiltro(id)}
            className={cx(
              "h-8 px-3 rounded-xl text-sm font-medium inline-flex items-center gap-2",
              filtro === id ? "bg-panel-2 text-texto" : "text-tenue hover:text-texto",
            )}
          >
            {t}
            <span className="text-xs tabular-nums opacity-70">{vistas[id].length}</span>
          </button>
        ))}
      </div>
      <div className="overflow-x-auto max-h-[520px] overflow-y-auto">
        <table className="w-full text-sm">
          <thead className="sticky top-0 bg-panel z-10">
            <tr className="text-left text-xs text-tenue border-b border-borde">
              <th className="font-medium px-4 py-2.5 w-14">Fila</th>
              <th className="font-medium px-3 py-2.5">Número</th>
              <th className="font-medium px-3 py-2.5">Operador</th>
              <th className="font-medium px-3 py-2.5 text-right">Monto</th>
              <th className="font-medium px-3 py-2.5">Envíos</th>
              <th className="font-medium px-3 py-2.5">Sale de</th>
              <th className="font-medium px-3 py-2.5">Estado</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-borde">
            {lista.length === 0 && (
              <tr>
                <td colSpan={7} className="px-4 py-10 text-center text-tenue">
                  Nada en esta vista
                </td>
              </tr>
            )}
            {lista.map((f) => (
              <tr key={f.fila} className={cx(f.estado === "invalida" && "bg-error-suave/40")}>
                <td className="px-4 py-2.5 text-tenue tabular-nums align-top">{f.fila}</td>
                <td className="px-3 py-2.5 font-mono align-top">
                  {f.numero ?? <span className="text-error">{f.original.numero || "(vacío)"}</span>}
                  {f.referencia && <div className="text-xs text-tenue font-sans">{f.referencia}</div>}
                </td>
                <td className="px-3 py-2.5 align-top">{f.operador ?? <span className="text-error">{f.original.operador || "(vacío)"}</span>}</td>
                <td className="px-3 py-2.5 text-right tabular-nums align-top">
                  {f.monto != null ? bs(f.monto) : <span className="text-error">{f.original.monto || "(vacío)"}</span>}
                </td>
                <td className="px-3 py-2.5 align-top">
                  <div className="flex flex-wrap gap-1">
                    {f.envios.map((m, i) => (
                      <span key={i} className="h-5.5 px-1.5 rounded-md bg-panel-2 text-xs font-mono tabular-nums inline-flex items-center">
                        {m}
                      </span>
                    ))}
                  </div>
                </td>
                <td className="px-3 py-2.5 align-top text-xs font-mono text-tenue">
                  {[...new Set(f.plan.map((p) => nombreChip(dispositivos, p.dispositivo, p.slot)))].join(", ")}
                  {f.sin_capacidad > 0 && (
                    <div className="text-aviso font-sans">
                      {f.sin_capacidad} envío{f.sin_capacidad > 1 ? "s" : ""} para mañana
                      {f.motivo_sin_capacidad && <span className="text-tenue"> · {f.motivo_sin_capacidad}</span>}
                    </div>
                  )}
                </td>
                <td className="px-3 py-2.5 align-top">
                  <EstadoValidacion estado={f.estado} />
                  {f.errores.map((e, i) => (
                    <div key={i} className="text-xs text-error mt-1 max-w-64">
                      {e}
                    </div>
                  ))}
                  {f.avisos.map((e, i) => (
                    <div key={i} className="text-xs text-aviso mt-1 max-w-64">
                      {e}
                    </div>
                  ))}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </Tarjeta>
  );
}

export function NuevoLote({
  dispositivos,
  volver,
  abrirLote,
}: {
  dispositivos: Dispositivo[];
  volver: () => void;
  abrirLote: (id: number) => void;
}) {
  const [csv, setCsv] = useState<string | null>(null);
  const [archivo, setArchivo] = useState<string | null>(null);
  const [pegar, setPegar] = useState(false);
  const [texto, setTexto] = useState("");
  const [consultarSaldos, setConsultarSaldos] = useState(true);
  const [validando, setValidando] = useState(false);
  const [v, setV] = useState<Validacion | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [nombre, setNombre] = useState("");
  const [simular, setSimular] = useState(true);
  const [confirmando, setConfirmando] = useState(false);
  const [creando, setCreando] = useState(false);
  const [arrastrando, setArrastrando] = useState(false);
  const input = useRef<HTMLInputElement>(null);

  async function validar(contenido: string, nombreArchivo: string | null) {
    setCsv(contenido);
    setArchivo(nombreArchivo);
    setNombre((n) => n || nombrePorDefecto(nombreArchivo));
    setValidando(true);
    setError(null);
    setConfirmando(false);
    try {
      setV(await api.validarLote(contenido, consultarSaldos));
    } catch (e) {
      const f = e as FalloApi;
      setError(`${f.codigo}: ${f.message}`);
      setV(null);
    } finally {
      setValidando(false);
    }
  }

  async function leer(f: File | undefined) {
    if (!f) return;
    if (f.size > 1_000_000) {
      setError("El archivo pesa más de 1 MB");
      return;
    }
    validar(await f.text(), f.name);
  }

  async function crear() {
    if (!csv || !v) return;
    if (!simular && !confirmando) {
      setConfirmando(true);
      return;
    }
    setCreando(true);
    try {
      const lote = await api.crearLote(nombre.trim() || nombrePorDefecto(archivo), csv, simular);
      await api.iniciarLote(lote.id);
      abrirLote(lote.id);
    } catch (e) {
      const f = e as FalloApi;
      setError(`${f.codigo}: ${f.message}`);
    } finally {
      setCreando(false);
      setConfirmando(false);
    }
  }

  const r = v?.resumen;
  const ejecutables = r ? r.validas : 0; // `validas` ya incluye las filas con avisos

  return (
    <div className="pb-28">
      <Titulo
        titulo="Nuevo lote"
        sub="Sube un CSV. Se valida todo antes de ejecutar: formato, operador, montos, cupo de los chips y saldo."
        acciones={
          <>
            <a href={api.urlPlantilla} download>
              <Boton variante="fantasma" icono={<Download className="size-4" />}>
                Plantilla CSV
              </Boton>
            </a>
            <Boton variante="fantasma" icono={<ArrowLeft className="size-4" />} onClick={volver}>
              Lotes
            </Boton>
          </>
        }
      />

      {/* Carga */}
      <Tarjeta className="p-4 mb-5">
        {pegar ? (
          <div className="space-y-3">
            <textarea
              value={texto}
              onChange={(e) => setTexto(e.target.value)}
              placeholder={"numero,monto,operador,referencia\n69869869,12,TIGO,Pedido 1001"}
              className="w-full h-40 rounded-xl border border-borde bg-panel px-3 py-2 font-mono text-sm outline-none focus:border-acento focus:ring-3 focus:ring-acento/15"
            />
            <div className="flex gap-2">
              <Boton variante="primario" disabled={!texto.trim()} cargando={validando} onClick={() => validar(texto, null)}>
                Validar
              </Boton>
              <Boton variante="fantasma" onClick={() => setPegar(false)}>
                Subir archivo
              </Boton>
            </div>
          </div>
        ) : (
          <div
            onDragOver={(e) => {
              e.preventDefault();
              setArrastrando(true);
            }}
            onDragLeave={() => setArrastrando(false)}
            onDrop={(e) => {
              e.preventDefault();
              setArrastrando(false);
              leer(e.dataTransfer.files[0]);
            }}
            onClick={() => input.current?.click()}
            className={cx(
              "flex flex-col items-center justify-center text-center rounded-xl border-2 border-dashed py-10 px-6 cursor-pointer transition-colors",
              arrastrando ? "border-texto/60 bg-panel-2" : "border-borde hover:border-tenue/60 hover:bg-panel-2",
            )}
          >
            <input
              ref={input}
              type="file"
              accept=".csv,.txt,text/csv"
              className="hidden"
              onChange={(e) => {
                leer(e.target.files?.[0]);
                e.target.value = "";
              }}
            />
            <div className="size-11 rounded-xl bg-panel-2 text-tenue grid place-items-center mb-3">
              {validando ? <RotateCw className="size-5 animate-spin" /> : <Upload className="size-5" />}
            </div>
            {validando ? (
              <p className="font-medium">{consultarSaldos ? "Validando y consultando saldos…" : "Validando…"}</p>
            ) : archivo ? (
              <p className="font-medium inline-flex items-center gap-2">
                <FileSpreadsheet className="size-4 text-tenue" />
                {archivo} <span className="text-tenue font-normal">· clic para cambiar</span>
              </p>
            ) : (
              <>
                <p className="font-medium">Arrastra el CSV aquí o haz clic para elegirlo</p>
                <p className="text-sm text-tenue mt-1">Columnas: numero, monto, operador (y opcionales accion, referencia)</p>
              </>
            )}
          </div>
        )}
        <div className="flex flex-wrap items-center justify-between gap-3 mt-3">
          <Interruptor
            activo={consultarSaldos}
            onCambio={setConsultarSaldos}
            etiqueta={
              <span>
                Consultar saldos al validar <span className="text-tenue">(~3.5 s por chip, gratis)</span>
              </span>
            }
          />
          <div className="flex gap-2">
            {!pegar && (
              <Boton variante="fantasma" onClick={() => setPegar(true)}>
                Pegar texto
              </Boton>
            )}
            {csv && (
              <Boton variante="secundario" icono={<RotateCw className="size-4" />} cargando={validando} onClick={() => validar(csv, archivo)}>
                Validar de nuevo
              </Boton>
            )}
          </div>
        </div>
      </Tarjeta>

      {error && (
        <div className="mb-5">
          <Aviso>{error}</Aviso>
        </div>
      )}

      {v && r && (
        <div className="space-y-5 aparecer">
          <div className="grid grid-cols-2 lg:grid-cols-4 gap-3">
            <Cifra
              etiqueta="Filas que se ejecutarán"
              valor={
                <>
                  {ejecutables}
                  <span className="text-base font-normal text-tenue"> / {r.filas}</span>
                </>
              }
              sub={r.con_avisos ? `${r.con_avisos} con avisos` : "sin avisos"}
            />
            <Cifra
              etiqueta="Filas con error"
              valor={r.invalidas}
              tono={r.invalidas ? "text-error" : undefined}
              sub={r.invalidas ? "se omiten al ejecutar" : "todo en orden"}
            />
            <Cifra etiqueta="Total a enviar" valor={bs(r.total_bs)} sub={`${r.envios} envíos · comisión ${bs(r.comision_total)}`} />
            <Cifra
              etiqueta="Entra hoy"
              valor={
                <>
                  {r.planificados}
                  <span className="text-base font-normal text-tenue"> / {r.envios} envíos</span>
                </>
              }
              tono={r.sin_capacidad ? "text-aviso" : "text-ok"}
              sub={r.sin_capacidad ? `${r.sin_capacidad} quedan para mañana` : "hay capacidad para todo"}
            />
          </div>

          {r.operadores.length > 0 && (
            <div className="grid gap-3 md:grid-cols-2">
              {r.operadores.map((o) => (
                <Tarjeta key={o.operador} className="p-4 space-y-3.5">
                  <div className="flex items-center justify-between">
                    <p className="font-semibold">{o.operador}</p>
                    <p className="text-xs text-tenue">
                      {o.chips} chip{o.chips === 1 ? "" : "s"} en línea · {o.envios} envíos
                    </p>
                  </div>
                  <Medidor etiqueta="Cupo de hoy (envíos)" tiene={o.cupo_hoy} necesita={o.envios} formato={(n) => String(n)} />
                  <Medidor etiqueta="Saldo conocido" tiene={o.saldo_conocido} necesita={o.necesario_bs} formato={(n) => bs(n)} />
                  {o.saldo_desconocido_chips > 0 && (
                    <p className="text-xs text-aviso">
                      {o.saldo_desconocido_chips} chip{o.saldo_desconocido_chips > 1 ? "s" : ""} sin saldo conocido: activa "Consultar saldos".
                    </p>
                  )}
                  {o.chips === 0 && <p className="text-xs text-error">No hay chips de {o.operador} en línea.</p>}
                </Tarjeta>
              ))}
            </div>
          )}

          {v.chips.length > 0 && (
            <Tarjeta className="overflow-hidden">
              <p className="px-4 pt-3.5 pb-2 text-sm font-semibold">Chips disponibles</p>
              <div className="overflow-x-auto">
                <table className="w-full text-sm">
                  <thead>
                    <tr className="text-left text-xs text-tenue border-y border-borde">
                      <th className="font-medium px-4 py-2">Chip</th>
                      <th className="font-medium px-3 py-2">Operador</th>
                      <th className="font-medium px-3 py-2 text-right">Saldo</th>
                      <th className="font-medium px-3 py-2">Consultado</th>
                      <th className="font-medium px-3 py-2 text-right">Envíos hoy</th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-borde">
                    {v.chips.map((c) => (
                      <tr key={`${c.dispositivo}-${c.slot}`}>
                        <td className="px-4 py-2 font-mono">{nombreChip(dispositivos, c.dispositivo, c.slot)}</td>
                        <td className="px-3 py-2">{c.operador}</td>
                        <td className="px-3 py-2 text-right tabular-nums">{bs(c.saldo)}</td>
                        <td className="px-3 py-2 text-tenue">{c.saldo_ms ? hace(c.saldo_ms) : "nunca"}</td>
                        <td className="px-3 py-2 text-right tabular-nums">
                          {c.agotado_hoy ? (
                            <span className="text-aviso">agotado por el operador</span>
                          ) : (
                            <>
                              {c.enviados_hoy} <span className="text-tenue">/ {c.enviados_hoy + c.cupo_hoy}</span>
                            </>
                          )}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </Tarjeta>
          )}

          <TablaFilas filas={v.filas} dispositivos={dispositivos} />
        </div>
      )}

      {/* Barra de acción fija */}
      {v && r && (
        <div className="fixed bottom-0 left-0 md:left-60 right-0 border-t border-borde bg-panel/95 backdrop-blur z-20">
          <div className="max-w-6xl mx-auto px-4 md:px-8 py-3 flex flex-wrap items-center gap-3">
            <div className="w-64">
              <Campo value={nombre} onChange={(e) => setNombre(e.target.value)} placeholder="Nombre del lote" />
            </div>
            <Interruptor
              activo={simular}
              onCambio={(x) => {
                setSimular(x);
                setConfirmando(false);
              }}
              etiqueta={<span>Simulación</span>}
            />
            <div className="ml-auto flex items-center gap-3">
              {!simular && (
                <span className="text-sm text-aviso">
                  Real: {bs(r.total_bs)} en {r.envios} envíos + {bs(r.comision_total)} de comisión
                </span>
              )}
              <Boton
                variante={!simular && confirmando ? "peligro" : "primario"}
                disabled={ejecutables === 0}
                cargando={creando}
                onClick={crear}
                icono={simular ? <FlaskConical className="size-4" /> : <Play className="size-4" />}
              >
                {simular ? `Simular ${ejecutables} filas` : confirmando ? "Confirmar: ejecutar real" : `Ejecutar ${ejecutables} filas`}
              </Boton>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
