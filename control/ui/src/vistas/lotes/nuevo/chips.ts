import { useCallback, useEffect, useState } from "react";
import { api, mensajeError } from "../../../api/cliente";
import { useAlReconectar, useEvento } from "../../../api/eventos";
import type { ChipInfo, Receta } from "../../../api/tipos";
import { useAgrupar } from "../../../util/datos";

/** Chips en línea (`GET /api/chips`), al día con los eventos de teléfonos y chips. */
export function useChipsEnLinea() {
  const [chips, setChips] = useState<ChipInfo[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [cargado, setCargado] = useState(false);
  const cargar = useCallback(async () => {
    try {
      setChips(await api.chips());
      setError(null);
    } catch (e) {
      setError(mensajeError(e));
    } finally {
      setCargado(true);
    }
  }, []);
  useEffect(() => {
    cargar();
  }, [cargar]);
  const agrupada = useAgrupar(cargar, 400);
  useEvento((e) => {
    if (e.tipo.startsWith("dispositivo_") || e.tipo === "chip_tipo" || e.tipo === "chip_cambiado") agrupada();
  });
  useAlReconectar(cargar);
  return { chips, error, cargado, recargar: cargar };
}

export const claveChip = (dispositivo: string, slot: number) => `${dispositivo}|${slot}`;

/**
 * La receta que correría un chip para una acción, con la regla del contrato ("Selección de receta"): un chip dealer usa
 * la receta dealer si existe y si no la de la compañía; un chip normal nunca usa una receta dealer. Solo para mostrar: el control
 * decide al validar.
 */
export function recetaParaChip(lista: Receta[], chip: Pick<ChipInfo, "operador" | "tipo">, accion: string): Receta | undefined {
  const delOperador = lista.filter((r) => r.operador.toUpperCase() === chip.operador.toUpperCase() && r.accion === accion);
  if (chip.tipo === "dealer") return delOperador.find((r) => r.chip_tipo === "dealer") ?? delOperador.find((r) => !r.chip_tipo);
  return delOperador.find((r) => !r.chip_tipo);
}

/** La receta pide el PIN del chip (parámetro `pin` secreto). */
export const pidePin = (r: Receta | undefined) => !!r?.parametros.some((p) => p.secreto && p.nombre === "pin");
