import { useEffect, useState } from "react";
import { FlaskConical, Play } from "../../componentes/iconos";
import { api, mensajeError } from "../../api/cliente";
import type { ChipInfo, Dispositivo, ResumenReceta, Resultado } from "../../api/tipos";
import { Aviso, Boton, Campo, Casilla, Dialogo, Interruptor, Selector, Tarjeta, Vacio, cx } from "../../componentes/base";
import { simsConChip } from "../../componentes/dispositivo";
import { etiquetaOperador, etiquetaTelefono, nombreSim, operadorDe, segundos, sinPrefijo } from "../../util/datos";
import { PasosEjecucion, colorTono, estadoUi } from "./comunes";

/** El parámetro `pin` secreto se completa en el control con el PIN guardado del chip. */
const usaPinGuardado = (p: { nombre: string; secreto?: boolean }, chip: ChipInfo | null) =>
  p.secreto && p.nombre === "pin" && !!chip?.tiene_pin;

function validar(r: ResumenReceta, valores: Record<string, string>, chip: ChipInfo | null): string | null {
  for (const p of r.parametros) {
    if (usaPinGuardado(p, chip) && !valores[p.nombre]) continue;
    const v = (valores[p.nombre] ?? "").trim();
    if (!v) return `Falta ${p.nombre}`;
    if (p.opciones?.length && !p.opciones.includes(v)) return `${p.nombre} debe ser uno de: ${p.opciones.join(", ")}`;
    try {
      if (p.patron && !new RegExp(p.patron).test(v)) return `${p.nombre} no tiene el formato esperado`;
    } catch {
      /* patrón que el navegador no entiende: decide el control */
    }
  }
  return null;
}

/** El monto siempre va en Bs (formato base): la receta lo traduce a lo que pide su USSD. */
const etiquetaParametro = (p: { nombre: string; descripcion?: string | null }) =>
  p.nombre === "monto" && !/\bBs\b/.test(p.descripcion ?? "") ? `${p.descripcion ?? "Monto"} (en Bs)` : (p.descripcion ?? p.nombre);

export function VistaResultado({ r }: { r: Resultado }) {
  const e = estadoUi[r.estado] ?? estadoUi.error;
  const Icono = e.icono;
  const total = r.pasos.reduce((s, p) => s + (p.ms ?? 0), 0);
  return (
    <Tarjeta className="p-5 aparecer">
      <div className="flex items-center justify-between gap-3">
        <div className="flex items-center gap-2.5">
          <Icono className={cx("size-5", colorTono[e.tono])} />
          <span className="font-semibold">{e.texto}</span>
        </div>
        <span className="text-xs text-tenue tabular-nums">{segundos(total)}</span>
      </div>
      {r.detalle && <p className="mt-2 text-sm text-tenue">{r.detalle}</p>}

      {Object.keys(r.salida).length > 0 && (
        <dl className="mt-4 grid grid-cols-[auto_1fr] gap-x-6 gap-y-1.5 text-sm">
          {Object.entries(r.salida).map(([k, v]) => (
            <div key={k} className="contents">
              <dt className="text-tenue">{k.replace(/_/g, " ")}</dt>
              <dd className="font-mono font-medium break-all">{v}</dd>
            </div>
          ))}
        </dl>
      )}
      <div className="mt-5">
        <PasosEjecucion pasos={r.pasos} />
      </div>
    </Tarjeta>
  );
}

/** Panel para correr la receta (simulada o real) y ver el resultado. */
export function Ejecutar({ receta, dispositivos }: { receta: ResumenReceta; dispositivos: Dispositivo[] }) {
  const conectados = dispositivos.filter((d) => d.conectado);
  const [dispId, setDispId] = useState("");
  const [slot, setSlot] = useState(0);
  const [valores, setValores] = useState<Record<string, string>>({});
  const [simular, setSimular] = useState(true);
  const [confirmando, setConfirmando] = useState(false);
  const [errorChip, setErrorChip] = useState<string | null>(null);
  const [corriendo, setCorriendo] = useState(false);
  const [resultado, setResultado] = useState<Resultado | null>(null);
  const [fallo, setFallo] = useState<string | null>(null);
  const [chip, setChip] = useState<ChipInfo | null>(null);
  const [recordarPin, setRecordarPin] = useState(true);
  const [otroPin, setOtroPin] = useState(false);

  const disp = conectados.find((d) => d.id === dispId);

  useEffect(() => {
    if (!disp && conectados[0]) setDispId(conectados[0].id);
  }, [disp, conectados]);
  useEffect(() => {
    setValores({});
    setResultado(null);
    setFallo(null);
    setConfirmando(false);
    setSimular(true);
  }, [receta.nombre]);
  // Al cambiar de receta o teléfono, elige solo el primer chip del operador de la receta.
  useEffect(() => {
    if (!disp) return;
    const s = disp.sims.find((x) => operadorDe(x) === receta.operador.toUpperCase());
    if (s) setSlot(s.slot);
  }, [receta.nombre, disp?.id]); // eslint-disable-line react-hooks/exhaustive-deps

  // Si el operador del chip no coincide con el de la receta, se avisa (no se bloquea: el nombre de red puede variar).
  const sim = disp?.sims.find((s) => s.slot === slot);
  const operadorDistinto = sim && operadorDe(sim) !== receta.operador.toUpperCase();
  // El chip elegido (para saber si tiene PIN guardado).
  useEffect(() => {
    setChip(null);
    setOtroPin(false);
    setErrorChip(null);
    if (!dispId || !receta.parametros.some((p) => p.secreto)) return;
    api.chips().then(
      (cs) => setChip(cs.find((c) => c.dispositivo === dispId && c.slot === slot) ?? null),
      (e) => setErrorChip(mensajeError(e)),
    );
  }, [dispId, slot, receta.nombre]); // eslint-disable-line react-hooks/exhaustive-deps
  const chipPin = otroPin ? null : chip;
  const invalido = validar(receta, valores, chipPin);
  // Las recetas de solo consulta se ejecutan directo; simular solo tiene sentido si mueven saldo.
  const mueve = receta.mueve_dinero ?? true;
  const simulando = mueve && simular;

  async function ejecutar(confirmado = false) {
    if (!disp || invalido) return;
    if (mueve && !simular && !confirmado) {
      setConfirmando(true);
      return;
    }
    setConfirmando(false);
    setCorriendo(true);
    setResultado(null);
    setFallo(null);
    try {
      // Un PIN escrito a mano se guarda en el chip si se pidió, y deja de quedar en pantalla.
      if (valores.pin && recordarPin && receta.parametros.some((p) => p.secreto && p.nombre === "pin")) {
        setChip(await api.fijarPinChip(disp.id, slot, valores.pin));
        setOtroPin(false);
      }
      setResultado(await api.ejecutarReceta(receta.nombre, disp.id, slot, valores, simulando));
      setValores(({ pin: _pin, ...resto }) => resto);
    } catch (e) {
      setFallo(mensajeError(e));
    } finally {
      setCorriendo(false);
    }
  }

  const fijarValor = (n: string, v: string) => setValores((x) => ({ ...x, [n]: v }));

  return (
    <div className="grid gap-4 xl:grid-cols-[minmax(0,340px)_minmax(0,1fr)] items-start">
      <Tarjeta className="p-4 space-y-3.5">
        <Selector etiqueta="Teléfono" value={dispId} onChange={(e) => setDispId(e.target.value)}>
          {conectados.length === 0 && <option value="">Sin teléfonos en línea</option>}
          {conectados.map((d) => (
            <option key={d.id} value={d.id}>
              {etiquetaTelefono(d)}
            </option>
          ))}
        </Selector>
        {disp && (
          <Selector etiqueta="Chip" value={slot} onChange={(e) => setSlot(Number(e.target.value))}>
            {simsConChip(disp).map((s) => (
              <option key={s.slot} value={s.slot}>
                {nombreSim(s.slot)} · {etiquetaOperador(s)} {s.numero ? `· ${s.numero}` : ""}
              </option>
            ))}
          </Selector>
        )}

        {receta.parametros.map((p) =>
          p.secreto ? (
            usaPinGuardado(p, chipPin) ? (
              <div key={p.nombre} className="flex items-center justify-between gap-2 rounded-xl bg-panel-2 px-3.5 h-10 text-sm">
                <span>
                  {p.nombre.toUpperCase()} <span className="text-tenue">guardado en este chip</span>
                </span>
                <button type="button" className="text-xs text-tenue hover:text-texto" onClick={() => setOtroPin(true)}>
                  Usar otro
                </button>
              </div>
            ) : (
              <div key={p.nombre} className="space-y-2">
                <Campo
                  etiqueta={p.descripcion ?? p.nombre}
                  type="password"
                  inputMode="numeric"
                  autoComplete="off"
                  className="font-mono"
                  value={valores[p.nombre] ?? ""}
                  onChange={(e) => fijarValor(p.nombre, e.target.value)}
                />
                {p.nombre === "pin" && (
                  <label className="flex items-center gap-2 text-xs text-tenue">
                    <input type="checkbox" checked={recordarPin} onChange={(e) => setRecordarPin(e.target.checked)} />
                    Guardar el PIN en este chip
                  </label>
                )}
              </div>
            )
          ) : p.opciones?.length ? (
            <Selector
              key={p.nombre}
              etiqueta={etiquetaParametro(p)}
              value={valores[p.nombre] ?? ""}
              onChange={(e) => fijarValor(p.nombre, e.target.value)}
            >
              <option value="">Elegir…</option>
              {p.opciones.map((o) => (
                <option key={o} value={o}>
                  {p.nombre === "monto" ? `${o} Bs` : o}
                </option>
              ))}
            </Selector>
          ) : (
            <Campo
              key={p.nombre}
              etiqueta={etiquetaParametro(p)}
              className="font-mono"
              inputMode={p.nombre === "monto" ? "decimal" : undefined}
              placeholder={p.nombre === "monto" ? "0.50, 5, 12.30" : undefined}
              value={valores[p.nombre] ?? ""}
              onChange={(e) => fijarValor(p.nombre, e.target.value)}
            />
          ),
        )}

        {mueve && (
          <div className="pt-1">
            <Interruptor
              activo={simular}
              onCambio={setSimular}
              etiqueta={
                <span>
                  Simular <span className="text-tenue">(no confirma)</span>
                </span>
              }
            />
          </div>
        )}

        {operadorDistinto && (
          <Aviso tono="aviso">
            El chip es de {sim && etiquetaOperador(sim)} y la receta es para {receta.operador}.
          </Aviso>
        )}
        {mueve && !simular && (
          <Aviso tono="aviso">Ejecución real: puede mover saldo. El paso de confirmación nunca se reintenta.</Aviso>
        )}
        {mueve && !simular && receta.sin_probar && (
          <Aviso tono="aviso">Esta versión aún no se ejecutó bien (ni simulada). Conviene simularla primero.</Aviso>
        )}
        {errorChip && <Aviso tono="aviso">No pude leer el chip (PIN guardado): {errorChip}</Aviso>}

        <Boton
          variante="primario"
          className="w-full"
          onClick={() => ejecutar()}
          cargando={corriendo}
          disabled={!!invalido || !disp}
          icono={simulando ? <FlaskConical className="size-4" /> : <Play className="size-4" />}
          title={invalido ?? undefined}
        >
          {simulando ? "Simular" : mueve ? "Ejecutar real…" : "Ejecutar"}
        </Boton>
        {invalido && receta.parametros.length > 0 && Object.keys(valores).length > 0 && <p className="text-xs text-tenue">{invalido}</p>}
      </Tarjeta>

      <DialogoReal
        abierto={confirmando}
        cerrar={() => setConfirmando(false)}
        confirmar={() => ejecutar(true)}
        receta={receta}
        valores={valores}
        chip={disp && sim ? `${etiquetaTelefono(disp)} · ${nombreSim(slot)} · ${etiquetaOperador(sim)}${sim.numero ? ` · ${sinPrefijo(sim.numero)}` : ""}` : ""}
      />

      <div className="min-w-0" aria-live="polite">
        {fallo && <Aviso>{fallo}</Aviso>}
        {resultado ? (
          <VistaResultado r={resultado} />
        ) : (
          !fallo && (
            <Tarjeta>
              <Vacio
                icono={<Play className="size-5" />}
                titulo={corriendo ? "Ejecutando…" : "El resultado aparece aquí"}
                texto="Cada paso muestra lo que se envió, lo que respondió el operador y cuánto tardó."
              />
            </Tarjeta>
          )
        )}
      </div>
    </div>
  );
}

/** Confirmación de una receta que mueve saldo, en real: muestra qué se va a enviar y desde qué chip. */
function DialogoReal({
  abierto,
  cerrar,
  confirmar,
  receta,
  valores,
  chip,
}: {
  abierto: boolean;
  cerrar: () => void;
  confirmar: () => void;
  receta: ResumenReceta;
  valores: Record<string, string>;
  chip: string;
}) {
  const [entendido, setEntendido] = useState(false);
  useEffect(() => setEntendido(false), [abierto]);
  const visibles = receta.parametros.filter((p) => !p.secreto && valores[p.nombre]);
  return (
    <Dialogo
      abierto={abierto}
      cerrar={cerrar}
      peligro
      titulo={`Ejecutar «${receta.titulo}» en real`}
      acciones={
        <>
          <Boton variante="fantasma" data-autofocus onClick={cerrar}>
            Volver
          </Boton>
          <Boton variante="peligro" icono={<Play className="size-4" />} disabled={!entendido} onClick={confirmar}>
            Ejecutar real
          </Boton>
        </>
      }
    >
      <p>
        Esta receta <b>mueve saldo de verdad</b>. El paso de confirmación nunca se reintenta: si se corta después, queda incierta.
      </p>
      {receta.sin_probar && (
        <Aviso tono="aviso" titulo="Esta versión aún no se ha probado" className="mt-4">
          Ninguna ejecución de esta versión terminó bien, ni siquiera simulada. Lo seguro es volver y simularla primero.
        </Aviso>
      )}
      <dl className="mt-4 grid grid-cols-[auto_1fr] gap-x-6 gap-y-2 rounded-2xl bg-panel-2 px-4 py-3.5">
        <dt className="text-tenue">Chip</dt>
        <dd className="text-right">{chip}</dd>
        {visibles.map((p) => (
          <div key={p.nombre} className="contents">
            <dt className="text-tenue">{p.nombre}</dt>
            <dd className="text-right font-mono font-semibold">{valores[p.nombre]}</dd>
          </div>
        ))}
      </dl>
      <div className="mt-4">
        <Casilla marcada={entendido} onCambio={setEntendido}>
          {receta.sin_probar ? "Sé que no se ha probado y revisé el chip y los datos: quiero ejecutarla en real" : "Revisé el chip y los datos: quiero ejecutarla en real"}
        </Casilla>
      </div>
    </Dialogo>
  );
}
