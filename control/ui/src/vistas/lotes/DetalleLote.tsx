import { useState } from "react";
import { api, mensajeError } from "../../api/cliente";
import type { Dispositivo, Lote } from "../../api/tipos";
import { Aviso, Boton, Casilla, Dialogo, Esqueleto, Migas, Titulo, useToast } from "../../componentes/base";
import { useRecetas } from "../../util/datos";
import { EnlaceManual, accionDeOrigen, bs, describirOrigen } from "./comunes";
import { CarrilesLote } from "./detalle/CarrilesLote";
import { AvisosLote } from "./detalle/AvisosLote";
import { CabeceraLote, type AccionLote } from "./detalle/CabeceraLote";
import { ResumenProgreso } from "./detalle/ResumenProgreso";
import { TablaFilasLote } from "./detalle/TablaFilasLote";
import { useLote } from "./detalle/useLote";
import { VerificarSaldos } from "./detalle/VerificarSaldos";

type Dialogos = "iniciar" | "cancelar" | "inciertas" | null;

const filas = (n: number) => (n === 1 ? "1 fila" : `${n} filas`);

export function DetalleLote({ id, dispositivos, volver }: { id: number; dispositivos: Dispositivo[]; volver: () => void }) {
  const { lote, error, setError, cargado, recargar, fijar } = useLote(id);
  const { lista: recetas } = useRecetas();
  const [accion, setAccion] = useState<AccionLote | null>(null);
  const [dialogo, setDialogo] = useState<Dialogos>(null);
  const [revisado, setRevisado] = useState(false);
  const avisar = useToast();

  async function hacer(nombre: AccionLote, fn: () => Promise<Lote>, ok?: string) {
    setAccion(nombre);
    setDialogo(null);
    try {
      fijar(await fn());
      setError(null);
      if (ok) avisar(ok, "ok");
    } catch (e) {
      setError(mensajeError(e));
    } finally {
      setAccion(null);
      recargar();
    }
  }

  if (!lote) {
    return (
      <div>
        <Titulo migas={<Migas tramos={[{ texto: "Lotes", ir: volver }, { texto: `#${id}` }]} />} titulo={`Lote #${id}`} />
        {error ? (
          <Aviso>{error}</Aviso>
        ) : (
          <div className="space-y-4" aria-busy={!cargado}>
            <Esqueleto className="h-40 rounded-3xl" />
            <Esqueleto className="h-72 rounded-3xl" />
          </div>
        )}
      </div>
    );
  }

  const p = lote.progreso;

  // ¿Se puede cuadrar con saldos? El control cuadra por chip y por tipo de saldo (principal para lo transferido, dealer
  // para lo recargado desde un chip dealer con PIN) y responde `409 no_aplica` si ningún chip se puede cuadrar: la
  // interfaz solo adivina el texto (recargas → saldo dealer) a partir de la acción del origen.
  const accionOrigen = accionDeOrigen(lote.origen, (n) => recetas.find((r) => r.nombre === n)?.accion);
  const conSaldoDealer = accionOrigen != null && accionOrigen !== "transferir";
  const puedeVerificar = !lote.simular && p.ok + p.parcial > 0 && lote.estado !== "en_curso" && lote.estado !== "listo";
  const origenTexto = describirOrigen(lote.origen, dispositivos);

  const iniciar = () => hacer("iniciar", () => api.iniciarLote(lote.id));
  /** Reintentar = volver a poner en cola y reanudar, siempre igual. Si `iniciar` falla, el lote queda re-encolado y se ve el error. */
  const reintentar = (inciertas: boolean) =>
    hacer(
      inciertas ? "reintentar-inciertas" : "reintentar",
      async () => {
        fijar(await api.reintentarFallidas(lote.id, inciertas));
        return api.iniciarLote(lote.id);
      },
      inciertas ? "Inciertas en cola: el lote se reanudó" : "Fallidas en cola: el lote se reanudó",
    );

  const noSeEnvian = p.pendientes + p.sin_capacidad;

  return (
    <div>
      <CabeceraLote
        lote={lote}
        dispositivos={dispositivos}
        accion={accion}
        volver={volver}
        alIniciar={() => (lote.estado === "listo" && !lote.simular ? setDialogo("iniciar") : iniciar())}
        alPausar={() => hacer("pausar", () => api.pausarLote(lote.id))}
        alCancelar={() => setDialogo("cancelar")}
        alArchivar={(a) => hacer("archivar", () => api.archivarLote(lote.id, a), a ? "Lote archivado" : "Lote fuera del archivo")}
      />

      {error && (
        <div className="mb-5">
          <Aviso
            acciones={
              <Boton variante="fantasma" className="h-8 px-3 -ml-3" onClick={() => setError(null)}>
                Cerrar
              </Boton>
            }
          >
            {error}
          </Aviso>
        </div>
      )}

      <AvisosLote
        lote={lote}
        accion={accion}
        alReintentar={() => reintentar(false)}
        alReintentarInciertas={() => {
          setRevisado(false);
          setDialogo("inciertas");
        }}
      />

      {puedeVerificar && <VerificarSaldos lote={lote} dealer={conSaldoDealer} dispositivos={dispositivos} recargar={recargar} />}

      <ResumenProgreso lote={lote} />

      <CarrilesLote lote={lote} dispositivos={dispositivos} />

      <TablaFilasLote lote={lote} dispositivos={dispositivos} />

      <Dialogo
        abierto={dialogo === "iniciar"}
        cerrar={() => setDialogo(null)}
        peligro
        titulo="Iniciar el lote real"
        acciones={
          <>
            <Boton variante="fantasma" data-autofocus onClick={() => setDialogo(null)}>
              Volver
            </Boton>
            <Boton variante="peligro" onClick={iniciar}>
              Iniciar y enviar {bs(p.total_bs)}
            </Boton>
          </>
        }
      >
        <p>
          Este lote <b>mueve dinero de verdad</b>: {filas(p.filas)} por {bs(p.total_bs)}
          {origenTexto && <>, {origenTexto}</>}
          .
        </p>
        <p className="mt-2 text-tenue">Antes de enviar consulta el saldo de los chips y lo guarda como saldo inicial (para verificar después).</p>
      </Dialogo>

      <Dialogo
        abierto={dialogo === "cancelar"}
        cerrar={() => setDialogo(null)}
        peligro
        titulo={`¿Cancelar «${lote.nombre}»?`}
        acciones={
          <>
            <Boton variante="fantasma" data-autofocus onClick={() => setDialogo(null)}>
              Volver
            </Boton>
            <Boton variante="peligro" cargando={accion === "cancelar"} onClick={() => hacer("cancelar", () => api.cancelarLote(lote.id), "Lote cancelado")}>
              Cancelar lote
            </Boton>
          </>
        }
      >
        <p>
          {noSeEnvian > 0 ? (
            <>
              <b>{filas(noSeEnvian)} no se enviará{noSeEnvian === 1 ? "" : "n"}</b>
              {p.sin_capacidad > 0 && ` (${p.pendientes} pendientes y ${p.sin_capacidad} para mañana)`}.
            </>
          ) : (
            "No queda nada pendiente."
          )}{" "}
          Lo que ya salió no se deshace.
        </p>
        <p className="mt-2 text-tenue">Un lote cancelado no se puede reanudar ni reintentar: habría que crear uno nuevo.</p>
      </Dialogo>

      <Dialogo
        abierto={dialogo === "inciertas"}
        cerrar={() => setDialogo(null)}
        peligro
        titulo={`Reintentar ${filas(p.inciertas)} incierta${p.inciertas === 1 ? "" : "s"}`}
        acciones={
          <>
            <Boton variante="fantasma" data-autofocus onClick={() => setDialogo(null)}>
              Volver
            </Boton>
            <Boton variante="peligro" disabled={!revisado} onClick={() => reintentar(true)}>
              Reintentar y reanudar
            </Boton>
          </>
        }
      >
        <p>
          Un envío incierto <b>pudo haberse hecho</b>: el operador no respondió claro después de confirmar. Si ya había llegado y lo
          reintentas, el destinatario recibe dos veces.
        </p>
        <p className="mt-2 text-tenue">
          Revisa el saldo o el historial (SMS) del chip de origen y confirma que esos envíos no llegaron. Se vuelven a poner en cola junto
          con las fallidas y el lote se reanuda. <EnlaceManual seccion="filas" />
        </p>
        <div className="mt-4 rounded-xl bg-panel-2 px-4 py-3">
          <Casilla marcada={revisado} onCambio={setRevisado}>
            Revisé el saldo/historial del chip y esos envíos no llegaron
          </Casilla>
        </div>
      </Dialogo>
    </div>
  );
}
