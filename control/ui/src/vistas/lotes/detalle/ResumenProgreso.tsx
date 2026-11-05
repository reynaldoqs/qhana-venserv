import type { Lote } from "../../../api/tipos";
import { Tarjeta, cx } from "../../../componentes/base";
import { BarraProgreso, EnlaceManual, bs, filasBien, procesadas } from "../comunes";

/** Tarjeta de progreso: cuántas filas se procesaron (OK o no), cuánto se envió y el desglose por estado. */
export function ResumenProgreso({ lote }: { lote: Lote }) {
  const p = lote.progreso;
  const celdas: [string, number, string][] = [
    [lote.simular ? "Simuladas" : "OK", lote.simular ? p.simulado : p.ok, lote.simular ? "text-info" : "text-ok"],
    ["Parciales", p.parcial, "text-aviso"],
    ["Para mañana", p.sin_capacidad, "text-aviso"],
    ["Fallidas", p.fallidas, "text-error"],
    ["Inciertas", p.inciertas, "text-error"],
    ["Pendientes", p.pendientes, "text-texto"],
  ];
  return (
    <Tarjeta className="p-5 mb-5">
      <div className="flex flex-wrap items-end justify-between gap-4 mb-3">
        <div>
          <p className="text-xs text-tenue font-medium">Filas procesadas</p>
          <p className="text-2xl font-semibold tabular-nums tracking-tight">
            {procesadas(p)} <span className="text-base font-normal text-tenue">/ {p.filas}</span>
          </p>
          <p className="text-xs text-tenue tabular-nums">
            {filasBien(p)} {lote.simular ? "simuladas" : "OK"}
            {p.fallidas + p.inciertas + p.parcial > 0 && ` · ${p.fallidas + p.inciertas + p.parcial} con problema`}
          </p>
        </div>
        <div className="text-right">
          <p className="text-xs text-tenue font-medium">{lote.simular ? "Simulado" : "Enviado"}</p>
          <p className="text-2xl font-semibold tabular-nums tracking-tight">
            {bs(p.enviado_bs)} <span className="text-base font-normal text-tenue">/ {bs(p.total_bs)}</span>
          </p>
          <p className="text-xs text-tenue">comisión {bs(p.comision_bs)}</p>
        </div>
      </div>
      <BarraProgreso p={p} alta />
      <dl className="mt-4 grid grid-cols-3 sm:grid-cols-6 gap-3 text-sm">
        {celdas.map(([t, n, c]) => (
          <div key={t}>
            <dt className="text-xs text-tenue">{t}</dt>
            <dd className={cx("font-semibold tabular-nums", n > 0 && c)}>{n}</dd>
          </div>
        ))}
      </dl>
      {p.omitidas > 0 && (
        <p className="mt-3 text-xs text-tenue">
          Además, {p.omitidas} fila{p.omitidas === 1 ? "" : "s"} omitida{p.omitidas === 1 ? "" : "s"} por error de validación (no se
          ejecutan). <EnlaceManual seccion="validacion" />
        </p>
      )}
    </Tarjeta>
  );
}
