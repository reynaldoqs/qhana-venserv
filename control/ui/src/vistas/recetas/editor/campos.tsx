import { createContext, useContext, useEffect, useId, useRef, useState, type ReactNode, type TextareaHTMLAttributes } from "react";
import type { EjecucionReceta, ProblemaReceta } from "../../../api/tipos";
import { cx } from "../../../componentes/base";
import type { Cambio, RecetaModelo } from "../modelo";

export interface CtxEditor {
  modelo: RecetaModelo;
  yaml: string;
  /** Aplica un cambio al YAML. Con `deshacer`, se ofrece deshacerlo ("Paso 3 borrado"). */
  cambiar: (c: Cambio, deshacer?: string) => void;
  errores: ProblemaReceta[];
  avisos: ProblemaReceta[];
  nuevo: boolean;
  /** Últimas ejecuciones (para probar un paso contra pantallas reales). */
  ultimas: EjecucionReceta[];
}

export const EditorCtx = createContext<CtxEditor | null>(null);
export function useEditor(): CtxEditor {
  const c = useContext(EditorCtx);
  if (!c) throw new Error("useEditor fuera del editor");
  return c;
}

/** Problemas de un paso (1-based) o de la receta entera (`paso: null`), opcionalmente de un campo. */
export function problemasDe(lista: ProblemaReceta[], paso: number | null, campos?: string[]): ProblemaReceta[] {
  return lista.filter((p) => (p.paso ?? null) === paso && (!campos || (p.campo != null && campos.includes(p.campo))));
}

export function MensajesCampo({ errores = [], avisos = [], id }: { errores?: ProblemaReceta[]; avisos?: ProblemaReceta[]; id?: string }) {
  if (!errores.length && !avisos.length) return null;
  return (
    <ul id={id} className="mt-1.5 space-y-0.5 text-xs">
      {errores.map((p, i) => (
        <li key={`e${i}`} className="text-error">
          {p.mensaje}
        </li>
      ))}
      {avisos.map((p, i) => (
        <li key={`a${i}`} className="text-aviso">
          {p.mensaje}
        </li>
      ))}
    </ul>
  );
}

export const claseCampo =
  "w-full rounded-xl border bg-panel px-3.5 text-sm outline-none transition-[border-color,box-shadow] duration-200 placeholder:text-tenue/70 hover:border-tenue/40 focus:border-acento focus:ring-4 focus:ring-acento/15 disabled:opacity-60";

export function SeccionEditor({ id, titulo, ayuda, acciones, children }: { id: string; titulo: string; ayuda?: ReactNode; acciones?: ReactNode; children: ReactNode }) {
  return (
    <section id={id} aria-labelledby={`${id}-t`} className="scroll-mt-4">
      <div className="flex flex-wrap items-end justify-between gap-2 mb-3">
        <div className="min-w-0">
          <h2 id={`${id}-t`} className="text-lg font-bold tracking-tight">
            {titulo}
          </h2>
          {ayuda && <p className="text-xs text-tenue mt-0.5 max-w-2xl">{ayuda}</p>}
        </div>
        {acciones}
      </div>
      {children}
    </section>
  );
}

/** Etiqueta + control + mensajes. `error` pinta el borde. */
export function Rotulo({
  etiqueta,
  ayuda,
  children,
  errores,
  avisos,
  htmlFor,
  extra,
}: {
  etiqueta: string;
  ayuda?: ReactNode;
  children: ReactNode;
  errores?: ProblemaReceta[];
  avisos?: ProblemaReceta[];
  htmlFor: string;
  extra?: ReactNode;
}) {
  return (
    <div className="min-w-0">
      <div className="flex items-center justify-between gap-2 mb-1.5">
        <label htmlFor={htmlFor} className="block text-xs font-medium text-tenue">
          {etiqueta}
        </label>
        {extra}
      </div>
      {children}
      {ayuda && <p className="mt-1 text-[11px] text-tenue/80">{ayuda}</p>}
      <MensajesCampo errores={errores} avisos={avisos} id={`${htmlFor}-m`} />
    </div>
  );
}

/**
 * Campo de texto para valores con `{parametros}`: al enfocarlo muestra los parámetros declarados como fichas, y un
 * clic inserta `{nombre}` donde está el cursor.
 */
export function CampoPlantilla({
  id,
  valor,
  onCambio,
  parametros,
  placeholder,
  invalido,
  mono = true,
  autoFocus,
  ariaLabel,
}: {
  id: string;
  valor: string;
  onCambio: (v: string) => void;
  parametros: string[];
  placeholder?: string;
  invalido?: boolean;
  mono?: boolean;
  autoFocus?: boolean;
  ariaLabel?: string;
}) {
  const ref = useRef<HTMLInputElement>(null);
  const [foco, setFoco] = useState(false);
  const insertar = (p: string) => {
    const el = ref.current;
    const ins = `{${p}}`;
    const a = el?.selectionStart ?? valor.length;
    const b = el?.selectionEnd ?? valor.length;
    onCambio(valor.slice(0, a) + ins + valor.slice(b));
    requestAnimationFrame(() => {
      el?.focus();
      el?.setSelectionRange(a + ins.length, a + ins.length);
    });
  };
  return (
    <div>
      <input
        ref={ref}
        id={id}
        value={valor}
        autoFocus={autoFocus}
        aria-label={ariaLabel}
        aria-invalid={invalido || undefined}
        aria-describedby={`${id}-m`}
        spellCheck={false}
        autoComplete="off"
        onChange={(e) => onCambio(e.target.value)}
        onFocus={() => setFoco(true)}
        onBlur={() => setFoco(false)}
        placeholder={placeholder}
        className={cx(claseCampo, "h-10", mono && "font-mono text-[13px]", invalido ? "border-error" : "border-borde")}
      />
      {foco && parametros.length > 0 && (
        <div className="mt-1.5 flex flex-wrap items-center gap-1 aparecer" aria-label="Insertar parámetro">
          <span className="text-[11px] text-tenue mr-0.5">Insertar:</span>
          {parametros.map((p) => (
            <button
              key={p}
              type="button"
              tabIndex={-1}
              // Sin perder el foco ni la posición del cursor.
              onMouseDown={(e) => {
                e.preventDefault();
                insertar(p);
              }}
              className="h-6 px-1.5 rounded-md bg-info-suave text-info font-mono text-[11.5px] hover:brightness-125"
            >
              {`{${p}}`}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

export function AreaTexto({ className, invalido, ...p }: TextareaHTMLAttributes<HTMLTextAreaElement> & { invalido?: boolean }) {
  return <textarea {...p} className={cx(claseCampo, "py-2.5 resize-y leading-relaxed", invalido ? "border-error" : "border-borde", className)} />;
}

/** Número que se puede escribir a medias ("0." mientras se escribe 0.20) sin que el YAML lo corrija bajo el cursor. */
export function CampoNumero({
  id,
  valor,
  onCambio,
  placeholder,
  invalido,
  decimales,
}: {
  id: string;
  valor: number | undefined;
  onCambio: (v: number | undefined) => void;
  placeholder?: string;
  invalido?: boolean;
  decimales?: boolean;
}) {
  const [txt, setTxt] = useState(valor == null ? "" : String(valor));
  useEffect(() => {
    // Solo si cambió desde afuera (deshacer, pestaña YAML).
    setTxt((t) => (Number(t) === valor || (t === "" && valor == null) ? t : valor == null ? "" : String(valor)));
  }, [valor]);
  return (
    <input
      id={id}
      inputMode={decimales ? "decimal" : "numeric"}
      value={txt}
      placeholder={placeholder}
      aria-invalid={invalido || undefined}
      onChange={(e) => {
        const t = e.target.value.replace(",", ".");
        setTxt(t);
        if (t.trim() === "") onCambio(undefined);
        else if (!Number.isNaN(Number(t)) && !t.endsWith(".")) onCambio(Number(t));
      }}
      className={cx(claseCampo, "h-10 tabular-nums", invalido ? "border-error" : "border-borde")}
    />
  );
}

export const useIdCampo = (sufijo: string) => `${useId().replace(/:/g, "")}-${sufijo}`;
