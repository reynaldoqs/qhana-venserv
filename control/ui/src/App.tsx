import { useEffect, useState, useSyncExternalStore } from "react";
import { Icono, type NombreIcono } from "./componentes/iconos";
import { useConexion } from "./api/eventos";
import { Aviso, Boton, Campo, Punto, ProveedorToast, Tarjeta, cx } from "./componentes/base";
import { api, mensajeError, sesion } from "./api/cliente";
import { Paleta } from "./componentes/Paleta";
import { Search } from "./componentes/iconos";
import { useDispositivos } from "./util/datos";
import { Panel } from "./vistas/Panel";
import { ConsolaUssd } from "./vistas/ConsolaUssd";
import { Recetas } from "./vistas/recetas/Recetas";
import { frena } from "./util/guardia";
import { Mensajes } from "./vistas/Mensajes";
import { Actividad } from "./vistas/Actividad";
import { Manual } from "./vistas/Manual";
import { Lotes } from "./vistas/lotes/Lotes";

type Vista = "panel" | "lotes" | "ussd" | "recetas" | "sms" | "actividad" | "manual";

const NAV: { id: Vista; titulo: string; icono: NombreIcono }[] = [
  { id: "panel", titulo: "Teléfonos", icono: "panel" },
  { id: "lotes", titulo: "Lotes", icono: "lotes" },
  { id: "ussd", titulo: "Consola USSD", icono: "ussd" },
  { id: "recetas", titulo: "Recetas", icono: "recetas" },
  { id: "sms", titulo: "SMS", icono: "sms" },
  { id: "actividad", titulo: "Actividad", icono: "actividad" },
  { id: "manual", titulo: "Manual", icono: "manual" },
];

/** La vista vive en el hash (#ussd, #lotes/3) para poder recargar o compartir el enlace. */
function useVista(): [Vista, string, (v: Vista, sub?: string) => void] {
  const leer = (): [Vista, string] => {
    const [h, ...resto] = location.hash.replace("#", "").split("/");
    return [NAV.some((n) => n.id === h) ? (h as Vista) : "panel", resto.join("/")];
  };
  const [v, setV] = useState<[Vista, string]>(leer);
  useEffect(() => {
    let actual = location.hash;
    const f = () => {
      // Una vista con cambios sin guardar puede frenar la salida: el hash vuelve atrás sin cambiar de vista.
      if (frena(location.hash)) {
        history.replaceState(null, "", actual || "#");
        return;
      }
      actual = location.hash;
      setV(leer());
    };
    window.addEventListener("hashchange", f);
    return () => window.removeEventListener("hashchange", f);
  }, []);
  return [v[0], v[1], (n, sub) => (location.hash = sub ? `${n}/${sub}` : n)];
}

export default function App() {
  const auth = useSyncExternalStore(sesion.suscribir, sesion.estado);
  // Al arrancar se pregunta si el control pide token (solo cuando se entra desde otra máquina, modo Wi-Fi).
  useEffect(() => {
    api
      .salud()
      .then((s) => {
        if (s.requiere_token && !sesion.estado().token) sesion.pedir();
      })
      .catch(() => {
        /* sin control: la propia app muestra "Sin conexión" */
      });
  }, []);
  return <ProveedorToast>{auth.pide ? <PantallaToken rechazado={auth.rechazado} /> : <Aplicacion />}</ProveedorToast>;
}

/** El control escucha en la red y pide token: se escribe una vez y queda guardado en este navegador. */
function PantallaToken({ rechazado }: { rechazado: boolean }) {
  const [token, setToken] = useState("");
  const [probando, setProbando] = useState(false);
  const [error, setError] = useState<string | null>(rechazado ? "El control rechazó el token guardado. Escríbelo de nuevo." : null);
  const entrar = async (e: React.FormEvent) => {
    e.preventDefault();
    const t = token.trim();
    if (!t) return;
    setProbando(true);
    setError(null);
    sesion.fijar(t);
    try {
      await api.dispositivos(); // si no vale, responde 401 y vuelve a esta pantalla
    } catch (err) {
      setError(mensajeError(err));
    } finally {
      setProbando(false);
    }
  };
  return (
    <main className="min-h-full grid place-items-center bg-fondo p-4">
      <Tarjeta className="w-full max-w-sm p-6 entrar-vista">
        <div className="size-12 rounded-2xl bg-acento text-sobre-acento grid place-items-center font-extrabold text-lg shadow-acento mb-5">Q</div>
        <h1 className="text-xl font-extrabold tracking-tight">Token de acceso</h1>
        <p className="text-sm text-tenue mt-1">
          El control está abierto a la red y pide el token de la API (<span className="font-mono">--api-token</span>). Se guarda en este
          navegador.
        </p>
        <form onSubmit={entrar} className="mt-5 space-y-3">
          <Campo
            etiqueta="Token"
            type="password"
            autoComplete="current-password"
            autoFocus
            value={token}
            onChange={(e) => setToken(e.target.value)}
            className="font-mono"
          />
          {error && <Aviso>{error}</Aviso>}
          <Boton type="submit" variante="primario" className="w-full" cargando={probando} disabled={!token.trim()}>
            Entrar
          </Boton>
        </form>
      </Tarjeta>
    </main>
  );
}

function Aplicacion() {
  const [vista, sub, ir] = useVista();
  const conexion = useConexion();
  const { lista, cargando, error, recargar } = useDispositivos();
  const [consolaInicial, setConsolaInicial] = useState<{ dispositivo: string; slot: number } | null>(null);
  const enLinea = lista.filter((d) => d.conectado).length;
  const [paleta, setPaleta] = useState(false);

  // Cmd/Ctrl+K abre y cierra la paleta desde cualquier vista.
  useEffect(() => {
    const f = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setPaleta((p) => !p);
      }
    };
    window.addEventListener("keydown", f);
    return () => window.removeEventListener("keydown", f);
  }, []);

  /** Salta a la consola USSD o a los SMS de un teléfono/chip concreto. */
  const abrirChip = (id: string, slot = 0, destino: "ussd" | "sms" = "ussd") => {
    setConsolaInicial({ dispositivo: id, slot });
    ir(destino);
  };

  const indice = Math.max(0, NAV.findIndex((n) => n.id === vista));
  const actual = NAV[indice];
  const estadoConexion = conexion === "conectado" ? "ok" : conexion === "conectando" ? "aviso" : "error";

  return (
    <div className="h-full flex bg-fondo">
      {/* Riel de iconos: la etiqueta aparece al pasar el mouse. */}
      <aside className="hidden md:flex w-[88px] shrink-0 flex-col items-center py-5 text-lateral-texto">
        <div className="size-12 rounded-2xl bg-acento text-sobre-acento grid place-items-center font-extrabold text-lg shadow-acento">Q</div>

        <nav className="relative mt-10">
          {/* Marca deslizante: se mueve al ítem activo en vez de aparecer/desaparecer. */}
          <span
            aria-hidden
            className="absolute left-0 top-0 size-12 rounded-2xl bg-panel-2 transition-transform duration-400 ease-resorte"
            style={{ transform: `translateY(${indice * 56}px)` }}
          />
          <span
            aria-hidden
            className="absolute -left-[22px] top-3 h-6 w-[3px] rounded-full bg-texto transition-transform duration-400 ease-resorte"
            style={{ transform: `translateY(${indice * 56}px)` }}
          />
          {NAV.map((n) => {
            const activo = vista === n.id;
            return (
              <button
                key={n.id}
                onClick={() => ir(n.id)}
                aria-label={n.titulo}
                aria-current={activo ? "page" : undefined}
                className={cx(
                  "group relative size-12 mb-2 rounded-2xl grid place-items-center transition-colors duration-200",
                  activo ? "text-texto" : "text-lateral-tenue hover:text-lateral-texto",
                )}
              >
                <Icono nombre={n.icono} className="size-[22px] transition-transform duration-300 ease-resorte group-hover:scale-110" />
                {n.id === "panel" && enLinea > 0 && (
                  <span className="absolute top-1.5 right-1.5 size-2 rounded-full bg-ok" />
                )}
                <span className="pointer-events-none absolute left-full ml-4 z-50 whitespace-nowrap rounded-xl bg-panel-2 px-3 py-1.5 text-xs font-semibold text-texto opacity-0 -translate-x-1 shadow-elevada transition-[opacity,transform] duration-200 group-hover:opacity-100 group-hover:translate-x-0 group-focus-visible:opacity-100 group-focus-visible:translate-x-0">
                  {n.titulo}
                </span>
              </button>
            );
          })}
        </nav>

        <div className="group relative mt-auto size-12 rounded-2xl grid place-items-center" aria-hidden>
          <Punto tono={estadoConexion} pulso={conexion !== "conectado"} />
          <span className="pointer-events-none absolute left-full ml-4 z-50 whitespace-nowrap rounded-xl bg-panel-2 px-3 py-1.5 text-xs font-semibold opacity-0 shadow-elevada transition-opacity duration-200 group-hover:opacity-100">
            {conexion === "conectado" ? "Control en línea" : conexion === "conectando" ? "Conectando…" : "Sin conexión con el control"}
          </span>
        </div>
      </aside>

      <div className="flex-1 min-w-0 flex flex-col">
        <main className="flex-1 overflow-y-auto md:m-3 md:ml-0 md:rounded-[2rem] bg-lienzo pb-20 md:pb-0">
          <div className="max-w-[88rem] mx-auto px-5 md:px-10 py-7 min-h-full flex flex-col">
            <div className="flex items-center justify-between gap-3 mb-6">
              <span className="text-[11px] uppercase tracking-[0.18em] text-tenue">Qhana · {actual.titulo}</span>
              <div className="flex items-center gap-2">
                <button
                  onClick={() => setPaleta(true)}
                  className="inline-flex items-center gap-2.5 h-9 pl-3.5 pr-2.5 rounded-full bg-panel text-xs font-medium text-tenue hover:text-texto transition-colors"
                >
                  <Search className="size-4" />
                  <span className="hidden sm:inline">Buscar</span>
                  <kbd className="hidden sm:inline rounded-md bg-panel-2 px-1.5 py-0.5 text-[11px]">⌘K</kbd>
                </button>
                <span
                  role="status"
                  aria-live="polite"
                  className="inline-flex items-center gap-2 h-9 px-4 rounded-full bg-panel text-xs font-semibold tabular-nums"
                  title={`${enLinea} de ${lista.length} teléfonos en línea`}
                >
                  <Punto tono={estadoConexion} pulso={conexion !== "conectado"} />
                  {conexion === "conectado" ? "En línea" : conexion === "conectando" ? "Conectando…" : "Sin conexión"}
                  <span className="text-tenue font-medium">
                    {enLinea}/{lista.length}
                    <span className="sr-only"> teléfonos en línea</span>
                  </span>
                </span>
              </div>
            </div>
            <div key={vista} className="entrar-vista flex-1 min-h-0">
              {vista === "panel" && (
                <Panel
                  dispositivos={lista}
                  cargando={cargando}
                  error={error}
                  recargar={recargar}
                  abrir={abrirChip}
                />
              )}
              {vista === "lotes" && <Lotes ruta={sub} ir={(r) => ir("lotes", r)} dispositivos={lista} />}
              {vista === "ussd" && <ConsolaUssd dispositivos={lista} inicial={consolaInicial} />}
              {vista === "recetas" && <Recetas ruta={sub} ir={(r) => ir("recetas", r)} dispositivos={lista} />}
              {vista === "sms" && <Mensajes dispositivos={lista} inicial={consolaInicial?.dispositivo} />}
              {vista === "actividad" && <Actividad />}
              {vista === "manual" && <Manual seccion={sub} ir={(r) => ir("manual", r)} />}
            </div>
          </div>
        </main>
      </div>

      <nav aria-label="Navegación" className="md:hidden fixed bottom-0 inset-x-0 z-40 flex bg-lienzo/95 backdrop-blur border-t border-borde pb-[env(safe-area-inset-bottom)]">
        {NAV.map((n) => (
          <button
            key={n.id}
            onClick={() => ir(n.id)}
            aria-current={vista === n.id ? "page" : undefined}
            className={cx(
              "flex-1 min-w-0 flex flex-col items-center gap-1 pt-2.5 pb-2 text-[11px] leading-tight font-medium transition-colors",
              vista === n.id ? "text-texto" : "text-tenue",
            )}
          >
            <Icono nombre={n.icono} className="size-5" />
            <span className="truncate max-w-full">{n.titulo.replace("Consola ", "")}</span>
          </button>
        ))}
      </nav>

      <Paleta
        abierta={paleta}
        cerrar={() => setPaleta(false)}
        vistas={NAV}
        dispositivos={lista}
        irVista={(id) => ir(id as Vista)}
        abrirChip={abrirChip}
      />
    </div>
  );
}
