import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { Download, FileSpreadsheet, FlaskConical, Play, RotateCw, Smartphone, Upload } from "../../componentes/iconos";
import { api, esCancelado, FalloApi, mensajeError } from "../../api/cliente";
import type { Dispositivo, OrigenChipsPedido, Validacion } from "../../api/tipos";
import { Aviso, Boton, BotonDescarga, Campo, Cifra, Dialogo, Interruptor, Migas, Tarjeta, Titulo, cx } from "../../componentes/base";
import { sinPrefijo, useRecetas, useRetrasado } from "../../util/datos";
import { bs, describirOrigen } from "./comunes";
import { dejarAviso } from "./detalle/useLote";
import { claveChip, useChipsEnLinea } from "./nuevo/chips";
import { armarPlan, PlanLote } from "./nuevo/PlanLote";
import { TablaValidacion } from "./nuevo/TablaValidacion";
import { ElegirChips, ElegirTipo, InventarioDealer, TIPOS, ejemploCsv, type TipoLote } from "./nuevo/TiposLote";

/**
 * Borrador del lote nuevo, fuera de React: la app remonta la vista al cambiar de pestaña y así lo validado
 * (tipo, chips, CSV, nombre…) sigue ahí al volver. Se vacía al crear el lote.
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

function nombrePorDefecto(archivo: string | null) {
  const base = archivo?.replace(/\.(csv|txt)$/i, "") ?? "Lote";
  const hoy = new Date().toLocaleDateString("es-BO", { day: "2-digit", month: "2-digit" });
  return `${base} · ${hoy}`;
}

/** Cabecera numerada de cada paso del formulario. */
function Paso({ n, titulo, extra, children }: { n: number; titulo: string; extra?: ReactNode; children: ReactNode }) {
  return (
    <Tarjeta className="p-4 sm:p-5 mb-4">
      <div className="flex flex-wrap items-center justify-between gap-x-3 gap-y-2 mb-3.5">
        <h2 className="flex items-center gap-2.5 font-semibold">
          <span className="size-6 rounded-full bg-panel-2 text-xs font-bold grid place-items-center tabular-nums">{n}</span>
          {titulo}
        </h2>
        {extra}
      </div>
      {children}
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
  const [tipo, setTipo] = useBorrador<TipoLote | null>("tipo", null);
  const [accion, setAccion] = useBorrador("accion", "recargar");
  const [seleccion, setSeleccion] = useBorrador<string[]>("seleccion", []);
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
  const [creando, setCreando] = useState(false);
  const [arrastrando, setArrastrando] = useState(false);
  /** Diálogo abierto: confirmar un lote real, o avisar que es igual a otro reciente (`detalle` del 409). */
  const [dialogo, setDialogo] = useState<{ tipo: "real" } | { tipo: "duplicado"; detalle: string } | null>(null);
  const input = useRef<HTMLInputElement>(null);
  const { chips, error: errorChips, cargado: chipsCargados } = useChipsEnLinea();
  const { lista: recetas } = useRecetas();

  // Solo los chips elegidos que siguen en línea.
  const elegidos = useMemo(() => {
    const enLinea = new Set(chips.map((c) => claveChip(c.dispositivo, c.slot)));
    return chipsCargados ? seleccion.filter((k) => enLinea.has(k)) : seleccion;
  }, [chips, chipsCargados, seleccion]);
  const operadoresElegidos = new Set(chips.filter((c) => elegidos.includes(claveChip(c.dispositivo, c.slot))).map((c) => c.operador));
  const origen: OrigenChipsPedido | null =
    tipo === "chips"
      ? {
          chips: [...elegidos].sort().map((k) => {
            const i = k.lastIndexOf("|");
            return { dispositivo: k.slice(0, i), slot: Number(k.slice(i + 1)) };
          }),
          accion,
        }
      : null;

  // Cambiar los chips o la acción invalida lo validado: se vuelve a validar (sin apuro, por si se marcan varios).
  const clave = tipo === "chips" ? `chips|${accion}|${[...elegidos].sort().join(",")}` : "operador";
  const claveQuieta = useRetrasado(clave, 700);
  const claveValidada = useRef(borrador.has("clave") ? (borrador.get("clave") as string) : clave);
  const turno = useRef(0);
  /** La validación en vuelo: una nueva la corta (si no, varias consultas de saldo seguidas ocupan las conexiones). */
  const enVuelo = useRef<AbortController | null>(null);
  useEffect(() => () => enVuelo.current?.abort(), []);
  useEffect(() => {
    if (csv && tipo && claveQuieta === clave && claveValidada.current !== clave) validar(csv, archivo);
  }, [claveQuieta]); // eslint-disable-line react-hooks/exhaustive-deps

  async function validar(contenido: string, nombreArchivo: string | null) {
    const mio = ++turno.current;
    setCsv(contenido);
    setArchivo(nombreArchivo);
    setNombre((n) => n || nombrePorDefecto(nombreArchivo));
    setError(null);
    claveValidada.current = clave;
    borrador.set("clave", clave);
    if (tipo === "chips" && (!origen || origen.chips.length === 0)) {
      enVuelo.current?.abort();
      setValidando(false);
      setV(null);
      setError("Elige al menos un chip");
      return;
    }
    setValidando(true);
    enVuelo.current?.abort();
    const ctl = new AbortController();
    enVuelo.current = ctl;
    try {
      const r = await api.validarLote(contenido, consultarSaldos, origen, ctl.signal);
      if (mio === turno.current) setV(r);
    } catch (e) {
      if (mio !== turno.current || esCancelado(e)) return;
      setError(mensajeError(e));
      setV(null);
    } finally {
      if (mio === turno.current) setValidando(false);
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
      id = (await api.crearLote(nombre.trim() || nombrePorDefecto(archivo), csv, simular, origen, forzar)).id;
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
  const plan = useMemo(() => (v ? armarPlan(v) : null), [v]);
  const info = TIPOS.find((t) => t.id === tipo);
  const variosOperadores = operadoresElegidos.size > 1;
  const columnas =
    tipo === "chips"
      ? variosOperadores
        ? "numero, monto, operador (y opcional referencia)"
        : "numero, monto (y opcionales operador, referencia)"
      : tipo === "recargas"
        ? "numero, monto, operador, accion = recargar (y opcional referencia)"
        : "numero, monto, operador (y opcionales accion, referencia)";
  const filasValidas = v?.filas.filter((f) => f.estado !== "invalida") ?? [];
  // Red de seguridad: "Recargas" con un CSV sin la columna accion haría transferencias.
  const recargasSinAccion = tipo === "recargas" && filasValidas.length > 0 && filasValidas.every((f) => f.accion === "transferir");
  const recargasEnTransferencias = tipo === "transferencias" ? filasValidas.filter((f) => f.accion !== "transferir").length : 0;
  const avisosLote = [...(r?.avisos ?? [])];
  const pasoCsv = tipo === "chips" ? 3 : 2;

  return (
    <div>
      <Titulo
        migas={<Migas tramos={[{ texto: "Lotes", ir: volver }, { texto: "Nuevo" }]} />}
        titulo="Nuevo lote"
        sub="Elige el tipo, sube el CSV y revisa el plan. Nada se envía hasta que lo confirmes."
      />

      <Paso n={1} titulo="Qué vas a enviar">
        <ElegirTipo tipo={tipo} onCambio={setTipo} />
        {tipo === "recargas" && <InventarioDealer chips={chips} recetas={recetas} cargado={chipsCargados} />}
      </Paso>

      {tipo === "chips" && (
        <Paso n={2} titulo="Chips que hacen los envíos">
          <ElegirChips
            dispositivos={dispositivos}
            chips={chips}
            recetas={recetas}
            error={errorChips}
            accion={accion}
            onAccion={setAccion}
            seleccion={elegidos}
            onSeleccion={setSeleccion}
          />
        </Paso>
      )}

      {tipo && info && (
        <Paso
          n={pasoCsv}
          titulo="El CSV"
          extra={
            <BotonDescarga
              variante="fantasma"
              className="h-8 px-3 text-xs -my-1"
              descargar={() => api.descargarPlantilla(tipo)}
              icono={<Download className="size-4" />}
            >
              Plantilla CSV
            </BotonDescarga>
          }
        >
          {pegar ? (
            <div className="space-y-3">
              <textarea
                aria-label="Contenido del CSV"
                aria-describedby="csv-ayuda"
                value={texto}
                onChange={(e) => setTexto(e.target.value)}
                placeholder={ejemploCsv(tipo, variosOperadores)}
                spellCheck={false}
                className="w-full h-40 rounded-xl border border-borde bg-panel px-3 py-2 font-mono text-sm outline-none placeholder:text-tenue/60 focus:border-acento focus:ring-3 focus:ring-acento/15"
              />
              <p id="csv-ayuda" className="text-xs text-tenue">
                Columnas: {columnas}. <b className="text-texto font-semibold">Montos en Bs</b>: 0.50, 5, 12.30.
              </p>
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
                  "w-full flex flex-col items-center justify-center text-center rounded-xl border-2 border-dashed py-9 px-5 cursor-pointer transition-colors disabled:cursor-progress",
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
                  <span className="font-medium inline-flex items-center gap-2 break-all">
                    <FileSpreadsheet className="size-4 text-tenue shrink-0" />
                    {archivo} <span className="text-tenue font-normal whitespace-nowrap">· elegir otro</span>
                  </span>
                ) : (
                  <span className="font-medium">Arrastra el CSV aquí o elige un archivo</span>
                )}
                <span id="archivo-csv-ayuda" className="text-sm text-tenue mt-1">
                  Columnas: {columnas}.
                  <br />
                  Montos en Bs: <span className="font-mono">0.50</span>, <span className="font-mono">5</span>, <span className="font-mono">12.30</span>.
                </span>
              </button>
            </>
          )}
          <div className="flex flex-wrap items-center justify-between gap-3 mt-3">
            <Interruptor
              activo={consultarSaldos}
              onCambio={setConsultarSaldos}
              etiqueta={
                <span className="text-left">
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
        </Paso>
      )}

      {error && (
        <div className="mb-5">
          <Aviso>{error}</Aviso>
        </div>
      )}

      {v && r && plan && tipo && (
        <div className={cx("space-y-5 aparecer transition-opacity duration-200", validando && "opacity-60")}>
          {recargasSinAccion && (
            <Aviso tono="error" titulo="Este CSV no dice «recargar»">
              Sin la columna <span className="font-mono">accion</span> (o con ella vacía) cada fila es una <b>transferencia</b>. Para recargas, pon{" "}
              <span className="font-mono">recargar</span> en la columna <span className="font-mono">accion</span> (mira la plantilla), o elige el tipo
              «Transferencias» si eso es lo que quieres.
            </Aviso>
          )}
          {recargasEnTransferencias > 0 && (
            <Aviso tono="info" titulo={`${recargasEnTransferencias} fila${recargasEnTransferencias === 1 ? "" : "s"} con otra acción`}>
              La columna <span className="font-mono">accion</span> pide algo distinto de transferir en esas filas (p. ej. recargar): van a los chips
              que tengan esa receta.
            </Aviso>
          )}
          {v.avisos_origen && v.avisos_origen.length > 0 && (
            <Aviso tono="aviso" titulo={v.avisos_origen.length === 1 ? "Un chip quedó fuera" : `${v.avisos_origen.length} chips quedaron fuera`}>
              <ul className={v.avisos_origen.length > 1 ? "list-disc pl-4 space-y-0.5" : undefined}>
                {v.avisos_origen.map((x, i) => (
                  <li key={i}>{x}</li>
                ))}
              </ul>
              <p className="mt-1 text-xs opacity-80">El lote se reparte entre los demás chips elegidos.</p>
            </Aviso>
          )}
          {avisosLote.length > 0 && (
            <Aviso tono="aviso" titulo={avisosLote.length === 1 ? "Aviso del lote" : "Avisos del lote"}>
              <ul className={avisosLote.length > 1 ? "list-disc pl-4 space-y-0.5" : undefined}>
                {avisosLote.map((x, i) => (
                  <li key={i}>{x}</li>
                ))}
              </ul>
              <p className="mt-1 text-xs opacity-80">No impiden crear el lote. Simular primero lo deja probado.</p>
            </Aviso>
          )}

          <div className="grid grid-cols-2 lg:grid-cols-4 gap-3">
            <Cifra etiqueta="Filas que se ejecutarán" valor={ejecutables} total={r.filas} sub={r.con_avisos ? `${r.con_avisos} con avisos` : "sin avisos"} />
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

          <PlanLote plan={plan} dispositivos={dispositivos} />

          <TablaValidacion filas={v.filas} dispositivos={dispositivos} />
        </div>
      )}

      {/* Barra de acción: pegada abajo dentro de la columna (sobre la navegación del móvil). */}
      {v && r && tipo && (
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
                disabled={ejecutables === 0 || validando || recargasSinAccion}
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

      {v && r && plan && (
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
            {v.origen && (
              <>
                <dt className="text-tenue">Origen</dt>
                <dd className="text-right">{describirOrigen(v.origen, dispositivos)}</dd>
              </>
            )}
          </dl>
          <div className="mt-3 rounded-2xl border border-borde/70 px-4 py-3">
            <p className="text-xs font-semibold text-tenue mb-2 inline-flex items-center gap-1.5">
              <Smartphone className="size-3.5" />
              {plan.telefonos === 1 ? "1 teléfono, en serie" : `${plan.telefonos} teléfonos trabajarán en paralelo`}
            </p>
            <ul className="space-y-1.5">
              {plan.operadores
                .filter((o) => o.envios > 0)
                .map((o) => (
                  <li key={o.operador} className="flex gap-3 text-sm">
                    <b className="font-semibold w-16 shrink-0">{o.operador}</b>
                    <span className="text-tenue min-w-0">
                      {o.chips
                        .filter((c) => c.envios > 0)
                        .map((c) => `${sinPrefijo(c.numero) ?? `SIM ${c.slot + 1}`}${c.dealer ? " (dealer)" : ""}: ${c.envios} envío${c.envios === 1 ? "" : "s"}`)
                        .join(", ")}
                    </span>
                  </li>
                ))}
            </ul>
          </div>
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
          Es el mismo CSV que un lote real de las últimas 24 h. Si lo creas igual, esos números pueden recibir dos veces. Revisa primero ese lote en
          la lista.
        </p>
      </Dialogo>
    </div>
  );
}
