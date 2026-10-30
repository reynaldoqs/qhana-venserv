import { createContext, forwardRef, useCallback, useContext, useRef, useState } from "react";
import type { ButtonHTMLAttributes, InputHTMLAttributes, ReactNode, SelectHTMLAttributes } from "react";
import { CheckCircle2, CircleAlert, Loader2 } from "./iconos";

export const cx = (...c: (string | false | null | undefined)[]) => c.filter(Boolean).join(" ");

type Variante = "primario" | "secundario" | "fantasma" | "peligro";

export function Boton({
  variante = "secundario",
  cargando,
  icono,
  children,
  className,
  disabled,
  ...p
}: ButtonHTMLAttributes<HTMLButtonElement> & {
  variante?: Variante;
  cargando?: boolean;
  icono?: ReactNode;
}) {
  const estilos: Record<Variante, string> = {
    primario: "degradado-cta text-sobre-acento shadow-acento hover:brightness-110 hover:-translate-y-px",
    secundario: "bg-panel border border-borde text-texto shadow-tarjeta hover:bg-panel-2 hover:border-tenue/40",
    fantasma: "text-tenue hover:text-texto hover:bg-panel-2",
    peligro: "bg-error-suave text-error hover:brightness-95",
  };
  return (
    <button
      {...p}
      disabled={disabled || cargando}
      className={cx(
        "inline-flex items-center justify-center gap-2 h-10 px-5 rounded-full text-sm font-semibold",
        "transition-[filter,background-color,color,transform,border-color,box-shadow] duration-200 ease-salida active:scale-[0.97] active:translate-y-0",
        "disabled:opacity-50 disabled:pointer-events-none focus-visible:outline-2 focus-visible:outline-acento focus-visible:outline-offset-2",
        estilos[variante],
        className,
      )}
    >
      {cargando ? <Loader2 className="size-4 animate-spin" /> : icono}
      {children}
    </button>
  );
}

type Tono = "neutro" | "ok" | "aviso" | "error" | "info";

const tonos: Record<Tono, string> = {
  neutro: "bg-texto/8 text-tenue",
  ok: "bg-ok-suave text-ok",
  aviso: "bg-aviso-suave text-aviso",
  error: "bg-error-suave text-error",
  info: "bg-info-suave text-info",
};

/** Etiqueta de estado: fondo translúcido del mismo tono + punto, en vez de un bloque de color sólido. */
export function Insignia({ tono = "neutro", children, className }: { tono?: Tono; children: ReactNode; className?: string }) {
  return (
    <span
      className={cx(
        "inline-flex items-center gap-1.5 h-6 px-2.5 rounded-full text-xs font-semibold whitespace-nowrap",
        "before:size-1.5 before:rounded-full before:bg-current before:content-['']",
        tonos[tono],
        className,
      )}
    >
      {children}
    </span>
  );
}

export function Punto({ tono = "neutro", pulso }: { tono?: Tono; pulso?: boolean }) {
  const color: Record<Tono, string> = {
    neutro: "bg-tenue",
    ok: "bg-ok",
    aviso: "bg-aviso",
    error: "bg-error",
    info: "bg-info",
  };
  return (
    <span className="relative inline-flex size-2">
      {pulso && <span className={cx("absolute inset-0 rounded-full opacity-60 animate-ping", color[tono])} />}
      <span className={cx("relative inline-flex size-2 rounded-full", color[tono])} />
    </span>
  );
}

export function Tarjeta({ children, className }: { children: ReactNode; className?: string }) {
  return <div className={cx("bg-panel border border-borde/60 rounded-3xl", className)}>{children}</div>;
}

export function Titulo({ titulo, sub, acciones }: { titulo: string; sub?: ReactNode; acciones?: ReactNode }) {
  return (
    <div className="flex flex-wrap items-end justify-between gap-3 mb-7 entrar-vista">
      <div>
        <h1 className="text-3xl font-extrabold tracking-tight">{titulo}</h1>
        {sub && <p className="text-sm text-tenue mt-1">{sub}</p>}
      </div>
      {acciones && <div className="flex items-center gap-2">{acciones}</div>}
    </div>
  );
}

export function Vacio({ icono, titulo, texto }: { icono: ReactNode; titulo: string; texto?: ReactNode }) {
  return (
    <div className="flex flex-col items-center justify-center text-center py-16 px-6">
      <div className="size-12 rounded-2xl bg-panel-2 text-tenue grid place-items-center mb-4 aparecer">{icono}</div>
      <p className="font-medium">{titulo}</p>
      {texto && <p className="text-sm text-tenue mt-1 max-w-sm">{texto}</p>}
    </div>
  );
}

const campo =
  "h-10 w-full rounded-xl border border-borde bg-panel px-3.5 text-sm outline-none transition-[border-color,box-shadow] duration-200 placeholder:text-tenue hover:border-tenue/40 focus:border-acento focus:ring-4 focus:ring-acento/15 disabled:opacity-50";

export const Campo = forwardRef<HTMLInputElement, InputHTMLAttributes<HTMLInputElement> & { etiqueta?: string }>(
  function Campo({ etiqueta, className, ...p }, ref) {
    const input = <input ref={ref} {...p} className={cx(campo, className)} />;
    return etiqueta ? (
      <label className="block">
        <span className="block text-xs font-medium text-tenue mb-1.5">{etiqueta}</span>
        {input}
      </label>
    ) : (
      input
    );
  },
);

export function Selector({
  etiqueta,
  className,
  children,
  ...p
}: SelectHTMLAttributes<HTMLSelectElement> & { etiqueta?: string }) {
  const sel = (
    <select {...p} className={cx(campo, "pr-8 appearance-none bg-no-repeat cursor-pointer", className)}
      style={{
        backgroundImage:
          "url(\"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='16' height='16' fill='none' stroke='%238b94a3' stroke-width='2'%3E%3Cpath d='m4 6 4 4 4-4'/%3E%3C/svg%3E\")",
        backgroundPosition: "right 0.6rem center",
      }}
    >
      {children}
    </select>
  );
  return etiqueta ? (
    <label className="block">
      <span className="block text-xs font-medium text-tenue mb-1.5">{etiqueta}</span>
      {sel}
    </label>
  ) : (
    sel
  );
}

export function Interruptor({
  activo,
  onCambio,
  etiqueta,
}: {
  activo: boolean;
  onCambio: (v: boolean) => void;
  etiqueta: ReactNode;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={activo}
      onClick={() => onCambio(!activo)}
      className="inline-flex items-center gap-2.5 text-sm select-none"
    >
      <span
        className={cx(
          "relative h-5 w-9 rounded-full transition-colors duration-200",
          activo ? "bg-ok" : "bg-borde",
        )}
      >
        <span
          className={cx(
            "absolute top-0.5 left-0.5 size-4 rounded-full bg-white shadow transition-transform duration-300 ease-resorte",
            activo && "translate-x-4",
          )}
        />
      </span>
      {etiqueta}
    </button>
  );
}

export function Aviso({ tono = "error", children }: { tono?: Tono; children: ReactNode }) {
  return <div className={cx("rounded-xl px-4 py-3 text-sm aparecer", tonos[tono])}>{children}</div>;
}

/** Bloque gris que late mientras llegan los datos; reemplaza al "Cargando…" suelto. */
export function Esqueleto({ className }: { className?: string }) {
  return <div aria-hidden className={cx("esqueleto rounded-xl", className)} />;
}

/** Control segmentado neutro para filtros y pestañas (el naranja queda para marca y acciones). */
export function Segmentado<T extends string>({
  opciones,
  valor,
  onCambio,
  etiqueta,
}: {
  opciones: { id: T; texto: string; n?: number }[];
  valor: T;
  onCambio: (v: T) => void;
  etiqueta: string;
}) {
  return (
    <div role="tablist" aria-label={etiqueta} className="inline-flex flex-wrap gap-1 p-1 rounded-2xl bg-panel border border-borde/60">
      {opciones.map((o) => (
        <button
          key={o.id}
          role="tab"
          aria-selected={valor === o.id}
          onClick={() => onCambio(o.id)}
          className={cx(
            "h-8 px-3.5 rounded-xl text-sm font-medium whitespace-nowrap transition-colors duration-200",
            valor === o.id ? "bg-panel-2 text-texto" : "text-tenue hover:text-texto",
          )}
        >
          {o.texto}
          {o.n !== undefined && <span className="ml-1.5 text-xs tabular-nums opacity-70">{o.n}</span>}
        </button>
      ))}
    </div>
  );
}

// ───────────── Avisos efímeros ─────────────

type AvisoToast = { id: number; texto: string; tono: "ok" | "error" | "neutro" };
const ToastCtx = createContext<(texto: string, tono?: AvisoToast["tono"]) => void>(() => {});
export const useToast = () => useContext(ToastCtx);

export function ProveedorToast({ children }: { children: ReactNode }) {
  const [lista, setLista] = useState<AvisoToast[]>([]);
  const n = useRef(0);
  const avisar = useCallback((texto: string, tono: AvisoToast["tono"] = "neutro") => {
    const id = ++n.current;
    setLista((l) => [...l.slice(-3), { id, texto, tono }]);
    window.setTimeout(() => setLista((l) => l.filter((x) => x.id !== id)), tono === "error" ? 6000 : 3000);
  }, []);
  return (
    <ToastCtx.Provider value={avisar}>
      {children}
      <div role="status" aria-live="polite" className="fixed z-[60] bottom-20 md:bottom-6 left-1/2 -translate-x-1/2 flex flex-col items-center gap-2 pointer-events-none">
        {lista.map((t) => (
          <div
            key={t.id}
            className="aparecer pointer-events-auto flex items-center gap-2.5 rounded-full bg-panel-2 border border-borde pl-3.5 pr-4 h-11 text-sm font-medium shadow-elevada"
          >
            {t.tono === "ok" && <CheckCircle2 className="size-4 text-ok" />}
            {t.tono === "error" && <CircleAlert className="size-4 text-error" />}
            {t.texto}
          </div>
        ))}
      </div>
    </ToastCtx.Provider>
  );
}
