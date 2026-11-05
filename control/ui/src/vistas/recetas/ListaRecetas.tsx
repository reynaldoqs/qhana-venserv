import { useMemo, useState } from "react";
import type { ResumenReceta } from "../../api/tipos";
import { Campo, Segmentado, Tarjeta, cx } from "../../componentes/base";
import { InsigniasReceta, LineaUso } from "./comunes";

const sinAcentos = (t: string) => t.normalize("NFD").replace(/[̀-ͯ]/g, "").toLowerCase();

/** Lista con búsqueda y filtro por operador (los operadores salen de las recetas mismas). */
export function ListaRecetas({
  lista,
  seleccionada,
  abrir,
}: {
  lista: ResumenReceta[];
  seleccionada: string | null;
  abrir: (nombre: string) => void;
}) {
  const [filtro, setFiltro] = useState("todas");
  const [q, setQ] = useState("");
  const operadores = useMemo(() => [...new Set(lista.map((r) => r.operador.toUpperCase()))].sort(), [lista]);
  const filtroValido = filtro === "todas" || operadores.includes(filtro) ? filtro : "todas";
  const visibles = useMemo(() => {
    const t = sinAcentos(q.trim());
    return lista
      .filter((r) => filtroValido === "todas" || r.operador.toUpperCase() === filtroValido)
      .filter((r) => !t || sinAcentos(`${r.titulo} ${r.nombre} ${r.descripcion ?? ""} ${r.accion ?? ""} ${r.nombres_anteriores?.join(" ") ?? ""}`).includes(t))
      .sort((a, b) => a.operador.localeCompare(b.operador) || a.titulo.localeCompare(b.titulo));
  }, [lista, filtroValido, q]);

  return (
    <div className="space-y-3 min-w-0">
      <Campo type="search" aria-label="Buscar recetas" placeholder="Buscar receta…" value={q} onChange={(e) => setQ(e.target.value)} />
      {operadores.length > 1 && (
        <Segmentado
          etiqueta="Filtrar recetas por operador"
          valor={filtroValido}
          onCambio={setFiltro}
          className="w-full"
          opciones={[
            { id: "todas", texto: "Todas", n: lista.length },
            ...operadores.map((o) => ({
              id: o,
              texto: o.charAt(0) + o.slice(1).toLowerCase(),
              n: lista.filter((r) => r.operador.toUpperCase() === o).length,
            })),
          ]}
        />
      )}
      <Tarjeta className="p-1.5">
        {visibles.length === 0 && <p className="px-3 py-6 text-center text-sm text-tenue">Ninguna receta coincide.</p>}
        <ul className="escalonar">
          {visibles.map((r, i) => (
            <li key={r.nombre} style={{ "--i": Math.min(i, 8) } as React.CSSProperties}>
              <button
                type="button"
                onClick={() => abrir(r.nombre)}
                aria-current={r.nombre === seleccionada || undefined}
                className={cx(
                  "w-full text-left rounded-xl px-3 py-2.5 transition-colors duration-150",
                  r.nombre === seleccionada ? "bg-panel-2" : "hover:bg-panel-2/60",
                )}
              >
                <div className="flex items-start justify-between gap-2">
                  <span className="font-medium text-sm min-w-0 break-words">{r.titulo}</span>
                  <span className="text-[11px] font-semibold text-tenue shrink-0 pt-0.5">{r.operador}</span>
                </div>
                <div className="mt-1.5 flex flex-wrap items-center gap-1">
                  <InsigniasReceta r={r} />
                </div>
                <LineaUso r={r} className="mt-1.5" />
              </button>
            </li>
          ))}
        </ul>
      </Tarjeta>
    </div>
  );
}
