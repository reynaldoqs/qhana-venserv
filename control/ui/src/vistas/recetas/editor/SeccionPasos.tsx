import { useState } from "react";
import { Boton, Casilla, Insignia, cx } from "../../../componentes/base";
import { ArrowDown, ArrowUp, Copy, Plus, TestTube, Trash, Wallet, X } from "../../../componentes/iconos";
import { AYUDA_ACCION, AYUDA_MODIFICADOR } from "../comunes";
import {
  ACCIONES,
  MODIFICADORES,
  borrarItem,
  cambiarAccion,
  duplicarItem,
  fijarModificador,
  fijarValorPaso,
  insertarPaso,
  mover,
  type Accion,
  type Modificador,
  type PasoModelo,
} from "../modelo";
import { CampoNumero, CampoPlantilla, MensajesCampo, Rotulo, SeccionEditor, problemasDe, useEditor, useIdCampo } from "./campos";
import { BotonIcono } from "./SeccionParametros";
import { ProbarPaso } from "./ProbarPaso";

/** Separador entre pasos: "+" abre las cinco acciones para insertar ahí. */
function InsertarPaso({ en, siempre }: { en: number; siempre?: boolean }) {
  const { cambiar } = useEditor();
  const [abierto, setAbierto] = useState(false);
  return (
    <div className={cx("group relative flex items-center justify-center", siempre ? "py-1" : "h-7")}>
      {!siempre && <span aria-hidden className="absolute inset-x-6 top-1/2 h-px bg-borde/0 group-hover:bg-borde/70 group-focus-within:bg-borde/70 transition-colors" />}
      {abierto ? (
        <div className="relative z-10 flex flex-wrap justify-center gap-1 rounded-2xl bg-panel border border-borde p-1 shadow-elevada aparecer" role="group" aria-label={`Insertar paso ${en + 1}`}>
          {ACCIONES.map((a) => (
            <button
              key={a}
              type="button"
              onClick={() => {
                cambiar(insertarPaso(en, a));
                setAbierto(false);
              }}
              className="h-8 px-3 rounded-xl text-sm hover:bg-panel-2 transition-colors"
            >
              {AYUDA_ACCION[a].titulo}
            </button>
          ))}
          <button type="button" aria-label="Cerrar" onClick={() => setAbierto(false)} className="size-8 grid place-items-center rounded-xl text-tenue hover:text-texto">
            <X className="size-4" />
          </button>
        </div>
      ) : siempre ? (
        <Boton icono={<Plus className="size-4" />} onClick={() => setAbierto(true)}>
          Agregar paso
        </Boton>
      ) : (
        <button
          type="button"
          onClick={() => setAbierto(true)}
          aria-label={`Insertar un paso antes del ${en + 1}`}
          className="relative z-10 inline-flex items-center gap-1 h-6 px-2.5 rounded-full bg-lienzo border border-borde text-[11px] text-tenue opacity-60 md:opacity-0 group-hover:opacity-100 focus-visible:opacity-100 hover:text-texto transition-opacity duration-150"
        >
          <Plus className="size-3" /> Insertar
        </button>
      )}
    </div>
  );
}

const CAMPOS_VALOR = (a: Accion | null) => ["valor", "accion", ...(a ? [a] : [])];

function TarjetaPaso({ p, i, total, destacado }: { p: PasoModelo; i: number; total: number; destacado: boolean }) {
  const { cambiar, errores, avisos, modelo } = useEditor();
  const id = useIdCampo(`paso${i}`);
  const [probar, setProbar] = useState(false);
  const n = i + 1;
  const dinero = p.accion === "confirmar" || !!p.mueve_dinero;
  const ayuda = p.accion ? AYUDA_ACCION[p.accion] : null;
  const params = modelo.parametros.map((x) => x.nombre).filter(Boolean);
  const errPaso = problemasDe(errores, n);
  const avPaso = problemasDe(avisos, n);
  const conocidos = new Set<string>([...CAMPOS_VALOR(p.accion), ...MODIFICADORES, "mueve_dinero"]);
  const sueltosE = errPaso.filter((x) => !x.campo || !conocidos.has(x.campo));
  const sueltosA = avPaso.filter((x) => !x.campo || !conocidos.has(x.campo));
  const faltan = MODIFICADORES.filter((m) => p[m] == null);

  return (
    <div
      id={`paso-${n}`}
      className={cx(
        "scroll-mt-4 rounded-3xl border bg-panel p-4 transition-[border-color,box-shadow] duration-300",
        errPaso.length ? "border-error/60" : dinero ? "border-aviso/50" : "border-borde/60",
        destacado && "ring-2 ring-info/60",
      )}
    >
      <div className="flex flex-wrap items-center gap-2">
        <span className="size-7 shrink-0 rounded-full bg-panel-2 text-xs font-bold grid place-items-center tabular-nums">{n}</span>
        <select
          aria-label={`Acción del paso ${n}`}
          value={p.accion ?? ""}
          onChange={(e) => cambiar(cambiarAccion(i, e.target.value as Accion))}
          className="h-9 rounded-xl border border-borde bg-panel pl-3 pr-2 text-sm font-semibold outline-none cursor-pointer hover:border-tenue/40 focus:border-acento focus:ring-4 focus:ring-acento/15"
        >
          {!p.accion && <option value="">¿Acción?</option>}
          {ACCIONES.map((a) => (
            <option key={a} value={a}>
              {AYUDA_ACCION[a].titulo}
            </option>
          ))}
        </select>
        {dinero && (
          <Insignia tono="aviso">
            <Wallet className="size-3.5" /> Mueve dinero
          </Insignia>
        )}
        <div className="ml-auto flex items-center">
          <BotonIcono etiqueta={`Subir paso ${n}`} disabled={i === 0} onClick={() => cambiar(mover("pasos", i, i - 1))}>
            <ArrowUp className="size-4" />
          </BotonIcono>
          <BotonIcono etiqueta={`Bajar paso ${n}`} disabled={i === total - 1} onClick={() => cambiar(mover("pasos", i, i + 1))}>
            <ArrowDown className="size-4" />
          </BotonIcono>
          <BotonIcono etiqueta={`Duplicar paso ${n}`} onClick={() => cambiar(duplicarItem("pasos", i))}>
            <Copy className="size-4" />
          </BotonIcono>
          <BotonIcono etiqueta={`Borrar paso ${n}`} peligro onClick={() => cambiar(borrarItem("pasos", i), `Paso ${n} borrado`)}>
            <Trash className="size-4" />
          </BotonIcono>
        </div>
      </div>
      {ayuda && <p className="mt-2 text-xs text-tenue">{ayuda.ayuda}</p>}

      <div className="mt-3">
        <Rotulo
          etiqueta={ayuda?.valor ?? "Valor"}
          htmlFor={`${id}-v`}
          errores={problemasDe(errores, n, CAMPOS_VALOR(p.accion))}
          avisos={problemasDe(avisos, n, CAMPOS_VALOR(p.accion))}
        >
          <CampoPlantilla
            id={`${id}-v`}
            valor={p.valor}
            parametros={params}
            invalido={problemasDe(errores, n, CAMPOS_VALOR(p.accion)).length > 0}
            placeholder={p.accion === "marcar" ? "*105#" : p.accion === "enviar" ? "{monto}" : "Texto de la opción"}
            onCambio={(v) => p.accion && cambiar(fijarValorPaso(i, p.accion, v))}
          />
        </Rotulo>
      </div>

      {MODIFICADORES.some((m) => p[m] != null) && (
        <div className="mt-3 grid gap-3 md:grid-cols-2">
          {MODIFICADORES.filter((m) => p[m] != null).map((m) => (
            <Modificador key={m} campo={m} p={p} i={i} id={`${id}-${m}`} params={params} />
          ))}
        </div>
      )}

      <div className="mt-3 flex flex-wrap items-center gap-1.5">
        {faltan.map((m) => (
          <button
            key={m}
            type="button"
            title={AYUDA_MODIFICADOR[m].ayuda}
            onClick={() => cambiar(fijarModificador(i, m, m === "timeout_ms" ? 20000 : ""))}
            className="inline-flex items-center gap-1 h-7 px-2.5 rounded-full border border-dashed border-borde text-xs text-tenue hover:text-texto hover:border-tenue/60 transition-colors"
          >
            <Plus className="size-3" /> {AYUDA_MODIFICADOR[m].titulo}
          </button>
        ))}
        <button
          type="button"
          aria-expanded={probar}
          onClick={() => setProbar((x) => !x)}
          className={cx(
            "ml-auto inline-flex items-center gap-1.5 h-7 px-3 rounded-full text-xs font-semibold transition-colors",
            probar ? "bg-info-suave text-info" : "bg-panel-2 text-texto hover:brightness-125",
          )}
        >
          <TestTube className="size-3.5" /> Probar
        </button>
      </div>

      {(p.accion === "enviar" || p.accion === "marcar") && (
        <div className={cx("mt-3 rounded-xl px-3 py-2.5", p.mueve_dinero ? "bg-aviso-suave" : "bg-panel-2/50")}>
          <Casilla marcada={!!p.mueve_dinero} onCambio={(v) => cambiar(fijarModificador(i, "mueve_dinero", v))}>
            {p.accion === "marcar" ? "Este código mueve dinero" : "Este envío mueve dinero"}{" "}
            <span className="text-tenue text-xs">
              {p.accion === "marcar"
                ? "(hace la operación de una vez, sin confirmación: nunca se reintenta y al simular no se marca)"
                : "(p. ej. el PIN que confirma: nunca se reintenta y al simular no se envía)"}
            </span>
          </Casilla>
          <MensajesCampo errores={problemasDe(errores, n, ["mueve_dinero"])} avisos={problemasDe(avisos, n, ["mueve_dinero"])} />
        </div>
      )}

      <MensajesCampo errores={sueltosE} avisos={sueltosA} />
      {probar && <ProbarPaso i={i} paso={p} />}
    </div>
  );
}

function Modificador({ campo, p, i, id, params }: { campo: Modificador; p: PasoModelo; i: number; id: string; params: string[] }) {
  const { cambiar, errores, avisos } = useEditor();
  const n = i + 1;
  const a = AYUDA_MODIFICADOR[campo];
  const err = problemasDe(errores, n, [campo]);
  return (
    <Rotulo
      etiqueta={a.titulo}
      htmlFor={id}
      ayuda={a.ayuda}
      errores={err}
      avisos={problemasDe(avisos, n, [campo])}
      extra={
        <button type="button" aria-label={`Quitar ${a.titulo}`} title={`Quitar ${a.titulo}`} onClick={() => cambiar(fijarModificador(i, campo, undefined))} className="text-tenue hover:text-texto">
          <X className="size-3.5" />
        </button>
      }
    >
      {campo === "timeout_ms" ? (
        <CampoNumero id={id} valor={p.timeout_ms} placeholder={a.placeholder} invalido={err.length > 0} onCambio={(v) => v != null && cambiar(fijarModificador(i, campo, v))} />
      ) : (
        <CampoPlantilla
          id={id}
          valor={p[campo] ?? ""}
          parametros={params}
          placeholder={a.placeholder}
          invalido={err.length > 0}
          onCambio={(v) => cambiar(fijarModificador(i, campo, v))}
        />
      )}
    </Rotulo>
  );
}

export function SeccionPasos({ destacado }: { destacado: number | null }) {
  const { modelo: m, errores } = useEditor();
  const generales = problemasDe(errores, null, ["pasos"]);
  return (
    <SeccionEditor
      id="seccion-pasos"
      titulo="Pasos"
      ayuda="En orden, lo que se marca y se contesta. Cada paso puede exigir una respuesta (esperar), cortar si ve un error (error si) o dar la receta por terminada (éxito)."
    >
      {generales.map((p, k) => (
        <p key={k} className="text-xs text-error mb-2">
          {p.mensaje}
        </p>
      ))}
      <ol>
        {m.pasos.map((p, i) => (
          <li key={i}>
            {i > 0 && <InsertarPaso en={i} />}
            <TarjetaPaso p={p} i={i} total={m.pasos.length} destacado={destacado === i + 1} />
          </li>
        ))}
      </ol>
      <div className="mt-3">
        <InsertarPaso en={m.pasos.length} siempre />
      </div>
    </SeccionEditor>
  );
}
