import { Boton, Tarjeta, cx } from "../../../componentes/base";
import { Plus, Trash } from "../../../componentes/iconos";
import { agregarSalida, borrarSalida, fijarSalida, renombrarSalida } from "../modelo";
import { MensajesCampo, SeccionEditor, claseCampo, problemasDe, useEditor } from "./campos";
import { BotonIcono } from "./SeccionParametros";

/** `salida`: datos que se sacan de las pantallas con una regex de un grupo (`saldo_principal: 'BS\s*([\d.,]+)'`). */
export function SeccionSalida() {
  const { modelo: m, cambiar, errores, avisos } = useEditor();
  const agregar = () => {
    let n = m.accion === "saldo" && !m.salida.some(([k]) => k === "saldo_principal") ? "saldo_principal" : "dato";
    for (let k = 1; m.salida.some(([x]) => x === n); k++) n = `dato${k}`;
    cambiar(agregarSalida(n));
  };
  return (
    <SeccionEditor
      id="seccion-salida"
      titulo="Salida"
      ayuda={
        <>
          Datos que se leen de las pantallas. Cada regex lleva un grupo entre paréntesis: lo que coincide con él es el valor. Las recetas de saldo deben
          sacar <span className="font-mono">saldo_principal</span>.
        </>
      }
      acciones={
        <Boton icono={<Plus className="size-4" />} onClick={agregar}>
          Dato
        </Boton>
      }
    >
      <MensajesCampo errores={problemasDe(errores, null, ["salida"])} avisos={problemasDe(avisos, null, ["salida"])} />
      {m.salida.length === 0 ? (
        <p className="text-sm text-tenue rounded-2xl border border-dashed border-borde px-4 py-4">No extrae datos.</p>
      ) : (
        <Tarjeta className="p-3 space-y-2">
          {m.salida.map(([k, v], i) => {
            const err = problemasDe(errores, null, [`salida.${k}`, k]);
            return (
              <div key={i}>
                <div className="grid grid-cols-[minmax(0,10rem)_minmax(0,1fr)_auto] gap-2 items-center">
                  <input
                    aria-label={`Nombre del dato ${i + 1}`}
                    value={k}
                    spellCheck={false}
                    onChange={(e) => cambiar(renombrarSalida(i, e.target.value.replace(/[^a-z0-9_]/gi, "_").toLowerCase()))}
                    className={cx(claseCampo, "h-10 font-mono text-[13px] border-borde")}
                  />
                  <input
                    aria-label={`Regex del dato ${k}`}
                    value={v}
                    spellCheck={false}
                    placeholder="saldo de ([\d.]+)"
                    onChange={(e) => cambiar(fijarSalida(i, e.target.value))}
                    className={cx(claseCampo, "h-10 font-mono text-[13px]", err.length ? "border-error" : "border-borde")}
                  />
                  <BotonIcono etiqueta={`Borrar dato ${k}`} peligro onClick={() => cambiar(borrarSalida(i), `Dato «${k}» borrado`)}>
                    <Trash className="size-4" />
                  </BotonIcono>
                </div>
                <MensajesCampo errores={err} />
              </div>
            );
          })}
        </Tarjeta>
      )}
    </SeccionEditor>
  );
}
