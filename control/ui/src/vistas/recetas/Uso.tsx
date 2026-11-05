import { useCallback, useEffect, useState } from "react";
import { api, esCancelado, esControlViejo, mensajeError } from "../../api/cliente";
import { useEvento } from "../../api/eventos";
import type { EstadisticasReceta, EstadoResultado } from "../../api/tipos";
import { Aviso, Cifra, Esqueleto, Insignia, Segmentado, Tarjeta, Vacio, cx } from "../../componentes/base";
import { Activity } from "../../componentes/iconos";
import { fechaHora, haceLargo, nombreSim, segundos, useAgrupar } from "../../util/datos";
import { AvisoViejo, PasosEjecucion, estadoUi, versionCorta } from "./comunes";

const ESTADOS: EstadoResultado[] = ["ok", "simulado", "abortado", "incierto", "error"];
const COLOR_BARRA: Record<EstadoResultado, string> = {
  ok: "bg-ok",
  simulado: "bg-info",
  abortado: "bg-aviso",
  incierto: "bg-error",
  error: "bg-error/60",
};

/** Hook: estadísticas de una receta, que se releen cuando termina una ejecución. */
export function useEstadisticas(nombre: string, dias = 30) {
  const [datos, setDatos] = useState<EstadisticasReceta | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [cargando, setCargando] = useState(true);
  const cargar = useCallback(
    async (signal?: AbortSignal) => {
      try {
        setDatos(await api.estadisticasReceta(nombre, dias, signal));
        setError(null);
      } catch (e) {
        if (!esCancelado(e)) setError(e);
      } finally {
        setCargando(false);
      }
    },
    [nombre, dias],
  );
  useEffect(() => {
    const c = new AbortController();
    setCargando(true);
    cargar(c.signal);
    return () => c.abort();
  }, [cargar]);
  const agrupada = useAgrupar(() => cargar(), 800);
  useEvento((e) => {
    if ((e.tipo === "receta" && e.fase === "fin") || (e.tipo === "receta_cambiada" && e.nombre === nombre)) agrupada();
  });
  return { datos, error, cargando };
}

const DIAS = ["7", "30", "90"] as const;

export function Uso({ nombre, editarPaso }: { nombre: string; editarPaso: (n: number) => void }) {
  const [dias, setDias] = useState<(typeof DIAS)[number]>("30");
  const { datos, error, cargando } = useEstadisticas(nombre, Number(dias));
  const [abierta, setAbierta] = useState<number | null>(null);

  if (error && esControlViejo(error)) return <AvisoViejo que="Estadísticas" />;
  if (error) return <Aviso>No pude leer las estadísticas: {mensajeError(error)}</Aviso>;
  if (cargando && !datos)
    return (
      <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
        {[0, 1, 2, 3].map((i) => (
          <Esqueleto key={i} className="h-28 rounded-3xl" />
        ))}
      </div>
    );
  if (!datos) return null;

  const total = datos.ejecuciones;
  const bien = (datos.por_estado.ok ?? 0) + (datos.por_estado.simulado ?? 0);

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <p className="text-sm text-tenue">Ejecuciones de esta receta (cualquier versión).</p>
        <Segmentado
          etiqueta="Período"
          valor={dias}
          onCambio={setDias}
          opciones={DIAS.map((d) => ({ id: d, texto: `${d} días` }))}
        />
      </div>

      {total === 0 ? (
        <Tarjeta>
          <Vacio icono={<Activity className="size-5" />} titulo={`Sin ejecuciones en ${datos.dias} días`} texto="Simúlala desde la pestaña Ejecutar." />
        </Tarjeta>
      ) : (
        <>
          <div className="grid gap-3 grid-cols-2 xl:grid-cols-4">
            <Cifra etiqueta="Ejecuciones" valor={total} sub={`${datos.dias} días`} />
            <Cifra
              etiqueta="Terminaron bien"
              valor={`${Math.round((bien / total) * 100)} %`}
              sub={`${bien} de ${total} (ok o simuladas)`}
              tono={bien === total ? "text-ok" : undefined}
            />
            <Cifra
              etiqueta="Tiempo"
              valor={datos.ms_promedio != null ? segundos(datos.ms_promedio) : "—"}
              sub={datos.ms_p90 != null ? `9 de cada 10 en menos de ${segundos(datos.ms_p90)}` : "promedio"}
            />
            <Cifra etiqueta="Última que salió bien" valor={haceLargo(datos.ultima_ok_ms)} sub={datos.ultima_ok_ms ? fechaHora(datos.ultima_ok_ms) : undefined} />
          </div>

          <Tarjeta className="p-4">
            <div className="flex h-2.5 rounded-full overflow-hidden bg-panel-2" role="img" aria-label="Ejecuciones por estado">
              {ESTADOS.map((e) => {
                const n = datos.por_estado[e] ?? 0;
                return n ? <span key={e} className={COLOR_BARRA[e]} style={{ width: `${(n / total) * 100}%` }} /> : null;
              })}
            </div>
            <ul className="mt-3 flex flex-wrap gap-x-5 gap-y-1.5 text-sm">
              {ESTADOS.map((e) => (
                <li key={e} className="inline-flex items-center gap-2">
                  <span className={cx("size-2 rounded-full", COLOR_BARRA[e])} />
                  <span className="text-tenue">{estadoUi[e].corto}</span>
                  <span className="font-semibold tabular-nums">{datos.por_estado[e] ?? 0}</span>
                </li>
              ))}
            </ul>
          </Tarjeta>

          {datos.por_paso_fallido.length > 0 && (
            <Tarjeta className="p-4">
              <h3 className="text-xs font-semibold uppercase tracking-[0.14em] text-tenue">Dónde se cortan</h3>
              <ul className="mt-2.5 divide-y divide-borde/60">
                {datos.por_paso_fallido.map((f) => (
                  <li key={f.paso} className="py-2.5 flex flex-wrap items-center gap-x-3 gap-y-1 text-sm">
                    <span className="font-semibold">Paso {f.paso}</span>
                    <span className="text-tenue tabular-nums">{f.veces} {f.veces === 1 ? "vez" : "veces"}</span>
                    {f.ultimo_detalle && <span className="text-tenue text-xs min-w-0 break-words flex-1">{f.ultimo_detalle}</span>}
                    <button type="button" onClick={() => editarPaso(f.paso)} className="text-xs font-semibold text-acento-texto hover:underline">
                      Editar este paso
                    </button>
                  </li>
                ))}
              </ul>
            </Tarjeta>
          )}
        </>
      )}

      {datos.ultimas.length > 0 && (
        <Tarjeta className="p-1.5">
          <h3 className="px-3 pt-3 pb-1 text-xs font-semibold uppercase tracking-[0.14em] text-tenue">Últimas ejecuciones</h3>
          <ul>
            {datos.ultimas.map((u) => {
              const e = estadoUi[u.estado] ?? estadoUi.error;
              const abiertaEsta = abierta === u.id;
              const fallido = datos.por_paso_fallido.length && u.estado !== "ok" && u.estado !== "simulado" ? u.pasos.length : null;
              return (
                <li key={u.id}>
                  <button
                    type="button"
                    aria-expanded={abiertaEsta}
                    onClick={() => setAbierta(abiertaEsta ? null : u.id)}
                    className="w-full text-left rounded-xl px-3 py-2.5 hover:bg-panel-2 transition-colors flex flex-wrap items-center gap-x-3 gap-y-1"
                  >
                    <Insignia tono={e.tono}>{e.corto}</Insignia>
                    {u.simular && u.estado !== "simulado" && <Insignia tono="info">Simulación</Insignia>}
                    <span className="text-sm tabular-nums">{fechaHora(u.inicio_ms)}</span>
                    <span className="text-xs text-tenue">
                      {nombreSim(u.slot)} · v {versionCorta(u.version)}
                    </span>
                    <span className="ml-auto text-xs text-tenue tabular-nums">{u.duracion_ms != null ? segundos(u.duracion_ms) : ""}</span>
                    {u.detalle && <span className="basis-full text-xs text-tenue break-words">{u.detalle}</span>}
                  </button>
                  {abiertaEsta && (
                    <div className="px-3 pb-4 pt-2 aparecer">
                      {u.pasos.length ? <PasosEjecucion pasos={u.pasos} resaltar={fallido} /> : <p className="text-sm text-tenue">Sin pasos guardados.</p>}
                    </div>
                  )}
                </li>
              );
            })}
          </ul>
        </Tarjeta>
      )}
    </div>
  );
}
