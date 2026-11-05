import { useEffect, useMemo, useRef, useState } from "react";
import { BookOpen, CornerDownLeft, Hash, PhoneCall, X, Smartphone } from "../componentes/iconos";
import { api, mensajeError } from "../api/cliente";
import type { Dispositivo, ModoUssd } from "../api/tipos";
import { Boton, Campo, Insignia, Segmentado, Selector, Tarjeta, Titulo, Vacio, cx } from "../componentes/base";
import { ChipSim, simsConChip } from "../componentes/dispositivo";
import { etiquetaTelefono, hora, nombreSim, operadorDe, opcionesMenu, segundos } from "../util/datos";
import { DialogoDesdeSesion, type PasoSesion } from "./recetas/DesdeSesion";

type Mensaje =
  | { id: number; de: "yo"; texto: string; ts: number }
  | { id: number; de: "red"; texto: string; ts: number; ms: number; abierta: boolean }
  | { id: number; de: "sistema"; texto: string; ts: number; tono: "error" | "neutro" };

const ATAJOS = ["*105#", "*66#1#", "*133#"];

let contador = 0;
const nuevoId = () => ++contador;

export function ConsolaUssd({
  dispositivos,
  inicial,
}: {
  dispositivos: Dispositivo[];
  inicial?: { dispositivo: string; slot: number } | null;
}) {
  const conectados = dispositivos.filter((d) => d.conectado);
  const [dispId, setDispId] = useState<string>(inicial?.dispositivo ?? conectados[0]?.id ?? "");
  const [slot, setSlot] = useState<number>(inicial?.slot ?? 0);
  const [modo, setModo] = useState<ModoUssd>("pantalla");
  const [codigo, setCodigo] = useState("*105#");
  const [respuesta, setRespuesta] = useState("");
  const [mensajes, setMensajes] = useState<Mensaje[]>([]);
  const [sesion, setSesion] = useState<{ id: string; disp: string } | null>(null);
  const [ocupado, setOcupado] = useState(false);
  // La sesión grabada (solo en memoria) para "Guardar como receta": el código y cada respuesta con su pantalla.
  const [grabacion, setGrabacion] = useState<PasoSesion[]>([]);
  const [comoReceta, setComoReceta] = useState(false);
  const fin = useRef<HTMLDivElement>(null);
  const entrada = useRef<HTMLInputElement>(null);

  const disp = dispositivos.find((d) => d.id === dispId);

  // Si no hay selección válida, toma el primer teléfono en línea.
  useEffect(() => {
    if (!disp && conectados[0]) setDispId(conectados[0].id);
  }, [disp, conectados]);

  useEffect(() => {
    fin.current?.scrollIntoView({ behavior: "smooth", block: "end" });
  }, [mensajes]);

  // Al salir de la vista con una sesión abierta, se cierra para no dejar la ventana colgada en el teléfono.
  const sesionRef = useRef(sesion);
  sesionRef.current = sesion;
  useEffect(
    () => () => {
      const s = sesionRef.current;
      if (s) api.ussdCancelar(s.disp, s.id).catch(() => {});
    },
    [],
  );

  const ultimo = [...mensajes].reverse().find((m) => m.de === "red") as Extract<Mensaje, { de: "red" }> | undefined;
  const opciones = useMemo(() => (sesion && ultimo ? opcionesMenu(ultimo.texto) : []), [sesion, ultimo]);

  const agregar = (m: Mensaje) => setMensajes((l) => [...l, m]);

  const fallo = (e: unknown) => {
    agregar({
      id: nuevoId(),
      de: "sistema",
      texto: mensajeError(e),
      ts: Date.now(),
      tono: "error",
    });
    setSesion(null);
  };

  async function marcar() {
    if (!disp || !codigo.trim()) return;
    if (sesion) await cancelar(true);
    setOcupado(true);
    agregar({ id: nuevoId(), de: "yo", texto: codigo.trim(), ts: Date.now() });
    setGrabacion([]);
    try {
      const r = await api.ussdIniciar(disp.id, slot, codigo.trim(), modo);
      agregar({ id: nuevoId(), de: "red", texto: r.texto, ts: Date.now(), ms: r.ms, abierta: r.abierta });
      setGrabacion([{ enviado: codigo.trim(), texto: r.texto }]);
      setSesion(r.abierta ? { id: r.sesion, disp: disp.id } : null);
      if (r.abierta) setTimeout(() => entrada.current?.focus(), 50);
    } catch (e) {
      fallo(e);
    } finally {
      setOcupado(false);
    }
  }

  async function responder(texto: string) {
    if (!sesion || !texto.trim()) return;
    setOcupado(true);
    setRespuesta("");
    agregar({ id: nuevoId(), de: "yo", texto: texto.trim(), ts: Date.now() });
    try {
      const r = await api.ussdResponder(sesion.disp, sesion.id, texto.trim());
      agregar({ id: nuevoId(), de: "red", texto: r.texto, ts: Date.now(), ms: r.ms, abierta: r.abierta });
      setGrabacion((g) => (g.length ? [...g, { enviado: texto.trim(), texto: r.texto }] : g));
      if (!r.abierta) setSesion(null);
      else setTimeout(() => entrada.current?.focus(), 50);
    } catch (e) {
      fallo(e);
    } finally {
      setOcupado(false);
    }
  }

  async function cancelar(silencioso = false) {
    const s = sesion;
    if (!s) return;
    setSesion(null);
    try {
      await api.ussdCancelar(s.disp, s.id);
      if (!silencioso)
        agregar({ id: nuevoId(), de: "sistema", texto: "Sesión cancelada", ts: Date.now(), tono: "neutro" });
    } catch (e) {
      if (!silencioso) fallo(e);
    }
  }

  if (conectados.length === 0) {
    return (
      <div>
        <Titulo titulo="Consola USSD" />
        <Tarjeta>
          <Vacio
            icono={<Smartphone className="size-5" />}
            titulo="No hay teléfonos en línea"
            texto="Conecta un teléfono con la app agente para marcar códigos."
          />
        </Tarjeta>
      </div>
    );
  }

  return (
    <div className="h-full flex flex-col">
      <Titulo titulo="Consola USSD" sub="Marca un código y responde los menús como en el teléfono." />

      <div className="grid gap-4 lg:grid-cols-[320px_1fr] flex-1 min-h-0">
        {/* Panel de control */}
        <Tarjeta className="p-4 h-fit space-y-4">
          <Selector etiqueta="Teléfono" value={dispId} onChange={(e) => setDispId(e.target.value)} disabled={!!sesion}>
            {conectados.map((d) => (
              <option key={d.id} value={d.id}>
                {etiquetaTelefono(d)}
              </option>
            ))}
          </Selector>

          {disp && (
            <div>
              <span className="block text-xs font-medium text-tenue mb-1.5">Chip</span>
              <div className="grid gap-2">
                {simsConChip(disp).map((s) => (
                  <button
                    key={s.slot}
                    disabled={!!sesion}
                    onClick={() => setSlot(s.slot)}
                    className={cx(
                      "text-left rounded-xl ring-2 transition-shadow disabled:opacity-60",
                      slot === s.slot ? "ring-texto/70" : "ring-transparent hover:ring-borde",
                    )}
                  >
                    <ChipSim sim={s} />
                  </button>
                ))}
              </div>
            </div>
          )}

          <div>
            <span id="modo-ussd" className="block text-xs font-medium text-tenue mb-1.5">Modo</span>
            <Segmentado
              etiqueta="Modo"
              valor={modo}
              onCambio={setModo}
              deshabilitado={!!sesion}
              className="w-full grid grid-cols-2"
              opciones={[
                { id: "pantalla", texto: "Menús" },
                { id: "api", texto: "Un paso" },
              ]}
            />
            <p className="mt-1.5 text-xs text-tenue">
              {modo === "pantalla"
                ? "Lee la ventana del teléfono: permite contestar menús."
                : "Más rápido y sin ventana, pero no permite contestar."}
            </p>
          </div>

          <form
            onSubmit={(e) => {
              e.preventDefault();
              marcar();
            }}
            className="space-y-2"
          >
            <Campo
              etiqueta="Código"
              value={codigo}
              onChange={(e) => setCodigo(e.target.value)}
              className="font-mono"
              placeholder="*105#"
            />
            <div className="flex flex-wrap gap-1.5">
              {ATAJOS.map((a) => (
                <button
                  type="button"
                  key={a}
                  onClick={() => setCodigo(a)}
                  className="h-7 px-2 rounded-md bg-panel-2 text-xs font-mono text-tenue hover:text-texto"
                >
                  {a}
                </button>
              ))}
            </div>
            <Boton
              type="submit"
              variante="primario"
              className="w-full"
              cargando={ocupado && !sesion}
              icono={<PhoneCall className="size-4" />}
              disabled={!disp || ocupado}
            >
              Marcar
            </Boton>
          </form>
        </Tarjeta>

        {/* Conversación */}
        <Tarjeta className="flex flex-col min-h-[460px] overflow-hidden">
          <div className="flex flex-wrap items-center justify-between gap-2 px-4 py-2 min-h-12 border-b border-borde">
            <div className="flex items-center gap-2 text-sm">
              <Hash className="size-4 text-tenue" />
              <span className="font-medium">{disp?.modelo}</span>
              <span className="text-tenue">· {nombreSim(slot)}</span>
            </div>
            <div className="flex items-center gap-2">
              {sesion ? <Insignia tono="info">Sesión abierta</Insignia> : <Insignia>Sin sesión</Insignia>}
              {grabacion.length > 0 && (
                <Boton variante="fantasma" className="h-7 px-2" icono={<BookOpen className="size-4" />} onClick={() => setComoReceta(true)} title="Armar una receta con esta sesión">
                  <span className="hidden sm:inline">Guardar como receta</span>
                  <span className="sm:hidden">Receta</span>
                </Boton>
              )}
              {mensajes.length > 0 && !sesion && (
                <Boton
                  variante="fantasma"
                  className="h-7 px-2"
                  onClick={() => {
                    setMensajes([]);
                    setGrabacion([]);
                  }}
                >
                  Limpiar
                </Boton>
              )}
            </div>
          </div>

          <div className="flex-1 overflow-y-auto px-4 py-5 space-y-3">
            {mensajes.length === 0 && (
              <p className="text-sm text-tenue text-center mt-16">Las respuestas del operador aparecen aquí.</p>
            )}
            {mensajes.map((m) =>
              m.de === "yo" ? (
                <div key={m.id} className="flex justify-end aparecer">
                  <div className="max-w-[75%] rounded-2xl rounded-br-md bg-acento text-sobre-acento px-3.5 py-2 font-mono text-sm">
                    {m.texto}
                  </div>
                </div>
              ) : m.de === "red" ? (
                <div key={m.id} className="flex flex-col items-start aparecer">
                  <div className="max-w-[85%] rounded-2xl rounded-bl-md bg-panel-2 px-4 py-3 font-mono text-[13px] leading-relaxed whitespace-pre-wrap">
                    {m.texto || <span className="text-tenue">(respuesta vacía)</span>}
                  </div>
                  <span className="mt-1 ml-1 text-[11px] text-tenue">
                    {hora(m.ts)} · {segundos(m.ms)}
                    {!m.abierta && " · sesión terminada"}
                  </span>
                </div>
              ) : (
                <div key={m.id} className="flex justify-center aparecer">
                  <span
                    className={cx(
                      "text-xs rounded-full px-3 py-1",
                      m.tono === "error" ? "bg-error-suave text-error" : "bg-panel-2 text-tenue",
                    )}
                  >
                    {m.texto}
                  </span>
                </div>
              ),
            )}
            {ocupado && (
              <div className="flex items-center gap-1.5 pl-1 text-tenue" aria-label="Esperando respuesta">
                {[0, 1, 2].map((i) => (
                  <span
                    key={i}
                    className="size-1.5 rounded-full bg-current animate-bounce"
                    style={{ animationDelay: `${i * 120}ms` }}
                  />
                ))}
              </div>
            )}
            <div ref={fin} />
          </div>

          {sesion && (
            <div className="border-t border-borde p-3 space-y-2.5">
              {opciones.length > 0 && (
                <div className="flex flex-wrap gap-1.5">
                  {opciones.map((o) => (
                    <button
                      key={o.numero}
                      disabled={ocupado}
                      onClick={() => responder(o.numero)}
                      className="inline-flex items-center gap-1.5 h-8 pl-1.5 pr-3 rounded-xl border border-borde bg-panel text-sm hover:border-tenue/60 hover:bg-panel-2 transition-colors disabled:opacity-50"
                    >
                      <span className="grid place-items-center size-5 rounded-md bg-panel-2 text-[11px] font-semibold">
                        {o.numero}
                      </span>
                      {o.texto}
                    </button>
                  ))}
                </div>
              )}
              <form
                className="flex gap-2"
                onSubmit={(e) => {
                  e.preventDefault();
                  responder(respuesta);
                }}
              >
                <Campo
                  ref={entrada}
                  value={respuesta}
                  onChange={(e) => setRespuesta(e.target.value)}
                  placeholder="Escribe la respuesta…"
                  className="font-mono"
                  disabled={ocupado}
                  autoFocus
                />
                <Boton type="submit" variante="primario" disabled={ocupado || !respuesta.trim()} icono={<CornerDownLeft className="size-4" />}>
                  Enviar
                </Boton>
                <Boton type="button" variante="peligro" onClick={() => cancelar()} icono={<X className="size-4" />}>
                  Cancelar
                </Boton>
              </form>
            </div>
          )}
        </Tarjeta>
      </div>
      <DialogoDesdeSesion
        abierto={comoReceta}
        cerrar={() => setComoReceta(false)}
        pasos={grabacion}
        operadorInicial={(() => {
          const s = disp?.sims.find((x) => x.slot === slot);
          return s ? operadorDe(s) : "";
        })()}
      />
    </div>
  );
}
