import { api } from "../../../api/cliente";
import type { Dispositivo, Lote } from "../../../api/tipos";
import { Boton, BotonDescarga, Insignia, MenuAcciones, Migas, Titulo, type AccionMenu } from "../../../componentes/base";
import { Download, Pause, Play } from "../../../componentes/iconos";
import { fechaHora, sinPrefijo } from "../../../util/datos";
import { EstadoLoteInsignia, chipsDeOrigen, describirOrigen, nombreChip } from "../comunes";

export type AccionLote = "iniciar" | "pausar" | "cancelar" | "archivar" | "reintentar" | "reintentar-inciertas";

/** Lo que puede hacer el botón principal según el estado del lote (una sola acción a la vez). */
export function accionPrincipal(l: Lote): "iniciar" | "reanudar" | "pausar" | null {
  if (l.estado === "en_curso") return "pausar";
  if (l.estado === "listo") return "iniciar";
  if (l.estado === "pausado" || (l.estado === "terminado" && l.progreso.sin_capacidad > 0)) return "reanudar";
  return null;
}

export function CabeceraLote({
  lote,
  dispositivos,
  accion,
  volver,
  alIniciar,
  alPausar,
  alCancelar,
  alArchivar,
}: {
  lote: Lote;
  dispositivos: Dispositivo[];
  accion: AccionLote | null;
  volver: () => void;
  alIniciar: () => void;
  alPausar: () => void;
  alCancelar: () => void;
  alArchivar: (archivado: boolean) => void;
}) {
  const principal = accionPrincipal(lote);
  const origen = describirOrigen(lote.origen, dispositivos);
  const menu: AccionMenu[] = [];
  if (lote.estado === "terminado" || lote.estado === "cancelado") {
    menu.push(
      lote.archivado
        ? { texto: "Sacar del archivo", alElegir: () => alArchivar(false) }
        : { texto: "Archivar", alElegir: () => alArchivar(true) },
    );
  }
  if (lote.estado === "listo" || lote.estado === "en_curso" || lote.estado === "pausado") {
    menu.push({ texto: "Cancelar lote…", peligro: true, alElegir: alCancelar });
  }

  return (
    <Titulo
      migas={<Migas tramos={[{ texto: "Lotes", ir: volver }, { texto: lote.nombre }]} />}
      titulo={lote.nombre}
      sub={
        <span className="inline-flex flex-wrap items-center gap-2">
          <EstadoLoteInsignia estado={lote.estado} />
          {lote.simular ? <Insignia tono="info">Simulación</Insignia> : <Insignia tono="aviso">Real</Insignia>}
          {lote.archivado && <Insignia>Archivado</Insignia>}
          {origen && (
            <span title={chipsDeOrigen(lote.origen).map((c) => `${sinPrefijo(c.numero) ?? nombreChip(dispositivos, c.dispositivo, c.slot)}${c.receta ? ` · ${c.receta}` : ""}`).join("\n")}>
              <Insignia tono="neutro">{origen}</Insignia>
            </span>
          )}
          <span>creado {fechaHora(lote.creado_ms)}</span>
        </span>
      }
      acciones={
        <>
          <BotonDescarga descargar={() => api.descargarResultado(lote.id)} icono={<Download className="size-4" />}>
            Descargar CSV
          </BotonDescarga>
          {principal === "pausar" && (
            <Boton variante="primario" icono={<Pause className="size-4" />} cargando={accion === "pausar"} onClick={alPausar}>
              Pausar
            </Boton>
          )}
          {(principal === "iniciar" || principal === "reanudar") && (
            <Boton variante="primario" icono={<Play className="size-4" />} cargando={accion === "iniciar"} onClick={alIniciar}>
              {principal === "iniciar" ? "Iniciar" : "Reanudar"}
            </Boton>
          )}
          <MenuAcciones acciones={menu} etiqueta="Más acciones del lote" />
        </>
      }
    />
  );
}
