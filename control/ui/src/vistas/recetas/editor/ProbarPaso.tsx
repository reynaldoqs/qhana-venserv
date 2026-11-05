import { useEffect, useState } from "react";
import { api, mensajeError } from "../../../api/cliente";
import type { PruebaPaso } from "../../../api/tipos";
import { Aviso, Boton, Insignia, Selector, cx } from "../../../componentes/base";
import { CheckCircle2, CircleAlert, TestTube } from "../../../componentes/iconos";
import { fechaHora } from "../../../util/datos";
import { estadoUi } from "../comunes";
import type { PasoModelo } from "../modelo";
import { AreaTexto, useEditor, useIdCampo } from "./campos";

/** Pasos que eligen del menú: se evalúan contra la pantalla de antes (el menú) y la de después. `verificar` mira una sola. */
const CON_ANTES = new Set(["elegir", "confirmar"]);

function Chequeo({ nombre, valor, bueno = true }: { nombre: string; valor: boolean | null; bueno?: boolean }) {
  if (valor == null) return null;
  // `error_si` que coincide es malo; `esperar`/`exito`/`verificar` que coinciden son buenos.
  const ok = valor === bueno;
  return (
    <li className="flex items-center gap-2">
      {ok ? <CheckCircle2 className="size-4 text-ok" /> : <CircleAlert className="size-4 text-error" />}
      <span className="text-tenue">{nombre}</span>
      <span className={ok ? "text-ok" : "text-error"}>{valor ? "coincide" : "no coincide"}</span>
    </li>
  );
}

/**
 * Prueba un paso contra una pantalla (pegada o tomada de una ejecución reciente) con la misma lógica del motor,
 * sin marcar nada.
 */
export function ProbarPaso({ i, paso }: { i: number; paso: PasoModelo }) {
  const { yaml, ultimas, modelo } = useEditor();
  const id = useIdCampo(`probar${i}`);
  const [ejec, setEjec] = useState(ultimas.length ? "0" : "");
  const [texto, setTexto] = useState("");
  const [antes, setAntes] = useState("");
  const [valores, setValores] = useState<Record<string, string>>({});
  const [res, setRes] = useState<PruebaPaso | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [probando, setProbando] = useState(false);
  const conAntes = paso.accion ? CON_ANTES.has(paso.accion) : false;

  // Precarga las pantallas de la ejecución elegida: la de este paso y la del anterior.
  useEffect(() => {
    const e = ejec === "" ? null : ultimas[Number(ejec)];
    if (!e) return;
    setTexto(e.pasos[i]?.texto ?? "");
    setAntes(i > 0 ? (e.pasos[i - 1]?.texto ?? "") : "");
    setRes(null);
  }, [ejec, i, ultimas]);

  async function probar() {
    setProbando(true);
    setError(null);
    try {
      const params = Object.fromEntries(Object.entries(valores).filter(([, v]) => v.trim()));
      setRes(await api.probarPaso({ yaml, paso: i + 1, texto, texto_antes: conAntes && antes.trim() ? antes : null, parametros: params }));
    } catch (e) {
      setError(mensajeError(e));
      setRes(null);
    } finally {
      setProbando(false);
    }
  }

  const visibles = modelo.parametros.filter((p) => !p.secreto);
  const tono = res?.resultado === "exito" ? "ok" : res?.resultado === "abortaria" ? "aviso" : res?.resultado === "incierto" ? "error" : "info";

  return (
    <div className="mt-3 rounded-2xl bg-panel-2/50 border border-borde/60 p-3.5 space-y-3 aparecer">
      {ultimas.length > 0 && (
        <Selector etiqueta="Pantallas de una ejecución reciente" value={ejec} onChange={(e) => setEjec(e.target.value)}>
          <option value="">Escribir la pantalla a mano</option>
          {ultimas.map((u, k) => (
            <option key={u.id} value={k}>
              {fechaHora(u.inicio_ms)} · {(estadoUi[u.estado] ?? estadoUi.error).corto}
              {u.pasos[i] ? "" : " · no llegó a este paso"}
            </option>
          ))}
        </Selector>
      )}
      <div className={cx("grid gap-3", conAntes && "md:grid-cols-2")}>
        {conAntes && (
          <label className="block" htmlFor={`${id}-antes`}>
            <span className="block text-xs font-medium text-tenue mb-1.5">Menú antes del paso</span>
            <AreaTexto id={`${id}-antes`} rows={5} value={antes} onChange={(e) => setAntes(e.target.value)} className="font-mono text-[12.5px]" placeholder={"1.-Recarga Tigo\n2.-Paquetes"} />
          </label>
        )}
        <label className="block" htmlFor={`${id}-texto`}>
          <span className="block text-xs font-medium text-tenue mb-1.5">
            {paso.accion === "verificar" ? "Pantalla que se verifica" : "Pantalla que llegó después"}
          </span>
          <AreaTexto
            id={`${id}-texto`}
            rows={5}
            value={texto}
            onChange={(e) => setTexto(e.target.value)}
            className="font-mono text-[12.5px]"
            placeholder="Pega aquí la respuesta del operador"
          />
        </label>
      </div>
      {visibles.length > 0 && (
        <div className="flex flex-wrap gap-2">
          {visibles.map((p) => (
            <label key={p.nombre} className="flex items-center gap-1.5 text-xs">
              <span className="font-mono text-info">{p.nombre}</span>
              <input
                value={valores[p.nombre] ?? ""}
                onChange={(e) => setValores((v) => ({ ...v, [p.nombre]: e.target.value }))}
                placeholder="ejemplo"
                className="h-8 w-28 rounded-lg border border-borde bg-panel px-2 font-mono text-xs outline-none focus:border-acento"
              />
            </label>
          ))}
        </div>
      )}
      <div className="flex flex-wrap items-center gap-2">
        <Boton icono={<TestTube className="size-4" />} cargando={probando} onClick={probar} disabled={!texto.trim() && !antes.trim()}>
          Probar
        </Boton>
        <span className="text-xs text-tenue">No marca nada: evalúa el paso con la lógica del motor.</span>
      </div>
      {error && <Aviso>{error}</Aviso>}
      {res && (
        <div className="rounded-xl bg-panel border border-borde/60 p-3.5 text-sm space-y-2.5 aparecer" aria-live="polite">
          <div className="flex flex-wrap items-center gap-2">
            <Insignia tono={tono}>
              {res.resultado === "exito"
                ? "Terminaría con éxito"
                : res.resultado === "abortaria"
                  ? "Abortaría"
                  : res.resultado === "incierto"
                    ? "Quedaría incierta"
                    : "Seguiría"}
            </Insignia>
            {res.motivo && <span className="text-xs text-tenue break-words">{res.motivo}</span>}
          </div>
          {res.accion !== "verificar" && res.accion !== "marcar" && (
            <p>
              <span className="text-tenue">Enviaría: </span>
              {res.enviaria != null ? (
                <span className="font-mono font-semibold rounded bg-panel-2 px-1.5">{res.enviaria}</span>
              ) : (
                <span className="text-error">ninguna opción coincide</span>
              )}
            </p>
          )}
          {res.opciones && res.opciones.length > 0 && (
            <ul className="space-y-0.5 font-mono text-[12.5px]">
              {res.opciones.map((o) => (
                <li key={o.numero} className={cx("rounded-md px-2 py-0.5", o.coincide ? "bg-ok-suave text-ok font-semibold" : "text-tenue")}>
                  {o.numero}. {o.texto}
                  {o.coincide && " ←"}
                </li>
              ))}
            </ul>
          )}
          <ul className="space-y-1 text-[13px]">
            <Chequeo nombre="verificar" valor={res.verificar} />
            <Chequeo nombre="esperar" valor={res.esperar} />
            <Chequeo nombre="error si" valor={res.error_si} bueno={false} />
            <Chequeo nombre="éxito" valor={res.exito} />
          </ul>
          {res.parametros && Object.keys(res.parametros).length > 0 && (
            <p className="text-xs text-tenue">
              Con{" "}
              {Object.entries(res.parametros).map(([k, v], n) => (
                <span key={k}>
                  {n > 0 && ", "}
                  <span className="font-mono text-info">{k}</span> = <span className="font-mono text-texto">{v}</span>
                </span>
              ))}
            </p>
          )}
          {res.salida && Object.keys(res.salida).length > 0 && (
            <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-0.5 text-[13px] border-t border-borde/60 pt-2">
              {Object.entries(res.salida).map(([k, v]) => (
                <div key={k} className="contents">
                  <dt className="text-tenue">{k}</dt>
                  <dd className="font-mono break-all">{v}</dd>
                </div>
              ))}
            </dl>
          )}
        </div>
      )}
    </div>
  );
}
