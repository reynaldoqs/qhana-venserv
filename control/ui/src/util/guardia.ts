/**
 * Guardia de navegación: una vista con cambios sin guardar (el editor de recetas) puede frenar un cambio de hash.
 * `useVista` (App.tsx) la consulta antes de cambiar de vista; si frena, vuelve el hash atrás sin recargar nada y la
 * vista muestra su propio diálogo con el destino pendiente.
 */
type Guardia = (destino: string) => boolean;

let guardia: Guardia | null = null;
let saltarUna = false;

/** Instala la guardia (devuelve cómo quitarla). `fn` devuelve true si frena la navegación a `destino`. */
export function fijarGuardia(fn: Guardia): () => void {
  guardia = fn;
  return () => {
    if (guardia === fn) guardia = null;
  };
}

/** ¿Hay que frenar la navegación a este hash? Lo llama solo `useVista`. */
export function frena(destino: string): boolean {
  if (saltarUna) {
    saltarUna = false;
    return false;
  }
  return guardia ? guardia(destino) : false;
}

/** Navega sin pasar por la guardia (tras confirmar "descartar cambios"). */
export function irSinGuardia(hash: string) {
  const limpio = hash.replace(/^#/, "");
  if (location.hash.replace(/^#/, "") === limpio) return;
  saltarUna = true;
  location.hash = limpio;
}
