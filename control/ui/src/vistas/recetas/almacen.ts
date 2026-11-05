/**
 * Datos que pasan de una vista a otra sin ir en la URL. Solo viven en memoria (nunca en el navegador ni en el
 * control): un borrador hecho desde la consola USSD y el aviso "guardada sin probar".
 */

let borrador: { yaml: string; origen: string } | null = null;

/** La consola deja aquí el borrador; el editor de `#recetas/nueva?borrador=1` lo lee. */
export function dejarBorrador(yaml: string, origen: string) {
  borrador = { yaml, origen };
}

/** Se lee sin borrarlo (React puede montar el editor dos veces); se borra al guardar o descartar. */
export const verBorrador = () => borrador;

export function limpiarBorrador() {
  borrador = null;
}

let recienGuardada: string | null = null;

/** El editor guardó una receta que mueve dinero: la ficha muestra "simúlala ahora". */
export function marcarRecienGuardada(nombre: string) {
  recienGuardada = nombre;
}

/** Sigue pendiente hasta que se simula o se descarta el aviso (sobrevive al doble montaje de React). */
export const esRecienGuardada = (nombre: string) => recienGuardada === nombre;

export function olvidarRecienGuardada() {
  recienGuardada = null;
}
