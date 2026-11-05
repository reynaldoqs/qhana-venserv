import { BatteryCharging, BatteryLow, BatteryMedium, BatteryFull, Cable, Eye, EyeOff, Hash, MessageSquare, Smartphone } from "./iconos";
import type { ChipInfo, Dispositivo, Sim, TipoChip } from "../api/tipos";
import { etiquetaOperador, hace, nombreSim } from "../util/datos";
import { useState } from "react";
import { Insignia, Punto, Tarjeta, cx } from "./base";

/** Ranuras con chip puesto. Algunos teléfonos informan ranuras vacías (p. ej. la eSIM) que no interesan. */
export const simsConChip = (d: Dispositivo) => d.sims.filter((s) => s.estado !== "ausente");

/** Resume el estado de un chip en un tono y una etiqueta corta. */
export function estadoSim(s: Sim): { tono: "ok" | "aviso" | "error" | "neutro"; texto: string } {
  switch (s.estado) {
    case "ausente":
      return { tono: "neutro", texto: "Sin chip" };
    case "pin":
      return { tono: "aviso", texto: "Pide PIN" };
    case "puk":
    case "bloqueada":
      return { tono: "error", texto: "Bloqueada" };
    case "desconocido":
      return { tono: "aviso", texto: "Desconocido" };
  }
  if (s.voz === "en_servicio") return { tono: "ok", texto: "Lista" };
  if (s.voz === "solo_emergencia") return { tono: "error", texto: "Solo emergencia" };
  if (s.voz === "sin_servicio" || s.voz === "apagado") return { tono: "error", texto: "Sin voz" };
  return { tono: "ok", texto: "Lista" };
}

export function ChipSim({
  sim,
  compacto,
  chip,
  onUssd,
  onSms,
  onTipo,
  onPin,
}: {
  sim: Sim;
  compacto?: boolean;
  /** Saldo conocido de este chip (si ya se consultó). */
  chip?: ChipInfo;
  onUssd?: () => void;
  onSms?: () => void;
  /** Cambia el tipo del chip (normal ↔ dealer). */
  onTipo?: (tipo: TipoChip) => void;
  /** Guarda el PIN del chip (`null` lo borra). Devuelve si se guardó. */
  onPin?: (pin: string | null) => Promise<boolean>;
}) {
  const e = estadoSim(sim);
  const [editandoPin, setEditandoPin] = useState(false);
  const [pin, setPin] = useState("");
  const guardarPin = async (valor: string | null) => {
    if (onPin && (await onPin(valor))) {
      setEditandoPin(false);
      setPin("");
    }
  };
  const accion =
    "inline-flex items-center gap-1.5 h-7 px-2.5 rounded-full bg-black/25 text-xs font-medium text-tenue transition-colors hover:text-texto hover:bg-black/40";
  return (
    <div className={cx("rounded-xl bg-panel-2 px-3", compacto ? "py-2" : "py-2.5")}>
      <div className="flex items-center justify-between gap-2">
        <div className="flex items-center gap-2 min-w-0">
          <span className="text-[11px] font-semibold text-tenue">{nombreSim(sim.slot)}</span>
          <span className="font-medium truncate">{etiquetaOperador(sim)}</span>
          {chip?.tipo === "dealer" && <Insignia tono="info">Dealer</Insignia>}
          {chip?.tipo === "dealer" && chip.tiene_pin && <Insignia tono="ok">PIN</Insignia>}
        </div>
        <Insignia tono={e.tono}>{e.texto}</Insignia>
      </div>
      {!compacto && (
        <div className="mt-1 flex items-end justify-between gap-3">
          <div className="flex items-center gap-3 text-xs text-tenue font-mono min-w-0">
            <span className="truncate">{sim.numero ?? chip?.numero ?? "número desconocido"}</span>
            {sim.red && <span>{sim.red}</span>}
          </div>
          {chip && (
            <div className="text-right shrink-0 leading-tight">
              <p className="text-sm font-semibold tabular-nums">{chip.saldo === null ? "—" : `${chip.saldo.toFixed(2)} Bs`}</p>
              <p className="text-[11px] text-tenue">{chip.sin_verificar ? "sin verificar" : chip.saldo_ms ? hace(chip.saldo_ms) : "sin consultar"}</p>
              {chip.tipo === "dealer" && (
                <p
                  className="mt-1 text-[11px] text-tenue"
                  title={chip.saldo_dealer_ms ? `Saldo dealer consultado ${hace(chip.saldo_dealer_ms)}` : "Se consulta con «Consultar saldos» si el chip tiene PIN guardado"}
                >
                  Dealer{" "}
                  <span className="text-sm font-semibold text-texto tabular-nums">
                    {chip.saldo_dealer == null ? "—" : `${chip.saldo_dealer.toFixed(2)} Bs`}
                  </span>
                  {chip.saldo_dealer == null && !chip.tiene_pin && " · sin PIN"}
                </p>
              )}
            </div>
          )}
        </div>
      )}
      {(onUssd || onSms || (onTipo && chip)) && (
        <div className="mt-2.5 flex gap-1.5">
          {onUssd && (
            <button
              type="button"
              className={accion}
              onClick={(ev) => {
                ev.stopPropagation();
                onUssd();
              }}
            >
              <Hash className="size-3.5" /> USSD
            </button>
          )}
          {onSms && (
            <button
              type="button"
              className={accion}
              onClick={(ev) => {
                ev.stopPropagation();
                onSms();
              }}
            >
              <MessageSquare className="size-3.5" /> SMS
            </button>
          )}
          {onTipo && chip && (
            <button
              type="button"
              className={cx(accion, "ml-auto")}
              title={chip.tipo === "dealer" ? "Volver a tratarlo como chip normal" : "Marcar como chip dealer (usa recetas dealer si existen)"}
              onClick={(ev) => {
                ev.stopPropagation();
                const dealer = chip.tipo !== "dealer";
                onTipo(dealer ? "dealer" : "normal");
                // Las recargas dealer de Tigo piden el PIN del dealer: se ofrece guardarlo al marcar.
                if (dealer && onPin && chip.operador === "TIGO" && !chip.tiene_pin) setEditandoPin(true);
              }}
            >
              {chip.tipo === "dealer" ? "Quitar dealer" : "Marcar dealer"}
            </button>
          )}
          {onPin && chip?.tipo === "dealer" && !editandoPin && (
            <button
              type="button"
              className={accion}
              title="PIN del chip dealer, para las recetas dealer que lo piden"
              onClick={(ev) => {
                ev.stopPropagation();
                setEditandoPin(true);
              }}
            >
              {chip.tiene_pin ? "Cambiar PIN" : "Guardar PIN"}
            </button>
          )}
        </div>
      )}
      {editandoPin && onPin && (
        <form
          className="mt-2.5 flex items-center gap-1.5"
          onClick={(ev) => ev.stopPropagation()}
          onSubmit={(ev) => {
            ev.preventDefault();
            if (/^\d{4,8}$/.test(pin)) guardarPin(pin);
          }}
        >
          <input
            type="password"
            inputMode="numeric"
            autoComplete="off"
            autoFocus
            placeholder="PIN del dealer"
            value={pin}
            onChange={(ev) => setPin(ev.target.value.replace(/\D/g, "").slice(0, 8))}
            className="h-7 w-32 rounded-full bg-black/25 px-3 text-xs font-mono outline-none focus:ring-2 focus:ring-acento/40"
          />
          <button type="submit" disabled={!/^\d{4,8}$/.test(pin)} className={cx(accion, "disabled:opacity-40")}>
            Guardar
          </button>
          {chip?.tiene_pin && (
            <button type="button" className={accion} onClick={() => guardarPin(null)}>
              Borrar
            </button>
          )}
          <button type="button" className={cx(accion, "ml-auto")} onClick={() => setEditandoPin(false)}>
            Omitir
          </button>
        </form>
      )}
    </div>
  );
}

/** `nivel` null: Android no la informa. */
function Bateria({ nivel, cargando }: { nivel: number | null; cargando: boolean }) {
  const baja = nivel != null && nivel < 20 && !cargando;
  const Icono = cargando ? BatteryCharging : nivel == null ? BatteryMedium : nivel < 20 ? BatteryLow : nivel < 70 ? BatteryMedium : BatteryFull;
  return (
    <span
      className={cx("inline-flex items-center gap-1 text-xs", baja ? "text-error" : "text-tenue")}
      title={nivel == null ? "El teléfono no informa la batería" : `Batería ${nivel}%${cargando ? ", cargando" : ""}`}
    >
      <Icono className="size-4" />
      {nivel == null ? "—" : `${nivel}%`}
    </span>
  );
}

export function TarjetaDispositivo({
  d,
  seleccionado,
  onClick,
  i = 0,
  chips = [],
  onUssd,
  onSms,
  onTipo,
  onPin,
}: {
  i?: number;
  chips?: ChipInfo[];
  onUssd?: (slot: number) => void;
  onSms?: () => void;
  onTipo?: (slot: number, tipo: TipoChip) => void;
  onPin?: (slot: number, pin: string | null) => Promise<boolean>;
  d: Dispositivo;
  seleccionado?: boolean;
  onClick?: () => void;
}) {
  const faltan = d.estado?.permisos_faltantes ?? [];
  const sinAccesibilidad = d.estado && !d.estado.accesibilidad_activa;
  return (
    <div style={{ "--i": i } as React.CSSProperties}>
    <Tarjeta
      className={cx(
        "p-5 transition-[border-color,box-shadow,transform] duration-300 ease-salida",
        "hover:border-tenue/40",
        seleccionado && "border-texto/60 ring-3 ring-texto/10",
        !d.conectado && "opacity-75",
      )}
    >
      <div>
        <div className="flex items-start justify-between gap-3">
          <button
            type="button"
            onClick={onClick}
            disabled={!onClick}
            title={onClick ? "Abrir en la consola USSD" : undefined}
            className="flex items-center gap-3 min-w-0 text-left rounded-xl disabled:pointer-events-none"
          >
            <div className="size-11 shrink-0 rounded-xl bg-panel-2 grid place-items-center text-tenue">
              <Smartphone className="size-5" />
            </div>
            <div className="min-w-0">
              <p className="font-semibold truncate">{d.modelo}</p>
              <p className="text-xs text-tenue truncate">
                Android {d.android} · app {d.app_version}
              </p>
            </div>
          </button>
          <div className="flex items-center gap-2 shrink-0">
            {d.ocupado && <Insignia tono="info">En uso</Insignia>}
            <span className="inline-flex items-center gap-1.5 text-xs font-medium">
              <Punto tono={d.conectado ? "ok" : "neutro"} pulso={d.conectado && d.ocupado} />
              {d.conectado ? "En línea" : hace(d.ultima_vez_ms)}
            </span>
          </div>
        </div>

        <div className="mt-4 grid gap-2">
          {simsConChip(d).length === 0 ? (
            <p className="text-sm text-tenue">Sin chips detectados</p>
          ) : (
            simsConChip(d).map((s) => (
              <ChipSim
                key={s.slot}
                sim={s}
                chip={chips.find((c) => c.slot === s.slot)}
                onUssd={d.conectado && onUssd ? () => onUssd(s.slot) : undefined}
                onSms={d.conectado ? onSms : undefined}
                onTipo={d.conectado && onTipo ? (tipo) => onTipo(s.slot, tipo) : undefined}
                onPin={d.conectado && onPin ? (pin) => onPin(s.slot, pin) : undefined}
              />
            ))
          )}
        </div>

        <div className="mt-4 flex flex-wrap items-center gap-x-4 gap-y-2 text-xs text-tenue">
          {d.estado && <Bateria nivel={d.estado.bateria} cargando={d.estado.cargando} />}
          {d.adb_serial && (
            <span className="inline-flex items-center gap-1 font-mono" title="Conectado por USB (ADB)">
              <Cable className="size-3.5" />
              {d.adb_serial.slice(-6)}
            </span>
          )}
          {d.estado &&
            (sinAccesibilidad ? (
              <span className="inline-flex items-center gap-1 text-aviso">
                <EyeOff className="size-3.5" /> Accesibilidad apagada
              </span>
            ) : (
              <span className="inline-flex items-center gap-1">
                <Eye className="size-3.5" /> Accesibilidad
              </span>
            ))}
        </div>

        {faltan.length > 0 && (
          <p className="mt-3 text-xs text-aviso">Permisos faltantes: {faltan.join(", ")}</p>
        )}
      </div>
    </Tarjeta>
    </div>
  );
}
