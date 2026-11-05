import { useEffect, useState } from "react";
import { api, mensajeError } from "../../../api/cliente";
import type { RecetaCompleta } from "../../../api/tipos";
import { Aviso, Boton, Casilla, Dialogo, Esqueleto } from "../../../componentes/base";
import { fechaHora } from "../../../util/datos";
import { VistaDiff } from "../diff";

/**
 * `409 conflicto`: alguien guardó la receta desde que se abrió el editor. Se puede ver qué cambió, quedarse con la
 * versión guardada o, a propósito, guardar la propia encima (contra la versión recién leída).
 */
export function DialogoConflicto({
  abierto,
  cerrar,
  nombre,
  mio,
  guardando,
  usarSuya,
  sobrescribir,
}: {
  abierto: boolean;
  cerrar: () => void;
  nombre: string;
  mio: string;
  guardando: boolean;
  usarSuya: (yaml: string, version: string) => void;
  sobrescribir: (version: string) => void;
}) {
  const [suya, setSuya] = useState<RecetaCompleta | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [verDiff, setVerDiff] = useState(false);
  const [seguro, setSeguro] = useState(false);

  useEffect(() => {
    if (!abierto) return;
    setSuya(null);
    setError(null);
    setVerDiff(false);
    setSeguro(false);
    api.receta(nombre).then(setSuya, (e) => setError(mensajeError(e)));
  }, [abierto, nombre]);

  return (
    <Dialogo
      abierto={abierto}
      cerrar={cerrar}
      titulo="Alguien más cambió esta receta"
      ancho={verDiff ? "max-w-4xl" : "max-w-lg"}
      acciones={
        <>
          <Boton variante="fantasma" data-autofocus onClick={cerrar}>
            Seguir editando
          </Boton>
          <Boton disabled={!suya} onClick={() => suya && usarSuya(suya.yaml, suya.resumen.version ?? "")}>
            Usar la guardada
          </Boton>
          <Boton variante="peligro" disabled={!suya || !seguro} cargando={guardando} onClick={() => suya && sobrescribir(suya.resumen.version ?? "")}>
            Guardar la mía encima
          </Boton>
        </>
      }
    >
      <p>
        Desde que abriste el editor, la receta se guardó en otro lado
        {suya?.resumen.modificada_ms ? ` (${fechaHora(suya.resumen.modificada_ms)})` : ""}. No se guardó nada tuyo.
      </p>
      {error && <Aviso className="mt-3">No pude leer la versión guardada: {error}</Aviso>}
      {!suya && !error && <Esqueleto className="mt-3 h-10" />}
      {suya && (
        <div className="mt-3 space-y-3">
          <button type="button" className="text-sm font-semibold text-acento-texto hover:underline" aria-expanded={verDiff} onClick={() => setVerDiff((v) => !v)}>
            {verDiff ? "Ocultar diferencias" : "Ver diferencias"}
          </button>
          {verDiff && <VistaDiff antes={suya.yaml} despues={mio} etiquetas={["la guardada", "la tuya"]} />}
          <Casilla marcada={seguro} onCambio={setSeguro}>
            Entiendo que «Guardar la mía encima» reemplaza los cambios del otro (quedan en el historial de versiones)
          </Casilla>
        </div>
      )}
    </Dialogo>
  );
}

/** Salir del editor con cambios sin guardar. */
export function DialogoSalir({ destino, cerrar, descartar }: { destino: string | null; cerrar: () => void; descartar: () => void }) {
  return (
    <Dialogo
      abierto={destino !== null}
      cerrar={cerrar}
      titulo="Tienes cambios sin guardar"
      acciones={
        <>
          <Boton variante="fantasma" onClick={descartar}>
            Descartar y salir
          </Boton>
          <Boton variante="primario" data-autofocus onClick={cerrar}>
            Seguir editando
          </Boton>
        </>
      }
    >
      <p>Si sales ahora, se pierden los cambios de esta receta.</p>
    </Dialogo>
  );
}
