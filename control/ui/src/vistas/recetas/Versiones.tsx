import { useCallback, useEffect, useState } from "react";
import { api, esControlViejo, mensajeError } from "../../api/cliente";
import { useEvento } from "../../api/eventos";
import type { VersionReceta, VersionRecetaDetalle } from "../../api/tipos";
import { Aviso, Boton, Dialogo, Esqueleto, Insignia, Segmentado, Tarjeta, cx, useToast } from "../../componentes/base";
import { RotateCcw } from "../../componentes/iconos";
import { fechaHora, haceLargo } from "../../util/datos";
import { AvisoViejo, versionCorta } from "./comunes";
import { VistaDiff } from "./diff";

/** Historial de una receta: cada vez que se guarda o retira, el control copia la anterior a `recetas/viejo/`. */
export function Versiones({ nombre, yamlActual, versionActual }: { nombre: string; yamlActual: string; versionActual: string | undefined }) {
  const [lista, setLista] = useState<VersionReceta[] | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [elegida, setElegida] = useState<string | null>(null);
  const [detalle, setDetalle] = useState<VersionRecetaDetalle | null>(null);
  const [errorDetalle, setErrorDetalle] = useState<string | null>(null);
  const [modo, setModo] = useState<"diff" | "yaml">("diff");
  const [restaurando, setRestaurando] = useState(false);
  const [confirmar, setConfirmar] = useState(false);
  const avisar = useToast();

  const cargar = useCallback(async () => {
    try {
      const l = await api.versionesReceta(nombre);
      setLista(l);
      setError(null);
      setElegida((e) => (e && l.some((v) => v.id === e) ? e : (l.find((v) => !v.actual)?.id ?? l[0]?.id ?? null)));
    } catch (e) {
      setError(e);
    }
  }, [nombre]);
  useEffect(() => {
    setLista(null);
    cargar();
  }, [cargar]);
  useEvento((e) => {
    if (e.tipo === "receta_cambiada" && e.nombre === nombre) cargar();
  });

  useEffect(() => {
    setDetalle(null);
    setErrorDetalle(null);
    if (!elegida) return;
    api.versionReceta(nombre, elegida).then(setDetalle, (e) => setErrorDetalle(mensajeError(e)));
  }, [nombre, elegida]);

  const v = lista?.find((x) => x.id === elegida);

  async function restaurar() {
    if (!detalle || !versionActual) return;
    setRestaurando(true);
    try {
      await api.guardarReceta(nombre, detalle.yaml, versionActual);
      avisar("Versión restaurada. La que estaba quedó en el historial.", "ok");
      setConfirmar(false);
    } catch (e) {
      avisar(`No pude restaurar: ${mensajeError(e)}`, "error");
    } finally {
      setRestaurando(false);
    }
  }

  if (error && esControlViejo(error)) return <AvisoViejo que="Versiones" />;
  if (error) return <Aviso>No pude leer las versiones: {mensajeError(error)}</Aviso>;
  if (!lista) return <Esqueleto className="h-64 rounded-3xl" />;

  return (
    <div className="grid gap-4 lg:grid-cols-[minmax(0,280px)_minmax(0,1fr)] items-start">
      <Tarjeta className="p-1.5">
        <ul aria-label="Versiones guardadas">
          {lista.map((x) => (
            <li key={x.id}>
              <button
                type="button"
                aria-current={x.id === elegida || undefined}
                onClick={() => setElegida(x.id)}
                className={cx("w-full text-left rounded-xl px-3 py-2.5 transition-colors", x.id === elegida ? "bg-panel-2" : "hover:bg-panel-2")}
              >
                <div className="flex items-center justify-between gap-2">
                  <span className="text-sm font-medium tabular-nums">{fechaHora(x.guardada_ms)}</span>
                  {x.actual && <Insignia tono="ok">Actual</Insignia>}
                </div>
                <p className="text-xs text-tenue mt-0.5 tabular-nums">
                  {haceLargo(x.guardada_ms)} · <span className="font-mono">{versionCorta(x.version)}</span>
                </p>
              </button>
            </li>
          ))}
        </ul>
        {lista.length <= 1 && <p className="px-3 py-3 text-xs text-tenue">Aún no hay versiones anteriores: aparecen al guardar cambios.</p>}
      </Tarjeta>

      <div className="space-y-3 min-w-0">
        {v && (
          <div className="flex flex-wrap items-center justify-between gap-2">
            <Segmentado
              etiqueta="Cómo ver la versión"
              valor={modo}
              onCambio={setModo}
              opciones={[
                { id: "diff", texto: "Cambios vs actual" },
                { id: "yaml", texto: "YAML" },
              ]}
            />
            {!v.actual && (
              <Boton icono={<RotateCcw className="size-4" />} disabled={!detalle || !versionActual} onClick={() => setConfirmar(true)}>
                Restaurar esta versión
              </Boton>
            )}
          </div>
        )}
        {errorDetalle && <Aviso>{errorDetalle}</Aviso>}
        {!detalle && !errorDetalle && v && <Esqueleto className="h-64 rounded-2xl" />}
        {detalle &&
          (modo === "diff" ? (
            v?.actual ? (
              <p className="text-sm text-tenue">Es la versión actual. Elige una anterior para ver qué cambió.</p>
            ) : (
              <VistaDiff antes={detalle.yaml} despues={yamlActual} etiquetas={[`versión del ${v ? fechaHora(v.guardada_ms) : ""}`, "actual"]} />
            )
          ) : (
            <pre className="rounded-2xl border border-borde/60 bg-panel p-4 font-mono text-[12px] leading-[1.6] overflow-x-auto whitespace-pre">{detalle.yaml}</pre>
          ))}
      </div>

      <Dialogo
        abierto={confirmar}
        cerrar={() => setConfirmar(false)}
        titulo="¿Restaurar esta versión?"
        acciones={
          <>
            <Boton variante="fantasma" data-autofocus onClick={() => setConfirmar(false)}>
              Volver
            </Boton>
            <Boton variante="primario" cargando={restaurando} onClick={restaurar}>
              Restaurar
            </Boton>
          </>
        }
      >
        <p>
          La receta vuelve a como estaba el <b>{v ? fechaHora(v.guardada_ms) : ""}</b>. La versión actual no se pierde: queda en el historial y se puede
          volver a ella.
        </p>
        <p className="mt-2 text-tenue">Si esa versión nunca terminó bien (ni simulada), quedará «sin probar» hasta que la simules.</p>
      </Dialogo>
    </div>
  );
}
