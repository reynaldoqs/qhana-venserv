import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { FalloApi, api, esControlViejo, mensajeError } from "../../../api/cliente";
import { useEvento } from "../../../api/eventos";
import type { EjecucionReceta, ProblemaReceta } from "../../../api/tipos";
import { Aviso, Boton, Esqueleto, Migas, Segmentado, Titulo, useToast } from "../../../componentes/base";
import { RotateCcw } from "../../../componentes/iconos";
import { fijarGuardia, irSinGuardia } from "../../../util/guardia";
import { limpiarBorrador, marcarRecienGuardada, verBorrador } from "../almacen";
import { AvisoViejo } from "../comunes";
import { aplicar, leer, mueveDinero, plantillaCopia, plantillaNueva, type Cambio, type RecetaModelo } from "../modelo";
import { EditorCtx, type CtxEditor } from "./campos";
import { DialogoConflicto, DialogoSalir } from "./Dialogos";
import { EditorYaml } from "./EditorYaml";
import { Problemas } from "./Problemas";
import { SeccionDatos } from "./SeccionDatos";
import { SeccionParametros } from "./SeccionParametros";
import { SeccionPasos } from "./SeccionPasos";
import { SeccionSalida } from "./SeccionSalida";
import { useValidacion } from "./useValidacion";

type Base = { version: string; yaml: string };
type Carga = { estado: "cargando" } | { estado: "listo" } | { estado: "error"; error: unknown };

const MODELO_VACIO: RecetaModelo = {
  nombre: "",
  titulo: "",
  operador: "",
  descripcion: "",
  modo: "pantalla",
  accion: "",
  chip_tipo: "",
  nombres_anteriores: [],
  limites: {},
  parametros: [],
  pasos: [],
  salida: [],
};

/**
 * Editor de recetas: visual (formularios por paso) y YAML, sobre el mismo texto. Valida en vivo, prueba pasos contra
 * pantallas, guarda con control de versión y avisa si se sale con cambios sin guardar.
 */
export function Editor({
  nombre,
  desde,
  pasoInicial,
  borrador: conBorrador,
  ir,
}: {
  /** Receta a editar; null: receta nueva. */
  nombre: string | null;
  desde?: string | null;
  pasoInicial?: number | null;
  borrador?: boolean;
  ir: (ruta: string) => void;
}) {
  const nuevo = nombre === null;
  const avisar = useToast();
  const [carga, setCarga] = useState<Carga>({ estado: "cargando" });
  const [texto, setTextoEstado] = useState("");
  const textoRef = useRef("");
  const [original, setOriginal] = useState("");
  const [base, setBase] = useState<Base | null>(null);
  const [origen, setOrigen] = useState<string | null>(null);
  const [pestana, setPestana] = useState<"visual" | "yaml">("visual");
  const [bloqueoPestana, setBloqueoPestana] = useState<string | null>(null);
  const [guardando, setGuardando] = useState(false);
  const [errorGuardar, setErrorGuardar] = useState<string | null>(null);
  const [erroresGuardar, setErroresGuardar] = useState<ProblemaReceta[]>([]);
  const [conflicto, setConflicto] = useState(false);
  const [cambioExterno, setCambioExterno] = useState<"editada" | "retirada" | null>(null);
  const [deshacer, setDeshacer] = useState<{ texto: string; etiqueta: string; id: number } | null>(null);
  const [salida, setSalida] = useState<string | null>(null);
  const [ultimas, setUltimas] = useState<EjecucionReceta[]>([]);
  const [destacado, setDestacado] = useState<number | null>(null);

  const setTexto = useCallback((t: string) => {
    textoRef.current = t;
    setTextoEstado(t);
    setErroresGuardar([]);
  }, []);
  const cargarTexto = useCallback(
    (t: string, orig: string, b: Base | null) => {
      setTexto(t);
      setOriginal(orig);
      setBase(b);
    },
    [setTexto],
  );

  // ── Carga ──
  useEffect(() => {
    let vivo = true;
    const fallo = (e: unknown) => vivo && setCarga({ estado: "error", error: e });
    (async () => {
      try {
        if (nombre) {
          const d = await api.receta(nombre);
          if (!vivo) return;
          cargarTexto(d.yaml, d.yaml, { version: d.resumen.version ?? "", yaml: d.yaml });
        } else if (desde) {
          const d = await api.receta(desde);
          if (!vivo) return;
          cargarTexto(plantillaCopia(d.yaml, desde), "", null);
          setOrigen(`Copia de «${d.resumen.titulo}»`);
        } else if (conBorrador && verBorrador()) {
          const b = verBorrador()!;
          cargarTexto(b.yaml, "", null);
          setOrigen(b.origen);
        } else {
          const t = plantillaNueva();
          cargarTexto(t, t, null);
          if (conBorrador) setOrigen("El borrador de la consola se perdió al recargar la página: empieza de la plantilla.");
        }
        if (vivo) setCarga({ estado: "listo" });
      } catch (e) {
        fallo(e);
      }
    })();
    if (nombre)
      api.estadisticasReceta(nombre, 30).then(
        (s) => vivo && setUltimas(s.ultimas ?? []),
        () => {
          /* sin estadísticas: el probador pide la pantalla a mano */
        },
      );
    return () => {
      vivo = false;
    };
  }, [nombre, desde, conBorrador, cargarTexto]);

  // Ir al paso pedido (desde "Editar este paso").
  useEffect(() => {
    if (carga.estado !== "listo" || !pasoInicial) return;
    requestAnimationFrame(() => document.getElementById(`paso-${pasoInicial}`)?.scrollIntoView({ behavior: "smooth", block: "center" }));
    setDestacado(pasoInicial);
    const t = window.setTimeout(() => setDestacado(null), 2400);
    return () => window.clearTimeout(t);
  }, [carga.estado, pasoInicial]);

  // ── Lectura y validación ──
  const lectura = useMemo(() => leer(texto), [texto]);
  const ultimoModelo = useRef<RecetaModelo>(MODELO_VACIO);
  if (lectura.ok) ultimoModelo.current = lectura.modelo;
  const modelo = ultimoModelo.current;
  const { res, validando, sinServidor } = useValidacion(texto, carga.estado === "listo" && lectura.ok);
  // Con el YAML roto no se pregunta al control: lo que dijo antes ya no vale.
  const errores = useMemo(() => (lectura.ok ? [...(res?.errores ?? []), ...erroresGuardar] : []), [lectura.ok, res, erroresGuardar]);
  const avisos = lectura.ok ? (res?.avisos ?? []) : [];
  const errorLocal = lectura.ok ? null : `${lectura.linea ? `línea ${lectura.linea}: ` : ""}${lectura.error}`;

  const sucio = texto !== original;
  const sucioRef = useRef(false);
  sucioRef.current = sucio && carga.estado === "listo";

  // ── Cambios desde el editor visual ──
  const cambiar = useCallback(
    (c: Cambio, etiquetaDeshacer?: string) => {
      const antes = textoRef.current;
      const despues = aplicar(antes, c);
      if (despues === antes) return;
      setTexto(despues);
      if (etiquetaDeshacer) setDeshacer({ texto: antes, etiqueta: etiquetaDeshacer, id: Date.now() });
    },
    [setTexto],
  );
  useEffect(() => {
    if (!deshacer) return;
    const t = window.setTimeout(() => setDeshacer(null), 8000);
    return () => window.clearTimeout(t);
  }, [deshacer]);

  // ── Salir con cambios sin guardar ──
  const rutaPropia = nuevo ? "recetas/nueva" : `recetas/${encodeURIComponent(nombre!)}/editar`;
  useEffect(
    () =>
      fijarGuardia((destino) => {
        if (!sucioRef.current) return false;
        if (destino.replace(/^#/, "").startsWith(rutaPropia)) return false;
        setSalida(destino);
        return true;
      }),
    [rutaPropia],
  );
  useEffect(() => {
    const f = (e: BeforeUnloadEvent) => {
      if (!sucioRef.current) return;
      e.preventDefault();
      e.returnValue = "";
    };
    window.addEventListener("beforeunload", f);
    return () => window.removeEventListener("beforeunload", f);
  }, []);

  // ── Cambios hechos en otro lado ──
  const recargar = useCallback(async () => {
    if (!nombre) return;
    try {
      const d = await api.receta(nombre);
      cargarTexto(d.yaml, d.yaml, { version: d.resumen.version ?? "", yaml: d.yaml });
      setCambioExterno(null);
    } catch (e) {
      avisar(`No pude recargar: ${mensajeError(e)}`, "error");
    }
  }, [nombre, cargarTexto, avisar]);
  useEvento((e) => {
    if (nuevo || e.tipo !== "receta_cambiada" || e.nombre !== nombre) return;
    if (e.cambio === "retirada") return setCambioExterno("retirada");
    if (e.version && base && e.version === base.version) return; // es la nuestra
    if (sucioRef.current) setCambioExterno("editada");
    else {
      recargar();
      avisar("La receta cambió en otro lado: se cargó la versión nueva");
    }
  });

  // ── Guardar ──
  async function guardar(baseForzada?: string) {
    setGuardando(true);
    setErrorGuardar(null);
    try {
      const r = nuevo ? await api.crearReceta(texto) : await api.guardarReceta(nombre!, texto, baseForzada ?? base?.version ?? "");
      sucioRef.current = false;
      setOriginal(texto);
      setConflicto(false);
      limpiarBorrador();
      const nombreFinal = r?.nombre || modelo.nombre;
      if (r?.mueve_dinero ?? mueveDinero(modelo)) marcarRecienGuardada(nombreFinal);
      avisar("Receta guardada", "ok");
      irSinGuardia(`recetas/${encodeURIComponent(nombreFinal)}`);
    } catch (e) {
      if (e instanceof FalloApi && e.codigo === "conflicto") setConflicto(true);
      else if (e instanceof FalloApi && e.codigo === "existe") setErrorGuardar(`Ya hay una receta llamada «${modelo.nombre}». Elige otro nombre.`);
      else if (e instanceof FalloApi && e.codigo === "receta_invalida") {
        setErroresGuardar(e.errores ?? []);
        setErrorGuardar(e.errores?.length ? "El control encontró errores: corrígelos y vuelve a guardar." : mensajeError(e));
      } else setErrorGuardar(mensajeError(e));
    } finally {
      setGuardando(false);
    }
  }

  const cambiarPestana = (p: "visual" | "yaml") => {
    if (p === "visual" && !lectura.ok) {
      setBloqueoPestana(`No se puede pasar al editor visual: el YAML tiene un error (${errorLocal}). Corrígelo primero.`);
      return;
    }
    setBloqueoPestana(null);
    setPestana(p);
  };
  useEffect(() => {
    if (lectura.ok) setBloqueoPestana(null);
  }, [lectura.ok]);

  const volver = () => ir(nuevo ? (desde ? encodeURIComponent(desde) : "") : encodeURIComponent(nombre!));
  const puedeGuardar = carga.estado === "listo" && lectura.ok && errores.length === 0 && (nuevo || sucio) && !guardando;
  const titulo = nuevo ? "Nueva receta" : `Editar «${modelo.titulo || nombre}»`;

  if (carga.estado === "error") {
    return (
      <div>
        <Titulo migas={<Migas tramos={[{ texto: "Recetas", ir: () => ir("") }, { texto: titulo }]} />} titulo={titulo} />
        {esControlViejo(carga.error) ? <AvisoViejo que="Editar recetas" /> : <Aviso>No pude abrir la receta: {mensajeError(carga.error)}</Aviso>}
        <Boton className="mt-4" onClick={() => ir("")}>
          Volver a Recetas
        </Boton>
      </div>
    );
  }

  const ctx: CtxEditor = { modelo, yaml: texto, cambiar, errores, avisos, nuevo, ultimas };

  return (
    <EditorCtx.Provider value={ctx}>
      <Titulo
        migas={
          <Migas
            tramos={[
              { texto: "Recetas", ir: () => ir("") },
              ...(nombre ? [{ texto: modelo.titulo || nombre, ir: () => ir(encodeURIComponent(nombre)) }] : []),
              { texto: nuevo ? "Nueva" : "Editar" },
            ]}
          />
        }
        titulo={titulo}
        sub={origen ?? (sucio ? "Cambios sin guardar" : nuevo ? "Completa los datos y los pasos" : "Sin cambios")}
        acciones={
          <>
            <Boton variante="fantasma" onClick={volver}>
              Cancelar
            </Boton>
            <Boton variante="primario" cargando={guardando} disabled={!puedeGuardar} onClick={() => guardar()} title={errores.length ? "Corrige los errores para guardar" : undefined}>
              {nuevo ? "Crear receta" : "Guardar"}
            </Boton>
          </>
        }
      />

      <div className="space-y-3 mb-4 empty:hidden">
        {cambioExterno === "retirada" && (
          <Aviso tono="aviso" titulo="La receta se retiró">
            Alguien la retiró mientras la editabas. Para conservar tus cambios, cópialos desde la pestaña YAML o crea una receta nueva.
          </Aviso>
        )}
        {errorGuardar && <Aviso titulo="No se guardó">{errorGuardar}</Aviso>}
        {nuevo && modelo.pasos.some((p) => p.accion === "confirmar" || p.mueve_dinero) && (
          <Aviso tono="aviso">Esta receta mueve dinero. Después de crearla, simúlala antes de usarla en real.</Aviso>
        )}
      </div>

      {carga.estado === "cargando" ? (
        <div className="space-y-4">
          <Esqueleto className="h-10 w-48 rounded-2xl" />
          <Esqueleto className="h-72 rounded-3xl" />
        </div>
      ) : (
        <>
          <div className="mb-4 flex flex-wrap items-center gap-3">
            <Segmentado
              etiqueta="Modo del editor"
              controla="editor-cuerpo"
              valor={pestana}
              onCambio={cambiarPestana}
              opciones={[
                { id: "visual", texto: "Visual" },
                { id: "yaml", texto: "YAML" },
              ]}
            />
            <span className="text-xs text-tenue">Los dos editan el mismo archivo: los comentarios del YAML se conservan.</span>
          </div>
          {bloqueoPestana && (
            <Aviso className="mb-4" tono="error">
              {bloqueoPestana}
            </Aviso>
          )}

          <div className="grid gap-6 lg:grid-cols-[minmax(0,1fr)_minmax(0,300px)] items-start">
            <div id="editor-cuerpo" role="tabpanel" className="min-w-0 space-y-9">
              {pestana === "yaml" || !lectura.ok ? (
                <EditorYaml valor={texto} onCambio={setTexto} lineaError={lectura.ok ? undefined : lectura.linea} />
              ) : (
                <>
                  <SeccionDatos />
                  <SeccionParametros />
                  <SeccionPasos destacado={destacado} />
                  <SeccionSalida />
                </>
              )}
              <div className="flex flex-wrap justify-end gap-2 border-t border-borde/60 pt-5">
                <Boton variante="fantasma" onClick={volver}>
                  Cancelar
                </Boton>
                <Boton variante="primario" cargando={guardando} disabled={!puedeGuardar} onClick={() => guardar()}>
                  {nuevo ? "Crear receta" : "Guardar"}
                </Boton>
              </div>
            </div>
            <aside aria-label="Revisión de la receta" className="order-first lg:order-none lg:sticky lg:top-0 min-w-0 space-y-3">
              {cambioExterno === "editada" && (
          <Aviso
            tono="aviso"
            titulo="Cambió en otro lado"
            acciones={
              <Boton variante="secundario" icono={<RotateCcw className="size-4" />} onClick={() => setConflicto(true)}>
                Ver la otra versión
              </Boton>
            }
          >
            Alguien la guardó mientras la editabas. Puedes seguir: al guardar te avisará el conflicto y podrás elegir.
          </Aviso>
        )}
              <Problemas errores={errores} avisos={avisos} validando={validando} local={errorLocal} sinServidor={sinServidor} />
            </aside>
          </div>
        </>
      )}

      {deshacer && (
        <div
          role="status"
          className="fixed z-[60] bottom-20 md:bottom-6 left-1/2 -translate-x-1/2 flex items-center gap-3 rounded-full bg-panel-2 border border-borde pl-4 pr-1.5 h-11 text-sm font-medium shadow-elevada aparecer"
        >
          <span className="whitespace-nowrap">{deshacer.etiqueta}</span>
          <button
            type="button"
            onClick={() => {
              setTexto(deshacer.texto);
              setDeshacer(null);
            }}
            className="h-8 px-3 rounded-full bg-panel text-acento-texto font-semibold hover:brightness-125"
          >
            Deshacer
          </button>
        </div>
      )}

      {!nuevo && (
        <DialogoConflicto
          abierto={conflicto}
          cerrar={() => setConflicto(false)}
          nombre={nombre!}
          mio={texto}
          guardando={guardando}
          usarSuya={(yaml, version) => {
            cargarTexto(yaml, yaml, { version, yaml });
            setCambioExterno(null);
            setConflicto(false);
            avisar("Se cargó la versión guardada; tus cambios se descartaron");
          }}
          sobrescribir={(version) => {
            setCambioExterno(null);
            guardar(version);
          }}
        />
      )}
      <DialogoSalir
        destino={salida}
        cerrar={() => setSalida(null)}
        descartar={() => {
          const d = salida!;
          setSalida(null);
          sucioRef.current = false;
          if (conBorrador) limpiarBorrador();
          irSinGuardia(d);
        }}
      />
    </EditorCtx.Provider>
  );
}
