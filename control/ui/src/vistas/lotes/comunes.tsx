import type { ReactNode } from "react";
import type { ChipOrigen, Dispositivo, EstadoFilaLote, EstadoFilaValidada, EstadoLote, OrigenLote, ProgresoLote } from "../../api/tipos";
import { Insignia, cx } from "../../componentes/base";
import { sinPrefijo } from "../../util/datos";

type Tono = "neutro" | "ok" | "aviso" | "error" | "info";

/** Bs con centavos siempre a 2 decimales (0,50 Bs; 5 Bs; 12,30 Bs). */
export const bs = (n: number | null | undefined) => {
  if (n == null) return "—";
  const entero = Math.abs(n - Math.round(n)) < 1e-9;
  return `${n.toLocaleString("es-BO", { minimumFractionDigits: entero ? 0 : 2, maximumFractionDigits: 2 })} Bs`;
};

const FILA_LOTE: Record<EstadoFilaLote, [Tono, string]> = {
  omitida: ["neutro", "Omitida"],
  pendiente: ["neutro", "Pendiente"],
  en_curso: ["info", "En curso"],
  ok: ["ok", "OK"],
  simulado: ["info", "Simulada"],
  parcial: ["aviso", "Parcial"],
  fallida: ["error", "Fallida"],
  incierta: ["error", "Incierta"],
  sin_capacidad: ["aviso", "Para mañana"],
};

/** Sección del manual que explica cada estado de fila con problema. */
const MANUAL_FILA: Partial<Record<EstadoFilaLote, string>> = {
  incierta: "filas",
  fallida: "filas",
  parcial: "filas",
  sin_capacidad: "chips",
  omitida: "validacion",
};

export function EstadoFila({ estado }: { estado: EstadoFilaLote }) {
  const [t, txt] = FILA_LOTE[estado] ?? ["neutro", estado];
  const seccion = MANUAL_FILA[estado];
  const insignia = <Insignia tono={t}>{txt}</Insignia>;
  return seccion ? (
    <EnlaceManual seccion={seccion} titulo={`Qué significa «${txt}»`} className="rounded-full">
      {insignia}
    </EnlaceManual>
  ) : (
    insignia
  );
}

/** Enlace a una sección del manual (`#manual/filas`). Sin texto, muestra «¿Qué es esto?». */
export function EnlaceManual({
  seccion,
  children,
  titulo,
  className,
}: {
  seccion: string;
  children?: ReactNode;
  titulo?: string;
  className?: string;
}) {
  return (
    <a
      href={`#manual/${seccion}`}
      title={titulo ?? "Ver en el manual"}
      className={cx(children ? "inline-flex" : "underline underline-offset-2 decoration-current/40 hover:decoration-current", className)}
    >
      {children ?? "Ver en el manual"}
    </a>
  );
}

const FILA_VALIDADA: Record<EstadoFilaValidada, [Tono, string]> = {
  valida: ["ok", "Válida"],
  aviso: ["aviso", "Aviso"],
  invalida: ["error", "Error"],
};

export function EstadoValidacion({ estado }: { estado: EstadoFilaValidada }) {
  const [t, txt] = FILA_VALIDADA[estado];
  return <Insignia tono={t}>{txt}</Insignia>;
}

const LOTE: Record<EstadoLote, [Tono, string]> = {
  listo: ["neutro", "Listo"],
  en_curso: ["info", "En curso"],
  pausado: ["aviso", "Pausado"],
  terminado: ["ok", "Terminado"],
  cancelado: ["neutro", "Cancelado"],
};

export function EstadoLoteInsignia({ estado }: { estado: EstadoLote }) {
  const [t, txt] = LOTE[estado] ?? ["neutro", estado];
  return <Insignia tono={t}>{txt}</Insignia>;
}

/** Barra apilada con lo hecho, lo que falló y lo que queda. */
export function BarraProgreso({ p, alta }: { p: ProgresoLote; alta?: boolean }) {
  const total = Math.max(1, p.filas);
  const procesadas = p.filas - p.pendientes;
  const tramos: [number, string, string][] = [
    [p.ok, "bg-ok", "OK"],
    [p.simulado, "bg-info", "Simuladas"],
    [p.parcial, "bg-aviso", "Parciales"],
    [p.sin_capacidad, "bg-aviso/50", "Para mañana"],
    [p.fallidas + p.inciertas, "bg-error", "Con problema"],
  ];
  return (
    <div
      className={cx("w-full rounded-full bg-panel-2 overflow-hidden flex", alta ? "h-2.5" : "h-1.5")}
      role="progressbar"
      aria-label="Filas procesadas"
      aria-valuemin={0}
      aria-valuenow={procesadas}
      aria-valuemax={p.filas}
      aria-valuetext={`${procesadas} de ${p.filas} filas procesadas: ${p.ok + p.simulado} OK, ${p.fallidas + p.inciertas} con problema, ${p.pendientes} pendientes`}
    >
      {tramos.map(([n, color, titulo]) =>
        n > 0 ? (
          <div
            key={titulo}
            title={`${titulo}: ${n}`}
            className={cx(color, "h-full transition-[width] duration-500")}
            style={{ width: `${(n / total) * 100}%` }}
          />
        ) : null,
      )}
    </div>
  );
}

/** "SIM 2 · 69759675" a partir de dispositivo + slot. */
export function nombreChip(dispositivos: Dispositivo[], dispositivo: string | null, slot: number | null): string {
  if (dispositivo == null || slot == null) return "—";
  const d = dispositivos.find((x) => x.id === dispositivo);
  const s = d?.sims.find((x) => x.slot === slot);
  return sinPrefijo(s?.numero) ?? `SIM ${slot + 1}`;
}

/** Cuenta por estado con su color, para leer la barra sin pasar el mouse. */
export function LeyendaProgreso({ p }: { p: ProgresoLote }) {
  const items: [number, string, string][] = [
    [p.ok, "bg-ok", "OK"],
    [p.simulado, "bg-info", "simuladas"],
    [p.parcial, "bg-aviso", "parciales"],
    [p.fallidas + p.inciertas, "bg-error", "con problema"],
    [p.sin_capacidad, "bg-aviso/50", "para mañana"],
    [p.pendientes, "bg-borde", "pendientes"],
  ];
  return (
    <ul className="flex flex-wrap gap-x-3 gap-y-1 text-[11px] text-tenue">
      {items
        .filter(([n]) => n > 0)
        .map(([n, color, txt]) => (
          <li key={txt} className="inline-flex items-center gap-1.5">
            <span className={cx("size-1.5 rounded-full", color)} />
            <span className="tabular-nums">
              {n} {txt}
            </span>
          </li>
        ))}
    </ul>
  );
}

/** Filas ya procesadas (cualquier final, bueno o malo): todas menos las pendientes. */
export const procesadas = (p: ProgresoLote) => p.filas - p.pendientes;
/** Filas que salieron bien (OK o simuladas). */
export const filasBien = (p: ProgresoLote) => p.ok + p.simulado;

/** Grupos de estados de fila para los filtros (las omitidas van aparte: nunca se ejecutan). */
export type FiltroFilas = "todas" | "pendientes" | "hechas" | "problemas" | "mañana" | "omitidas";
export const GRUPOS_FILA: Record<Exclude<FiltroFilas, "todas">, EstadoFilaLote[]> = {
  pendientes: ["pendiente", "en_curso"],
  hechas: ["ok", "simulado"],
  problemas: ["fallida", "incierta", "parcial"],
  mañana: ["sin_capacidad"],
  omitidas: ["omitida"],
};

/** Cuántas filas hay en cada grupo, según el progreso del lote (sin pedir las filas). */
export function conteoGrupos(p: ProgresoLote): Record<FiltroFilas, number> {
  return {
    todas: p.filas + p.omitidas,
    pendientes: p.pendientes,
    hechas: p.ok + p.simulado,
    problemas: p.fallidas + p.inciertas + p.parcial,
    mañana: p.sin_capacidad,
    omitidas: p.omitidas,
  };
}

// ───────────── Origen del lote (forma nueva `chips` + forma vieja de un chip) ─────────────

/** Los chips de origen, venga como venga: `{ chips: […] }` o el viejo `{ dispositivo, slot, receta }`. */
export function chipsDeOrigen(o: OrigenLote | null | undefined): ChipOrigen[] {
  if (!o) return [];
  if (Array.isArray(o.chips) && o.chips.length > 0) return o.chips;
  if (o.dispositivo != null && o.slot != null) {
    return [{ dispositivo: o.dispositivo, slot: o.slot, operador: o.operador, numero: o.numero, receta: o.receta ?? null }];
  }
  return [];
}

/** La acción del origen. El viejo no la trae: se deduce del nombre de la receta (`tigo-dealer-recargar` → recargar). */
export function accionDeOrigen(o: OrigenLote | null | undefined, accionDeReceta?: (nombre: string) => string | null | undefined): string | null {
  if (!o) return null;
  if (o.accion) return o.accion;
  const receta = o.receta ?? chipsDeOrigen(o)[0]?.receta;
  if (!receta) return null;
  return accionDeReceta?.(receta) ?? (receta.includes("transferir") ? "transferir" : receta.includes("recargar") ? "recargar" : null);
}

/** "desde 3 chips · recargar", "desde 78815232 · tigo-dealer-recargar". */
export function describirOrigen(o: OrigenLote | null | undefined, dispositivos: Dispositivo[]): string | null {
  const chips = chipsDeOrigen(o);
  if (chips.length === 0) return null;
  const accion = o?.accion ?? null;
  if (chips.length > 1) return `desde ${chips.length} chips${accion ? ` · ${accion}` : ""}`;
  const c = chips[0];
  const quien = sinPrefijo(c.numero) ?? nombreChip(dispositivos, c.dispositivo, c.slot);
  const que = accion ?? c.receta ?? o?.receta;
  return `desde ${quien}${que ? ` · ${que}` : ""}`;
}

/** Duración corta: "8 s", "3 min", "1 h 05 min". */
export function duracionCorta(ms: number): string {
  const s = Math.max(0, Math.round(ms / 1000));
  if (s < 60) return `${s} s`;
  const m = Math.round(s / 60);
  if (m < 60) return `${m} min`;
  return `${Math.floor(m / 60)} h ${String(m % 60).padStart(2, "0")} min`;
}

/** Cronómetro: "0:07", "2:15". */
export function reloj(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}
