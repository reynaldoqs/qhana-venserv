import { useEffect, useMemo, useRef, useState } from "react";
import type { Dispositivo } from "../api/tipos";
import { Hash, MessageSquare, Search } from "./iconos";
import { Icono, type NombreIcono } from "./iconos";
import { cx } from "./base";
import { simsConChip } from "./dispositivo";
import { etiquetaOperador } from "../util/datos";

export interface DestinoVista {
  id: string;
  titulo: string;
  icono: NombreIcono;
}

interface Opcion {
  clave: string;
  titulo: string;
  detalle?: string;
  icono: React.ReactNode;
  ejecutar: () => void;
}

/** Paleta de comandos (Cmd/Ctrl+K): saltar a una vista o abrir la consola/SMS de un chip concreto. */
export function Paleta({
  abierta,
  cerrar,
  vistas,
  dispositivos,
  irVista,
  abrirChip,
}: {
  abierta: boolean;
  cerrar: () => void;
  vistas: DestinoVista[];
  dispositivos: Dispositivo[];
  irVista: (id: string) => void;
  abrirChip: (id: string, slot: number, vista: "ussd" | "sms") => void;
}) {
  const [q, setQ] = useState("");
  const [sel, setSel] = useState(0);
  const entrada = useRef<HTMLInputElement>(null);
  const lista = useRef<HTMLUListElement>(null);

  useEffect(() => {
    if (abierta) {
      setQ("");
      setSel(0);
      requestAnimationFrame(() => entrada.current?.focus());
    }
  }, [abierta]);

  const opciones = useMemo<Opcion[]>(() => {
    const out: Opcion[] = vistas.map((v) => ({
      clave: `v-${v.id}`,
      titulo: `Ir a ${v.titulo}`,
      icono: <Icono nombre={v.icono} className="size-[18px]" />,
      ejecutar: () => irVista(v.id),
    }));
    for (const d of dispositivos.filter((x) => x.conectado)) {
      for (const s of simsConChip(d)) {
        const quien = `${d.modelo} · SIM ${s.slot + 1} · ${s.operador ? etiquetaOperador(s) : "sin operador"}`;
        out.push({
          clave: `u-${d.id}-${s.slot}`,
          titulo: "Consola USSD",
          detalle: quien,
          icono: <Hash className="size-[18px]" />,
          ejecutar: () => abrirChip(d.id, s.slot, "ussd"),
        });
      }
      out.push({
        clave: `s-${d.id}`,
        titulo: "Mensajes SMS",
        detalle: d.modelo,
        icono: <MessageSquare className="size-[18px]" />,
        ejecutar: () => abrirChip(d.id, 0, "sms"),
      });
    }
    return out;
  }, [vistas, dispositivos, irVista, abrirChip]);

  const filtradas = useMemo(() => {
    const t = q.toLowerCase().split(/\s+/).filter(Boolean);
    return opciones.filter((o) => t.every((w) => `${o.titulo} ${o.detalle ?? ""}`.toLowerCase().includes(w)));
  }, [opciones, q]);

  useEffect(() => setSel(0), [q]);
  useEffect(() => {
    lista.current?.children[sel]?.scrollIntoView({ block: "nearest" });
  }, [sel]);

  if (!abierta) return null;

  const lanzar = (o?: Opcion) => {
    if (!o) return;
    cerrar();
    o.ejecutar();
  };

  return (
    <div
      className="fixed inset-0 z-50 grid place-items-start justify-center pt-[14vh] px-4 bg-black/60 backdrop-blur-sm entrar-vista"
      onMouseDown={(e) => e.target === e.currentTarget && cerrar()}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Buscar acciones"
        className="w-full max-w-xl rounded-3xl bg-panel border border-borde shadow-elevada overflow-hidden"
        onKeyDown={(e) => {
          if (e.key === "Escape") cerrar();
          else if (e.key === "ArrowDown") {
            e.preventDefault();
            setSel((i) => Math.min(i + 1, filtradas.length - 1));
          } else if (e.key === "ArrowUp") {
            e.preventDefault();
            setSel((i) => Math.max(i - 1, 0));
          } else if (e.key === "Enter") {
            e.preventDefault();
            lanzar(filtradas[sel]);
          }
        }}
      >
        <div className="flex items-center gap-3 px-5 h-14 border-b border-borde">
          <Search className="size-[18px] text-tenue shrink-0" />
          <input
            ref={entrada}
            value={q}
            onChange={(e) => setQ(e.target.value)}
            role="combobox"
            aria-expanded
            aria-controls="paleta-lista"
            placeholder="Ir a una vista, abrir USSD de un chip…"
            className="flex-1 bg-transparent outline-none text-sm placeholder:text-tenue"
          />
          <kbd className="text-[11px] text-tenue rounded-md bg-panel-2 px-1.5 py-0.5">Esc</kbd>
        </div>
        <ul id="paleta-lista" ref={lista} role="listbox" className="max-h-80 overflow-y-auto p-2">
          {filtradas.length === 0 && (
            <li className="px-4 py-8 text-center text-sm text-tenue">Nada coincide con “{q}”.</li>
          )}
          {filtradas.map((o, i) => (
            <li
              key={o.clave}
              role="option"
              aria-selected={i === sel}
              onMouseMove={() => setSel(i)}
              onClick={() => lanzar(o)}
              className={cx(
                "flex items-center gap-3 px-3 h-12 rounded-2xl cursor-pointer text-sm",
                i === sel ? "bg-panel-2 text-texto" : "text-tenue",
              )}
            >
              <span className="size-8 rounded-xl bg-black/25 grid place-items-center shrink-0">{o.icono}</span>
              <span className="font-medium truncate">{o.titulo}</span>
              {o.detalle && <span className="text-xs text-tenue truncate">{o.detalle}</span>}
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}

