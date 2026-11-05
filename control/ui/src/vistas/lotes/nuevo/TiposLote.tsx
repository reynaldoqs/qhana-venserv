import { useMemo, type KeyboardEvent, type ReactNode } from "react";
import type { ChipInfo, Dispositivo, Receta, TipoPlantilla } from "../../../api/tipos";
import { Aviso, Insignia, Segmentado, cx } from "../../../componentes/base";
import { Layers, Smartphone, Wallet } from "../../../componentes/iconos";
import { etiquetaTelefono, hace, nombreSim, sinPrefijo } from "../../../util/datos";
import { bs } from "../comunes";
import { claveChip, pidePin, recetaParaChip } from "./chips";

/** Los tres tipos de lote. Cada uno es una forma de armar el CSV y de elegir quién envía. */
export type TipoLote = TipoPlantilla;

interface InfoTipo {
  id: TipoLote;
  titulo: string;
  texto: ReactNode;
  columnas: string;
  icono: ReactNode;
}

export const TIPOS: InfoTipo[] = [
  {
    id: "transferencias",
    titulo: "Transferencias",
    texto: "Cada fila dice su operador; se reparten entre todos los chips de ese operador.",
    columnas: "numero,monto,operador[,accion,referencia]",
    icono: <Wallet className="size-5" />,
  },
  {
    id: "recargas",
    titulo: "Recargas de varios operadores",
    texto: "Cada fila va a los chips dealer de su operador. Tigo, Entel y Viva trabajan a la vez.",
    columnas: "numero,monto,operador,accion=recargar",
    icono: <Layers className="size-5" />,
  },
  {
    id: "chips",
    titulo: "Desde chips elegidos",
    texto: "Eliges uno o varios chips y la acción; el lote se reparte solo entre ellos.",
    columnas: "numero,monto[,operador,referencia]",
    icono: <Smartphone className="size-5" />,
  },
];

/** Ejemplo para el cuadro de pegar texto. Montos en Bs, con centavos donde la receta los acepta. */
export function ejemploCsv(tipo: TipoLote, variosOperadores: boolean): string {
  if (tipo === "recargas") return "numero,monto,operador,accion,referencia\n69869869,0.50,ENTEL,recargar,Cliente 1\n78815232,5,TIGO,recargar,Cliente 2\n71234567,12.30,TIGO,recargar,";
  if (tipo === "chips")
    return variosOperadores
      ? "numero,monto,operador,referencia\n69869869,5,TIGO,Cliente 1\n71234567,0.50,ENTEL,Cliente 2"
      : "numero,monto,referencia\n69869869,5,Cliente 1\n78815232,1.50,Cliente 2";
  return "numero,monto,operador,accion,referencia\n69869869,10,TIGO,transferir,Pedido 1001\n71234567,5,TIGO,,Pedido 1002";
}

/** Paso 1: tres tarjetas grandes, como un grupo de radio (flechas para moverse). */
export function ElegirTipo({ tipo, onCambio }: { tipo: TipoLote | null; onCambio: (t: TipoLote) => void }) {
  const mover = (e: KeyboardEvent<HTMLDivElement>) => {
    const d = e.key === "ArrowRight" || e.key === "ArrowDown" ? 1 : e.key === "ArrowLeft" || e.key === "ArrowUp" ? -1 : 0;
    if (!d) return;
    e.preventDefault();
    const i = TIPOS.findIndex((t) => t.id === tipo);
    const j = (Math.max(0, i) + d + TIPOS.length) % TIPOS.length;
    onCambio(TIPOS[j].id);
    e.currentTarget.querySelectorAll<HTMLButtonElement>("[role=radio]")[j]?.focus();
  };
  return (
    <div role="radiogroup" aria-label="Tipo de lote" onKeyDown={mover} className="grid gap-2.5 md:grid-cols-3">
      {TIPOS.map((t, i) => {
        const activo = t.id === tipo;
        return (
          <button
            key={t.id}
            type="button"
            role="radio"
            aria-checked={activo}
            tabIndex={activo || (tipo === null && i === 0) ? 0 : -1}
            onClick={() => onCambio(t.id)}
            className={cx(
              "group text-left rounded-2xl border px-4 py-3.5 flex gap-3.5 md:flex-col md:gap-3",
              "transition-[border-color,background-color,transform] duration-200 ease-salida active:scale-[0.985]",
              activo ? "border-acento/70 bg-acento-suave/40" : "border-borde bg-panel hover:border-tenue/50 hover:bg-panel-2/60",
            )}
          >
            <span
              className={cx(
                "size-10 shrink-0 rounded-xl grid place-items-center transition-colors duration-200",
                activo ? "bg-acento text-sobre-acento" : "bg-panel-2 text-tenue group-hover:text-texto",
              )}
            >
              {t.icono}
            </span>
            <span className="min-w-0">
              <span className="block font-semibold">{t.titulo}</span>
              <span className="block text-xs text-tenue mt-0.5 leading-relaxed">{t.texto}</span>
              <span className="block mt-2 font-mono text-[11px] text-tenue/90 break-all">{t.columnas}</span>
            </span>
          </button>
        );
      })}
    </div>
  );
}

/** Para "Recargas de varios operadores": qué chips dealer hay en línea por operador (y cuáles no tienen PIN). */
export function InventarioDealer({ chips, recetas, cargado }: { chips: ChipInfo[]; recetas: Receta[]; cargado: boolean }) {
  const porOperador = useMemo(() => {
    const m = new Map<string, { total: number; sinPin: number; listos: number }>();
    for (const c of chips) {
      if (c.tipo !== "dealer") continue;
      const r = recetaParaChip(recetas, c, "recargar");
      const sinPin = pidePin(r) && !c.tiene_pin;
      const x = m.get(c.operador) ?? { total: 0, sinPin: 0, listos: 0 };
      x.total++;
      if (sinPin) x.sinPin++;
      else if (r) x.listos++;
      m.set(c.operador, x);
    }
    return [...m.entries()].sort(([a], [b]) => a.localeCompare(b));
  }, [chips, recetas]);

  return (
    <div className="mt-4 rounded-2xl bg-panel-2/60 px-4 py-3 text-sm">
      <p className="text-tenue">
        Cada fila se reparte entre los <b className="text-texto font-semibold">chips dealer de su operador</b> que estén en línea. Cada teléfono hace
        un envío a la vez y los teléfonos trabajan en paralelo. Los montos van en Bs: <span className="font-mono">0.50</span>,{" "}
        <span className="font-mono">5</span>, <span className="font-mono">12.30</span>.
      </p>
      <div className="mt-2.5 flex flex-wrap gap-2">
        {cargado && porOperador.length === 0 && <span className="text-aviso text-xs">No hay chips dealer en línea: márcalos en Teléfonos.</span>}
        {porOperador.map(([op, x]) => (
          <span key={op} className="inline-flex items-center gap-2 h-7 px-3 rounded-full bg-black/25 text-xs">
            <b className="font-semibold">{op}</b>
            <span className="text-tenue tabular-nums">
              {x.listos} dealer listo{x.listos === 1 ? "" : "s"}
            </span>
            {x.sinPin > 0 && <span className="text-aviso tabular-nums">{x.sinPin} sin PIN</span>}
          </span>
        ))}
      </div>
    </div>
  );
}

/** Paso 2 de "Desde chips elegidos": la acción y los chips (agrupados por teléfono). */
export function ElegirChips({
  dispositivos,
  chips,
  recetas,
  error,
  accion,
  onAccion,
  seleccion,
  onSeleccion,
}: {
  dispositivos: Dispositivo[];
  chips: ChipInfo[];
  recetas: Receta[];
  error: string | null;
  accion: string;
  onAccion: (a: string) => void;
  seleccion: string[];
  onSeleccion: (s: string[]) => void;
}) {
  const elegidos = new Set(seleccion);
  const grupos = useMemo(() => {
    const m = new Map<string, ChipInfo[]>();
    for (const c of chips) m.set(c.dispositivo, [...(m.get(c.dispositivo) ?? []), c].sort((a, b) => a.slot - b.slot));
    return [...m.entries()];
  }, [chips]);
  const alternar = (k: string) => onSeleccion(elegidos.has(k) ? seleccion.filter((x) => x !== k) : [...seleccion, k]);
  const dealer = chips.filter((c) => c.tipo === "dealer").map((c) => claveChip(c.dispositivo, c.slot));
  const operadores = new Set(chips.filter((c) => elegidos.has(claveChip(c.dispositivo, c.slot))).map((c) => c.operador));
  const telefonos = new Set(chips.filter((c) => elegidos.has(claveChip(c.dispositivo, c.slot))).map((c) => c.dispositivo));
  const atajo = "h-7 px-3 rounded-full text-xs font-medium text-tenue bg-panel-2/70 hover:text-texto hover:bg-panel-2 transition-colors";

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
        <span className="text-xs font-medium text-tenue">Acción para todas las filas</span>
        <Segmentado
          etiqueta="Acción del lote"
          valor={accion}
          onCambio={onAccion}
          opciones={[
            { id: "recargar", texto: "Recargar" },
            { id: "transferir", texto: "Transferir" },
          ]}
        />
      </div>

      {error && <Aviso>No pude leer los chips: {error}</Aviso>}

      <div className="flex flex-wrap items-center gap-2">
        <span className="text-xs text-tenue mr-1 tabular-nums">
          {seleccion.length === 0
            ? "Ningún chip elegido"
            : `${seleccion.length} chip${seleccion.length === 1 ? "" : "s"} · ${telefonos.size} teléfono${telefonos.size === 1 ? "" : "s"} · ${[...operadores].join(", ")}`}
        </span>
        {dealer.length > 0 && (
          <button type="button" className={atajo} onClick={() => onSeleccion(dealer)}>
            Solo los dealer ({dealer.length})
          </button>
        )}
        <button type="button" className={atajo} onClick={() => onSeleccion(chips.map((c) => claveChip(c.dispositivo, c.slot)))}>
          Todos
        </button>
        {seleccion.length > 0 && (
          <button type="button" className={atajo} onClick={() => onSeleccion([])}>
            Ninguno
          </button>
        )}
      </div>

      {grupos.length === 0 && !error && <p className="text-sm text-tenue">No hay chips en línea.</p>}

      <div className="grid gap-3 lg:grid-cols-2">
        {grupos.map(([disp, lista]) => {
          const d = dispositivos.find((x) => x.id === disp);
          return (
            <fieldset key={disp} className="rounded-2xl border border-borde/70 p-2.5 min-w-0">
              <legend className="px-1.5 text-xs text-tenue inline-flex items-center gap-1.5">
                <Smartphone className="size-3.5" />
                {d ? etiquetaTelefono(d) : disp.slice(0, 8)}
                {lista.length > 1 && <span className="opacity-70">· sus chips se turnan</span>}
              </legend>
              <div className="space-y-1.5">
                {lista.map((c) => {
                  const k = claveChip(c.dispositivo, c.slot);
                  const r = recetaParaChip(recetas, c, accion);
                  const faltaPin = pidePin(r) && !c.tiene_pin;
                  const marcado = elegidos.has(k);
                  return (
                    <label
                      key={k}
                      className={cx(
                        "flex items-start gap-3 rounded-xl px-3 py-2.5 cursor-pointer select-none transition-colors duration-150",
                        marcado ? "bg-panel-2" : "hover:bg-panel-2/50",
                      )}
                    >
                      <input
                        type="checkbox"
                        checked={marcado}
                        onChange={() => alternar(k)}
                        className="mt-1 size-4 shrink-0 accent-[var(--color-acento)] cursor-pointer"
                      />
                      <span className="min-w-0 flex-1">
                        <span className="flex flex-wrap items-center gap-x-2 gap-y-1">
                          <span className="font-mono text-sm">{sinPrefijo(c.numero) ?? nombreSim(c.slot)}</span>
                          <span className="text-xs text-tenue">{nombreSim(c.slot)}</span>
                          <Insignia>{c.operador}</Insignia>
                          {c.tipo === "dealer" && <Insignia tono="info">Dealer</Insignia>}
                          {c.tiene_pin && <Insignia tono="ok">PIN guardado</Insignia>}
                        </span>
                        <span className="mt-1 flex flex-wrap gap-x-3 gap-y-0.5 text-xs text-tenue tabular-nums">
                          <span title={c.saldo_ms ? `Consultado ${hace(c.saldo_ms)}` : "Sin consultar"}>Saldo {bs(c.saldo)}</span>
                          {c.tipo === "dealer" && (
                            <span title={c.saldo_dealer_ms ? `Consultado ${hace(c.saldo_dealer_ms)}` : "Sin consultar"}>Saldo dealer {bs(c.saldo_dealer)}</span>
                          )}
                          {r ? (
                            <span className="font-mono">{r.nombre}</span>
                          ) : (
                            <span className="text-aviso">sin receta para {accion}</span>
                          )}
                          {faltaPin && <span className="text-error">pide PIN: guárdalo en Teléfonos</span>}
                        </span>
                      </span>
                    </label>
                  );
                })}
              </div>
            </fieldset>
          );
        })}
      </div>
    </div>
  );
}
