import { useEffect, useState } from "react";
import { api, esCancelado, esControlViejo, mensajeError, MENSAJE_VIEJO } from "../../../api/cliente";
import type { ValidacionReceta } from "../../../api/tipos";

/**
 * Valida el YAML en el control ~400 ms después del último cambio (la misma validación que al guardar).
 * Si el YAML ni siquiera se puede leer, no pregunta (`activo: false`).
 */
export function useValidacion(yaml: string, activo: boolean) {
  const [res, setRes] = useState<ValidacionReceta | null>(null);
  const [validando, setValidando] = useState(false);
  const [sinServidor, setSinServidor] = useState<string | null>(null);

  useEffect(() => {
    if (!activo) {
      setValidando(false);
      return;
    }
    setValidando(true);
    const c = new AbortController();
    const t = window.setTimeout(async () => {
      try {
        setRes(await api.validarReceta(yaml, c.signal));
        setSinServidor(null);
      } catch (e) {
        if (esCancelado(e)) return;
        setRes(null);
        setSinServidor(esControlViejo(e) ? MENSAJE_VIEJO : `No pude revisar en el control: ${mensajeError(e)}`);
      }
      if (!c.signal.aborted) setValidando(false);
    }, 400);
    return () => {
      window.clearTimeout(t);
      c.abort();
    };
  }, [yaml, activo]);

  return { res, validando, sinServidor };
}
