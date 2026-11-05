import { Interruptor, Segmentado, Tarjeta, cx } from "../../../componentes/base";
import { fijar, fijarNombresAnteriores } from "../modelo";
import { AreaTexto, CampoNumero, Rotulo, SeccionEditor, claseCampo, problemasDe, useEditor, useIdCampo } from "./campos";
import { Opciones } from "./SeccionParametros";

const OPERADORES = ["TIGO", "ENTEL", "VIVA"];
const ACCIONES_LOTE = ["saldo", "saldo_dealer", "transferir", "recargar", "movimientos"];

/** Nombre, título, operador, modo y los campos que usan los lotes (acción, chip dealer, límites). */
export function SeccionDatos() {
  const { modelo: m, cambiar, errores, avisos, nuevo } = useEditor();
  const id = useIdCampo("datos");
  const de = (campos: string[]) => ({ errores: problemasDe(errores, null, campos), avisos: problemasDe(avisos, null, campos) });
  const campo = (c: string, extra?: string) => {
    const e = problemasDe(errores, null, [c]).length > 0;
    return cx(claseCampo, "h-10", e ? "border-error" : "border-borde", extra);
  };
  const conLimites = m.accion === "transferir" || m.accion === "recargar" || m.limites.por_dia != null || m.limites.comision != null;

  return (
    <SeccionEditor id="seccion-datos" titulo="Datos" ayuda="Cómo se llama la receta y para qué operador es.">
      <Tarjeta className="p-4 md:p-5 grid gap-4 md:grid-cols-2">
        <Rotulo
          etiqueta="Nombre (archivo)"
          htmlFor={`${id}-nombre`}
          ayuda={nuevo ? "Minúsculas y guiones, p. ej. tigo-recargar. No se puede cambiar después." : "Para renombrar: duplica la receta y retira esta."}
          {...de(["nombre"])}
        >
          <input
            id={`${id}-nombre`}
            value={m.nombre}
            readOnly={!nuevo}
            spellCheck={false}
            onChange={(e) => cambiar(fijar(["nombre"], e.target.value.toLowerCase().replace(/[\s_]+/g, "-").replace(/[^a-z0-9-]/g, "")))}
            className={campo("nombre", cx("font-mono text-[13px]", !nuevo && "bg-panel-2 text-tenue cursor-not-allowed"))}
          />
        </Rotulo>
        <Rotulo etiqueta="Título" htmlFor={`${id}-titulo`} {...de(["titulo"])}>
          <input id={`${id}-titulo`} value={m.titulo} onChange={(e) => cambiar(fijar(["titulo"], e.target.value))} className={campo("titulo")} />
        </Rotulo>
        <Rotulo etiqueta="Operador" htmlFor={`${id}-operador`} {...de(["operador"])}>
          <input
            id={`${id}-operador`}
            list={`${id}-ops`}
            value={m.operador}
            onChange={(e) => cambiar(fijar(["operador"], e.target.value.toUpperCase()))}
            className={campo("operador")}
          />
          <datalist id={`${id}-ops`}>
            {OPERADORES.map((o) => (
              <option key={o} value={o} />
            ))}
          </datalist>
        </Rotulo>
        <div>
          <span className="block text-xs font-medium text-tenue mb-1.5" id={`${id}-modo`}>
            Modo
          </span>
          <Segmentado
            etiqueta="Modo"
            valor={m.modo}
            onCambio={(v) => cambiar(fijar(["modo"], v))}
            className="w-full grid grid-cols-2"
            opciones={[
              { id: "pantalla", texto: "Menús (pantalla)" },
              { id: "api", texto: "Un paso (API)" },
            ]}
          />
          <p className="mt-1 text-[11px] text-tenue/80">
            {m.modo === "api" ? "Más rápido y sin ventana, pero solo admite un paso marcar." : "Lee la ventana del teléfono: permite contestar menús."}
          </p>
          <MensajesModo />
        </div>
        <div className="md:col-span-2">
          <Rotulo etiqueta="Descripción" htmlFor={`${id}-desc`} {...de(["descripcion"])}>
            <AreaTexto
              id={`${id}-desc`}
              rows={Math.min(6, Math.max(2, Math.ceil(m.descripcion.length / 95) + 1))}
              value={m.descripcion}
              placeholder="Qué hace, cuánto cuesta, cuántas veces por día…"
              onChange={(e) => cambiar(fijar(["descripcion"], e.target.value, true))}
            />
          </Rotulo>
        </div>
        <div className="md:col-span-2">
          <Rotulo
            etiqueta="Nombres anteriores"
            htmlFor={`${id}-antes`}
            ayuda="Si la receta se renombró: los lotes y enlaces con el nombre viejo llegan a esta."
            {...de(["nombres_anteriores"])}
          >
            <Opciones
              id={`${id}-antes`}
              valores={m.nombres_anteriores}
              vacio="ninguno (escribe un nombre y Enter)"
              limpiar={(v) => v.toLowerCase().replace(/[\s_]+/g, "-").replace(/[^a-z0-9-]/g, "")}
              onCambio={(v) => cambiar(fijarNombresAnteriores(v))}
            />
          </Rotulo>
        </div>

        <div className="md:col-span-2 border-t border-borde/60 pt-4 grid gap-4 sm:grid-cols-2 lg:grid-cols-4 items-start">
          <Rotulo etiqueta="Acción (para lotes)" htmlFor={`${id}-accion`} ayuda="El lote busca la receta por operador y acción." {...de(["accion"])}>
            <input
              id={`${id}-accion`}
              list={`${id}-acc`}
              value={m.accion}
              placeholder="ninguna"
              onChange={(e) => cambiar(fijar(["accion"], e.target.value.trim(), true))}
              className={campo("accion", "font-mono text-[13px]")}
            />
            <datalist id={`${id}-acc`}>
              {ACCIONES_LOTE.map((o) => (
                <option key={o} value={o} />
              ))}
            </datalist>
          </Rotulo>
          <div className="sm:pt-6">
            <Interruptor
              activo={m.chip_tipo === "dealer"}
              onCambio={(v) => cambiar(fijar(["chip_tipo"], v ? "dealer" : null))}
              etiqueta={
                <span className="text-left">
                  Solo chips dealer
                  <span className="block text-[11px] text-tenue">Si no, vale para todos</span>
                </span>
              }
            />
          </div>
          {conLimites && (
            <>
              <Rotulo etiqueta="Máximo por día y chip" htmlFor={`${id}-pordia`} {...de(["limites", "limites.por_dia", "por_dia"])}>
                <CampoNumero
                  id={`${id}-pordia`}
                  valor={m.limites.por_dia}
                  placeholder="sin límite"
                  onCambio={(v) => cambiar((d) => {
                    fijar(["limites", "por_dia"], v ?? null)(d);
                    if (d.hasIn(["limites"]) && !d.hasIn(["limites", "por_dia"]) && !d.hasIn(["limites", "comision"])) d.delete("limites");
                  })}
                />
              </Rotulo>
              <Rotulo etiqueta="Comisión por envío (Bs)" htmlFor={`${id}-comision`} {...de(["limites.comision", "comision"])}>
                <CampoNumero
                  id={`${id}-comision`}
                  decimales
                  valor={m.limites.comision}
                  placeholder="0"
                  onCambio={(v) => cambiar((d) => {
                    fijar(["limites", "comision"], v ?? null)(d);
                    if (d.hasIn(["limites"]) && !d.hasIn(["limites", "por_dia"]) && !d.hasIn(["limites", "comision"])) d.delete("limites");
                  })}
                />
              </Rotulo>
            </>
          )}
        </div>
      </Tarjeta>
    </SeccionEditor>
  );
}

function MensajesModo() {
  const { errores } = useEditor();
  const e = problemasDe(errores, null, ["modo"]);
  return e.length ? <p className="mt-1 text-xs text-error">{e.map((x) => x.mensaje).join(" · ")}</p> : null;
}
