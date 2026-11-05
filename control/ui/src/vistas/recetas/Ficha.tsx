import { useCallback, useEffect, useState } from "react";
import { api, esCancelado, esControlViejo, mensajeError } from "../../api/cliente";
import { useEvento } from "../../api/eventos";
import type { Dispositivo, RecetaCompleta, ResumenReceta } from "../../api/tipos";
import { Aviso, Boton, Dialogo, Esqueleto, MenuAcciones, Segmentado, cx, useToast } from "../../componentes/base";
import { ArrowLeft, Copy, Download, FlaskConical, X } from "../../componentes/iconos";
import { fechaHora } from "../../util/datos";
import { esRecienGuardada, olvidarRecienGuardada } from "./almacen";
import { AvisoViejo, InsigniasReceta, LineaUso, guardarArchivo, versionCorta } from "./comunes";
import { Ejecutar } from "./Ejecutar";
import { PasosLectura } from "./PasosLectura";
import { Uso } from "./Uso";
import { Versiones } from "./Versiones";

export const PESTANAS_FICHA = ["ejecutar", "pasos", "uso", "versiones"] as const;
export type PestanaFicha = (typeof PESTANAS_FICHA)[number];
const TITULOS: Record<PestanaFicha, string> = { ejecutar: "Ejecutar", pasos: "Pasos", uso: "Uso", versiones: "Versiones" };

/** La receta completa (YAML incluido) desde el control; se relee cuando cambia. */
function useRecetaCompleta(nombre: string) {
  const [datos, setDatos] = useState<RecetaCompleta | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [retirada, setRetirada] = useState(false);
  const cargar = useCallback(
    async (signal?: AbortSignal) => {
      try {
        setDatos(await api.receta(nombre, signal));
        setError(null);
      } catch (e) {
        if (!esCancelado(e)) setError(e);
      }
    },
    [nombre],
  );
  useEffect(() => {
    const c = new AbortController();
    setDatos(null);
    setError(null);
    setRetirada(false);
    cargar(c.signal);
    return () => c.abort();
  }, [cargar]);
  useEvento((e) => {
    if (e.tipo !== "receta_cambiada" || e.nombre !== nombre) return;
    if (e.cambio === "retirada") setRetirada(true);
    else cargar();
  });
  return { datos, error, retirada };
}

export function Ficha({
  resumen,
  pestana,
  irPestana,
  ir,
  dispositivos,
}: {
  resumen: ResumenReceta;
  pestana: PestanaFicha;
  irPestana: (p: PestanaFicha) => void;
  ir: (ruta: string) => void;
  dispositivos: Dispositivo[];
}) {
  const nombre = resumen.nombre;
  const { datos, error, retirada } = useRecetaCompleta(nombre);
  const viejo = esControlViejo(error);
  const [recien, setRecienEstado] = useState(() => esRecienGuardada(nombre));
  const setRecien = (v: boolean) => {
    if (!v) olvidarRecienGuardada();
    setRecienEstado(v);
  };
  const [retirar, setRetirar] = useState(false);
  const [retirando, setRetirando] = useState(false);
  const avisar = useToast();

  // El resumen de la lista se relee con cada evento (ejecución terminada, receta cambiada): manda sobre el de la
  // receta completa, que se leyó al abrir.
  const r: ResumenReceta = { ...(datos?.resumen ?? {}), ...resumen };

  async function confirmarRetiro() {
    setRetirando(true);
    try {
      await api.retirarReceta(nombre);
      avisar(`«${r.titulo}» retirada. El archivo quedó en recetas/viejo.`, "ok");
      setRetirar(false);
      ir("");
    } catch (e) {
      avisar(`No pude retirarla: ${mensajeError(e)}`, "error");
    } finally {
      setRetirando(false);
    }
  }

  const idPanel = `ficha-${nombre}`;
  return (
    <section aria-labelledby={`${idPanel}-t`} className="min-w-0">
      <button type="button" onClick={() => ir("")} className="lg:hidden mb-3 inline-flex items-center gap-1.5 text-sm text-tenue hover:text-texto">
        <ArrowLeft className="size-4" /> Recetas
      </button>
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0">
          <h2 id={`${idPanel}-t`} className="text-2xl font-extrabold tracking-tight break-words">
            {r.titulo}
          </h2>
          <p className="text-xs text-tenue mt-1 flex flex-wrap gap-x-2">
            <span className="font-mono">{r.nombre}</span>
            <span>· {r.operador}</span>
            {r.accion && <span>· {r.accion}</span>}
            {r.nombres_anteriores && r.nombres_anteriores.length > 0 && (
              <span title="Nombres que tuvo antes: el control los sigue aceptando">
                · antes: <span className="font-mono">{r.nombres_anteriores.join(", ")}</span>
              </span>
            )}
            {r.version && (
              <span title={`Versión ${r.version}`}>
                · v <span className="font-mono">{versionCorta(r.version)}</span>
              </span>
            )}
            {r.modificada_ms ? <span>· editada {fechaHora(r.modificada_ms)}</span> : null}
          </p>
          <div className="mt-2 flex flex-wrap items-center gap-1.5">
            <InsigniasReceta r={r} />
            <LineaUso r={r} className="ml-1" />
          </div>
        </div>
        <div className="flex items-center gap-2">
          <Boton variante="primario" onClick={() => ir(`${nombre}/editar`)} disabled={retirada}>
            Editar
          </Boton>
          <MenuAcciones
            etiqueta="Más acciones de la receta"
            acciones={[
              { texto: "Duplicar", icono: <Copy className="size-4" />, alElegir: () => ir(`nueva?desde=${encodeURIComponent(nombre)}`), deshabilitado: !datos },
              {
                texto: "Descargar YAML",
                icono: <Download className="size-4" />,
                deshabilitado: !datos,
                alElegir: () => datos && guardarArchivo(datos.resumen.archivo ?? `${nombre}.yaml`, datos.yaml),
              },
              { texto: "Retirar…", icono: <X className="size-4" />, peligro: true, deshabilitado: retirada || viejo, alElegir: () => setRetirar(true) },
            ]}
          />
        </div>
      </div>

      {retirada && (
        <Aviso tono="aviso" className="mt-4" titulo="Esta receta se retiró">
          Alguien la retiró mientras la mirabas. Su archivo está en recetas/viejo.
        </Aviso>
      )}
      {recien && r.mueve_dinero && r.sin_probar !== false && (
        <Aviso
          tono="aviso"
          className="mt-4"
          titulo="Guardada. No se ha probado: simúlala"
          acciones={
            <>
              <Boton
                variante="primario"
                icono={<FlaskConical className="size-4" />}
                onClick={() => {
                  setRecien(false);
                  irPestana("ejecutar");
                }}
              >
                Simúlala ahora
              </Boton>
              <Boton variante="fantasma" onClick={() => setRecien(false)}>
                Después
              </Boton>
            </>
          }
        >
          Esta receta mueve dinero. Una simulación recorre todo el flujo y responde «No» en la confirmación.
        </Aviso>
      )}

      <div className="mt-5 mb-4 max-w-full overflow-x-auto">
        <Segmentado
          etiqueta="Secciones de la receta"
          controla={idPanel}
          valor={pestana}
          onCambio={irPestana}
          opciones={PESTANAS_FICHA.map((p) => ({ id: p, texto: TITULOS[p] }))}
        />
      </div>

      <div id={idPanel} role="tabpanel" aria-label={TITULOS[pestana]} className="min-w-0">
        {/* Ejecutar queda montado para no perder el resultado al mirar otra pestaña. */}
        <div hidden={pestana !== "ejecutar"}>
          <Ejecutar receta={r} dispositivos={dispositivos} />
        </div>
        {pestana === "pasos" &&
          (viejo ? (
            <AvisoViejo que="Ver los pasos" />
          ) : error ? (
            <Aviso>{mensajeError(error)}</Aviso>
          ) : datos ? (
            <PasosLectura yaml={datos.yaml} avisos={datos.avisos} />
          ) : (
            <Esqueleto className="h-64 rounded-3xl" />
          ))}
        {pestana === "uso" && <Uso nombre={nombre} editarPaso={(n) => ir(`${nombre}/editar?paso=${n}`)} />}
        {pestana === "versiones" &&
          (viejo ? (
            <AvisoViejo que="Versiones" />
          ) : datos ? (
            <Versiones nombre={nombre} yamlActual={datos.yaml} versionActual={datos.resumen.version} />
          ) : (
            <Esqueleto className={cx("h-64 rounded-3xl")} />
          ))}
      </div>

      <Dialogo
        abierto={retirar}
        cerrar={() => setRetirar(false)}
        peligro
        titulo={`¿Retirar «${r.titulo}»?`}
        acciones={
          <>
            <Boton variante="fantasma" data-autofocus onClick={() => setRetirar(false)}>
              Volver
            </Boton>
            <Boton variante="peligro" cargando={retirando} onClick={confirmarRetiro}>
              Retirar
            </Boton>
          </>
        }
      >
        <p>
          Deja de aparecer en la lista y los lotes nuevos no la usarán. El archivo no se borra: se mueve a <span className="font-mono">recetas/viejo/</span>{" "}
          junto con sus versiones.
        </p>
        <p className="mt-2 text-tenue">Los lotes que ya están corriendo siguen con su copia de la receta hasta terminar.</p>
      </Dialogo>
    </section>
  );
}
