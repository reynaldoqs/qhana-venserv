import { createContext, forwardRef, useCallback, useContext, useEffect, useId, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type {
  AnchorHTMLAttributes,
  ButtonHTMLAttributes,
  InputHTMLAttributes,
  KeyboardEvent as EventoTeclado,
  ReactNode,
  SelectHTMLAttributes,
} from "react";
import { CheckCircle2, CircleAlert, Loader2, TriangleAlert } from "./iconos";
import { mensajeError } from "../api/cliente";

export const cx = (...c: (string | false | null | undefined)[]) => c.filter(Boolean).join(" ");

type Variante = "primario" | "secundario" | "fantasma" | "peligro";

const ESTILOS_BOTON: Record<Variante, string> = {
  primario: "degradado-cta text-sobre-acento shadow-acento hover:brightness-110 hover:-translate-y-px",
  secundario: "bg-panel border border-borde text-texto shadow-tarjeta hover:bg-panel-2 hover:border-tenue/40",
  fantasma: "text-tenue hover:text-texto hover:bg-panel-2",
  peligro: "bg-error-suave text-error hover:brightness-95",
};

const claseBoton = (variante: Variante, className?: string) =>
  cx(
    "inline-flex items-center justify-center gap-2 h-10 px-5 rounded-full text-sm font-semibold whitespace-nowrap",
    "transition-[filter,background-color,color,transform,border-color,box-shadow] duration-200 ease-salida active:scale-[0.97] active:translate-y-0",
    "disabled:opacity-50 disabled:pointer-events-none focus-visible:outline-2 focus-visible:outline-acento focus-visible:outline-offset-2",
    ESTILOS_BOTON[variante],
    className,
  );

export const Boton = forwardRef<
  HTMLButtonElement,
  ButtonHTMLAttributes<HTMLButtonElement> & { variante?: Variante; cargando?: boolean; icono?: ReactNode }
>(function Boton({ variante = "secundario", cargando, icono, children, className, disabled, type = "button", ...p }, ref) {
  return (
    <button ref={ref} type={type} {...p} disabled={disabled || cargando} aria-busy={cargando || undefined} className={claseBoton(variante, className)}>
      {cargando ? <Loader2 className="size-4 animate-spin" /> : icono}
      {children}
    </button>
  );
});

/** Un enlace con aspecto de botón (descargas, por ejemplo). Nunca anidar `<a><Boton/></a>`. */
export function BotonEnlace({
  variante = "secundario",
  icono,
  children,
  className,
  ...p
}: AnchorHTMLAttributes<HTMLAnchorElement> & { variante?: Variante; icono?: ReactNode }) {
  return (
    <a {...p} className={claseBoton(variante, className)}>
      {icono}
      {children}
    </a>
  );
}

/** Botón que descarga un archivo de la API (con token por cabecera). Muestra el error en un aviso efímero. */
export function BotonDescarga({
  descargar,
  variante = "secundario",
  icono,
  children,
  className,
}: {
  descargar: () => Promise<void>;
  variante?: Variante;
  icono?: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  const [bajando, setBajando] = useState(false);
  const avisar = useToast();
  return (
    <Boton
      variante={variante}
      icono={icono}
      className={className}
      cargando={bajando}
      onClick={async () => {
        setBajando(true);
        try {
          await descargar();
        } catch (e) {
          avisar(`No pude descargar: ${mensajeError(e)}`, "error");
        } finally {
          setBajando(false);
        }
      }}
    >
      {children}
    </Boton>
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

export function Titulo({
  titulo,
  sub,
  acciones,
  migas,
}: {
  titulo: ReactNode;
  sub?: ReactNode;
  acciones?: ReactNode;
  /** Ruta encima del título ("Lotes › Pedidos 29-oct"). */
  migas?: ReactNode;
}) {
  return (
    <div className="mb-7 entrar-vista">
      {migas}
      <div className="flex flex-wrap items-end justify-between gap-3">
        <div className="min-w-0">
          <h1 className="text-3xl font-extrabold tracking-tight break-words">{titulo}</h1>
          {sub && <div className="text-sm text-tenue mt-1">{sub}</div>}
        </div>
        {acciones && <div className="flex flex-wrap items-center gap-2">{acciones}</div>}
      </div>
    </div>
  );
}

/** Migas de pan: cada tramo menos el último es un enlace. */
export function Migas({ tramos }: { tramos: { texto: string; ir?: () => void }[] }) {
  return (
    <nav aria-label="Ruta" className="mb-2">
      <ol className="flex flex-wrap items-center gap-1.5 text-sm text-tenue">
        {tramos.map((t, i) => {
          const ultimo = i === tramos.length - 1;
          return (
            <li key={i} className="inline-flex items-center gap-1.5 min-w-0">
              {t.ir && !ultimo ? (
                <button type="button" onClick={t.ir} className="rounded-md hover:text-texto transition-colors">
                  {t.texto}
                </button>
              ) : (
                <span aria-current={ultimo ? "page" : undefined} className="truncate max-w-[40ch]">
                  {t.texto}
                </span>
              )}
              {!ultimo && <span aria-hidden>›</span>}
            </li>
          );
        })}
      </ol>
    </nav>
  );
}

/** Una cifra destacada: etiqueta, valor grande (con "/ total" opcional) y una línea de apoyo. */
export function Cifra({
  etiqueta,
  valor,
  total,
  sub,
  icono,
  tono,
  className,
}: {
  etiqueta: string;
  valor: ReactNode;
  total?: ReactNode;
  sub?: ReactNode;
  icono?: ReactNode;
  /** Clase de color del valor (`text-error`, `text-aviso`…). */
  tono?: string;
  className?: string;
}) {
  return (
    <div className={cx("rounded-3xl bg-panel border border-borde/60 px-5 py-4", className)}>
      <div className="flex items-start justify-between gap-2">
        <p className="text-xs font-semibold text-tenue">{etiqueta}</p>
        {icono && <span className="size-9 -mt-1.5 -mr-2 rounded-xl bg-black/20 text-texto grid place-items-center shrink-0">{icono}</span>}
      </div>
      <p className={cx("mt-2 text-[1.75rem] leading-tight font-extrabold tabular-nums tracking-tight", tono)}>
        {valor}
        {total !== undefined && <span className="text-base font-medium text-tenue"> / {total}</span>}
      </p>
      {sub && <p className="text-xs text-tenue mt-0.5">{sub}</p>}
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

/**
 * Franja de aviso. Con `titulo` lleva icono y el texto va debajo; `acciones` van al pie.
 * Los de tono error se anuncian enseguida (`role="alert"`); los demás, con cortesía (`role="status"`).
 */
export function Aviso({
  tono = "error",
  titulo,
  acciones,
  children,
  className,
}: {
  tono?: Tono;
  titulo?: ReactNode;
  acciones?: ReactNode;
  children?: ReactNode;
  className?: string;
}) {
  const rol = tono === "error" ? "alert" : "status";
  if (!titulo) {
    return (
      <div role={rol} className={cx("rounded-xl px-4 py-3 text-sm aparecer", tonos[tono], className)}>
        {children}
        {acciones && <div className="mt-3 flex flex-wrap items-center gap-2">{acciones}</div>}
      </div>
    );
  }
  const Icono = tono === "ok" ? CheckCircle2 : tono === "info" || tono === "neutro" ? CircleAlert : TriangleAlert;
  return (
    <div role={rol} className={cx("flex gap-3 rounded-xl px-4 py-3.5 aparecer", tonos[tono], className)}>
      <Icono className="size-5 shrink-0 mt-0.5" />
      <div className="text-sm flex-1 min-w-0">
        <p className="font-semibold">{titulo}</p>
        {children && <div className="mt-0.5 opacity-90">{children}</div>}
        {acciones && <div className="mt-3 flex flex-wrap items-center gap-2">{acciones}</div>}
      </div>
    </div>
  );
}

/** Bloque gris que late mientras llegan los datos; reemplaza al "Cargando…" suelto. */
export function Esqueleto({ className }: { className?: string }) {
  return <div aria-hidden className={cx("esqueleto rounded-xl", className)} />;
}

/**
 * Filtros/pestañas con el mismo aspecto en toda la app. Es un `tablist`: Tab entra al seleccionado y las flechas
 * (más Inicio/Fin) cambian de opción, como pide ARIA. `controla`: id de la región que filtra.
 */
export function Segmentado<T extends string>({
  opciones,
  valor,
  onCambio,
  etiqueta,
  controla,
  className,
  deshabilitado,
}: {
  opciones: { id: T; texto: ReactNode; n?: number | null; titulo?: string }[];
  valor: T;
  onCambio: (v: T) => void;
  etiqueta: string;
  controla?: string;
  className?: string;
  deshabilitado?: boolean;
}) {
  const caja = useRef<HTMLDivElement>(null);
  const mover = (e: EventoTeclado<HTMLDivElement>) => {
    const i = opciones.findIndex((o) => o.id === valor);
    const destino =
      e.key === "ArrowRight" || e.key === "ArrowDown"
        ? (i + 1) % opciones.length
        : e.key === "ArrowLeft" || e.key === "ArrowUp"
          ? (i - 1 + opciones.length) % opciones.length
          : e.key === "Home"
            ? 0
            : e.key === "End"
              ? opciones.length - 1
              : -1;
    if (destino < 0 || deshabilitado) return;
    e.preventDefault();
    onCambio(opciones[destino].id);
    caja.current?.querySelectorAll<HTMLButtonElement>("[role=tab]")[destino]?.focus();
  };
  return (
    <div
      ref={caja}
      role="tablist"
      aria-label={etiqueta}
      onKeyDown={mover}
      className={cx("inline-flex flex-wrap gap-1 p-1 rounded-2xl bg-panel border border-borde/60", className)}
    >
      {opciones.map((o) => {
        const activo = valor === o.id;
        return (
          <button
            key={o.id}
            type="button"
            role="tab"
            aria-selected={activo}
            aria-controls={controla}
            tabIndex={activo ? 0 : -1}
            title={o.titulo}
            disabled={deshabilitado}
            onClick={() => onCambio(o.id)}
            className={cx(
              "h-8 px-3.5 rounded-xl text-sm font-medium whitespace-nowrap transition-colors duration-200 disabled:opacity-50",
              activo ? "bg-panel-2 text-texto" : "text-tenue hover:text-texto",
            )}
          >
            {o.texto}
            {o.n != null && <span className="ml-1.5 text-xs tabular-nums opacity-70">{o.n}</span>}
          </button>
        );
      })}
    </div>
  );
}

/** Casilla de verificación con su texto al lado. */
export function Casilla({ marcada, onCambio, children }: { marcada: boolean; onCambio: (v: boolean) => void; children: ReactNode }) {
  return (
    <label className="flex items-start gap-2.5 text-sm cursor-pointer select-none">
      <input
        type="checkbox"
        checked={marcada}
        onChange={(e) => onCambio(e.target.checked)}
        className="mt-0.5 size-4 shrink-0 accent-[var(--color-acento)] cursor-pointer"
      />
      <span>{children}</span>
    </label>
  );
}

// ───────────── Diálogo modal ─────────────

const ENFOCABLES =
  'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

/** Encierra el foco (Tab/Shift+Tab) dentro de `caja`, cierra con Esc y devuelve el foco al salir. */
export function useAtraparFoco(activo: boolean, caja: React.RefObject<HTMLElement>, cerrar: () => void) {
  const cerrarRef = useRef(cerrar);
  cerrarRef.current = cerrar;
  useEffect(() => {
    if (!activo) return;
    const antes = document.activeElement as HTMLElement | null;
    const el = caja.current;
    const inicial = el?.querySelector<HTMLElement>("[data-autofocus]") ?? el?.querySelector<HTMLElement>(ENFOCABLES) ?? el;
    requestAnimationFrame(() => inicial?.focus());
    const tecla = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        cerrarRef.current();
        return;
      }
      if (e.key !== "Tab" || !el) return;
      const lista = [...el.querySelectorAll<HTMLElement>(ENFOCABLES)].filter((x) => x.offsetParent !== null);
      if (lista.length === 0) {
        e.preventDefault();
        return;
      }
      const primero = lista[0];
      const ultimo = lista[lista.length - 1];
      if (e.shiftKey && (document.activeElement === primero || !el.contains(document.activeElement))) {
        e.preventDefault();
        ultimo.focus();
      } else if (!e.shiftKey && (document.activeElement === ultimo || !el.contains(document.activeElement))) {
        e.preventDefault();
        primero.focus();
      }
    };
    document.addEventListener("keydown", tecla, true);
    return () => {
      document.removeEventListener("keydown", tecla, true);
      if (antes && document.contains(antes)) antes.focus();
    };
  }, [activo, caja]);
}

/**
 * Diálogo modal accesible: `role="dialog"`, `aria-modal`, foco atrapado, Esc cierra y el foco vuelve a quien lo abrió.
 * El primer elemento con `data-autofocus` recibe el foco (en un diálogo de peligro, el botón seguro).
 */
export function Dialogo({
  abierto,
  cerrar,
  titulo,
  children,
  acciones,
  peligro,
  ancho = "max-w-lg",
}: {
  abierto: boolean;
  cerrar: () => void;
  titulo: ReactNode;
  children?: ReactNode;
  acciones?: ReactNode;
  peligro?: boolean;
  ancho?: string;
}) {
  const caja = useRef<HTMLDivElement>(null);
  const id = useId();
  useAtraparFoco(abierto, caja, cerrar);
  if (!abierto) return null;
  // Portal al body: dentro de una vista animada (con transform), `fixed` quedaría atado a la vista y no a la pantalla.
  return createPortal(
    <div
      className="fixed inset-0 z-[70] grid place-items-center p-4 bg-black/65 backdrop-blur-sm entrar-vista"
      onMouseDown={(e) => e.target === e.currentTarget && cerrar()}
    >
      <div
        ref={caja}
        role={peligro ? "alertdialog" : "dialog"}
        aria-modal="true"
        aria-labelledby={`${id}-t`}
        aria-describedby={`${id}-c`}
        tabIndex={-1}
        className={cx("w-full rounded-3xl bg-panel border border-borde shadow-elevada outline-none max-h-[calc(100dvh-2rem)] flex flex-col", ancho)}
      >
        <div className="px-6 pt-5 pb-1 flex items-start gap-3">
          {peligro && (
            <span className="size-9 shrink-0 rounded-xl bg-error-suave text-error grid place-items-center">
              <TriangleAlert className="size-5" />
            </span>
          )}
          <h2 id={`${id}-t`} className="text-lg font-bold tracking-tight pt-1">
            {titulo}
          </h2>
        </div>
        <div id={`${id}-c`} className="px-6 py-3 text-sm overflow-y-auto">
          {children}
        </div>
        {acciones && <div className="px-6 pb-5 pt-2 flex flex-wrap justify-end gap-2">{acciones}</div>}
      </div>
    </div>,
    document.body,
  );
}

// ───────────── Menú de acciones (⋯) ─────────────

export interface AccionMenu {
  texto: string;
  icono?: ReactNode;
  peligro?: boolean;
  deshabilitado?: boolean;
  alElegir: () => void;
}

/** Botón "⋯" que abre un menú (`role="menu"`): flechas para moverse, Esc cierra y devuelve el foco. */
export function MenuAcciones({ acciones, etiqueta = "Más acciones" }: { acciones: AccionMenu[]; etiqueta?: string }) {
  const [abierto, setAbierto] = useState(false);
  const boton = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const id = useId();

  const items = () => [...(menu.current?.querySelectorAll<HTMLButtonElement>("[role=menuitem]:not([disabled])") ?? [])];
  const cerrar = (devolverFoco = true) => {
    setAbierto(false);
    if (devolverFoco) boton.current?.focus();
  };

  useEffect(() => {
    if (!abierto) return;
    requestAnimationFrame(() => items()[0]?.focus());
    const fuera = (e: MouseEvent) => {
      if (!menu.current?.contains(e.target as Node) && !boton.current?.contains(e.target as Node)) cerrar(false);
    };
    document.addEventListener("mousedown", fuera);
    return () => document.removeEventListener("mousedown", fuera);
  }, [abierto]); // eslint-disable-line react-hooks/exhaustive-deps

  const tecla = (e: EventoTeclado<HTMLDivElement>) => {
    const l = items();
    const i = l.indexOf(document.activeElement as HTMLButtonElement);
    if (e.key === "ArrowDown") l[(i + 1) % l.length]?.focus();
    else if (e.key === "ArrowUp") l[(i - 1 + l.length) % l.length]?.focus();
    else if (e.key === "Home") l[0]?.focus();
    else if (e.key === "End") l[l.length - 1]?.focus();
    else if (e.key === "Escape" || e.key === "Tab") return e.key === "Escape" ? (e.preventDefault(), cerrar()) : cerrar(false);
    else return;
    e.preventDefault();
  };

  if (acciones.length === 0) return null;
  return (
    <div className="relative">
      <button
        ref={boton}
        type="button"
        aria-label={etiqueta}
        title={etiqueta}
        aria-haspopup="menu"
        aria-expanded={abierto}
        aria-controls={abierto ? id : undefined}
        onClick={() => setAbierto((a) => !a)}
        onKeyDown={(e) => {
          if (e.key === "ArrowDown") {
            e.preventDefault();
            setAbierto(true);
          }
        }}
        className={cx(claseBoton("secundario"), "w-10 px-0 text-lg leading-none")}
      >
        <span aria-hidden>⋯</span>
      </button>
      {abierto && (
        <div
          ref={menu}
          id={id}
          role="menu"
          aria-label={etiqueta}
          onKeyDown={tecla}
          className="absolute right-0 top-full mt-2 z-50 min-w-56 rounded-2xl bg-panel-2 border border-borde shadow-elevada p-1.5 aparecer"
        >
          {acciones.map((a) => (
            <button
              key={a.texto}
              type="button"
              role="menuitem"
              tabIndex={-1}
              disabled={a.deshabilitado}
              onClick={() => {
                cerrar();
                a.alElegir();
              }}
              className={cx(
                "w-full flex items-center gap-2.5 h-10 px-3 rounded-xl text-sm font-medium text-left transition-colors disabled:opacity-40",
                a.peligro ? "text-error hover:bg-error-suave focus:bg-error-suave" : "text-texto hover:bg-panel focus:bg-panel",
                "outline-none",
              )}
            >
              {a.icono}
              {a.texto}
            </button>
          ))}
        </div>
      )}
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
