import type { ProblemaReceta } from "../../../api/tipos";
import { Tarjeta, cx } from "../../../componentes/base";
import { CheckCircle2, CircleAlert, Loader2, TriangleAlert } from "../../../componentes/iconos";

const donde = (p: ProblemaReceta) => (p.paso ? `Paso ${p.paso}` : p.campo ? p.campo : "Receta");

/** Va al paso o sección del problema y lo resalta un momento. */
function irA(p: ProblemaReceta) {
  const sec: Record<string, string> = { parametros: "seccion-parametros", salida: "seccion-salida" };
  const raiz = p.campo?.split(/[.[]/)[0] ?? "";
  const id = p.paso ? `paso-${p.paso}` : (sec[raiz] ?? "seccion-datos");
  const el = document.getElementById(id);
  el?.scrollIntoView({ behavior: "smooth", block: "start" });
  el?.querySelector<HTMLElement>("input, select, textarea")?.focus({ preventScroll: true });
}

/**
 * Resumen de la validación en vivo. Los errores impiden guardar; los avisos no.
 * `local`: error de sintaxis del YAML (no hace falta preguntar al control).
 */
export function Problemas({
  errores,
  avisos,
  validando,
  local,
  sinServidor,
  compacto,
}: {
  errores: ProblemaReceta[];
  avisos: ProblemaReceta[];
  validando: boolean;
  local: string | null;
  sinServidor: string | null;
  compacto?: boolean;
}) {
  const total = errores.length + (local ? 1 : 0);
  return (
    <Tarjeta className={cx("p-4", compacto && "py-3")}>
      <div className="flex items-center gap-2 text-sm font-semibold" role="status" aria-live="polite">
        {validando ? (
          <>
            <Loader2 className="size-4 animate-spin text-tenue" /> Revisando…
          </>
        ) : total > 0 ? (
          <>
            <CircleAlert className="size-4 text-error" /> {total} {total === 1 ? "error impide guardar" : "errores impiden guardar"}
          </>
        ) : sinServidor ? (
          <>
            <TriangleAlert className="size-4 text-aviso" /> Sin revisar
          </>
        ) : (
          <>
            <CheckCircle2 className="size-4 text-ok" /> Sin errores
          </>
        )}
        {avisos.length > 0 && <span className="ml-auto text-xs font-medium text-aviso">{avisos.length} {avisos.length === 1 ? "aviso" : "avisos"}</span>}
      </div>
      {sinServidor && <p className="mt-2 text-xs text-tenue">{sinServidor}</p>}
      {local && <p className="mt-2 text-xs text-error break-words">YAML: {local}</p>}
      {(errores.length > 0 || avisos.length > 0) && (
        <ul className={cx("mt-2.5 space-y-1 overflow-y-auto", compacto ? "max-h-40" : "max-h-[50vh]")}>
          {[...errores.map((p) => ["e", p] as const), ...avisos.map((p) => ["a", p] as const)].map(([t, p], k) => (
            <li key={k}>
              <button
                type="button"
                onClick={() => irA(p)}
                className="w-full text-left rounded-lg px-2 py-1.5 -mx-2 text-xs hover:bg-panel-2 transition-colors flex gap-2"
              >
                <span className={cx("font-semibold shrink-0", t === "e" ? "text-error" : "text-aviso")}>{donde(p)}</span>
                <span className="text-tenue break-words min-w-0">{p.mensaje}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
      {!compacto && (
        <p className="mt-3 text-[11px] text-tenue/80 border-t border-borde/60 pt-2.5">Los errores impiden guardar. Los avisos no: son cosas que conviene revisar.</p>
      )}
    </Tarjeta>
  );
}
