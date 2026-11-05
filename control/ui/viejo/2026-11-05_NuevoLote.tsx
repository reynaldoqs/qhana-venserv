import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Download, FileSpreadsheet, FlaskConical, Play, RotateCw, Upload } from "../../componentes/iconos";
import { api, FalloApi, mensajeError } from "../../api/cliente";
import type { ChipInfo, Dispositivo, FilaValidada, OrigenLote, Receta, Validacion } from "../../api/tipos";
import {
  Aviso,
  Boton,
  BotonDescarga,
  Campo,
  Cifra,
  Dialogo,
  Insignia,
  Interruptor,
  Migas,
  Segmentado,
  Selector,
  Tarjeta,
  Titulo,
  cx,
} from "../../componentes/base";
import { simsConChip } from "../../componentes/dispositivo";
import { etiquetaOperador, etiquetaTelefono, hace, nombreSim, sinPrefijo, useRecetas } from "../../util/datos";
import { EstadoValidacion, bs, nombreChip } from "./comunes";
import { dejarAviso } from "./detalle/useLote";

/**
 * Borrador del lote nuevo, fuera de React: la app remonta la vista al cambiar de pestaña y así lo validado
 * (CSV, nombre, chip elegido…) sigue ahí al volver. Se vacía al crear el lote.
 */
const borrador = new Map<string, unknown>();

function useBorrador<T>(clave: string, inicial: T): [T, (v: T | ((x: T) => T)) => void] {
  const [v, setV] = useState<T>(() => (borrador.has(clave) ? (borrador.get(clave) as T) : inicial));
  const fijar = useCallback(
    (x: T | ((ant: T) => T)) => {
      const ant = (borrador.has(clave) ? borrador.get(clave) : inicial) as T;
      const nuevo = typeof x === "function" ? (x as (a: T) => T)(ant) : x;
      borrador.set(clave, nuevo);
      setV(nuevo);
    },
    [clave], // eslint-disable-line react-hooks/exhaustive-deps
  );
  return [v, fijar];
}

type Filtro = "todas" | "errores" | "avisos" | "validas";
type Modo = "operador" | "chip";

/** Recetas que un lote de un chip puede usar: del operador del chip, que mueven saldo, con número y monto, y PDA
 * solo si el chip es PDA. El control vuelve a revisar todo al validar. */
function recetasParaChip(lista: Receta[], chip: ChipInfo | undefined): Receta[] {
  if (!chip) return [];
  return lista.filter(
    (r) =>
      r.operador.toUpperCase() === chip.operador &&
      r.mueve_dinero &&
      (r.chip_tipo !== "pda" || chip.tipo === "pda") &&
      ["numero", "monto"].every((n) => r.parametros.some((p) => p.nombre === n)),
  );
}

/** Elegir el chip que hace todos los envíos y la receta (acción) que corre. */
function ElegirOrigen({
  dispositivos,
  origen,
  onCambio,
}: {
  dispositivos: Dispositivo[];
  origen: OrigenLote | null;
  onCambio: (o: OrigenLote | null) => void;
}) {
  const { lista, error: errorRecetas } = useRecetas();
  const [chips, setChips] = useState<ChipInfo[]>([]);
  const [errorChips, setErrorChips] = useState<string | null>(null);
  useEffect(() => {
    api.chips().then(
      (c) => {
        setChips(c);
        setErrorChips(null);
      },
      (e) => setErrorChips(mensajeError(e)),
    );
  }, []);
  const conectados = dispositivos.filter((d) => d.conectado);
  const disp = conectados.find((d) => d.id === origen?.dispositivo) ?? conectados[0];
  const slot = origen?.slot ?? (disp ? simsConChip(disp)[0]?.slot : undefined) ?? 0;
  const chip = chips.find((c) => c.dispositivo === disp?.id && c.slot === slot);
  const recetas = recetasParaChip(lista, chip);
  // La PDA primero: si el chip es PDA, lo normal es usar su receta.
  recetas.sort((a, b) => Number(b.chip_tipo === "pda") - Number(a.chip_tipo === "pda"));
  const receta = recetas.find((r) => r.nombre === origen?.receta) ?? recetas[0];

  // Mantiene el origen al día con lo elegido (y con la primera opción válida).
  useEffect(() => {
    const nuevo = disp && chip && receta ? { dispositivo: disp.id, slot, receta: receta.nombre } : null;
    if (nuevo?.dispositivo !== origen?.dispositivo || nuevo?.slot !== origen?.slot || nuevo?.receta !== origen?.receta) onCambio(nuevo);
  }, [disp?.id, slot, chip?.operador, receta?.nombre]); // eslint-disable-line react-hooks/exhaustive-deps

  const pidePin = receta?.parametros.some((p) => p.secreto && p.nombre === "pin");
  return (
    <div className="grid gap-3 md:grid-cols-3 mt-4">
      <Selector
        etiqueta="Teléfono"
        value={disp?.id ?? ""}
        onChange={(e) => {
          const d = conectados.find((x) => x.id === e.target.value);
          if (d) onCambio({ dispositivo: d.id, slot: simsConChip(d)[0]?.slot ?? 0, receta: origen?.receta ?? "" });
        }}
      >
        {conectados.length === 0 && <option value="">Sin teléfonos en línea</option>}
        {conectados.map((d) => (
          <option key={d.id} value={d.id}>
            {etiquetaTelefono(d)}
          </option>
        ))}
      </Selector>
      <Selector
        etiqueta="Chip que hace los envíos"
        value={slot}
        onChange={(e) => disp && onCambio({ dispositivo: disp.id, slot: Number(e.target.value), receta: origen?.receta ?? "" })}
      >
        {disp &&
          simsConChip(disp).map((s) => {
            const c = chips.find((x) => x.dispositivo === disp.id && x.slot === s.slot);
            return (
              <option key={s.slot} value={s.slot}>
                {nombreSim(s.slot)} · {etiquetaOperador(s)} {s.numero ?? c?.numero ?? ""} {c?.tipo === "pda" ? "· PDA" : ""}
              </option>
            );
          })}
      </Selector>
      <Selector
        etiqueta="Acción (receta)"
        value={receta?.nombre ?? ""}
        onChange={(e) => origen && onCambio({ ...origen, receta: e.target.value })}
      >
        {recetas.length === 0 && <option value="">{chip ? `Sin recetas para este chip` : "Chip sin servicio"}</option>}
        {recetas.map((r) => (
          <option key={r.nombre} value={r.nombre}>
            {r.titulo}
          </option>
        ))}
      </Selector>
      {(errorChips || errorRecetas) && (
        <div className="md:col-span-3">
          <Aviso>
            {errorChips ? `No pude leer los chips: ${errorChips}` : `No pude leer las recetas: ${errorRecetas}`}
          </Aviso>
        </div>
      )}
      {chip && receta && (
        <p className="md:col-span-3 text-xs text-tenue flex flex-wrap items-center gap-2">
          <Insignia>{chip.operador}</Insignia>
          {chip.tipo === "pda" && <Insignia tono="info">PDA</Insignia>}
          {pidePin && (chip.tiene_pin ? <Insignia tono="ok">PIN guardado</Insignia> : <Insignia tono="error">Sin PIN: guárdalo en el Panel</Insignia>)}
          Todas las filas salen de este chip con «{receta.titulo}». El CSV basta con <span className="font-mono">numero,monto</span>
          (y opcional <span className="font-mono">referencia</span>).
        </p>
      )}
    </div>
  );
}

function nombrePorDefecto(archivo: string | null) {
  const base = archivo?.replace(/\.(csv|txt)$/i, "") ?? "Lote";
  const hoy = new Date().toLocaleDateString("es-BO", { day: "2-digit", month: "2-digit" });
  return `${base} · ${hoy}`;
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
  return (
    <Tarjeta className="overflow-hidden">
      <div className="p-2 border-b border-borde">
        <Segmentado
          etiqueta="Filtrar filas validadas"
          controla="filas-validadas"
          valor={filtro}
          onCambio={setFiltro}
          className="border-0 bg-transparent p-0"
          opciones={[
            { id: "todas", texto: "Todas", n: vistas.todas.length },
            { id: "errores", texto: "Con errores", n: vistas.errores.length },
            { id: "avisos", texto: "Avisos", n: vistas.avisos.length },
            { id: "validas", texto: "Válidas", n: vistas.validas.length },
          ]}
        />
      </div>
      <div id="filas-validadas" role="tabpanel" className="overflow-auto max-h-[520px]">
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
  const [csv, setCsv] = useBorrador<string | null>("csv", null);
  const [archivo, setArchivo] = useBorrador<string | null>("archivo", null);
  const [pegar, setPegar] = useBorrador("pegar", false);
  const [texto, setTexto] = useBorrador("texto", "");
  const [consultarSaldos, setConsultarSaldos] = useBorrador("consultarSaldos", true);
  const [validando, setValidando] = useBorrador("validando", false);
  const [v, setV] = useBorrador<Validacion | null>("v", null);
  const [error, setError] = useBorrador<string | null>("error", null);
  const [nombre, setNombre] = useBorrador("nombre", "");
  const [simular, setSimular] = useBorrador("simular", true);
  const [modo, setModo] = useBorrador<Modo>("modo", "operador");
  const [origen, setOrigen] = useBorrador<OrigenLote | null>("origen", null);
  const [creando, setCreando] = useState(false);
  const [arrastrando, setArrastrando] = useState(false);
  /** Diálogo abierto: confirmar un lote real, o avisar que es igual a otro reciente (`detalle` del 409). */
  const [dialogo, setDialogo] = useState<{ tipo: "real" } | { tipo: "duplicado"; detalle: string } | null>(null);
  const input = useRef<HTMLInputElement>(null);
  const origenUsado = modo === "chip" ? origen : null;
  // Cambiar el chip o la acción invalida lo validado: se vuelve a validar.
  const clave = `${modo}|${origenUsado?.dispositivo}|${origenUsado?.slot}|${origenUsado?.receta}`;
  const claveValidada = useRef(borrador.has("clave") ? (borrador.get("clave") as string) : clave);
  useEffect(() => {
    if (csv && claveValidada.current !== clave && (modo === "operador" || origenUsado)) validar(csv, archivo);
  }, [clave]); // eslint-disable-line react-hooks/exhaustive-deps

  async function validar(contenido: string, nombreArchivo: string | null) {
    setCsv(contenido);
    setArchivo(nombreArchivo);
    setNombre((n) => n || nombrePorDefecto(nombreArchivo));
    setValidando(true);
    setError(null);
    claveValidada.current = clave;
    borrador.set("clave", clave);
    if (modo === "chip" && !origenUsado) {
      setValidando(false);
      setV(null);
      setError("Elige el chip y la acción del lote");
      return;
    }
    try {
      setV(await api.validarLote(contenido, consultarSaldos, origenUsado));
    } catch (e) {
      setError(mensajeError(e));
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

  /** Simulación: se crea y arranca directo. Real: primero el diálogo de confirmación. */
  function pedirCrear() {
    if (!csv || !v) return;
    if (simular) crear(false);
    else setDialogo({ tipo: "real" });
  }

  /**
   * Crea y arranca. Crear y arrancar son dos pasos: si el lote se creó, siempre se abre (aunque `iniciar` falle,
   * el error se muestra allá) y nunca se vuelve a crear. Un `409 duplicado` ofrece crearlo igual (`forzar`).
   */
  async function crear(forzar: boolean) {
    if (!csv || !v) return;
    setDialogo(null);
    setCreando(true);
    let id: number;
    try {
      id = (await api.crearLote(nombre.trim() || nombrePorDefecto(archivo), csv, simular, origenUsado, forzar)).id;
    } catch (e) {
      setCreando(false);
      if (e instanceof FalloApi && e.codigo === "duplicado") setDialogo({ tipo: "duplicado", detalle: e.message });
      else setError(mensajeError(e));
      return;
    }
    borrador.clear();
    try {
      await api.iniciarLote(id);
    } catch (e) {
      dejarAviso(id, `El lote se creó, pero no arrancó: ${mensajeError(e)}. Revísalo y pulsa Iniciar.`);
    }
    setCreando(false);
    abrirLote(id);
  }

  const r = v?.resumen;
  const ejecutables = r ? r.validas : 0; // `validas` ya incluye las filas con avisos

  return (
    <div>
      <Titulo
        migas={<Migas tramos={[{ texto: "Lotes", ir: volver }, { texto: "Nuevo" }]} />}
        titulo="Nuevo lote"
        sub="Sube un CSV. Se valida todo antes de ejecutar: formato, operador, montos, cupo de los chips y saldo."
        acciones={
          <BotonDescarga variante="fantasma" descargar={api.descargarPlantilla} icono={<Download className="size-4" />}>
            Plantilla CSV
          </BotonDescarga>
        }
      />

      {/* Quién envía */}
      <Tarjeta className="p-4 mb-5">
        <Segmentado
          etiqueta="Cómo se reparten los envíos"
          valor={modo}
          onCambio={setModo}
          opciones={[
            { id: "operador", texto: "Por operador (todos los chips)" },
            { id: "chip", texto: "Desde un chip" },
          ]}
        />
        {modo === "chip" ? (
          <ElegirOrigen dispositivos={dispositivos} origen={origen} onCambio={setOrigen} />
        ) : (
          <p className="text-xs text-tenue mt-3">
            Cada fila dice su operador (y su acción, por defecto transferir); los envíos se reparten entre los chips de ese operador.
          </p>
        )}
      </Tarjeta>

      {/* Carga */}
      <Tarjeta className="p-4 mb-5">
        {pegar ? (
          <div className="space-y-3">
            <textarea
              aria-label="Contenido del CSV"
              value={texto}
              onChange={(e) => setTexto(e.target.value)}
              placeholder={
                modo === "chip" ? "numero,monto\n69869869,1.50\n78815232,5" : "numero,monto,operador,referencia\n69869869,12,TIGO,Pedido 1001"
              }
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
          <>
            <input
              ref={input}
              id="archivo-csv"
              type="file"
              accept=".csv,.txt,text/csv"
              className="sr-only"
              tabIndex={-1}
              aria-hidden
              onChange={(e) => {
                leer(e.target.files?.[0]);
                e.target.value = "";
              }}
            />
            <button
              type="button"
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
              disabled={validando}
              aria-describedby="archivo-csv-ayuda"
              className={cx(
                "w-full flex flex-col items-center justify-center text-center rounded-xl border-2 border-dashed py-10 px-6 cursor-pointer transition-colors disabled:cursor-progress",
                arrastrando ? "border-texto/60 bg-panel-2" : "border-borde hover:border-tenue/60 hover:bg-panel-2",
              )}
            >
              <span className="size-11 rounded-xl bg-panel-2 text-tenue grid place-items-center mb-3">
                {validando ? <RotateCw className="size-5 animate-spin" /> : <Upload className="size-5" />}
              </span>
              {validando ? (
                <span className="font-medium" role="status">
                  {consultarSaldos ? "Validando y consultando saldos…" : "Validando…"}
                </span>
              ) : archivo ? (
                <span className="font-medium inline-flex items-center gap-2">
                  <FileSpreadsheet className="size-4 text-tenue" />
                  {archivo} <span className="text-tenue font-normal">· elegir otro</span>
                </span>
              ) : (
                <span className="font-medium">Arrastra el CSV aquí o elige un archivo</span>
              )}
              <span id="archivo-csv-ayuda" className="text-sm text-tenue mt-1">
                {modo === "chip"
                  ? "Columnas: numero, monto (y opcional referencia)"
                  : "Columnas: numero, monto, operador (y opcionales accion, referencia)"}
              </span>
            </button>
          </>
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
          {r.avisos && r.avisos.length > 0 && (
            <Aviso tono="aviso" titulo={r.avisos.length === 1 ? "Aviso del lote" : "Avisos del lote"}>
              <ul className={r.avisos.length > 1 ? "list-disc pl-4 space-y-0.5" : undefined}>
                {r.avisos.map((x, i) => (
                  <li key={i}>{x}</li>
                ))}
              </ul>
              <p className="mt-1 text-xs opacity-80">No impiden crear el lote. Simular primero lo deja probado.</p>
            </Aviso>
          )}
          <div className="grid grid-cols-2 lg:grid-cols-4 gap-3">
            <Cifra
              etiqueta="Filas que se ejecutarán"
              valor={ejecutables}
              total={r.filas}
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
              etiqueta="Entra hoy (envíos)"
              valor={r.planificados}
              total={r.envios}
              tono={r.sin_capacidad ? "text-aviso" : "text-ok"}
              sub={r.sin_capacidad ? `${r.sin_capacidad} quedan para mañana` : "hay capacidad para todo"}
            />
          </div>

          {r.operadores.length > 0 && !(v.origen && v.filas.some((f) => f.accion !== "transferir")) && (
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

      {/* Barra de acción: pegada abajo dentro de la columna (sobre la navegación del móvil). */}
      {v && r && (
        <div className="sticky z-30 bottom-[calc(4rem+env(safe-area-inset-bottom))] md:bottom-4 mt-6">
          <div className="rounded-3xl border border-borde bg-panel/95 backdrop-blur shadow-elevada px-4 py-3 flex flex-wrap items-end gap-x-4 gap-y-3">
            <div className="w-full sm:w-64">
              <Campo etiqueta="Nombre del lote" value={nombre} onChange={(e) => setNombre(e.target.value)} placeholder={nombrePorDefecto(archivo)} className="h-9" />
            </div>
            <div className="h-9 flex items-center">
              <Interruptor activo={simular} onCambio={setSimular} etiqueta={<span>Simulación</span>} />
            </div>
            <div className="ml-auto flex flex-wrap items-center justify-end gap-3">
              {!simular && (
                <span className="text-sm text-aviso">
                  Real: {bs(r.total_bs)} en {r.envios} envíos + {bs(r.comision_total)} de comisión
                </span>
              )}
              <Boton
                variante="primario"
                disabled={ejecutables === 0 || validando}
                cargando={creando}
                onClick={pedirCrear}
                icono={simular ? <FlaskConical className="size-4" /> : <Play className="size-4" />}
              >
                {simular ? `Simular ${ejecutables} filas` : `Ejecutar ${ejecutables} filas…`}
              </Boton>
            </div>
          </div>
        </div>
      )}

      {v && r && (
        <Dialogo
          abierto={dialogo?.tipo === "real"}
          cerrar={() => setDialogo(null)}
          peligro
          titulo="Ejecutar el lote real"
          acciones={
            <>
              <Boton variante="fantasma" data-autofocus onClick={() => setDialogo(null)}>
                Volver
              </Boton>
              <Boton variante="peligro" icono={<Play className="size-4" />} onClick={() => crear(false)}>
                Enviar {bs(r.total_bs)}
              </Boton>
            </>
          }
        >
          <p>
            Esto <b>mueve dinero de verdad</b> y no se puede deshacer. Revisa el resumen:
          </p>
          <dl className="mt-4 grid grid-cols-2 gap-x-6 gap-y-2.5 rounded-2xl bg-panel-2 px-4 py-3.5">
            <dt className="text-tenue">Total a enviar</dt>
            <dd className="text-right font-semibold tabular-nums">{bs(r.total_bs)}</dd>
            <dt className="text-tenue">Comisión</dt>
            <dd className="text-right tabular-nums">{bs(r.comision_total)}</dd>
            <dt className="text-tenue">Envíos</dt>
            <dd className="text-right tabular-nums">
              {r.envios}
              {r.sin_capacidad > 0 && <span className="text-aviso"> ({r.sin_capacidad} para mañana)</span>}
            </dd>
            <dt className="text-tenue">Filas</dt>
            <dd className="text-right tabular-nums">
              {ejecutables}
              {r.invalidas > 0 && <span className="text-tenue"> ({r.invalidas} omitidas)</span>}
            </dd>
            <dt className="text-tenue">Sale de</dt>
            <dd className="text-right">
              {v.origen ? (
                <Insignia>
                  {sinPrefijo(v.origen.numero) ?? nombreChip(dispositivos, v.origen.dispositivo, v.origen.slot)} · {v.origen.receta}
                </Insignia>
              ) : (
                <span>
                  {r.operadores.map((o) => `${o.operador} (${o.chips} chip${o.chips === 1 ? "" : "s"})`).join(", ") || "—"}
                </span>
              )}
            </dd>
          </dl>
          <p className="mt-3 text-tenue">
            «{nombre.trim() || nombrePorDefecto(archivo)}». Al iniciar se consultan los saldos de los chips y se guardan para verificar después.
          </p>
        </Dialogo>
      )}

      <Dialogo
        abierto={dialogo?.tipo === "duplicado"}
        cerrar={() => setDialogo(null)}
        peligro
        titulo="Ya hay un lote igual"
        acciones={
          <>
            <Boton variante="fantasma" data-autofocus onClick={() => setDialogo(null)}>
              No crear
            </Boton>
            <Boton variante="peligro" onClick={() => crear(true)}>
              Crear igual
            </Boton>
          </>
        }
      >
        <p>{dialogo?.tipo === "duplicado" ? dialogo.detalle : ""}</p>
        <p className="mt-2 text-tenue">
          Es el mismo CSV que un lote real de las últimas 24 h. Si lo creas igual, esos números pueden recibir dos veces. Revisa primero
          ese lote en la lista.
        </p>
      </Dialogo>
    </div>
  );
}
