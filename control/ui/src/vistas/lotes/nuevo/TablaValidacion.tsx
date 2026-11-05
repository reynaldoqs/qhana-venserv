import { useMemo, useState } from "react";
import type { Dispositivo, FilaValidada } from "../../../api/tipos";
import { Segmentado, Tarjeta, cx } from "../../../componentes/base";
import { EstadoValidacion, bs, nombreChip } from "../comunes";

type Filtro = "todas" | "errores" | "avisos" | "validas";

/** Las filas validadas, con filtro: número, operador, monto, piezas, de qué chip sale y errores/avisos. */
export function TablaValidacion({ filas, dispositivos }: { filas: FilaValidada[]; dispositivos: Dispositivo[] }) {
  const [filtro, setFiltro] = useState<Filtro>(() => (filas.some((f) => f.estado === "invalida") ? "errores" : "todas"));
  const vistas = useMemo(
    () => ({
      todas: filas,
      errores: filas.filter((f) => f.estado === "invalida"),
      avisos: filas.filter((f) => f.estado === "aviso" || f.sin_capacidad > 0),
      validas: filas.filter((f) => f.estado !== "invalida"),
    }),
    [filas],
  );
  const lista = vistas[filtro];
  return (
    <Tarjeta className="overflow-hidden">
      <div className="p-2 border-b border-borde">
        <Segmentado
          etiqueta="Filtrar filas validadas"
          controla="filas-validadas"
          valor={filtro}
          onCambio={setFiltro}
          className="border-0 bg-transparent p-0"
          opciones={[
            { id: "todas", texto: "Todas", n: vistas.todas.length },
            { id: "errores", texto: "Con errores", n: vistas.errores.length },
            { id: "avisos", texto: "Avisos", n: vistas.avisos.length },
            { id: "validas", texto: "Válidas", n: vistas.validas.length },
          ]}
        />
      </div>
      <div id="filas-validadas" role="tabpanel" className="overflow-auto max-h-[520px]">
        <table className="w-full text-sm">
          <thead className="sticky top-0 bg-panel z-10">
            <tr className="text-left text-xs text-tenue border-b border-borde">
              <th className="font-medium px-4 py-2.5 w-14">Fila</th>
              <th className="font-medium px-3 py-2.5">Número</th>
              <th className="font-medium px-3 py-2.5">Operador</th>
              <th className="font-medium px-3 py-2.5 text-right">Monto</th>
              <th className="font-medium px-3 py-2.5">Envíos</th>
              <th className="font-medium px-3 py-2.5">Sale de</th>
              <th className="font-medium px-3 py-2.5">Estado</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-borde">
            {lista.length === 0 && (
              <tr>
                <td colSpan={7} className="px-4 py-10 text-center text-tenue">
                  Nada en esta vista
                </td>
              </tr>
            )}
            {lista.map((f) => (
              <tr key={f.fila} className={cx(f.estado === "invalida" && "bg-error-suave/40")}>
                <td className="px-4 py-2.5 text-tenue tabular-nums align-top">{f.fila}</td>
                <td className="px-3 py-2.5 font-mono align-top">
                  {f.numero ?? <span className="text-error">{f.original.numero || "(vacío)"}</span>}
                  {f.referencia && <div className="text-xs text-tenue font-sans">{f.referencia}</div>}
                </td>
                <td className="px-3 py-2.5 align-top">
                  {f.operador ?? <span className="text-error">{f.original.operador || "(vacío)"}</span>}
                  {f.accion && f.accion !== "transferir" && <div className="text-xs text-tenue">{f.accion}</div>}
                </td>
                <td className="px-3 py-2.5 text-right tabular-nums align-top">
                  {f.monto != null ? bs(f.monto) : <span className="text-error">{f.original.monto || "(vacío)"}</span>}
                </td>
                <td className="px-3 py-2.5 align-top">
                  <div className="flex flex-wrap gap-1">
                    {f.envios.map((m, i) => (
                      <span key={i} className="h-5.5 px-1.5 rounded-md bg-panel-2 text-xs font-mono tabular-nums inline-flex items-center">
                        {m}
                      </span>
                    ))}
                  </div>
                </td>
                <td className="px-3 py-2.5 align-top text-xs font-mono text-tenue">
                  {[...new Set(f.plan.map((p) => nombreChip(dispositivos, p.dispositivo, p.slot)))].join(", ")}
                  {f.sin_capacidad > 0 && (
                    <div className="text-aviso font-sans">
                      {f.sin_capacidad} envío{f.sin_capacidad > 1 ? "s" : ""} para mañana
                      {f.motivo_sin_capacidad && <span className="text-tenue"> · {f.motivo_sin_capacidad}</span>}
                    </div>
                  )}
                </td>
                <td className="px-3 py-2.5 align-top">
                  <EstadoValidacion estado={f.estado} />
                  {f.errores.map((e, i) => (
                    <div key={i} className="text-xs text-error mt-1 max-w-64">
                      {e}
                    </div>
                  ))}
                  {f.avisos.map((e, i) => (
                    <div key={i} className="text-xs text-aviso mt-1 max-w-64">
                      {e}
                    </div>
                  ))}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </Tarjeta>
  );
}
