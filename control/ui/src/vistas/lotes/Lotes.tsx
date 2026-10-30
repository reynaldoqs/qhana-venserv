import { useCallback, useEffect, useRef, useState } from "react";
import { Layers, Plus } from "../../componentes/iconos";
import { api } from "../../api/cliente";
import { useEvento } from "../../api/eventos";
import type { Dispositivo, Lote } from "../../api/tipos";
import { Aviso, Boton, Esqueleto, Insignia, Tarjeta, Titulo, Vacio } from "../../componentes/base";
import { fechaHora } from "../../util/datos";
import { BarraProgreso, EstadoLoteInsignia, LeyendaProgreso, bs } from "./comunes";
import { NuevoLote } from "./NuevoLote";
import { DetalleLote } from "./DetalleLote";

function ListaLotes({ nuevo, abrir }: { nuevo: () => void; abrir: (id: number) => void }) {
  const [lotes, setLotes] = useState<Lote[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [cargando, setCargando] = useState(true);
  const pendiente = useRef<number | null>(null);

  const cargar = useCallback(() => {
    api
      .lotes()
      .then((l) => {
        setLotes([...l].sort((a, b) => b.creado_ms - a.creado_ms));
        setError(null);
      })
      .catch((e) => setError((e as Error).message))
      .finally(() => setCargando(false));
  }, []);
  useEffect(cargar, [cargar]);
  useEvento((e) => {
    if (e.tipo !== "lote") return;
    if (pendiente.current) window.clearTimeout(pendiente.current);
    pendiente.current = window.setTimeout(cargar, 300);
  });

  return (
    <div>
      <Titulo
        titulo="Lotes"
        sub="Operaciones en lote desde un CSV: se validan, se reparten entre los chips y se reportan."
        acciones={
          <Boton variante="primario" icono={<Plus className="size-4" />} onClick={nuevo}>
            Nuevo lote
          </Boton>
        }
      />
      {error && (
        <div className="mb-4">
          <Aviso>{error}</Aviso>
        </div>
      )}
      {cargando && lotes.length === 0 ? (
        <div className="space-y-3" aria-busy>
          <Esqueleto className="h-24 rounded-3xl" />
          <Esqueleto className="h-24 rounded-3xl" />
        </div>
      ) : lotes.length === 0 ? (
        <Tarjeta>
          <Vacio
            icono={<Layers className="size-5" />}
            titulo="Todavía no hay lotes"
            texto="Sube un CSV con número, monto y operador. Puedes simularlo primero sin mover saldo."
          />
        </Tarjeta>
      ) : (
        <div className="space-y-3 escalonar">
          {lotes.map((l, i) => {
            const activas = l.progreso.filas;
            const hechas = activas - l.progreso.pendientes;
            return (
              <div key={l.id} style={{ "--i": Math.min(i, 8) } as React.CSSProperties}>
                <button
                  onClick={() => abrir(l.id)}
                  className="w-full text-left rounded-3xl bg-panel border border-borde/60 p-5 transition-[transform,border-color,box-shadow] duration-300 ease-salida hover:-translate-y-0.5 hover:border-tenue/40 hover:shadow-elevada grid gap-4 md:grid-cols-[1fr_minmax(220px,320px)_auto] md:items-center"
                >
                  <div className="min-w-0">
                    <div className="flex flex-wrap items-center gap-2">
                      <span className="font-semibold truncate">{l.nombre}</span>
                      {l.simular && <Insignia tono="info">Simulación</Insignia>}
                      {l.verificacion && (
                        <Insignia tono={l.verificacion.confirmado ? "ok" : "error"}>
                          {l.verificacion.confirmado ? "Verificado" : "Revisar saldos"}
                        </Insignia>
                      )}
                    </div>
                    <p className="text-xs text-tenue mt-1">
                      #{l.id} · {fechaHora(l.creado_ms)} · {activas} filas · {bs(l.progreso.total_bs)}
                    </p>
                  </div>
                  <div className="space-y-2">
                    <BarraProgreso p={l.progreso} />
                    <LeyendaProgreso p={l.progreso} />
                  </div>
                  <div className="flex md:flex-col items-center md:items-end justify-between gap-2">
                    <p className="text-2xl font-extrabold tabular-nums tracking-tight leading-none">
                      {hechas}
                      <span className="text-sm font-medium text-tenue"> / {activas}</span>
                    </p>
                    <EstadoLoteInsignia estado={l.estado} />
                  </div>
                </button>
              </div>
            );
          })}
          <button
            onClick={nuevo}
            className="w-full h-16 rounded-3xl border-2 border-dashed border-borde text-sm font-medium text-tenue hover:text-texto hover:border-tenue/60 hover:bg-panel transition-colors inline-flex items-center justify-center gap-2"
          >
            <Plus className="size-4" /> Nuevo lote
          </button>
        </div>
      )}
    </div>
  );
}

/** Subrutas: #lotes, #lotes/nuevo, #lotes/12 */
export function Lotes({ ruta, ir, dispositivos }: { ruta: string; ir: (r: string) => void; dispositivos: Dispositivo[] }) {
  if (ruta === "nuevo") return <NuevoLote dispositivos={dispositivos} volver={() => ir("")} abrirLote={(id) => ir(String(id))} />;
  const id = Number(ruta);
  if (ruta && Number.isFinite(id)) return <DetalleLote key={id} id={id} dispositivos={dispositivos} volver={() => ir("")} />;
  return <ListaLotes nuevo={() => ir("nuevo")} abrir={(id) => ir(String(id))} />;
}
