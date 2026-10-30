import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ArrowLeft, CircleCheck, Download, Pause, Play, RotateCcw, Scale, Square, TriangleAlert } from "../../componentes/iconos";
import { api, FalloApi } from "../../api/cliente";
import { useEvento } from "../../api/eventos";
import type { Dispositivo, EstadoFilaLote, FilaLote, Lote } from "../../api/tipos";
import { Aviso, Boton, Insignia, Tarjeta, Titulo, cx } from "../../componentes/base";
import { fechaHora } from "../../util/datos";
import { BarraProgreso, EstadoFila, EstadoLoteInsignia, bs, nombreChip } from "./comunes";

type Filtro = "todas" | "pendientes" | "hechas" | "problemas" | "mañana";

const GRUPOS: Record<Exclude<Filtro, "todas">, EstadoFilaLote[]> = {
  pendientes: ["pendiente", "en_curso"],
  hechas: ["ok", "simulado"],
  problemas: ["fallida", "incierta", "parcial", "omitida"],
  mañana: ["sin_capacidad"],
};

const COLOR_ENVIO: Record<string, string> = {
  ok: "bg-ok",
  simulado: "bg-info",
  incierto: "bg-error",
  fallido: "bg-error",
  error: "bg-error",
  abortado: "bg-aviso",
  en_curso: "bg-info animate-pulse",
  pendiente: "bg-borde",
};

function Envios({ f, dispositivos }: { f: FilaLote; dispositivos: Dispositivo[] }) {
  if (f.envios.length === 0) return <span className="text-tenue">—</span>;
  return (
    <div className="flex flex-col gap-1">
      {f.envios.map((e, i) => (
        <div key={i} className="flex items-center gap-2 text-xs" title={e.detalle ?? e.estado}>
          <span className={cx("size-2 rounded-full shrink-0", COLOR_ENVIO[e.estado] ?? "bg-borde")} />
          <span className="font-mono tabular-nums w-8">{e.monto}</span>
          <span className="font-mono text-tenue">{e.numero_origen?.replace(/^\+591/, "") ?? nombreChip(dispositivos, e.dispositivo, e.slot)}</span>
          <span className="text-tenue">
            {e.estado}
            {(e.intentos ?? 1) > 1 && ` · ${e.intentos} intentos`}
          </span>
        </div>
      ))}
    </div>
  );
}

export function DetalleLote({ id, dispositivos, volver }: { id: number; dispositivos: Dispositivo[]; volver: () => void }) {
  const [lote, setLote] = useState<Lote | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [accion, setAccion] = useState<string | null>(null);
  const [confirmarCancelar, setConfirmarCancelar] = useState(false);
  const [filtro, setFiltro] = useState<Filtro>("todas");
  const [verificando, setVerificando] = useState(false);
  const pendiente = useRef<number | null>(null);

  const cargar = useCallback(async () => {
    try {
      setLote(await api.lote(id));
      setError(null);
    } catch (e) {
      setError((e as Error).message);
    }
  }, [id]);

  useEffect(() => {
    cargar();
  }, [cargar]);

  // Mientras corre, también se refresca cada 5 s por si se pierde algún evento.
  useEffect(() => {
    if (lote?.estado !== "en_curso") return;
    const t = setInterval(cargar, 5000);
    return () => clearInterval(t);
  }, [lote?.estado, cargar]);

  useEvento((e) => {
    if (e.tipo !== "lote" || Number(e.lote_id) !== id) return;
    if (pendiente.current) window.clearTimeout(pendiente.current);
    pendiente.current = window.setTimeout(cargar, 200);
  });

  async function hacer(nombre: string, fn: () => Promise<Lote>) {
    setAccion(nombre);
    try {
      setLote(await fn());
      await cargar();
    } catch (e) {
      const f = e as FalloApi;
      setError(`${f.codigo}: ${f.message}`);
    } finally {
      setAccion(null);
      setConfirmarCancelar(false);
    }
  }

  async function verificar() {
    setVerificando(true);
    try {
      await api.verificarLote(id); // el servidor la guarda en el lote
      await cargar();
      setError(null);
    } catch (e) {
      const f = e as FalloApi;
      setError(`${f.codigo}: ${f.message}`);
    } finally {
      setVerificando(false);
    }
  }

  const filas = lote?.filas ?? [];
  const vistas = useMemo(() => {
    const v: Record<Filtro, FilaLote[]> = { todas: filas, pendientes: [], hechas: [], problemas: [], mañana: [] };
    for (const f of filas)
      for (const [g, estados] of Object.entries(GRUPOS)) if (estados.includes(f.estado)) v[g as Filtro].push(f);
    return v;
  }, [filas]);

  if (!lote) {
    return (
      <div>
        <Titulo titulo="Lote" acciones={<Boton variante="fantasma" icono={<ArrowLeft className="size-4" />} onClick={volver}>Lotes</Boton>} />
        {error ? <Aviso>{error}</Aviso> : <p className="text-tenue">Cargando…</p>}
      </div>
    );
  }

  const p = lote.progreso;
  const verif = lote.verificacion ?? null;
  // Un servidor viejo no manda `revision`: no romper la pantalla por eso.
  const revision = lote.revision ?? { completo: false, motivos: [] };
  const activas = p.filas;
  const terminadas = activas - p.pendientes;
  const fallidos = filas.reduce((n, f) => n + f.envios.filter((e) => e.estado === "fallido").length, 0);
  const puedeReintentar = fallidos > 0 && p.inciertas === 0 && ["listo", "pausado", "terminado"].includes(lote.estado);
  const puedeVerificar = !lote.simular && p.ok + p.parcial > 0 && lote.estado !== "en_curso" && lote.estado !== "listo";
  const hecho = (lote.estado === "terminado" || lote.estado === "cancelado") && lote.revision != null;
  const puedeIniciar = lote.estado === "listo" || lote.estado === "pausado" || (lote.estado === "terminado" && p.sin_capacidad > 0);
  const etiquetas: [Filtro, string][] = [
    ["todas", "Todas"],
    ["pendientes", "Pendientes"],
    ["hechas", lote.simular ? "Simuladas" : "Hechas"],
    ["problemas", "Con problema"],
    ["mañana", "Para mañana"],
  ];

  return (
    <div>
      <Titulo
        titulo={lote.nombre}
        sub={
          <span className="inline-flex flex-wrap items-center gap-2">
            <EstadoLoteInsignia estado={lote.estado} />
            {lote.simular ? <Insignia tono="info">Simulación</Insignia> : <Insignia tono="aviso">Real</Insignia>}
            <span>creado {fechaHora(lote.creado_ms)}</span>
          </span>
        }
        acciones={
          <>
            <Boton variante="fantasma" icono={<ArrowLeft className="size-4" />} onClick={volver}>
              Lotes
            </Boton>
            <a href={api.urlResultado(lote.id)} download>
              <Boton icono={<Download className="size-4" />}>Resultado CSV</Boton>
            </a>
            {puedeReintentar && (
              <Boton
                icono={<RotateCcw className="size-4" />}
                cargando={accion === "reintentar"}
                onClick={() => hacer("reintentar", () => api.reintentarFallidas(lote.id))}
              >
                Reintentar fallidas ({fallidos})
              </Boton>
            )}
            {lote.estado === "en_curso" && (
              <Boton icono={<Pause className="size-4" />} cargando={accion === "pausar"} onClick={() => hacer("pausar", () => api.pausarLote(lote.id))}>
                Pausar
              </Boton>
            )}
            {puedeIniciar && (
              <Boton
                variante="primario"
                icono={<Play className="size-4" />}
                cargando={accion === "iniciar"}
                onClick={() => hacer("iniciar", () => api.iniciarLote(lote.id))}
              >
                {lote.estado === "listo" ? "Iniciar" : "Reanudar"}
              </Boton>
            )}
            {(lote.estado === "en_curso" || lote.estado === "pausado" || lote.estado === "listo") && (
              <Boton
                variante="peligro"
                icono={<Square className="size-4" />}
                cargando={accion === "cancelar"}
                onClick={() => (confirmarCancelar ? hacer("cancelar", () => api.cancelarLote(lote.id)) : setConfirmarCancelar(true))}
              >
                {confirmarCancelar ? "¿Cancelar el lote?" : "Cancelar"}
              </Boton>
            )}
          </>
        }
      />

      {lote.motivo_pausa && (
        <div className="mb-5 flex gap-3 rounded-xl bg-error-suave text-error px-4 py-3.5">
          <TriangleAlert className="size-5 shrink-0 mt-0.5" />
          <div className="text-sm">
            <p className="font-semibold">Lote en pausa</p>
            <p className="mt-0.5">{lote.motivo_pausa}</p>
            {p.inciertas > 0 && (
              <p className="mt-1 opacity-90">
                Revisa el saldo del chip de origen antes de reanudar: un envío incierto pudo haberse hecho o no.
              </p>
            )}
          </div>
        </div>
      )}
      {error && (
        <div className="mb-5">
          <Aviso>{error}</Aviso>
        </div>
      )}

      {hecho && (
        <div
          className={cx(
            "mb-5 flex gap-3 rounded-xl px-4 py-3.5",
            revision.completo ? "bg-ok-suave text-ok" : "bg-aviso-suave text-aviso",
          )}
        >
          {revision.completo ? (
            <CircleCheck className="size-5 shrink-0 mt-0.5" />
          ) : (
            <TriangleAlert className="size-5 shrink-0 mt-0.5" />
          )}
          <div className="text-sm flex-1">
            <p className="font-semibold">
              {revision.completo
                ? lote.simular
                  ? "Simulación completa: todas las filas llegaron a la confirmación"
                  : "Lote completo: todas las filas salieron OK"
                : "El lote terminó, pero no está completo"}
            </p>
            {!revision.completo && (
              <ul className="mt-1 list-disc pl-4">
                {revision.motivos.map((m) => (
                  <li key={m}>{m}</li>
                ))}
              </ul>
            )}
            {!revision.completo && fallidos > 0 && p.inciertas === 0 && (
              <p className="mt-1 opacity-90">
                «Reintentar fallidas» solo repite los envíos que fallaron sin mover dinero; luego pulsa Iniciar.
              </p>
            )}
          </div>
        </div>
      )}
      {puedeVerificar && (
        <Tarjeta className="p-5 mb-5">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div>
              <p className="font-semibold">Cuadrar saldos</p>
              <p className="text-xs text-tenue mt-0.5">
                Consulta el saldo real de los chips (solo lectura, ~4 s por chip) y lo compara con lo que el lote dice que
                envió. Solo vale si nadie usó esos chips por fuera.
              </p>
            </div>
            <Boton icono={<Scale className="size-4" />} cargando={verificando} onClick={verificar}>
              {verif ? "Verificar de nuevo" : "Verificar saldos"}
            </Boton>
          </div>
          {verif && (
            <div className="mt-4">
              <p className="text-xs text-tenue mb-2">Verificado {fechaHora(verif.verificado_ms)}</p>
              <Aviso tono={verif.confirmado ? "ok" : verif.cuadra ? "aviso" : "error"}>
                {verif.confirmado
                  ? "Confirmado: el lote está completo y los saldos cuadran."
                  : verif.cuadra
                    ? "Los saldos cuadran, pero el lote no está completo."
                    : verif.chips.length === 0
                      ? "No hay envíos ok que cuadrar."
                      : "Los saldos NO cuadran: revisa los chips marcados."}
              </Aviso>
              {verif.chips.length > 0 && (
                <div className="overflow-x-auto mt-3">
                  <table className="w-full text-sm">
                    <thead>
                      <tr className="text-left text-xs text-tenue border-b border-borde">
                        <th className="font-medium py-2 pr-3">Chip</th>
                        <th className="font-medium py-2 px-3 text-right">Saldo inicial</th>
                        <th className="font-medium py-2 px-3 text-right">Saldo ahora</th>
                        <th className="font-medium py-2 px-3 text-right">Descontado</th>
                        <th className="font-medium py-2 px-3 text-right">Esperado</th>
                        <th className="font-medium py-2 pl-3">Estado</th>
                      </tr>
                    </thead>
                    <tbody className="divide-y divide-borde">
                      {verif.chips.map((c) => (
                        <tr key={`${c.dispositivo}-${c.slot}`}>
                          <td className="py-2 pr-3 font-mono">
                            {c.numero?.replace(/^\+591/, "") ?? nombreChip(dispositivos, c.dispositivo, c.slot)}
                            <span className="text-xs text-tenue font-sans"> · {c.envios_ok} envíos</span>
                          </td>
                          <td className="py-2 px-3 text-right tabular-nums">{c.saldo_inicial == null ? "—" : bs(c.saldo_inicial)}</td>
                          <td className="py-2 px-3 text-right tabular-nums">{c.saldo_actual == null ? "—" : bs(c.saldo_actual)}</td>
                          <td className="py-2 px-3 text-right tabular-nums">{c.descontado_real == null ? "—" : bs(c.descontado_real)}</td>
                          <td className="py-2 px-3 text-right tabular-nums">{bs(c.descontado_esperado)}</td>
                          <td className="py-2 pl-3">
                            {c.estado === "cuadra" && <Insignia tono="ok">Cuadra</Insignia>}
                            {c.estado === "diferencia" && (
                              <Insignia tono="error">
                                {(c.diferencia_bs ?? 0) > 0 ? "+" : ""}
                                {c.diferencia_bs} Bs
                              </Insignia>
                            )}
                            {c.estado === "sin_dato" && <Insignia tono="aviso">Sin dato</Insignia>}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              )}
              {verif.avisos.length > 0 && <p className="mt-2 text-xs text-tenue">{verif.avisos.join(" · ")}</p>}
            </div>
          )}
        </Tarjeta>
      )}

      <Tarjeta className="p-5 mb-5">
        <div className="flex flex-wrap items-end justify-between gap-4 mb-3">
          <div>
            <p className="text-xs text-tenue font-medium">Progreso</p>
            <p className="text-2xl font-semibold tabular-nums tracking-tight">
              {terminadas} <span className="text-base font-normal text-tenue">/ {activas} filas</span>
            </p>
          </div>
          <div className="text-right">
            <p className="text-xs text-tenue font-medium">{lote.simular ? "Simulado" : "Enviado"}</p>
            <p className="text-2xl font-semibold tabular-nums tracking-tight">
              {bs(p.enviado_bs)} <span className="text-base font-normal text-tenue">/ {bs(p.total_bs)}</span>
            </p>
          </div>
        </div>
        <BarraProgreso p={p} alta />
        <div className="mt-4 grid grid-cols-3 sm:grid-cols-6 gap-3 text-sm">
          {(
            [
              ["OK", p.ok, "text-ok"],
              ["Simuladas", p.simulado, "text-info"],
              ["Parciales", p.parcial, "text-aviso"],
              ["Para mañana", p.sin_capacidad, "text-aviso"],
              ["Fallidas", p.fallidas, "text-error"],
              ["Inciertas", p.inciertas, "text-error"],
            ] as const
          ).map(([t, n, c]) => (
            <div key={t}>
              <p className="text-xs text-tenue">{t}</p>
              <p className={cx("font-semibold tabular-nums", n > 0 && c)}>{n}</p>
            </div>
          ))}
        </div>
        <p className="mt-3 text-xs text-tenue">
          Comisión {bs(p.comision_bs)} · {p.pendientes} pendientes · {p.omitidas} omitidas por error de validación
        </p>
      </Tarjeta>

      <Tarjeta className="overflow-hidden">
        <div className="flex flex-wrap gap-1 p-2 border-b border-borde">
          {etiquetas.map(([f, t]) => (
            <button
              key={f}
              onClick={() => setFiltro(f)}
              className={cx(
                "h-8 px-3 rounded-xl text-sm font-medium inline-flex items-center gap-2",
                filtro === f ? "bg-panel-2 text-texto" : "text-tenue hover:text-texto",
              )}
            >
              {t}
              <span className="text-xs tabular-nums opacity-70">{vistas[f].length}</span>
            </button>
          ))}
        </div>
        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead>
              <tr className="text-left text-xs text-tenue border-b border-borde">
                <th className="font-medium px-4 py-2.5 w-14">Fila</th>
                <th className="font-medium px-3 py-2.5">Número</th>
                <th className="font-medium px-3 py-2.5">Operador</th>
                <th className="font-medium px-3 py-2.5 text-right">Monto</th>
                <th className="font-medium px-3 py-2.5 text-right">Enviado</th>
                <th className="font-medium px-3 py-2.5">Envíos</th>
                <th className="font-medium px-3 py-2.5">Estado</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-borde">
              {vistas[filtro].map((f) => (
                <tr key={f.fila} className={cx(f.estado === "en_curso" && "bg-info-suave", f.estado === "incierta" && "bg-error-suave/50")}>
                  <td className="px-4 py-2.5 text-tenue tabular-nums align-top">{f.fila}</td>
                  <td className="px-3 py-2.5 font-mono align-top">
                    {f.numero ?? "—"}
                    {f.referencia && <div className="text-xs text-tenue font-sans">{f.referencia}</div>}
                  </td>
                  <td className="px-3 py-2.5 align-top">{f.operador ?? "—"}</td>
                  <td className="px-3 py-2.5 text-right tabular-nums align-top">{bs(f.monto)}</td>
                  <td className="px-3 py-2.5 text-right tabular-nums align-top">{bs(f.enviado_bs)}</td>
                  <td className="px-3 py-2.5 align-top">
                    <Envios f={f} dispositivos={dispositivos} />
                  </td>
                  <td className="px-3 py-2.5 align-top">
                    <EstadoFila estado={f.estado} />
                    {f.detalle && <div className="text-xs text-tenue mt-1 max-w-72">{f.detalle}</div>}
                  </td>
                </tr>
              ))}
              {vistas[filtro].length === 0 && (
                <tr>
                  <td colSpan={7} className="px-4 py-10 text-center text-tenue">
                    Nada en esta vista
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      </Tarjeta>
    </div>
  );
}
