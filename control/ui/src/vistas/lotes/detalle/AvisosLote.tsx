import type { Lote } from "../../../api/tipos";
import { Aviso, Boton } from "../../../componentes/base";
import { RotateCcw } from "../../../componentes/iconos";
import { EnlaceManual } from "../comunes";
import type { AccionLote } from "./CabeceraLote";

const filas = (n: number) => (n === 1 ? "1 fila" : `${n} filas`);

/**
 * Las franjas del lote: pausa, filas con problema (con las dos acciones de reintento, en un solo lugar),
 * revisión al terminar y lo que quedó para mañana. Los números son siempre de **filas** (como en el progreso).
 */
export function AvisosLote({
  lote,
  accion,
  alReintentar,
  alReintentarInciertas,
}: {
  lote: Lote;
  accion: AccionLote | null;
  alReintentar: () => void;
  alReintentarInciertas: () => void;
}) {
  const p = lote.progreso;
  // Un servidor viejo no manda `revision`: no romper la pantalla por eso.
  const revision = lote.revision ?? { completo: false, motivos: [] };
  const terminado = lote.estado === "terminado" || lote.estado === "cancelado";
  const conProblema = p.fallidas + p.inciertas + p.parcial;
  const reintentables = p.fallidas + p.parcial;
  const detenido = lote.estado === "listo" || lote.estado === "pausado" || lote.estado === "terminado";

  return (
    <div className="space-y-3 mb-5 empty:hidden">
      {lote.motivo_pausa && (
        <Aviso titulo="Lote en pausa">
          {lote.motivo_pausa}
        </Aviso>
      )}

      {terminado && lote.revision != null && revision.completo && (
        <Aviso tono="ok" titulo={lote.simular ? "Simulación completa: todas las filas llegaron a la confirmación" : "Lote completo: todas las filas salieron OK"} />
      )}
      {terminado && lote.revision != null && !revision.completo && (
        <Aviso tono="aviso" titulo={lote.estado === "cancelado" ? "Lote cancelado antes de completarse" : "El lote terminó, pero no está completo"}>
          <ul className="mt-1 list-disc pl-4">
            {revision.motivos.map((m) => (
              <li key={m}>{m}</li>
            ))}
          </ul>
        </Aviso>
      )}

      {conProblema > 0 && (
        <Aviso
          tono={p.inciertas > 0 ? "error" : "aviso"}
          titulo={`${filas(conProblema)} con problema${lote.estado === "en_curso" ? " · el lote sigue con las demás" : ""}`}
          acciones={
            lote.estado === "cancelado" ? null : detenido ? (
              <>
                {reintentables > 0 && (
                  <Boton
                    variante="primario"
                    className="h-9"
                    icono={<RotateCcw className="size-4" />}
                    cargando={accion === "reintentar"}
                    disabled={accion !== null}
                    onClick={alReintentar}
                  >
                    Reintentar fallidas ({filas(reintentables)})
                  </Boton>
                )}
                {p.inciertas > 0 && (
                  <Boton
                    variante="secundario"
                    className="h-9"
                    icono={<RotateCcw className="size-4" />}
                    cargando={accion === "reintentar-inciertas"}
                    disabled={accion !== null}
                    onClick={alReintentarInciertas}
                  >
                    Reintentar inciertas ({filas(p.inciertas)})…
                  </Boton>
                )}
              </>
            ) : (
              <span className="text-xs">Para reintentar, espera a que el lote termine o páusalo.</span>
            )
          }
        >
          <ul className="mt-1 space-y-1">
            {p.inciertas > 0 && (
              <li>
                <b>{filas(p.inciertas)} incierta{p.inciertas === 1 ? "" : "s"}:</b> el envío pudo haberse hecho o no. Revisa el saldo o el
                historial del chip de origen antes de reintentar. <EnlaceManual seccion="filas" />
              </li>
            )}
            {p.fallidas > 0 && (
              <li>
                <b>{filas(p.fallidas)} fallida{p.fallidas === 1 ? "" : "s"}:</b> no movieron dinero (nunca pasaron la confirmación).
              </li>
            )}
            {p.parcial > 0 && (
              <li>
                <b>{filas(p.parcial)} parcial{p.parcial === 1 ? "" : "es"}:</b> salieron algunas piezas; al reintentar solo se repiten las que
                fallaron.
              </li>
            )}
            {reintentables > 0 && detenido && (
              <li className="opacity-90">
                «Reintentar fallidas» vuelve a poner en cola solo los envíos fallidos y reanuda el lote. Lo que salió OK no se repite.
              </li>
            )}
          </ul>
        </Aviso>
      )}

      {p.sin_capacidad > 0 && lote.estado === "terminado" && (
        <Aviso tono="info" titulo={`${filas(p.sin_capacidad)} para mañana`}>
          No había cupo o saldo en los chips. Con «Reanudar» (hoy con más saldo, u otro día) se retoma solo lo pendiente.{" "}
          <EnlaceManual seccion="chips" />
        </Aviso>
      )}
    </div>
  );
}
