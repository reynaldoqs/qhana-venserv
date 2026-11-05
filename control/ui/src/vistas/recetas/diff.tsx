import { useState } from "react";
import { cx } from "../../componentes/base";

export type LineaDiff = { tipo: "igual" | "quitada" | "agregada"; texto: string; a?: number; b?: number };

/** Diferencia por líneas (subsecuencia común más larga). Las recetas son cortas: O(n·m) sobra. */
export function diffLineas(antes: string, despues: string): LineaDiff[] {
  const a = antes.replace(/\n$/, "").split("\n");
  const b = despues.replace(/\n$/, "").split("\n");
  const n = a.length;
  const m = b.length;
  // lcs[i][j]: largo de la LCS de a[i..] y b[j..]
  const lcs = Array.from({ length: n + 1 }, () => new Uint16Array(m + 1));
  for (let i = n - 1; i >= 0; i--)
    for (let j = m - 1; j >= 0; j--) lcs[i][j] = a[i] === b[j] ? lcs[i + 1][j + 1] + 1 : Math.max(lcs[i + 1][j], lcs[i][j + 1]);
  const out: LineaDiff[] = [];
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (a[i] === b[j]) {
      out.push({ tipo: "igual", texto: a[i], a: i + 1, b: j + 1 });
      i++;
      j++;
    } else if (lcs[i + 1][j] >= lcs[i][j + 1]) out.push({ tipo: "quitada", texto: a[i], a: ++i });
    else out.push({ tipo: "agregada", texto: b[j], b: ++j });
  }
  while (i < n) out.push({ tipo: "quitada", texto: a[i], a: ++i });
  while (j < m) out.push({ tipo: "agregada", texto: b[j], b: ++j });
  return out;
}

const CONTEXTO = 3;

/**
 * Diferencias en formato unificado: `-` lo que había antes, `+` lo nuevo. Los tramos largos sin cambios se pliegan
 * (se abren con un clic).
 */
export function VistaDiff({ antes, despues, etiquetas }: { antes: string; despues: string; etiquetas: [string, string] }) {
  const lineas = diffLineas(antes, despues);
  const [abiertos, setAbiertos] = useState<Set<number>>(new Set());
  const cambios = lineas.filter((l) => l.tipo !== "igual").length;

  // Agrupa en bloques: cambios con 3 líneas de contexto, y lo demás plegado.
  const visible = lineas.map((_, k) => lineas.slice(Math.max(0, k - CONTEXTO), k + CONTEXTO + 1).some((l) => l.tipo !== "igual"));
  const bloques: { inicio: number; fin: number; plegado: boolean }[] = [];
  lineas.forEach((_, k) => {
    const plegado = !visible[k];
    const ult = bloques[bloques.length - 1];
    if (ult && ult.plegado === plegado) ult.fin = k;
    else bloques.push({ inicio: k, fin: k, plegado });
  });

  return (
    <div className="rounded-2xl border border-borde/60 overflow-hidden">
      <div className="flex flex-wrap items-center gap-x-4 gap-y-1 px-3.5 py-2 bg-panel-2 text-xs">
        <span className="text-error">− {etiquetas[0]}</span>
        <span className="text-ok">+ {etiquetas[1]}</span>
        <span className="ml-auto text-tenue tabular-nums">{cambios === 0 ? "Sin diferencias" : `${cambios} líneas distintas`}</span>
      </div>
      <div className="overflow-x-auto">
        <pre className="font-mono text-[12px] leading-[1.6] min-w-fit">
          {bloques.map((bl, bi) =>
            bl.plegado && !abiertos.has(bi) ? (
              <button
                key={bi}
                type="button"
                onClick={() => setAbiertos((s) => new Set(s).add(bi))}
                className="block w-full text-left px-3.5 py-1 bg-panel text-tenue hover:text-texto text-[11px]"
              >
                ⋯ {bl.fin - bl.inicio + 1} líneas sin cambios
              </button>
            ) : (
              lineas.slice(bl.inicio, bl.fin + 1).map((l, k) => (
                <div
                  key={`${bi}-${k}`}
                  className={cx(
                    "grid grid-cols-[2.5rem_2.5rem_1rem_1fr] pr-3",
                    l.tipo === "quitada" && "bg-error-suave",
                    l.tipo === "agregada" && "bg-ok-suave",
                  )}
                >
                  <span className="text-right pr-2 text-tenue/60 select-none">{l.a ?? ""}</span>
                  <span className="text-right pr-2 text-tenue/60 select-none">{l.b ?? ""}</span>
                  <span className={cx("select-none", l.tipo === "quitada" ? "text-error" : l.tipo === "agregada" ? "text-ok" : "text-tenue/40")}>
                    {l.tipo === "quitada" ? "−" : l.tipo === "agregada" ? "+" : " "}
                  </span>
                  <span className="whitespace-pre">{l.texto || " "}</span>
                </div>
              ))
            ),
          )}
        </pre>
      </div>
    </div>
  );
}
