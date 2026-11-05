import { useState } from "react";
import { Boton, Interruptor, Tarjeta, cx } from "../../../componentes/base";
import { ArrowDown, ArrowUp, Plus, Trash, X } from "../../../componentes/iconos";
import type { ParametroReceta } from "../../../api/tipos";
import { agregarParametro, borrarItem, fijar, fijarOpciones, mover, parametrosUsados } from "../modelo";
import { Rotulo, SeccionEditor, claseCampo, problemasDe, useEditor, useIdCampo } from "./campos";

export const BotonIcono = ({ etiqueta, onClick, children, disabled, peligro }: { etiqueta: string; onClick: () => void; children: React.ReactNode; disabled?: boolean; peligro?: boolean }) => (
  <button
    type="button"
    aria-label={etiqueta}
    title={etiqueta}
    onClick={onClick}
    disabled={disabled}
    className={cx(
      "size-8 grid place-items-center rounded-lg text-tenue transition-colors disabled:opacity-30 disabled:pointer-events-none active:scale-95",
      peligro ? "hover:text-error hover:bg-error-suave" : "hover:text-texto hover:bg-panel-2",
    )}
  >
    {children}
  </button>
);

/** Lista como fichas: Enter o coma agrega, × quita. */
export function Opciones({
  id,
  valores,
  onCambio,
  vacio = "cualquiera (o escribe y Enter)",
  limpiar = (v: string) => v.replace(",", ""),
}: {
  id: string;
  valores: string[];
  onCambio: (v: string[]) => void;
  /** Texto cuando la lista está vacía. */
  vacio?: string;
  /** Limpia lo que se escribe. */
  limpiar?: (v: string) => string;
}) {
  const [nuevo, setNuevo] = useState("");
  const agregar = () => {
    const v = nuevo.trim();
    if (v && !valores.includes(v)) onCambio([...valores, v]);
    setNuevo("");
  };
  return (
    <div className={cx(claseCampo, "border-borde min-h-10 py-1.5 flex flex-wrap items-center gap-1.5")}>
      {valores.map((o) => (
        <span key={o} className="inline-flex items-center gap-1 h-6 pl-2 pr-1 rounded-md bg-panel-2 font-mono text-xs">
          {o}
          <button type="button" aria-label={`Quitar ${o}`} onClick={() => onCambio(valores.filter((x) => x !== o))} className="text-tenue hover:text-texto">
            <X className="size-3" />
          </button>
        </span>
      ))}
      <input
        id={id}
        value={nuevo}
        onChange={(e) => setNuevo(limpiar(e.target.value))}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === ",") {
            e.preventDefault();
            agregar();
          } else if (e.key === "Backspace" && !nuevo && valores.length) onCambio(valores.slice(0, -1));
        }}
        onBlur={agregar}
        placeholder={valores.length ? "" : vacio}
        className="flex-1 min-w-24 bg-transparent outline-none font-mono text-xs h-6"
      />
    </div>
  );
}

function TarjetaParametro({ p, i, total }: { p: ParametroReceta; i: number; total: number }) {
  const { cambiar, errores, avisos, modelo } = useEditor();
  const id = useIdCampo(`param${i}`);
  const usado = parametrosUsados(modelo).has(p.nombre);
  const camposDe = (c: string) => [`parametros[${i}].${c}`, `parametros.${i}.${c}`, `${p.nombre}.${c}`];
  const ruta = (c: string) => ["parametros", i, c];
  return (
    <Tarjeta className="p-4">
      <div className="flex items-center gap-2">
        <span className="font-mono text-info text-sm">{`{${p.nombre || "…"}}`}</span>
        {!usado && p.nombre && <span className="text-[11px] text-aviso">no se usa en ningún paso</span>}
        <div className="ml-auto flex items-center">
          <BotonIcono etiqueta="Subir parámetro" disabled={i === 0} onClick={() => cambiar(mover("parametros", i, i - 1))}>
            <ArrowUp className="size-4" />
          </BotonIcono>
          <BotonIcono etiqueta="Bajar parámetro" disabled={i === total - 1} onClick={() => cambiar(mover("parametros", i, i + 1))}>
            <ArrowDown className="size-4" />
          </BotonIcono>
          <BotonIcono etiqueta={`Borrar parámetro ${p.nombre}`} peligro onClick={() => cambiar(borrarItem("parametros", i), `Parámetro «${p.nombre}» borrado`)}>
            <Trash className="size-4" />
          </BotonIcono>
        </div>
      </div>
      <div className="mt-3 grid gap-3 sm:grid-cols-2">
        <Rotulo etiqueta="Nombre" htmlFor={`${id}-n`} errores={problemasDe(errores, null, camposDe("nombre"))}>
          <input
            id={`${id}-n`}
            value={p.nombre}
            spellCheck={false}
            onChange={(e) => cambiar(fijar(ruta("nombre"), e.target.value.replace(/[^a-z0-9_]/gi, "_").toLowerCase()))}
            className={cx(claseCampo, "h-10 font-mono text-[13px] border-borde")}
          />
        </Rotulo>
        <Rotulo etiqueta="Descripción" htmlFor={`${id}-d`}>
          <input
            id={`${id}-d`}
            value={p.descripcion ?? ""}
            placeholder="Lo que ve quien ejecuta"
            onChange={(e) => cambiar(fijar(ruta("descripcion"), e.target.value, true))}
            className={cx(claseCampo, "h-10 border-borde")}
          />
        </Rotulo>
        <Rotulo
          etiqueta="Formato (regex)"
          htmlFor={`${id}-p`}
          ayuda="Opcional. P. ej. ^[67]\d{7}$ para un número de 8 dígitos."
          errores={problemasDe(errores, null, camposDe("patron"))}
          avisos={problemasDe(avisos, null, camposDe("patron"))}
        >
          <input
            id={`${id}-p`}
            value={p.patron ?? ""}
            spellCheck={false}
            placeholder="cualquier texto"
            onChange={(e) => cambiar(fijar(ruta("patron"), e.target.value, true))}
            className={cx(claseCampo, "h-10 font-mono text-[13px] border-borde")}
          />
        </Rotulo>
        <Rotulo etiqueta="Valores permitidos" htmlFor={`${id}-o`} ayuda="Opcional: solo estos (p. ej. los montos del menú).">
          <Opciones id={`${id}-o`} valores={p.opciones ?? []} onCambio={(v) => cambiar(fijarOpciones(i, v))} />
        </Rotulo>
      </div>
      <div className="mt-3">
        <Interruptor
          activo={!!p.secreto}
          onCambio={(v) => cambiar(fijar(ruta("secreto"), v ? true : null))}
          etiqueta={
            <span className="text-left">
              Secreto <span className="text-tenue text-xs">(un PIN: se muestra como •••• y no queda en el historial)</span>
            </span>
          }
        />
      </div>
    </Tarjeta>
  );
}

export function SeccionParametros() {
  const { modelo: m, cambiar, errores } = useEditor();
  const generales = problemasDe(errores, null, ["parametros"]);
  const agregar = () => {
    let n = "param";
    for (let k = 1; m.parametros.some((p) => p.nombre === n); k++) n = `param${k}`;
    cambiar(agregarParametro(n));
  };
  return (
    <SeccionEditor
      id="seccion-parametros"
      titulo="Parámetros"
      ayuda={
        <>
          Lo que se pide al ejecutar. En los pasos se usan como <span className="font-mono text-info">{"{nombre}"}</span>. Un parámetro secreto llamado{" "}
          <span className="font-mono">pin</span> se completa solo con el PIN guardado del chip.
        </>
      }
      acciones={
        <Boton icono={<Plus className="size-4" />} onClick={agregar}>
          Parámetro
        </Boton>
      }
    >
      {generales.map((p, k) => (
        <p key={k} className="text-xs text-error mb-2">
          {p.mensaje}
        </p>
      ))}
      {m.parametros.length === 0 ? (
        <p className="text-sm text-tenue rounded-2xl border border-dashed border-borde px-4 py-4">Sin parámetros: la receta corre siempre igual.</p>
      ) : (
        <div className="grid gap-3 xl:grid-cols-2">
          {m.parametros.map((p, i) => (
            <TarjetaParametro key={i} p={p} i={i} total={m.parametros.length} />
          ))}
        </div>
      )}
    </SeccionEditor>
  );
}
