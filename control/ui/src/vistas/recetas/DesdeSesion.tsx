import { useEffect, useState } from "react";
import { api, mensajeError } from "../../api/cliente";
import { Aviso, Boton, Campo, Casilla, Dialogo } from "../../componentes/base";
import { dejarBorrador } from "./almacen";

/** Lo que pasó en una sesión de la consola: el código marcado y cada respuesta, con la pantalla que llegó. */
export interface PasoSesion {
  enviado: string;
  texto: string;
}

const PIDE_PIN = /\b(pin|clave|contrase)/i;

/**
 * "Guardar como receta": manda la sesión al control, que arma un borrador (no guarda nada), y abre el editor con él.
 * Las respuestas marcadas como PIN viajan como `{pin}`: el PIN nunca sale de la consola ni se guarda.
 */
export function DialogoDesdeSesion({
  abierto,
  cerrar,
  pasos,
  operadorInicial,
}: {
  abierto: boolean;
  cerrar: () => void;
  pasos: PasoSesion[];
  operadorInicial: string;
}) {
  const [operador, setOperador] = useState(operadorInicial);
  const [pines, setPines] = useState<Set<number>>(new Set());
  const [creando, setCreando] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Al abrir: marca como PIN las respuestas a una pantalla que lo pedía (se puede desmarcar).
  useEffect(() => {
    if (!abierto) return;
    setOperador(operadorInicial);
    setError(null);
    setPines(new Set(pasos.map((p, k) => (k > 0 && /^\d{4,8}$/.test(p.enviado) && PIDE_PIN.test(pasos[k - 1].texto) ? k : -1)).filter((k) => k > 0)));
  }, [abierto]); // eslint-disable-line react-hooks/exhaustive-deps

  async function crear() {
    setCreando(true);
    setError(null);
    try {
      const r = await api.recetaDesdeSesion(
        operador.trim().toUpperCase(),
        pasos.map((p, k) => ({ enviado: pines.has(k) ? "{pin}" : p.enviado, texto: p.texto })),
      );
      dejarBorrador(r.yaml, "Borrador desde la consola USSD: revisa cada paso, ponle nombre y marca el paso que mueve dinero, si lo hay.");
      cerrar();
      location.hash = "recetas/nueva?borrador=1";
    } catch (e) {
      setError(mensajeError(e));
    } finally {
      setCreando(false);
    }
  }

  return (
    <Dialogo
      abierto={abierto}
      cerrar={cerrar}
      titulo="Guardar la sesión como receta"
      acciones={
        <>
          <Boton variante="fantasma" onClick={cerrar}>
            Volver
          </Boton>
          <Boton variante="primario" cargando={creando} disabled={!operador.trim() || pasos.length === 0} onClick={crear}>
            Abrir en el editor
          </Boton>
        </>
      }
    >
      <p className="text-tenue">
        Se arma un borrador con lo que marcaste y respondiste. No se guarda hasta que lo revises en el editor.
      </p>
      <div className="mt-4">
        <Campo etiqueta="Operador" value={operador} onChange={(e) => setOperador(e.target.value)} className="uppercase" data-autofocus />
      </div>
      <ol className="mt-4 space-y-1.5 rounded-2xl bg-panel-2 p-3">
        {pasos.map((p, k) => (
          <li key={k} className="flex flex-wrap items-center justify-between gap-2 text-sm">
            <span className="min-w-0">
              <span className="text-tenue text-xs mr-2">{k === 0 ? "Marcó" : `Respuesta ${k}`}</span>
              <span className="font-mono">{pines.has(k) ? "••••" : p.enviado}</span>
            </span>
            {k > 0 && (
              <Casilla
                marcada={pines.has(k)}
                onCambio={(v) =>
                  setPines((s) => {
                    const n = new Set(s);
                    if (v) n.add(k);
                    else n.delete(k);
                    return n;
                  })
                }
              >
                <span className="text-xs">Es un PIN</span>
              </Casilla>
            )}
          </li>
        ))}
      </ol>
      <p className="mt-2 text-xs text-tenue">Un PIN se reemplaza por {"{pin}"} y nunca se guarda. Al ejecutar, se toma el PIN guardado del chip.</p>
      {error && <Aviso className="mt-3">{error}</Aviso>}
    </Dialogo>
  );
}
