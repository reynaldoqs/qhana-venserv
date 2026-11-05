import { useRef } from "react";
import { cx } from "../../../componentes/base";

/** El YAML a mano, con números de línea. La línea con error de sintaxis se marca. */
export function EditorYaml({ valor, onCambio, lineaError }: { valor: string; onCambio: (v: string) => void; lineaError?: number }) {
  const gutter = useRef<HTMLDivElement>(null);
  const lineas = valor.split("\n").length;
  return (
    <div className="relative flex rounded-3xl border border-borde/60 bg-panel overflow-hidden focus-within:border-acento focus-within:ring-4 focus-within:ring-acento/15 transition-[border-color,box-shadow]">
      <div ref={gutter} aria-hidden className="shrink-0 overflow-hidden select-none py-4 bg-panel-2/40 border-r border-borde/60">
        {Array.from({ length: lineas }, (_, k) => (
          <div
            key={k}
            className={cx(
              "px-3 text-right font-mono text-[12.5px] leading-[1.7] tabular-nums",
              lineaError === k + 1 ? "bg-error-suave text-error" : "text-tenue/50",
            )}
          >
            {k + 1}
          </div>
        ))}
      </div>
      <textarea
        aria-label="YAML de la receta"
        value={valor}
        spellCheck={false}
        autoCapitalize="off"
        autoCorrect="off"
        wrap="off"
        onChange={(e) => onCambio(e.target.value)}
        onScroll={(e) => {
          if (gutter.current) gutter.current.scrollTop = e.currentTarget.scrollTop;
        }}
        rows={Math.min(Math.max(lineas + 2, 16), 40)}
        className="flex-1 min-w-0 resize-y bg-transparent py-4 px-4 font-mono text-[12.5px] leading-[1.7] outline-none whitespace-pre overflow-auto"
      />
    </div>
  );
}
