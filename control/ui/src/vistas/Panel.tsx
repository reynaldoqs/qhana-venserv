import { useEffect, useState } from "react";
import type { ReactNode } from "react";
import { Cable, RefreshCw, Smartphone, Wallet, CheckCircle2, TriangleAlert, Activity } from "../componentes/iconos";
import { api, FalloApi, mensajeError } from "../api/cliente";
import { bus, useAlReconectar, useEvento } from "../api/eventos";
import type { AdbDispositivo, ChipInfo, Dispositivo, TipoChip } from "../api/tipos";
import { Aviso, Boton, Cifra as CifraBase, Esqueleto, Insignia, Tarjeta, Titulo, Vacio, cx, useToast } from "../componentes/base";
import { TarjetaDispositivo, estadoSim, simsConChip } from "../componentes/dispositivo";
import { etiquetaOperador, hace, useAgrupar } from "../util/datos";

/** Cifra del resumen, con su retraso para la entrada escalonada. */
function Cifra({ i, icono, etiqueta, valor, total, tono }: { i: number; icono: ReactNode; etiqueta: string; valor: number; total?: number; tono?: string }) {
  return (
    <div style={{ "--i": i } as React.CSSProperties}>
      <CifraBase
        etiqueta={etiqueta}
        valor={valor}
        total={total}
        icono={icono}
        tono={tono}
        className="bg-panel-2 border-transparent transition-[transform,box-shadow] duration-300 ease-salida hover:-translate-y-1 hover:shadow-elevada"
      />
    </div>
  );
}

/** Teléfonos que ADB ve pero que todavía no tienen la app conectada (hay que provisionarlos). */
function SinAgente({ dispositivos }: { dispositivos: Dispositivo[] }) {
  const [adb, setAdb] = useState<AdbDispositivo[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [instalando, setInstalando] = useState<string | null>(null);
  const avisar = useToast();
  const cargar = () =>
    api.adb().then(
      (a) => {
        setAdb(a);
        setError(null);
      },
      (e) => {
        // Sin ADB en el control (`--sin-adb`) no es un error que mostrar: simplemente no hay teléfonos USB.
        setAdb([]);
        setError(e instanceof FalloApi && e.codigo === "sin_adb" ? null : mensajeError(e));
      },
    );
  useEffect(() => {
    cargar();
  }, []);
  // Varios eventos seguidos → una sola consulta a ADB.
  const agrupada = useAgrupar(cargar, 400);
  useEvento((e) => {
    if (e.tipo === "adb" || e.tipo.startsWith("dispositivo_")) agrupada();
  });
  useAlReconectar(cargar);
  const conAgente = new Set(dispositivos.filter((d) => d.conectado).map((d) => d.adb_serial));
  const pendientes = adb.filter((a) => !conAgente.has(a.serial));
  if (error)
    return (
      <div className="mt-8">
        <Aviso tono="aviso">No pude leer los teléfonos USB (ADB): {error}</Aviso>
      </div>
    );
  if (pendientes.length === 0) return null;

  /** Instala y configura el agente por USB (lo mismo que scripts/provisionar). */
  const instalar = async (serial: string) => {
    setInstalando(serial);
    try {
      const r = await api.provisionar(serial);
      const fallos = r.pasos.filter((p) => !p.ok);
      if (r.ok) avisar("Agente instalado. En unos segundos aparece en línea.", "ok");
      else avisar(`Falló: ${fallos.map((p) => p.paso).join(", ")}${fallos[0]?.salida ? ` (${fallos[0].salida.slice(0, 80)})` : ""}`, "error");
    } catch (e) {
      avisar(`No pude instalar: ${mensajeError(e)}`, "error");
    } finally {
      setInstalando(null);
      cargar();
    }
  };

  return (
    <div className="mt-8">
      <h2 className="text-sm font-semibold mb-3">Conectados por USB sin la app activa</h2>
      <Tarjeta className="divide-y divide-borde">
        {pendientes.map((a) => (
          <div key={a.serial} className="flex items-center justify-between gap-3 px-4 py-3 text-sm">
            <div className="flex items-center gap-3 min-w-0">
              <Cable className="size-4 text-tenue shrink-0" />
              <span className="font-mono truncate">{a.serial}</span>
              <span className="text-tenue truncate">{a.modelo ?? ""}</span>
            </div>
            <div className="flex items-center gap-2 shrink-0">
              {a.estado !== "device" ? (
                <Insignia tono="aviso">{a.estado === "unauthorized" ? "Aceptar depuración en el teléfono" : a.estado}</Insignia>
              ) : (
                <>
                  <Insignia tono={a.reverse_ok ? "neutro" : "aviso"}>{a.reverse_ok ? "Esperando la app" : "Sin túnel"}</Insignia>
                  <Boton variante="secundario" className="h-8 px-4" cargando={instalando === a.serial} onClick={() => instalar(a.serial)}>
                    Instalar agente
                  </Boton>
                </>
              )}
            </div>
          </div>
        ))}
      </Tarjeta>
    </div>
  );
}

type Nivel = "error" | "aviso";
interface Alerta {
  id: string;
  nivel: Nivel;
  dispositivo: Dispositivo;
  texto: string;
}

/** Lo que pide atención ahora mismo en los teléfonos conectados; si no hay nada, la franja no aparece. */
const DIA_MS = 24 * 3600 * 1000;

function alertasDe(dispositivos: Dispositivo[]): Alerta[] {
  const out: Alerta[] = [];
  // Un teléfono que estaba y se fue es lo más urgente; se ignoran los que no se ven hace más de un día
  // (el que se guardó en un cajón no debe avisar para siempre).
  for (const d of dispositivos.filter((x) => !x.conectado && Date.now() - x.ultima_vez_ms < DIA_MS)) {
    out.push({ id: `${d.id}-fuera`, nivel: "error", dispositivo: d, texto: `Desconectado (${hace(d.ultima_vez_ms)}). Revisa el cable USB, la app y la notificación del agente.` });
  }
  for (const d of dispositivos.filter((x) => x.conectado)) {
    const poner = (clave: string, nivel: Nivel, texto: string) => out.push({ id: `${d.id}-${clave}`, nivel, dispositivo: d, texto });
    const e = d.estado;
    if (e && !e.accesibilidad_activa) poner("acc", "error", "Accesibilidad apagada: sin ella no puede leer los menús USSD en modo pantalla.");
    if (e && e.bateria != null && e.bateria < 20 && !e.cargando) poner("bat", "aviso", `Batería al ${e.bateria}% y sin cargar.`);
    if (e && e.permisos_faltantes.length > 0) poner("perm", "aviso", `Faltan permisos: ${e.permisos_faltantes.join(", ")}.`);
    for (const s of simsConChip(d)) {
      const est = estadoSim(s);
      if (est.tono === "error" || est.tono === "aviso") poner(`sim${s.slot}`, est.tono, `SIM ${s.slot + 1} (${s.operador ? etiquetaOperador(s) : "sin operador"}): ${est.texto.toLowerCase()}.`);
    }
  }
  return out.sort((a, b) => Number(b.nivel === "error") - Number(a.nivel === "error"));
}

function Alertas({ alertas, abrir }: { alertas: Alerta[]; abrir: (id: string) => void }) {
  if (alertas.length === 0) return null;
  return (
    <section aria-label="Requiere atención" className="mb-7 aparecer">
      <div className="flex items-center gap-2 mb-3">
        <h2 className="text-sm font-semibold">Requiere atención</h2>
        <Insignia tono={alertas.some((a) => a.nivel === "error") ? "error" : "aviso"}>{alertas.length}</Insignia>
      </div>
      <Tarjeta className="divide-y divide-borde overflow-hidden">
        {alertas.map((a) => (
          <div key={a.id} className="flex items-center gap-3 px-4 py-3 text-sm">
            <span className={cx("size-2 rounded-full shrink-0", a.nivel === "error" ? "bg-error" : "bg-aviso")} />
            <p className="min-w-0 flex-1">
              <span className="font-medium">{a.dispositivo.modelo}</span>
              <span className="text-tenue"> · {a.texto}</span>
            </p>
            <button
              onClick={() => abrir(a.dispositivo.id)}
              className="shrink-0 text-xs font-semibold text-tenue hover:text-texto rounded-full px-3 h-7 bg-panel-2 transition-colors"
            >
              Ver
            </button>
          </div>
        ))}
      </Tarjeta>
    </section>
  );
}

/** Saldos conocidos de los chips y la consulta manual (corre la receta `saldo` del operador, solo lectura). */
function useChips(dispositivos: Dispositivo[]) {
  const [chips, setChips] = useState<ChipInfo[]>([]);
  const [consultando, setConsultando] = useState(false);
  const avisar = useToast();
  const cargar = () => api.chips().then(setChips, () => setChips([]));
  useEffect(() => {
    cargar();
  }, [dispositivos.length]);
  // `dispositivo_actualizado` llega cada 30 s por teléfono y una receta emite varios eventos: se juntan.
  const agrupada = useAgrupar(cargar, 500);
  useEvento((e) => {
    if (e.tipo.startsWith("dispositivo_") || e.tipo === "chip_cambiado" || e.tipo === "chip_tipo" || (e.tipo === "receta" && e.fase !== "inicio"))
      agrupada();
  });
  const consultar = async () => {
    setConsultando(true);
    try {
      const r = await api.consultarSaldos();
      setChips(r);
      avisar(`Saldos actualizados (${r.filter((c) => c.saldo !== null).length}/${r.length} chips)`, "ok");
    } catch (e) {
      avisar(`No pude consultar los saldos: ${mensajeError(e)}`, "error");
    } finally {
      setConsultando(false);
    }
  };
  const marcarTipo = async (dispositivo: string, slot: number, tipo: TipoChip) => {
    try {
      const c = await api.marcarTipoChip(dispositivo, slot, tipo);
      setChips((cs) => cs.map((x) => (x.dispositivo === c.dispositivo && x.slot === c.slot ? c : x)));
      avisar(tipo === "dealer" ? "Chip marcado como dealer" : "Chip marcado como normal", "ok");
    } catch (e) {
      avisar(`No pude cambiar el tipo del chip: ${mensajeError(e)}`, "error");
    }
  };
  const guardarPin = async (dispositivo: string, slot: number, pin: string | null) => {
    try {
      const c = await api.fijarPinChip(dispositivo, slot, pin);
      setChips((cs) => cs.map((x) => (x.dispositivo === c.dispositivo && x.slot === c.slot ? c : x)));
      avisar(pin ? "PIN guardado para este chip" : "PIN borrado", "ok");
      return true;
    } catch (e) {
      avisar(`No pude guardar el PIN: ${mensajeError(e)}`, "error");
      return false;
    }
  };
  useAlReconectar(cargar);
  return { chips, consultando, consultar, marcarTipo, guardarPin, cargar };
}

export function Panel({
  dispositivos,
  cargando,
  error,
  recargar,
  abrir,
}: {
  dispositivos: Dispositivo[];
  cargando: boolean;
  error: string | null;
  recargar: () => void;
  abrir: (id: string, slot?: number, vista?: "ussd" | "sms") => void;
}) {
  const conectados = dispositivos.filter((d) => d.conectado);
  const sims = conectados.flatMap(simsConChip);
  const listas = sims.filter((s) => estadoSim(s).tono === "ok").length;
  const conProblema = sims.filter((s) => ["error", "aviso"].includes(estadoSim(s).tono)).length;
  const alertas = alertasDe(dispositivos);
  const { chips, consultando, consultar, marcarTipo, guardarPin, cargar: cargarChips } = useChips(dispositivos);
  const [reconectando, setReconectando] = useState(false);
  const avisar = useToast();

  /** Tras dormir el PC: reabre los eventos, rehace el túnel USB, despierta la app de los caídos y relee todo. */
  const reconectarTodo = async () => {
    setReconectando(true);
    bus.reconectar();
    try {
      const r = await api.reconectar();
      const despertadas = r.adb.filter((a) => a.app_abierta).length;
      const sinTunel = r.adb.filter((a) => !a.reverse_ok).length;
      const partes = [`${r.refrescados.length} al día`];
      if (despertadas) partes.push(`${despertadas} app${despertadas > 1 ? "s" : ""} despertada${despertadas > 1 ? "s" : ""} (aparece en unos segundos)`);
      if (r.ocupados.length) partes.push(`${r.ocupados.length} ocupado${r.ocupados.length > 1 ? "s" : ""} (no se tocó)`);
      if (r.sin_respuesta.length) partes.push(`${r.sin_respuesta.length} sin respuesta: se reinició su conexión`);
      if (sinTunel) partes.push(`${sinTunel} sin túnel USB: revisa el cable`);
      avisar(`Reconectar: ${partes.join(" · ")}`, r.sin_respuesta.length || sinTunel ? "error" : "ok");
    } catch (e) {
      avisar(`No pude reconectar: ${mensajeError(e)}`, "error");
    } finally {
      setReconectando(false);
      recargar();
      cargarChips();
    }
  };

  return (
    <div>
      <Titulo
        titulo="Teléfonos"
        sub="Cada teléfono corre la app agente y se conecta al control por USB."
        acciones={
          <>
            <Boton
              variante="fantasma"
              icono={<RefreshCw className="size-4" />}
              cargando={reconectando}
              onClick={reconectarTodo}
              title="Rehace el túnel USB, despierta la app de los teléfonos desconectados y actualiza el estado de todos. Úsalo si el PC se durmió."
            >
              {reconectando ? "Reconectando…" : "Reconectar"}
            </Boton>
            <Boton variante="primario" icono={<Wallet className="size-4" />} cargando={consultando} disabled={chips.length === 0} onClick={consultar}>
              {consultando ? "Consultando…" : "Consultar saldos"}
            </Boton>
          </>
        }
      />

      <div className="grid grid-cols-2 lg:grid-cols-4 gap-4 mb-7 escalonar">
        <Cifra i={0} icono={<Smartphone className="size-5" />} etiqueta="Teléfonos en línea" valor={conectados.length} total={dispositivos.length} />
        <Cifra i={1} icono={<CheckCircle2 className="size-5" />} etiqueta="Chips listos" valor={listas} total={sims.length} />
        <Cifra i={2} icono={<TriangleAlert className="size-5" />} etiqueta="Chips con problema" valor={conProblema} tono={conProblema ? "text-aviso" : ""} />
        <Cifra i={3} icono={<Activity className="size-5" />} etiqueta="En uso ahora" valor={conectados.filter((d) => d.ocupado).length} />
      </div>

      {error && (
        <div className="mb-4">
          <Aviso>No pude leer los teléfonos: {error}</Aviso>
        </div>
      )}

      <Alertas alertas={alertas} abrir={(id) => abrir(id)} />

      {cargando && dispositivos.length === 0 ? (
        <div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-3" aria-busy>
          <Esqueleto className="h-72 rounded-3xl" />
          <Esqueleto className="h-72 rounded-3xl hidden sm:block" />
          <Esqueleto className="h-72 rounded-3xl hidden xl:block" />
        </div>
      ) : dispositivos.length === 0 ? (
        <Tarjeta>
          <Vacio
            icono={<Smartphone className="size-5" />}
            titulo="Todavía no hay teléfonos"
            texto="Conecta un Android por USB con la depuración activada y provisiónalo con scripts/provisionar. Aparecerá aquí solo."
          />
        </Tarjeta>
      ) : (
        <div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-3 2xl:grid-cols-4 escalonar">
          {dispositivos.map((d, i) => (
            <TarjetaDispositivo
              key={d.id}
              d={d}
              i={i}
              chips={chips.filter((c) => c.dispositivo === d.id)}
              onClick={() => abrir(d.id)}
              onUssd={(slot) => abrir(d.id, slot, "ussd")}
              onSms={() => abrir(d.id, 0, "sms")}
              onTipo={(slot, tipo) => marcarTipo(d.id, slot, tipo)}
              onPin={(slot, pin) => guardarPin(d.id, slot, pin)}
            />
          ))}
        </div>
      )}

      <SinAgente dispositivos={dispositivos} />
    </div>
  );
}
