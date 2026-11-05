import { Fragment, type ReactNode } from "react";
import { Wallet } from "../../componentes/iconos";
import { Aviso, Insignia, Tarjeta, cx } from "../../componentes/base";
import { AYUDA_ACCION, AYUDA_MODIFICADOR } from "./comunes";
import { MODIFICADORES, leer, type PasoModelo } from "./modelo";

/** Marca los `{param}` dentro de un valor para que se lean de un vistazo. */
export function ConParametros({ texto }: { texto: string }) {
  const partes = texto.split(/(\{[a-z_][a-z0-9_]*\})/gi);
  return (
    <>
      {partes.map((p, i) =>
        /^\{[a-z_][a-z0-9_]*\}$/i.test(p) ? (
          <span key={i} className="rounded bg-info-suave text-info px-1">
            {p}
          </span>
        ) : (
          <Fragment key={i}>{p}</Fragment>
        ),
      )}
    </>
  );
}

const Mono = ({ children }: { children: ReactNode }) => <span className="font-mono text-[12.5px] break-all">{children}</span>;

function TarjetaPasoLectura({ p, n }: { p: PasoModelo; n: number }) {
  const dinero = p.accion === "confirmar" || p.mueve_dinero;
  const ayuda = p.accion ? AYUDA_ACCION[p.accion] : null;
  return (
    <li
      id={`lectura-paso-${n}`}
      className={cx("relative rounded-2xl border px-4 py-3.5 bg-panel", dinero ? "border-aviso/50 bg-aviso-suave/40" : "border-borde/60")}
    >
      <div className="flex flex-wrap items-center gap-2">
        <span className="size-6 shrink-0 rounded-full bg-panel-2 text-xs font-bold grid place-items-center tabular-nums">{n}</span>
        <span className="text-xs font-semibold uppercase tracking-wide text-tenue" title={ayuda?.ayuda}>
          {ayuda?.titulo ?? "¿acción?"}
        </span>
        {dinero && (
          <Insignia tono="aviso">
            <Wallet className="size-3.5" /> Mueve dinero
          </Insignia>
        )}
      </div>
      <p className="mt-2 rounded-xl bg-panel-2 px-3 py-2">
        <Mono>
          <ConParametros texto={p.valor || "—"} />
        </Mono>
      </p>
      {MODIFICADORES.some((m) => p[m] != null) && (
        <dl className="mt-2.5 grid grid-cols-[auto_minmax(0,1fr)] gap-x-4 gap-y-1 text-sm">
          {MODIFICADORES.filter((m) => p[m] != null).map((m) => (
            <div key={m} className="contents">
              <dt className={cx("text-tenue", m === "error_si" && "text-error/90", m === "exito" && "text-ok/90")}>{AYUDA_MODIFICADOR[m].titulo}</dt>
              <dd>
                <Mono>{m === "timeout_ms" ? `${p[m]} ms` : <ConParametros texto={String(p[m])} />}</Mono>
              </dd>
            </div>
          ))}
        </dl>
      )}
    </li>
  );
}

/** Vista de solo lectura de la receta: parámetros, pasos en orden (el que mueve dinero resaltado), salida y límites. */
export function PasosLectura({ yaml, avisos }: { yaml: string; avisos?: string[] }) {
  const l = leer(yaml);
  if (!l.ok) return <Aviso>No pude leer el YAML: {l.error}</Aviso>;
  const m = l.modelo;
  return (
    <div className="grid gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(0,300px)] items-start">
      <div className="space-y-3 min-w-0">
        {avisos && avisos.length > 0 && (
          <Aviso tono="aviso" titulo="Avisos de la receta">
            <ul className="list-disc pl-4 space-y-0.5">
              {avisos.map((a, i) => (
                <li key={i}>{a}</li>
              ))}
            </ul>
          </Aviso>
        )}
        <ol className="space-y-2.5">
          {m.pasos.map((p, i) => (
            <TarjetaPasoLectura key={i} p={p} n={i + 1} />
          ))}
        </ol>
      </div>
      <div className="space-y-3 min-w-0">
        <Tarjeta className="p-4">
          <h3 className="text-xs font-semibold uppercase tracking-[0.14em] text-tenue">Parámetros</h3>
          {m.parametros.length === 0 ? (
            <p className="text-sm text-tenue mt-2">Ninguno.</p>
          ) : (
            <ul className="mt-2.5 space-y-2.5">
              {m.parametros.map((p) => (
                <li key={p.nombre} className="text-sm">
                  <div className="flex flex-wrap items-center gap-1.5">
                    <span className="font-mono text-info">{`{${p.nombre}}`}</span>
                    {p.secreto && <Insignia>Secreto</Insignia>}
                  </div>
                  {p.descripcion && <p className="text-xs text-tenue mt-0.5">{p.descripcion}</p>}
                  {p.opciones?.length ? (
                    <div className="mt-1 flex flex-wrap gap-1">
                      {p.opciones.map((o) => (
                        <span key={o} className="rounded-md bg-panel-2 px-1.5 py-0.5 font-mono text-xs">
                          {o}
                        </span>
                      ))}
                    </div>
                  ) : null}
                  {p.patron && <p className="mt-1 text-xs"><Mono>{p.patron}</Mono></p>}
                </li>
              ))}
            </ul>
          )}
        </Tarjeta>
        <Tarjeta className="p-4">
          <h3 className="text-xs font-semibold uppercase tracking-[0.14em] text-tenue">Salida</h3>
          {m.salida.length === 0 ? (
            <p className="text-sm text-tenue mt-2">No extrae datos.</p>
          ) : (
            <dl className="mt-2.5 space-y-2 text-sm">
              {m.salida.map(([k, v]) => (
                <div key={k}>
                  <dt className="font-medium">{k}</dt>
                  <dd className="text-xs text-tenue">
                    <Mono>{v}</Mono>
                  </dd>
                </div>
              ))}
            </dl>
          )}
        </Tarjeta>
        <Tarjeta className="p-4">
          <h3 className="text-xs font-semibold uppercase tracking-[0.14em] text-tenue">Para lotes</h3>
          <dl className="mt-2.5 grid grid-cols-[auto_1fr] gap-x-4 gap-y-1.5 text-sm">
            <dt className="text-tenue">Acción</dt>
            <dd className="font-mono">{m.accion || "—"}</dd>
            <dt className="text-tenue">Chip</dt>
            <dd>{m.chip_tipo === "dealer" ? "Solo dealer" : "Todos"}</dd>
            <dt className="text-tenue">Por día</dt>
            <dd className="tabular-nums">{m.limites.por_dia ?? "sin límite"}</dd>
            <dt className="text-tenue">Comisión</dt>
            <dd className="tabular-nums">{m.limites.comision != null ? `${m.limites.comision} Bs` : "—"}</dd>
            <dt className="text-tenue">Modo</dt>
            <dd>{m.modo === "api" ? "Un paso (API)" : "Menús (pantalla)"}</dd>
          </dl>
        </Tarjeta>
      </div>
    </div>
  );
}
