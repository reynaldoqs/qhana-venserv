import { useState } from "react";
import { api, mensajeError } from "../../../api/cliente";
import type { Dispositivo, Lote } from "../../../api/tipos";
import { Aviso, Boton, Insignia, Tarjeta } from "../../../componentes/base";
import { Scale } from "../../../componentes/iconos";
import { fechaHora, sinPrefijo } from "../../../util/datos";
import { EnlaceManual, bs, nombreChip } from "../comunes";

/**
 * Cuadrar saldos: consulta el saldo real de los chips y lo compara con lo que el lote dice que envió.
 * `dealer`: lote de recargas desde un chip dealer, que cuadra con el saldo dealer de ese chip.
 */
export function VerificarSaldos({
  lote,
  dealer,
  dispositivos,
  recargar,
}: {
  lote: Lote;
  dealer: boolean;
  dispositivos: Dispositivo[];
  recargar: () => Promise<void> | void;
}) {
  const [verificando, setVerificando] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const verif = lote.verificacion ?? null;
  // Columna "Saldo" (principal/dealer) cuando el lote cuadró más de un tipo de saldo, o cuando el control dice cuál por chip.
  const porChip = !!verif && (verif.saldo === "mixto" || new Set(verif.chips.map((c) => c.saldo ?? verif.saldo)).size > 1);
  const soloDealer = verif?.saldo === "dealer";

  async function verificar() {
    setVerificando(true);
    setError(null);
    try {
      await api.verificarLote(lote.id); // el control la guarda en el lote
      await recargar();
    } catch (e) {
      setError(mensajeError(e));
    } finally {
      setVerificando(false);
    }
  }

  return (
    <Tarjeta className="p-5 mb-5">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="min-w-0 flex-1">
          <p className="font-semibold">{dealer && verif?.saldo !== "mixto" && verif?.saldo !== "principal" ? "Cuadrar saldo dealer" : "Cuadrar saldos"}</p>
          <p className="text-xs text-tenue mt-0.5">
            {dealer
              ? "Consulta el saldo dealer de los chips que recargaron (envía el PIN guardado) y, si también hubo transferencias, el saldo principal de los que transfirieron. Lo compara con el de inicio del lote, contra lo enviado. Solo vale si nadie usó esos chips por fuera."
              : "Consulta el saldo real de los chips (solo lectura, ~4 s por chip) y lo compara con lo que el lote dice que envió. Solo vale si nadie usó esos chips por fuera."}{" "}
            <EnlaceManual seccion="verificar" />
          </p>
        </div>
        <Boton icono={<Scale className="size-4" />} cargando={verificando} onClick={verificar}>
          {verif ? "Verificar de nuevo" : "Verificar saldos"}
        </Boton>
      </div>
      {error && (
        <div className="mt-4">
          <Aviso>{error}</Aviso>
        </div>
      )}
      {verif && (
        <div className="mt-4">
          <p className="text-xs text-tenue mb-2">
            Verificado {fechaHora(verif.verificado_ms)}
            {verif.saldo === "mixto" ? " · saldo principal y dealer, según lo que hizo cada chip" : verif.saldo === "dealer" ? " · saldo dealer" : ""}
          </p>
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
                    <th scope="col" className="font-medium py-2 pr-3">Chip</th>
                    {porChip && <th scope="col" className="font-medium py-2 px-3">Saldo</th>}
                    <th scope="col" className="font-medium py-2 px-3 text-right">{soloDealer ? "Saldo dealer inicial" : "Inicial"}</th>
                    <th scope="col" className="font-medium py-2 px-3 text-right">{soloDealer ? "Saldo dealer ahora" : "Ahora"}</th>
                    <th scope="col" className="font-medium py-2 px-3 text-right">Descontado</th>
                    <th scope="col" className="font-medium py-2 px-3 text-right">Esperado</th>
                    <th scope="col" className="font-medium py-2 pl-3">Estado</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-borde">
                  {verif.chips.map((c) => (
                    <tr key={`${c.dispositivo}-${c.slot}`}>
                      <td className="py-2 pr-3 font-mono">
                        {sinPrefijo(c.numero) ?? nombreChip(dispositivos, c.dispositivo, c.slot)}
                        <span className="text-xs text-tenue font-sans"> · {c.envios_ok} envíos</span>
                      </td>
                      {porChip && (
                        <td className="py-2 px-3">
                          {(c.saldo ?? verif.saldo) === "dealer" ? (
                            <Insignia tono="info">Dealer</Insignia>
                          ) : (c.saldo ?? verif.saldo) === "principal" ? (
                            <Insignia>Principal</Insignia>
                          ) : (
                            <span className="text-tenue">—</span>
                          )}
                        </td>
                      )}
                      <td className="py-2 px-3 text-right tabular-nums">{bs(c.saldo_inicial)}</td>
                      <td className="py-2 px-3 text-right tabular-nums">{bs(c.saldo_actual)}</td>
                      <td className="py-2 px-3 text-right tabular-nums">{bs(c.descontado_real)}</td>
                      <td className="py-2 px-3 text-right tabular-nums">{bs(c.descontado_esperado)}</td>
                      <td className="py-2 pl-3">
                        {c.estado === "cuadra" && <Insignia tono="ok">Cuadra</Insignia>}
                        {c.estado === "diferencia" && (
                          <Insignia tono="error">
                            {(c.diferencia_bs ?? 0) > 0 ? "+" : ""}
                            {bs(c.diferencia_bs)}
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
  );
}
