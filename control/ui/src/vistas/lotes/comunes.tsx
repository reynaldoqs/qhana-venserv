import type { Dispositivo, EstadoFilaLote, EstadoFilaValidada, EstadoLote, ProgresoLote } from "../../api/tipos";
import { Insignia, cx } from "../../componentes/base";

type Tono = "neutro" | "ok" | "aviso" | "error" | "info";

export const bs = (n: number | null | undefined) =>
  n == null ? "—" : `${n.toLocaleString("es-BO", { minimumFractionDigits: 0, maximumFractionDigits: 2 })} Bs`;

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

export function EstadoFila({ estado }: { estado: EstadoFilaLote }) {
  const [t, txt] = FILA_LOTE[estado] ?? ["neutro", estado];
  return <Insignia tono={t}>{txt}</Insignia>;
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
      aria-valuenow={total - p.pendientes}
      aria-valuemax={total}
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
  const num = s?.numero?.replace(/^\+591/, "");
  return num ?? `SIM ${slot + 1}`;
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
