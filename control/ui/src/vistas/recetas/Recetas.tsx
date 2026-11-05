import { BookOpen, Plus } from "../../componentes/iconos";
import type { Dispositivo } from "../../api/tipos";
import { Aviso, Boton, Esqueleto, Tarjeta, Titulo, Vacio, cx } from "../../componentes/base";
import { useRecetas } from "../../util/datos";
import { Editor } from "./editor/Editor";
import { Ficha, PESTANAS_FICHA, type PestanaFicha } from "./Ficha";
import { ListaRecetas } from "./ListaRecetas";

/**
 * Rutas (en el hash):
 * - `#recetas`: lista (y en pantalla ancha, la primera receta abierta al lado).
 * - `#recetas/<nombre>[/<pestaña>]`: ficha de la receta (ejecutar, pasos, uso, versiones).
 * - `#recetas/<nombre>/editar[?paso=3]`: editor.
 * - `#recetas/nueva[?desde=<nombre>]`: receta nueva (o copia de otra).
 */
function leerRuta(ruta: string) {
  const [camino, q = ""] = ruta.split("?");
  const [a = "", b = ""] = camino.split("/").map((x) => decodeURIComponent(x));
  return { a, b, q: new URLSearchParams(q) };
}

export function Recetas({ ruta, ir, dispositivos }: { ruta: string; ir: (r: string) => void; dispositivos: Dispositivo[] }) {
  const { lista, error, cargando } = useRecetas();
  const { a, b, q } = leerRuta(ruta);

  if (a === "nueva")
    return <Editor key={`nueva-${q.get("desde") ?? ""}`} nombre={null} desde={q.get("desde")} borrador={q.get("borrador") === "1"} ir={ir} />;
  if (a && b === "editar") {
    const paso = Number(q.get("paso"));
    return <Editor key={`editar-${a}`} nombre={a} pasoInicial={paso > 0 ? paso : null} ir={ir} />;
  }

  // Un enlace con el nombre anterior de una receta (`#recetas/<nombre viejo>`) abre la de hoy.
  const seleccionada =
    lista.find((r) => r.nombre === a) ?? (a ? lista.find((r) => r.nombres_anteriores?.includes(a)) : lista[0]);
  const pestana: PestanaFicha = (PESTANAS_FICHA as readonly string[]).includes(b) ? (b as PestanaFicha) : "ejecutar";
  // Cambiar de pestaña reemplaza la entrada del historial (Atrás no recorre pestañas).
  const irPestana = (p: PestanaFicha) => seleccionada && location.replace(`#recetas/${encodeURIComponent(seleccionada.nombre)}/${p}`);

  return (
    <div>
      <Titulo
        titulo="Recetas"
        sub="Flujos USSD guardados por operador. Simula primero: llega hasta la confirmación y responde No."
        acciones={
          <Boton variante="secundario" icono={<Plus className="size-4" />} onClick={() => ir("nueva")}>
            Nueva receta
          </Boton>
        }
      />

      {error && <Aviso className="mb-4">No pude leer las recetas: {error}</Aviso>}

      {cargando && lista.length === 0 ? (
        <div className="grid gap-4 lg:grid-cols-[340px_1fr]">
          <Esqueleto className="h-80 rounded-3xl" />
          <Esqueleto className="h-80 rounded-3xl hidden lg:block" />
        </div>
      ) : lista.length === 0 && !error ? (
        <Tarjeta>
          <Vacio
            icono={<BookOpen className="size-5" />}
            titulo="Sin recetas"
            texto="Crea una con «Nueva receta», desde una sesión de la Consola USSD, o agrega archivos YAML en control/recetas."
          />
        </Tarjeta>
      ) : (
        <div className="grid gap-6 lg:grid-cols-[minmax(0,320px)_minmax(0,1fr)] items-start">
          {/* En el teléfono: la lista o la ficha, una a la vez. */}
          <div className={cx(a && "hidden lg:block", "lg:sticky lg:top-0")}>
            <ListaRecetas lista={lista} seleccionada={seleccionada?.nombre ?? null} abrir={(n) => ir(encodeURIComponent(n))} />
          </div>
          <div className={cx(!a && "hidden lg:block", "min-w-0")}>
            {seleccionada ? (
              <Ficha key={seleccionada.nombre} resumen={seleccionada} pestana={pestana} irPestana={irPestana} ir={ir} dispositivos={dispositivos} />
            ) : (
              a &&
              !cargando && (
                <Tarjeta>
                  <Vacio
                    icono={<BookOpen className="size-5" />}
                    titulo={`No hay una receta «${a}»`}
                    texto="Puede que la hayan retirado o renombrado."
                  />
                </Tarjeta>
              )
            )}
          </div>
        </div>
      )}
    </div>
  );
}
