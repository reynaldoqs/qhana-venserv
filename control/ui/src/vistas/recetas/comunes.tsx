import type { ReactNode } from "react";
import { CheckCircle2, CircleAlert, CircleDashed, FlaskConical, Loader2, TriangleAlert } from "../../componentes/iconos";
import type { EstadoResultado, PasoResultado, ResumenReceta } from "../../api/tipos";
import { Aviso, Insignia, cx } from "../../componentes/base";
import { MENSAJE_VIEJO } from "../../api/cliente";
import { haceLargo, segundos } from "../../util/datos";
import type { Accion, Modificador } from "./modelo";

type Tono = "ok" | "aviso" | "error" | "info" | "neutro";

export const estadoUi: Record<EstadoResultado | "en_curso", { tono: Tono; texto: string; corto: string; icono: typeof CheckCircle2 }> = {
  ok: { tono: "ok", texto: "Completada", corto: "OK", icono: CheckCircle2 },
  simulado: { tono: "info", texto: "Simulada", corto: "Simulada", icono: FlaskConical },
  abortado: { tono: "aviso", texto: "Abortada sin mover nada", corto: "Abortada", icono: CircleDashed },
  incierto: { tono: "error", texto: "Incierta: revisar saldo", corto: "Incierta", icono: TriangleAlert },
  error: { tono: "error", texto: "Error", corto: "Error", icono: CircleAlert },
  en_curso: { tono: "info", texto: "En curso", corto: "En curso", icono: Loader2 },
};

export const colorTono: Record<Tono, string> = {
  ok: "text-ok",
  aviso: "text-aviso",
  error: "text-error",
  info: "text-info",
  neutro: "text-tenue",
};

/** Qué hace cada acción, dicho corto (de la tabla de docs/protocolo.md). */
export const AYUDA_ACCION: Record<Accion, { titulo: string; valor: string; ayuda: string }> = {
  marcar: {
    titulo: "Marcar",
    valor: "Código",
    ayuda: "Inicia la sesión con este código. El primer paso siempre es marcar; se puede volver a marcar más adelante solo si la sesión anterior ya cerró.",
  },
  enviar: { titulo: "Enviar", valor: "Texto a enviar", ayuda: "Contesta este texto tal cual (con los {parámetros} reemplazados)." },
  elegir: {
    titulo: "Elegir",
    valor: "Opción del menú (regex)",
    ayuda: "Busca la opción del menú cuyo texto coincide completo con la regex (sin importar mayúsculas) y contesta su número.",
  },
  verificar: {
    titulo: "Verificar",
    valor: "La pantalla debe decir (regex)",
    ayuda: "No envía nada: exige que la pantalla actual coincida. Si no, aborta antes de seguir. Ponlo antes del paso que mueve dinero.",
  },
  confirmar: {
    titulo: "Confirmar",
    valor: "Opción que confirma (regex)",
    ayuda: "Como elegir, pero es el paso que MUEVE DINERO: nunca se reintenta y al simular elige «No».",
  },
};

export const AYUDA_MODIFICADOR: Record<Modificador, { titulo: string; ayuda: string; placeholder: string }> = {
  esperar: { titulo: "Esperar", ayuda: "La respuesta de este paso debe coincidir o se aborta.", placeholder: "regex, p. ej. monto" },
  error_si: { titulo: "Error si", ayuda: "Si la respuesta coincide, aborta con ese texto como motivo.", placeholder: "regex, p. ej. saldo insuficiente" },
  exito: { titulo: "Éxito", ayuda: "Si coincide, la receta terminó bien.", placeholder: "regex, p. ej. transferido" },
  timeout_ms: { titulo: "Tiempo máximo (ms)", ayuda: "Tiempo máximo de este paso.", placeholder: "20000" },
};

/** Porcentaje de éxito (ok + simulado) en 30 días, o null si no corrió. */
export function tasaExito(r: Pick<ResumenReceta, "uso">): number | null {
  const u = r.uso;
  if (!u || !u.ejecuciones_30d) return null;
  return Math.round((u.ok_30d / u.ejecuciones_30d) * 100);
}

/** Las insignias de una receta (lista y ficha). */
export function InsigniasReceta({ r, operador = false }: { r: ResumenReceta; operador?: boolean }) {
  return (
    <>
      {r.mueve_dinero && <Insignia tono="aviso">Mueve saldo</Insignia>}
      {r.chip_tipo === "dealer" && <Insignia tono="info">Dealer</Insignia>}
      {r.sin_probar && (
        <span title="Esta versión aún no se ejecutó bien (ni simulada). Una simulación que termine bien la da por probada.">
          <Insignia>Sin probar</Insignia>
        </span>
      )}
      {operador && <Insignia>{r.operador}</Insignia>}
    </>
  );
}

/** "OK hace 2 h · 93 % en 30 d". */
export function LineaUso({ r, className }: { r: ResumenReceta; className?: string }) {
  const u = r.uso;
  if (u === undefined) return null;
  if (!u || !u.ultima_ms) return <p className={cx("text-xs text-tenue", className)}>Nunca se ejecutó</p>;
  const e = u.ultimo_estado ? estadoUi[u.ultimo_estado] : null;
  const tasa = tasaExito(r);
  return (
    <p className={cx("text-xs text-tenue tabular-nums", className)}>
      {e && <span className={colorTono[e.tono]}>{e.corto}</span>} {haceLargo(u.ultima_ms)}
      {tasa != null && (
        <>
          {" · "}
          <span title={`${u.ok_30d} de ${u.ejecuciones_30d} ejecuciones bien en 30 días`}>{tasa} % en 30 d</span>
        </>
      )}
    </p>
  );
}

/** Pasos de una ejecución: lo enviado, la pantalla que llegó y cuánto tardó. */
export function PasosEjecucion({ pasos, resaltar, accionPaso }: { pasos: PasoResultado[]; resaltar?: number | null; accionPaso?: (i: number) => ReactNode }) {
  return (
    <ol className="relative border-l border-borde ml-1.5 space-y-4">
      {pasos.map((p, i) => (
        <li key={i} className="pl-5 relative min-w-0">
          <span
            className={cx(
              "absolute -left-[5px] top-1.5 size-2.5 rounded-full bg-panel border-2",
              resaltar === i + 1 ? "border-error" : "border-tenue",
            )}
          />
          <div className="flex flex-wrap items-center gap-2 text-xs">
            <span className="font-semibold uppercase tracking-wide text-tenue">
              {i + 1}. {p.accion}
            </span>
            {p.enviado && <span className="font-mono rounded bg-panel-2 text-texto px-1.5 py-0.5 break-all">{p.enviado}</span>}
            {p.ms != null && <span className="text-tenue tabular-nums">{segundos(p.ms)}</span>}
            {accionPaso?.(i)}
          </div>
          {p.texto && (
            <pre className="mt-1.5 rounded-xl bg-panel-2 px-3 py-2 text-[12.5px] leading-relaxed whitespace-pre-wrap break-words font-mono">
              {p.texto}
            </pre>
          )}
        </li>
      ))}
    </ol>
  );
}

export function AvisoViejo({ que }: { que?: string }) {
  return (
    <Aviso tono="aviso" titulo={que ? `${que}: falta actualizar el control` : "Falta actualizar el control"}>
      {MENSAJE_VIEJO}
    </Aviso>
  );
}

/** Versión corta (12 caracteres) para mostrar. */
export const versionCorta = (v?: string | null) => (v ? v.slice(0, 12) : "—");

/** Guarda un texto como archivo desde el navegador. */
export function guardarArchivo(nombre: string, contenido: string, tipo = "text/yaml") {
  const url = URL.createObjectURL(new Blob([contenido], { type: `${tipo};charset=utf-8` }));
  const a = document.createElement("a");
  a.href = url;
  a.download = nombre;
  a.style.display = "none";
  document.body.appendChild(a);
  a.click();
  a.remove();
  window.setTimeout(() => URL.revokeObjectURL(url), 1000);
}
