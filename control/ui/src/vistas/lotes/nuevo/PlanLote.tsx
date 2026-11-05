import { useMemo } from "react";
import type { ChipInfo, Dispositivo, ResumenOperador, Validacion } from "../../../api/tipos";
import { Insignia, Tarjeta, cx } from "../../../componentes/base";
import { Smartphone } from "../../../componentes/iconos";
import { etiquetaTelefono, hace, nombreSim, sinPrefijo } from "../../../util/datos";
import { EnlaceManual, bs, chipsDeOrigen } from "../comunes";

export interface ChipPlan {
  clave: string;
  dispositivo: string;
  slot: number;
  operador: string;
  numero: string | null;
  dealer: boolean;
  receta: string | null;
  envios: number;
  bs: number;
  /** El chip en la validación (datos generales: tipo, saldo…). */
  info?: ChipInfo;
  /** Sus entradas en `Validacion.chips`: una por acción del lote que hace (cupo de esa acción). */
  infos: ChipInfo[];
}

export interface OperadorPlan {
  operador: string;
  chips: ChipPlan[];
  envios: number;
  bs: number;
  sinCapacidad: number;
  motivos: [string, number][];
  /** Una entrada por acción de sus filas (cupo de esa acción; saldo principal solo en `transferir`). */
  resumenes: ResumenOperador[];
}

export interface Plan {
  operadores: OperadorPlan[];
  /** Teléfonos distintos con algún envío: los que trabajan en paralelo. */
  telefonos: number;
  chipsConEnvios: number;
}

/** Agrupa el plan de la validación: operador → chips, con envíos y Bs por chip y lo que no entra (con su motivo). */
export function armarPlan(v: Validacion): Plan {
  const ops = new Map<string, OperadorPlan & { _m: Map<string, number>; _c: Map<string, ChipPlan> }>();
  const origen = chipsDeOrigen(v.origen);
  const op = (nombre: string) => {
    let o = ops.get(nombre);
    if (!o) {
      o = { operador: nombre, chips: [], envios: 0, bs: 0, sinCapacidad: 0, motivos: [], resumenes: [], _m: new Map(), _c: new Map() };
      ops.set(nombre, o);
    }
    return o;
  };
  const chip = (o: ReturnType<typeof op>, dispositivo: string, slot: number) => {
    const clave = `${dispositivo}|${slot}`;
    let c = o._c.get(clave);
    if (!c) {
      const infos = v.chips.filter((x) => x.dispositivo === dispositivo && x.slot === slot);
      const info = infos[0];
      const ori = origen.find((x) => x.dispositivo === dispositivo && x.slot === slot);
      c = {
        clave,
        dispositivo,
        slot,
        operador: info?.operador ?? ori?.operador ?? o.operador,
        numero: info?.numero ?? ori?.numero ?? null,
        dealer: (info?.tipo ?? ori?.tipo) === "dealer",
        receta: ori?.receta ?? null,
        envios: 0,
        bs: 0,
        info,
        infos,
      };
      o._c.set(clave, c);
    }
    return c;
  };

  for (const f of v.filas) {
    if (f.estado === "invalida") continue;
    const o = op(f.operador ?? "—");
    for (const p of f.plan) {
      const c = chip(o, p.dispositivo, p.slot);
      c.envios++;
      c.bs += p.monto;
      o.envios++;
      o.bs += p.monto;
    }
    if (f.sin_capacidad > 0) {
      o.sinCapacidad += f.sin_capacidad;
      const m = f.motivo_sin_capacidad || "sin cupo o saldo";
      o._m.set(m, (o._m.get(m) ?? 0) + f.sin_capacidad);
    }
  }
  // Chips que podían entrar y no reciben nada: del origen elegido, o del operador (para ver que existen).
  for (const c of origen) {
    const nombre = c.operador ?? v.chips.find((x) => x.dispositivo === c.dispositivo && x.slot === c.slot)?.operador;
    if (nombre && ops.has(nombre)) chip(op(nombre), c.dispositivo, c.slot);
  }
  if (origen.length === 0) for (const c of v.chips) if (ops.has(c.operador)) chip(op(c.operador), c.dispositivo, c.slot);

  const operadores = [...ops.values()]
    .map(({ _m, _c, ...o }) => ({
      ...o,
      chips: [..._c.values()].sort((a, b) => b.envios - a.envios || a.dispositivo.localeCompare(b.dispositivo) || a.slot - b.slot),
      motivos: [..._m.entries()].sort((a, b) => b[1] - a[1]),
      bs: Math.round(o.bs * 100) / 100,
      resumenes: v.resumen.operadores.filter((r) => r.operador === o.operador),
    }))
    .sort((a, b) => b.envios - a.envios);
  const conEnvios = operadores.flatMap((o) => o.chips).filter((c) => c.envios > 0);
  return { operadores, telefonos: new Set(conEnvios.map((c) => c.dispositivo)).size, chipsConEnvios: conEnvios.length };
}

/** Barra "tengo X de lo que necesito". */
function Medidor({ etiqueta, tiene, necesita, formato }: { etiqueta: string; tiene: number; necesita: number; formato: (n: number) => string }) {
  const ok = tiene >= necesita;
  const pct = necesita > 0 ? Math.min(100, (tiene / necesita) * 100) : 100;
  return (
    <div>
      <div className="flex justify-between gap-3 text-xs mb-1.5">
        <span className="text-tenue">{etiqueta}</span>
        <span className={cx("font-medium tabular-nums", ok ? "text-ok" : "text-aviso")}>
          {formato(tiene)} / {formato(necesita)}
        </span>
      </div>
      <div className="h-1.5 rounded-full bg-panel-2 overflow-hidden">
        <div className={cx("h-full rounded-full", ok ? "bg-ok" : "bg-aviso")} style={{ width: `${pct}%` }} />
      </div>
    </div>
  );
}

const plural = (n: number, uno: string, varios: string) => `${n} ${n === 1 ? uno : varios}`;

/** Barra neutra llena: la capacidad no tiene tope (sin límite diario). */
function MedidorSinLimite({ etiqueta, necesita }: { etiqueta: string; necesita: number }) {
  return (
    <div>
      <div className="flex justify-between gap-3 text-xs mb-1.5">
        <span className="text-tenue">{etiqueta}</span>
        <span className="font-medium tabular-nums text-tenue">
          {plural(necesita, "envío", "envíos")} · sin límite diario
        </span>
      </div>
      <div className="h-1.5 rounded-full bg-panel-2 overflow-hidden">
        <div className="h-full w-full rounded-full bg-texto/30" />
      </div>
    </div>
  );
}

/** "N hoy / cupo M" del chip para una acción ("sin límite" si su receta no tiene `por_dia`). */
function CupoChip({ info, conAccion }: { info: ChipInfo; conAccion: boolean }) {
  const pre = conAccion && info.accion ? `${info.accion}: ` : "";
  if (info.agotado_hoy) return <span className="text-aviso">{pre}agotado hoy</span>;
  if (info.sin_limite)
    return (
      <span>
        {pre}
        {info.enviados_hoy} hoy · sin límite
      </span>
    );
  return (
    <span>
      {pre}
      {info.enviados_hoy} hoy / cupo {info.enviados_hoy + info.cupo_hoy}
    </span>
  );
}

/** La entrada del chip con su saldo principal (la de transferir, o la general): las demás acciones no lo usan. */
const saldoPrincipal = (c: ChipPlan) => c.infos.find((i) => i.accion === undefined || i.accion === "transferir");

/** Vista previa del plan: por operador, qué chip hace cuántos envíos y cuánto, y qué queda fuera. */
export function PlanLote({ plan, dispositivos }: { plan: Plan; dispositivos: Dispositivo[] }) {
  const max = useMemo(() => Math.max(1, ...plan.operadores.flatMap((o) => o.chips.map((c) => c.envios))), [plan]);
  if (plan.operadores.length === 0) return null;
  return (
    <section aria-labelledby="plan-titulo">
      <div className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1 mb-3 px-1">
        <h2 id="plan-titulo" className="text-base font-semibold">
          Plan de envío
        </h2>
        <p className="text-xs text-tenue">
          {plan.telefonos > 0 ? (
            <>
              {plural(plan.telefonos, "teléfono", "teléfonos")} en paralelo · {plural(plan.chipsConEnvios, "chip", "chips")} con envíos · un envío a
              la vez por teléfono
            </>
          ) : (
            "Ningún chip recibe envíos"
          )}
        </p>
      </div>
      <div className="grid gap-3 lg:grid-cols-2">
        {plan.operadores.map((o) => (
          <Tarjeta key={o.operador} className="p-4">
            <div className="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-1">
              <p className="font-semibold">{o.operador}</p>
              <p className="text-xs text-tenue tabular-nums">
                {plural(o.envios, "envío", "envíos")} · {bs(o.bs)}
              </p>
            </div>

            {o.resumenes.length > 0 && (
              <div className="mt-3 grid gap-3 sm:grid-cols-2">
                {o.resumenes.map((r) => {
                  const etiqueta = o.resumenes.length > 1 ? `Cupo de hoy (${r.accion})` : "Cupo de hoy (envíos)";
                  return (
                    <div key={r.accion} className="contents">
                      {r.cupo_sin_limite ? (
                        <MedidorSinLimite etiqueta={etiqueta} necesita={r.envios} />
                      ) : (
                        <Medidor etiqueta={etiqueta} tiene={r.cupo_hoy} necesita={r.envios} formato={(n) => String(n)} />
                      )}
                      {/* El saldo principal solo cuenta para transferir (las recargas gastan otro saldo). */}
                      {r.accion === "transferir" && (
                        <Medidor etiqueta="Saldo conocido" tiene={r.saldo_conocido} necesita={r.necesario_bs} formato={(n) => bs(n)} />
                      )}
                    </div>
                  );
                })}
              </div>
            )}

            <ul className="mt-3 divide-y divide-borde/60">
              {o.chips.map((c) => {
                const d = dispositivos.find((x) => x.id === c.dispositivo);
                const sp = saldoPrincipal(c);
                return (
                  <li key={c.clave} className={cx("py-2.5", c.envios === 0 && "opacity-60")}>
                    <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
                      <span className="font-mono text-sm">{sinPrefijo(c.numero) ?? nombreSim(c.slot)}</span>
                      {c.dealer && <Insignia tono="info">Dealer</Insignia>}
                      <span className="text-xs text-tenue inline-flex items-center gap-1 min-w-0">
                        <Smartphone className="size-3.5 shrink-0" />
                        <span className="truncate">{d ? etiquetaTelefono(d) : c.dispositivo.slice(0, 8)}</span>
                      </span>
                      <span className="ml-auto text-sm tabular-nums whitespace-nowrap">
                        {c.envios > 0 ? (
                          <>
                            <b className="font-semibold">{plural(c.envios, "envío", "envíos")}</b>
                            <span className="text-tenue"> · {bs(c.bs)}</span>
                          </>
                        ) : (
                          <span className="text-tenue">sin envíos</span>
                        )}
                      </span>
                    </div>
                    <div className="mt-1.5 h-1 rounded-full bg-panel-2 overflow-hidden">
                      <div className="h-full rounded-full bg-texto/70" style={{ width: `${(c.envios / max) * 100}%` }} />
                    </div>
                    <p className="mt-1 text-[11px] text-tenue flex flex-wrap gap-x-3 tabular-nums">
                      {c.receta && <span className="font-mono">{c.receta}</span>}
                      {sp && <span title={sp.saldo_ms ? `Consultado ${hace(sp.saldo_ms)}` : "Sin consultar"}>saldo {bs(sp.saldo)}</span>}
                      {c.info?.tipo === "dealer" && c.info.saldo_dealer != null && <span>saldo dealer {bs(c.info.saldo_dealer)}</span>}
                      {/* Sin `accion`: el chip no hace nada del lote; su cupo de transferir solo importa si hay transferencias. */}
                      {c.infos
                        .filter((i) => i.accion !== undefined || o.resumenes.some((r) => r.accion === "transferir"))
                        .map((i) => (
                          <CupoChip key={i.accion ?? "-"} info={i} conAccion={c.infos.length > 1} />
                        ))}
                      {c.info?.sin_verificar && <span className="text-aviso">sin verificar</span>}
                    </p>
                  </li>
                );
              })}
            </ul>

            {o.sinCapacidad > 0 && (
              <div className="mt-2 rounded-xl bg-aviso-suave text-aviso px-3 py-2 text-xs">
                <b className="font-semibold">{plural(o.sinCapacidad, "envío queda", "envíos quedan")} fuera</b>
                {": "}
                {o.motivos.map(([m, n]) => `${m}${o.motivos.length > 1 ? ` (${n})` : ""}`).join(" · ")}. <EnlaceManual seccion="chips" />
              </div>
            )}
            {o.resumenes.map(
              (r) =>
                r.accion === "transferir" &&
                r.saldo_desconocido_chips > 0 && (
                  <p key={r.accion} className="mt-2 text-xs text-aviso">
                    {plural(r.saldo_desconocido_chips, "chip", "chips")} sin saldo conocido: activa «Consultar saldos».
                  </p>
                ),
            )}
            {o.chips.length === 0 && <p className="mt-2 text-xs text-error">No hay chips de {o.operador} que puedan hacer estas filas.</p>}
          </Tarjeta>
        ))}
      </div>
    </section>
  );
}
