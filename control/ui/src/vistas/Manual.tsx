import { useEffect, type ReactNode } from "react";
import { api } from "../api/cliente";
import { Aviso, BotonDescarga, Insignia, Tarjeta, Titulo, cx } from "../componentes/base";
import { Download } from "../componentes/iconos";

type Tono = "neutro" | "ok" | "aviso" | "error" | "info";

/** Una fila de glosario: la insignia tal como se ve en pantalla + qué significa + qué hacer. */
type Estado = [tono: Tono, etiqueta: string, significa: string, hacer?: string];

const SECCIONES = [
  ["inicio", "Primeros pasos"],
  ["telefonos", "Teléfonos y chips"],
  ["csv", "El CSV del lote"],
  ["recargas", "Recargas y chips dealer"],
  ["validacion", "Validar antes de ejecutar"],
  ["lote", "Estados del lote"],
  ["filas", "Estados de cada fila"],
  ["verificar", "Verificar saldos"],
  ["resultado", "CSV de resultados"],
  ["chips", "Cupos, saldos y chips dealer"],
  ["catalogo", "Catálogo de recetas"],
  ["ussd", "USSD, recetas y SMS"],
  ["editar", "Editar recetas"],
  ["problemas", "Si algo sale mal"],
] as const;


type RecetaDoc = {
  nombre: string;
  titulo: string;
  operador: string;
  tipo: "Consulta" | "Mueve dinero";
  codigo: string;
  pasos: string;
  params: string;
  salida: string;
  nota: string;
  dealer?: boolean;
};

const RECETAS: RecetaDoc[] = [
  {
    nombre: "tigo-saldo",
    titulo: "Saldo Tigo",
    operador: "TIGO",
    tipo: "Consulta",
    codigo: "*105#",
    pasos: "Marca *105# y lee la respuesta.",
    params: "—",
    salida: "linea, saldo_total, saldo_principal, doble_carga",
    nota: "Gratis. Tigo deja la sesión abierta, por eso va en modo pantalla. Es la que alimenta el saldo de los lotes.",
  },
  {
    nombre: "tigo-movimientos",
    titulo: "Últimas transferencias Tigo",
    operador: "TIGO",
    tipo: "Consulta",
    codigo: "*66#1#",
    pasos: "Marca *66#1# y elige «Ultimos movimientos».",
    params: "—",
    salida: "El texto completo de la lista",
    nota: "Útil para investigar una diferencia al verificar o un envío incierto.",
  },
  {
    nombre: "tigo-transferir",
    titulo: "Transferir saldo Tigo",
    operador: "TIGO",
    tipo: "Mueve dinero",
    codigo: "*66#1#",
    pasos: "Marca *66#1# → «Transferir por unica vez» → número → monto → verifica el resumen → confirma «Si».",
    params: "numero (8 dígitos, empieza en 6 o 7) · monto: 1, 2, 5, 10 o 20 Bs",
    salida: "costo",
    nota: "Comisión 0.20 Bs, máximo 5 por día por chip. Es la receta que usan los lotes. Si el operador dice «máximo de transferencias», el chip queda agotado hoy.",
  },
  {
    nombre: "entel-saldo",
    titulo: "Saldo Entel",
    operador: "ENTEL",
    tipo: "Consulta",
    codigo: "*105# y *#66#",
    pasos: "Marca *105# (saldo), cierra, y marca *#66# (número de la línea).",
    params: "—",
    salida: "saldo_principal, linea",
    nota: "Los chips de Entel no traen su número grabado: se lee con *#66#.",
  },
  {
    nombre: "entel-transferencias",
    titulo: "Últimas transferencias Entel",
    operador: "ENTEL",
    tipo: "Consulta",
    codigo: "*133#",
    pasos: "Marca *133# → «Consulta de transferencia» → «Ultimas transferencias».",
    params: "—",
    salida: "ultima (número, monto, fecha y hora)",
    nota: "Solo consulta.",
  },
  {
    nombre: "entel-dealer-recargar",
    titulo: "Recargar saldo (Entel dealer)",
    operador: "ENTEL",
    tipo: "Mueve dinero",
    codigo: "*133*número*monto#",
    pasos: "Marca el código completo, verifica «recargar Bs … al telefono», confirma «Si».",
    params: "numero (8 dígitos) · monto en Bs: 0.50 o enteros (1, 5, 100…)",
    salida: "saldo_transferencia, monto_bs",
    nota: "Solo para chips marcados dealer. El código lleva Bs enteros (*133*N*1# = Bs 1); la única fracción es 0.50, que se marca 050. Otros centavos (1.50, 0.25) son error. Cada fila de un lote es un solo envío: Bs 100 se marca *133*N*100#.",
    dealer: true,
  },
  {
    nombre: "entel-dealer-saldo",
    titulo: "Saldo dealer",
    operador: "ENTEL",
    tipo: "Consulta",
    codigo: "*133# → 1",
    pasos: "Marca *133# y elige «Saldo para transferencia».",
    params: "—",
    salida: "saldo_dealer",
    nota: "Solo chips Entel marcados dealer. No pide PIN ni mueve dinero. Es el saldo que gastan las recargas y con el que se cuadra un lote de recargas Entel. El texto de la respuesta aún no está medido.",
    dealer: true,
  },
  {
    nombre: "tigo-dealer-saldo",
    titulo: "Saldo dealer",
    operador: "TIGO",
    tipo: "Consulta",
    codigo: "*555# → 8",
    pasos: "Marca *555#, elige 8 y, al pedir el PIN, envía el PIN del dealer.",
    params: "pin (se toma del PIN guardado en el chip)",
    salida: "saldo_dealer",
    nota: "Solo chips Tigo marcados dealer. No mueve dinero, pero envía el PIN: varios PIN errados pueden bloquear el dealer. El control no la corre solo (no es la consulta de saldo automática). Textos de *555# aún sin medir.",
    dealer: true,
  },
  {
    nombre: "tigo-dealer-recargar",
    titulo: "Recarga de crédito dealer",
    operador: "TIGO",
    tipo: "Mueve dinero",
    codigo: "*555# → 3 → 2",
    pasos: "Marca *555#, elige 3 y luego 2, envía el monto en Bs, el número del cliente y, al pedir el PIN, el PIN del dealer.",
    params: "numero (8 dígitos) · monto en Bs (p. ej. 1.50) · pin (se toma del PIN guardado en el chip)",
    salida: "—",
    nota: "Solo para chips Tigo marcados dealer. El PIN se guarda con «Guardar PIN» en la tarjeta del chip y nunca aparece en el historial. Simular se detiene antes de enviar el PIN. Los textos de *555# aún no están medidos: la receta solo exige que la pantalla antes del PIN pida el PIN.",
    dealer: true,
  },
  {
    nombre: "viva-saldo",
    titulo: "Saldo Viva",
    operador: "VIVA",
    tipo: "Consulta",
    codigo: "*105#",
    pasos: "Marca *105# y lee la respuesta; el motor cierra el menú que Viva deja abierto.",
    params: "—",
    salida: "linea, saldo_total, saldo_principal, datos",
    nota: "Gratis y solo consulta.",
  },
  {
    nombre: "viva-dealer-saldo",
    titulo: "Saldo dealer",
    operador: "VIVA",
    tipo: "Consulta",
    codigo: "*604#",
    pasos: "Marca *604# y lee el saldo de la respuesta.",
    params: "—",
    salida: "saldo_dealer",
    nota: "Solo chips Viva marcados dealer. No mueve dinero. La pantalla aún no está medida: si *604# muestra un menú o pide PIN, termina en error sin enviar nada y hay que ajustar la receta.",
    dealer: true,
  },
  {
    nombre: "viva-dealer-recargar",
    titulo: "Recarga de crédito dealer",
    operador: "VIVA",
    tipo: "Mueve dinero",
    codigo: "*601*PIN*número*monto#",
    pasos: "Marca el código completo. No hay pantalla de confirmación: la recarga se hace al marcar.",
    params: "numero (8 dígitos) · monto en Bs enteros (1, 5, 100…) · pin (se toma del PIN guardado en el chip)",
    salida: "saldo_dealer (si la respuesta lo trae)",
    nota: "Solo chips Viva marcados dealer, con PIN guardado. Simular no marca nada (no hay cómo llegar a una confirmación). Centavos (1.50) son error. El PIN va dentro del código pero queda tapado en el historial. La respuesta aún no está medida: hasta ajustarla, una recarga real puede quedar «Incierta»; revisa el saldo.",
    dealer: true,
  },
];

const EJEMPLO = `numero;monto;operador;referencia
69869869;12;TIGO;Pedido 1001
+591 7881-5232;Bs 5;tigo;Pedido 1002
71234567;20;TIGO;Pedido 1003`;

const EJEMPLO_RECARGAS = `numero,monto,operador,accion,referencia
69869869,0.50,ENTEL,recargar,Cliente 1
78815232,5,TIGO,recargar,Cliente 2
71234567,12.30,TIGO,recargar,Cliente 3`;

const EJEMPLO_MIXTO = `numero,monto,operador,accion,referencia
69869869,10,TIGO,transferir,Pedido 1001
78815232,5,TIGO,recargar,Pedido 1002
71234567,0.50,ENTEL,recargar,Pedido 1003`;

const EJEMPLO_CHIPS = `numero,monto,operador,referencia
69869869,5,TIGO,Cliente 1
71234567,0.50,ENTEL,Cliente 2`;

type TipoLoteDoc = { titulo: string; quien: string; obligatorias: string; opcionales: string; ejemplo: string; nota: string };

const TIPOS_LOTE: TipoLoteDoc[] = [
  {
    titulo: "1. Transferencias",
    quien: "Cada fila va a los chips de su operador que estén en línea.",
    obligatorias: "numero, monto, operador",
    opcionales: "accion (vacía = transferir), referencia",
    ejemplo: `numero,monto,operador
69869869,10,TIGO`,
    nota: "Transferir Tigo solo tiene 1, 2, 5, 10 y 20 Bs: 12 se manda como 10 + 2 (dos envíos, dos comisiones) y 12.50 es error.",
  },
  {
    titulo: "2. Recargas de varios operadores",
    quien: "Cada fila va a los chips dealer de su operador. Tigo, Entel y Viva trabajan a la vez.",
    obligatorias: "numero, monto, operador, accion (= recargar en cada fila)",
    opcionales: "referencia",
    ejemplo: `numero,monto,operador,accion
71234567,100,ENTEL,recargar`,
    nota: "accion es obligatoria: una fila sin accion se toma como transferencia (la pantalla avisa «Este CSV no dice «recargar»»). Tigo dealer acepta centavos (12.30) y necesita PIN guardado; Entel dealer acepta 0.50 o enteros y no pide PIN; Viva dealer acepta solo enteros y necesita PIN guardado.",
  },
  {
    titulo: "3. Desde chips elegidos",
    quien: "Marcas uno o varios chips y la acción en la pantalla; el lote se reparte solo entre ellos.",
    obligatorias: "numero, monto (y operador si los chips son de varios operadores)",
    opcionales: "referencia",
    ejemplo: `numero,monto
69869869,5`,
    nota: "La acción se elige en la pantalla: una columna accion se ignora. Un chip que no sirve para esa acción (normal para recargar, Tigo dealer sin PIN) queda fuera con el motivo.",
  },
];

const COLUMNAS: [string, string, string][] = [
  ["numero", "Sí", "8 dígitos. Se limpian espacios, guiones, puntos y el prefijo +591 o 591."],
  ["monto", "Sí", "Bs con hasta 2 decimales: 12, 12,00, «Bs 12», 1.50. Con centavos solo si la receta no tiene montos fijos: transferir Tigo (1, 2, 5, 10, 20) no los acepta; la recarga dealer sí."],
  ["operador", "Sí (en «Desde chips elegidos», solo si los chips son de varios operadores)", "TIGO, ENTEL o VIVA (también NUEVATEL). Mayúsculas y acentos no importan. No se deduce del número porque hay portabilidad."],
  ["accion", "Solo en «Recargas de varios operadores»", "Qué hacer: «transferir» (si falta) o «recargar». En «Recargas de varios operadores» cada fila debe decir «recargar». En «Desde chips elegidos» manda la acción elegida y la columna se ignora."],
  ["referencia", "No", "Texto libre (n.º de pedido, nombre). Vuelve igual en el CSV de resultados."],
];

const ESTADOS_SIM: Estado[] = [
  ["ok", "Lista", "El chip tiene servicio de voz y puede marcar USSD.", "Nada, está bien."],
  ["neutro", "Sin chip", "La ranura está vacía.", "Pon el chip."],
  ["aviso", "Pide PIN", "El chip tiene PIN activo y el teléfono no lo tiene.", "Desbloquéalo a mano en el teléfono (o quítale el PIN)."],
  ["error", "Bloqueada", "El chip pide PUK o está bloqueado.", "Se resuelve con el operador; el control no puede."],
  ["error", "Sin voz", "El chip está, pero sin señal o con la radio apagada.", "Revisa antena, modo avión o saldo/plan del operador."],
  ["error", "Solo emergencia", "Sin red del operador: solo permite llamadas de emergencia.", "Suele ser falta de cobertura o chip suspendido."],
  ["aviso", "Desconocido", "Android no informó el estado del chip.", "Espera unos segundos; si sigue, reconecta el teléfono."],
];

const ESTADOS_VALIDACION: Estado[] = [
  ["ok", "Válida", "La fila está bien y tiene chips con capacidad."],
  ["aviso", "Aviso", "Es válida pero hay algo que revisar: número repetido en el lote o monto alto (más de 100 Bs). Puede ser intencional."],
  ["error", "Error", "No se ejecutará. El motivo aparece en la fila: número mal formado, monto con centavos o que no se puede formar con los montos permitidos, operador desconocido, o no hay receta para ese operador y acción."],
];

const ESTADOS_LOTE: Estado[] = [
  ["neutro", "Listo", "Creado y validado, aún no se inició.", "Pulsa Iniciar."],
  ["info", "En curso", "Se están enviando las filas. Cada teléfono hace un envío a la vez y los teléfonos trabajan en paralelo.", "Espera, o pausa si hace falta."],
  ["aviso", "Pausado", "Lo pausaste tú, se reinició el control, o hubo un envío incierto (el motivo se muestra arriba).", "Resuelve el motivo y reanuda."],
  ["ok", "Terminado", "El ejecutor ya no tiene nada más que intentar. No significa que todo salió bien.", "Mira el banner de revisión: dice si quedó completo o qué falta."],
  ["neutro", "Cancelado", "Lo detuviste definitivamente.", "Crea un lote nuevo si hace falta."],
];

const ESTADOS_FILA: Estado[] = [
  ["neutro", "Omitida", "La fila era inválida y se dejó fuera del lote. No se ejecuta."],
  ["neutro", "Pendiente", "Todavía no se intentó."],
  ["info", "En curso", "Se está enviando ahora."],
  ["ok", "OK", "Todos los envíos de la fila se confirmaron."],
  ["info", "Simulada", "Lote de simulación: llegó hasta la confirmación y respondió «No». No movió dinero."],
  ["aviso", "Parcial", "Una fila de 12 Bs se parte en 10 + 2. Parcial = algunos envíos salieron y otros no.", "Reintenta fallidas: solo repite las piezas fallidas, nunca las OK."],
  ["error", "Fallida", "No se movió dinero (falló antes de confirmar).", "Es seguro reintentar."],
  ["error", "Incierta", "Se confirmó, pero no llegó la respuesta del operador. No se sabe si el dinero salió. El lote sigue con las demás filas.", "Revisa el saldo del chip y el SMS del operador. Nunca se reintenta sola: si no llegó, usa «Reintentar inciertas» (pide confirmar que lo revisaste)."],
  ["aviso", "Para mañana", "Ningún chip tiene cupo o saldo para esta fila.", "Vuelve a Iniciar otro día (o con más saldo): retoma solo lo pendiente."],
];

const ESTADOS_CUADRE: Estado[] = [
  ["ok", "Cuadra", "Lo que bajó el saldo coincide con lo enviado (±0.10 Bs)."],
  ["error", "Diferencia", "Se descontó más o menos de lo esperado. Positivo = de más.", "Mira el historial del chip: recargas o envíos a mano también cuentan."],
  ["neutro", "Sin dato", "No hubo saldo inicial o no se pudo consultar el actual."],
];

function Fila({ e }: { e: Estado }) {
  const [tono, etiqueta, significa, hacer] = e;
  return (
    <tr className="border-t border-borde/50 align-top">
      <td className="py-3 pr-4 whitespace-nowrap">
        <Insignia tono={tono}>{etiqueta}</Insignia>
      </td>
      <td className="py-3 pr-4 text-sm">{significa}</td>
      {hacer !== undefined && <td className="py-3 text-sm text-tenue">{hacer}</td>}
    </tr>
  );
}

function TablaEstados({ filas, conAccion = true }: { filas: Estado[]; conAccion?: boolean }) {
  const hay = conAccion && filas.some((f) => f[3]);
  return (
    <div className="overflow-x-auto">
      <table className="w-full text-left">
        <thead>
          <tr className="text-[10px] uppercase tracking-[0.14em] text-tenue">
            <th className="pb-2 pr-4 font-semibold">Se ve</th>
            <th className="pb-2 pr-4 font-semibold">Significa</th>
            {hay && <th className="pb-2 font-semibold">Qué hacer</th>}
          </tr>
        </thead>
        <tbody>
          {filas.map((f) => (
            <Fila key={f[1]} e={hay ? [f[0], f[1], f[2], f[3] ?? ""] : [f[0], f[1], f[2]]} />
          ))}
        </tbody>
      </table>
    </div>
  );
}

function Seccion({ id, titulo, intro, children }: { id: string; titulo: string; intro?: ReactNode; children: ReactNode }) {
  return (
    <section id={`manual-${id}`} className="scroll-mt-6">
      <Tarjeta className="p-6 md:p-8">
        <h2 className="text-xl font-extrabold tracking-tight">{titulo}</h2>
        {intro && <p className="text-sm text-tenue mt-1.5 max-w-2xl">{intro}</p>}
        <div className="mt-5 space-y-5 text-sm leading-relaxed">{children}</div>
      </Tarjeta>
    </section>
  );
}

const Sub = ({ children }: { children: ReactNode }) => (
  <h3 className="text-[11px] font-semibold uppercase tracking-[0.14em] text-tenue">{children}</h3>
);

const Codigo = ({ children }: { children: ReactNode }) => (
  <code className="rounded-md bg-panel-2 px-1.5 py-0.5 font-mono text-[12px]">{children}</code>
);

function Pasos({ items }: { items: ReactNode[] }) {
  return (
    <ol className="space-y-2.5">
      {items.map((t, i) => (
        <li key={i} className="flex gap-3">
          <span className="size-6 shrink-0 rounded-full bg-panel-2 text-xs font-bold grid place-items-center tabular-nums">{i + 1}</span>
          <span className="pt-0.5">{t}</span>
        </li>
      ))}
    </ol>
  );
}

export function Manual({ seccion, ir }: { seccion: string; ir: (id: string) => void }) {
  // La sección va en el hash (#manual/csv): al entrar o cambiar, baja hasta ella.
  useEffect(() => {
    if (!seccion) return;
    document.getElementById(`manual-${seccion}`)?.scrollIntoView({ behavior: "smooth", block: "start" });
  }, [seccion]);

  return (
    <div>
      <Titulo titulo="Manual" sub="Cómo preparar un lote, qué significa cada estado y qué hacer cuando algo no sale." />

      <div className="grid lg:grid-cols-[13rem_1fr] gap-6 items-start">
        <nav aria-label="Secciones del manual" className="lg:sticky lg:top-4 flex lg:flex-col gap-1 overflow-x-auto pb-1 lg:pb-0">
          {SECCIONES.map(([id, nombre]) => (
            <button
              key={id}
              onClick={() => ir(id)}
              className={cx(
                "shrink-0 text-left rounded-xl px-3 py-2 text-sm font-medium transition-colors",
                seccion === id ? "bg-panel-2 text-texto" : "text-tenue hover:text-texto hover:bg-panel",
              )}
            >
              {nombre}
            </button>
          ))}
        </nav>

        <div className="space-y-5 min-w-0">
          <Seccion id="inicio" titulo="Primeros pasos" intro="El camino normal para enviar un lote, de principio a fin.">
            <Pasos
              items={[
                <>
                  En <b>Teléfonos</b> confirma que los teléfonos estén en línea y sus chips en <Insignia tono="ok">Lista</Insignia>.
                </>,
                <>
                  En <b>Lotes → Nuevo</b> carga tu CSV (formato en <button className="underline" onClick={() => ir("csv")}>El CSV del lote</button>).
                </>,
                <>
                  Pulsa <b>Validar</b>: no toca ningún teléfono (salvo consultar saldos, que es solo lectura). Corrige las filas con <Insignia tono="error">Error</Insignia>.
                </>,
                <>
                  Crea el lote en modo <b>Simular</b> primero. Recorre todo el flujo pero responde «No» en la confirmación: no mueve dinero.
                </>,
                <>
                  Cuando la simulación salga bien, crea el lote real, <b>Inicia</b> y mira el progreso.
                </>,
                <>
                  Al terminar, revisa el banner, <b>verifica los saldos</b> y descarga el CSV de resultados.
                </>,
              ]}
            />
            <Aviso tono="aviso">
              Un lote real <b>mueve dinero de verdad</b>. Simula siempre antes. Un envío nunca se reintenta después de confirmar.
            </Aviso>
          </Seccion>

          <Seccion id="telefonos" titulo="Teléfonos y chips" intro="Cada teléfono tiene dos ranuras. El estado de un chip sale de lo que informa Android.">
            <TablaEstados filas={ESTADOS_SIM} />
            <Sub>Otras señales de la tarjeta del teléfono</Sub>
            <ul className="list-disc pl-5 space-y-1.5 text-tenue">
              <li>
                <b className="text-texto">Accesibilidad inactiva</b>: la app agente necesita el servicio de accesibilidad para leer los menús USSD. Sin él no se puede operar.
              </li>
              <li>
                <b className="text-texto">Permisos faltantes</b>: la app lista cuáles. Se dan con el script de provisión.
              </li>
              <li>
                <b className="text-texto">Batería</b> en rojo (menos de 20 % sin cargar): conecta el cargador antes de un lote largo.
              </li>
              <li>
                <b className="text-texto">En línea n/m</b> (arriba a la derecha): teléfonos conectados al control sobre el total conocido.
              </li>
              <li>
                <b className="text-texto">Aceptar depuración en el teléfono</b> / <b className="text-texto">Sin túnel</b>: el teléfono está por USB pero la app aún no se conecta. Acepta el aviso de depuración en la pantalla del teléfono.
              </li>
            </ul>
          </Seccion>

          <Seccion id="csv" titulo="El CSV del lote" intro="Una fila por operación. Se puede armar en Excel y guardar como CSV.">
            <Sub>Los tres tipos de lote</Sub>
            <p className="text-tenue">
              En <b className="text-texto">Lotes → Nuevo</b> eliges el tipo. Cambia quién envía y qué columnas lleva el CSV. En los tres el monto va en Bs.
            </p>
            <div className="grid gap-3 md:grid-cols-3">
              {TIPOS_LOTE.map((t) => (
                <div key={t.titulo} className="rounded-2xl bg-panel-2/60 p-4 flex flex-col gap-2.5 min-w-0">
                  <h4 className="font-semibold">{t.titulo}</h4>
                  <p className="text-tenue text-[13px]">{t.quien}</p>
                  <div>
                    <div className="text-[10px] uppercase tracking-[0.14em] text-tenue font-semibold">Obligatorias</div>
                    <div className="font-mono text-[12px] mt-0.5 break-words">{t.obligatorias}</div>
                  </div>
                  <div>
                    <div className="text-[10px] uppercase tracking-[0.14em] text-tenue font-semibold">Opcionales</div>
                    <div className="font-mono text-[12px] mt-0.5 break-words">{t.opcionales}</div>
                  </div>
                  <pre className="rounded-xl bg-black/25 p-3 font-mono text-[12px] overflow-x-auto">{t.ejemplo}</pre>
                  <p className="text-tenue text-[12px]">{t.nota}</p>
                </div>
              ))}
            </div>

            <Sub>Columnas</Sub>
            <div className="overflow-x-auto">
              <table className="w-full text-left">
                <thead>
                  <tr className="text-[10px] uppercase tracking-[0.14em] text-tenue">
                    <th className="pb-2 pr-4 font-semibold">Columna</th>
                    <th className="pb-2 pr-4 font-semibold">Obligatoria</th>
                    <th className="pb-2 font-semibold">Acepta</th>
                  </tr>
                </thead>
                <tbody>
                  {COLUMNAS.map(([c, o, a]) => (
                    <tr key={c} className="border-t border-borde/50 align-top">
                      <td className="py-3 pr-4">
                        <Codigo>{c}</Codigo>
                      </td>
                      <td className="py-3 pr-4 text-tenue">{o}</td>
                      <td className="py-3">{a}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>

            <Sub>Ejemplo</Sub>
            <pre className="rounded-xl bg-panel-2 p-4 font-mono text-[12px] overflow-x-auto">{EJEMPLO}</pre>
            <BotonDescarga descargar={() => api.descargarPlantilla()} className="h-9 px-4 text-xs" icono={<Download className="size-4" />}>
              Descargar plantilla.csv
            </BotonDescarga>

            <Sub>Reglas del archivo</Sub>
            <ul className="list-disc pl-5 space-y-1.5">
              <li>
                Separador <Codigo>,</Codigo> o <Codigo>;</Codigo>: se detecta solo (Excel en español usa <Codigo>;</Codigo>).
              </li>
              <li>UTF-8, con o sin BOM.</li>
              <li>
                Los encabezados son flexibles: <Codigo>número</Codigo>, <Codigo>teléfono</Codigo>, <Codigo>celular</Codigo>, <Codigo>cantidad</Codigo>, <Codigo>importe</Codigo>, <Codigo>operadora</Codigo>, <Codigo>ref</Codigo>, <Codigo>nota</Codigo>… también valen.
              </li>
              <li>
                Si la primera fila no parece encabezado, se asume el orden <Codigo>numero, monto, operador</Codigo>.
              </li>
              <li>Máximo 5 000 filas por lote.</li>
              <li>
                <b>Desde chips elegidos</b>: en Nuevo lote elige ese tipo, marca uno o varios chips y la acción (recargar o transferir). Ver <a href="#manual/recargas" className="underline underline-offset-2">Recargas y chips dealer</a>.
              </li>
              <li>
                <b>Con montos fijos, los montos grandes se parten</b> en los que permite la receta (transferir Tigo: 12 → 10 + 2; 37 → 20 + 10 + 5 + 2), usando la menor cantidad de envíos. Cada envío cuesta comisión. Las recargas dealer no tienen montos fijos: cada fila es un solo envío.
              </li>
            </ul>
          </Seccion>

          <Seccion
            id="recargas"
            titulo="Lotes de recargas y chips dealer"
            intro="Recargas de crédito desde chips dealer, de uno o varios operadores, en paralelo."
          >
            <Sub>Los montos van en Bs</Sub>
            <p>
              Siempre en Bs, con punto o coma: <Codigo>0.50</Codigo>, <Codigo>5</Codigo>, <Codigo>12.30</Codigo>. Cada receta lo traduce sola a lo que
              pide su USSD (Entel dealer marca <Codigo>050</Codigo> para Bs 0.50 y el entero tal cual: Bs 100 → <Codigo>*133*N*100#</Codigo>). Una receta con montos fijos (transferir Tigo: 1, 2, 5, 10, 20) no
              acepta centavos.
            </p>

            <Sub>Recargas de varios operadores</Sub>
            <p>
              Tipo «Recargas de varios operadores». Cada fila dice su operador y <Codigo>accion</Codigo> <Codigo>recargar</Codigo>; va a los chips dealer
              de ese operador (los normales no corren recetas dealer). Un chip dealer cuya receta pide PIN y no lo tiene guardado no recibe envíos.
            </p>
            <pre className="rounded-xl bg-panel-2 p-4 font-mono text-[12px] overflow-x-auto">{EJEMPLO_RECARGAS}</pre>

            <Sub>Transferencias y recargas mezcladas</Sub>
            <p>
              En «Transferencias», la columna <Codigo>accion</Codigo> decide fila por fila: <Codigo>transferir</Codigo> (o vacía) usa los chips del
              operador; <Codigo>recargar</Codigo>, sus chips dealer.
            </p>
            <pre className="rounded-xl bg-panel-2 p-4 font-mono text-[12px] overflow-x-auto">{EJEMPLO_MIXTO}</pre>

            <Sub>Desde chips elegidos</Sub>
            <p>
              Marcas los chips (p. ej. «Solo los dealer») y la acción, que vale para todas las filas. Si todos los chips son de un operador basta con{" "}
              <Codigo>numero,monto</Codigo>; si son de varios, va la columna <Codigo>operador</Codigo>. Un chip elegido que no sirve (sin PIN, sin
              receta) queda fuera con un aviso si los demás alcanzan.
            </p>
            <pre className="rounded-xl bg-panel-2 p-4 font-mono text-[12px] overflow-x-auto">{EJEMPLO_CHIPS}</pre>
            <div className="flex flex-wrap gap-2">
              {(["recargas", "chips"] as const).map((t) => (
                <BotonDescarga key={t} descargar={() => api.descargarPlantilla(t)} className="h-9 px-4 text-xs" icono={<Download className="size-4" />}>
                  Plantilla de {t === "recargas" ? "recargas" : "chips elegidos"}
                </BotonDescarga>
              ))}
            </div>

            <Sub>Cómo trabajan en paralelo</Sub>
            <ul className="list-disc pl-5 space-y-1.5">
              <li>
                Cada <b>teléfono</b> hace un envío a la vez (Android permite una sola sesión USSD por teléfono). Los teléfonos trabajan en paralelo,
                sin importar el operador.
              </li>
              <li>Los dos chips de un mismo teléfono se turnan: mientras uno envía, el otro espera.</li>
              <li>Con un solo chip, el lote va en serie. Para ir más rápido, reparte los chips dealer en teléfonos distintos.</li>
              <li>
                En el detalle del lote, «Carriles por teléfono» muestra qué hace cada chip, cuánto lleva y cuándo termina más o menos.
              </li>
              <li>
                Al verificar, cada chip se cuadra con su saldo: el principal lo transferido y el dealer lo recargado (en Tigo consultarlo envía el PIN guardado; Entel no pide PIN).
              </li>
            </ul>
          </Seccion>

          <Seccion id="validacion" titulo="Validar antes de ejecutar" intro="Revisa cada fila y arma el plan de qué chip envía cada pieza, sin tocar dinero.">
            <TablaEstados filas={ESTADOS_VALIDACION} conAccion={false} />
            <Sub>El plan</Sub>
            <p>
              Cada envío se reparte entre los chips del operador que estén en línea y <Insignia tono="ok">Lista</Insignia>, según su <b>cupo de hoy</b> y su <b>saldo conocido</b> (debe alcanzar para monto + comisión). Se prefiere el chip con más saldo y menos envíos hoy. Lo que no entra queda <Insignia tono="aviso">Para mañana</Insignia> con el motivo.
            </p>
            <p className="text-tenue">
              <b className="text-texto">Consultar saldos</b> (opción al validar) pregunta el saldo a los chips que no tengan un dato de menos de 5 minutos. Es solo lectura, gratis y tarda unos 3.5 s por chip.
            </p>
            <Sub>Cifras del resumen</Sub>
            <ul className="list-disc pl-5 space-y-1.5 text-tenue">
              <li>
                <b className="text-texto">Total Bs</b>: suma de montos de las filas válidas. <b className="text-texto">Necesario</b>: total + comisiones; compáralo con el saldo conocido por operador.
              </li>
              <li>
                <b className="text-texto">Envíos</b>: piezas después de partir los montos. Una fila puede tener varios.
              </li>
              <li>
                <b className="text-texto">Planificados / sin capacidad</b>: cuántas piezas tienen chip asignado y cuántas no.
              </li>
            </ul>
          </Seccion>

          <Seccion id="lote" titulo="Estados del lote" intro="Dónde está el lote en su ciclo de vida.">
            <TablaEstados filas={ESTADOS_LOTE} />
            <Aviso tono="info">
              <b>Terminado ≠ éxito.</b> Para saber si todo salió bien mira el banner de revisión del lote: solo es «completo» si todas las filas quedaron OK (o simuladas, en una simulación). Si no, lista los motivos: «2 filas fallidas», «1 fila para mañana», «lote pausado»…
            </Aviso>
            <Sub>Acciones</Sub>
            <ul className="list-disc pl-5 space-y-1.5">
              <li>
                <b>Iniciar / reanudar</b>: en un lote real primero consulta los saldos y los guarda como <b>saldo inicial</b> (sirve para verificar).
              </li>
              <li>
                <b>Pausar</b>: termina los envíos en vuelo y no empieza otros.
              </li>
              <li>
                <b>Cancelar</b>: no se puede deshacer.
              </li>
              <li>
                <b>Reintentar fallidas</b>: devuelve a pendiente solo los envíos fallidos (seguro: no movieron dinero) y reanuda el lote. De una fila parcial solo se repiten las piezas que fallaron.
              </li>
              <li>
                <b>Reintentar inciertas</b>: igual, pero también con las filas <Insignia tono="error">Incierta</Insignia>. Solo después de revisar el saldo o el historial del chip y ver que no llegaron: si llegaron, se envían dos veces.
              </li>
              <li>
                <b>Archivar</b> (menú «⋯» de un lote terminado o cancelado): lo saca de la lista por defecto. Sigue en la pestaña «Archivados» y se puede sacar del archivo.
              </li>
              <li>Si se reinicia el control, un lote «En curso» pasa a «Pausado».</li>
            </ul>
          </Seccion>

          <Seccion id="filas" titulo="Estados de cada fila" intro="Cada fila del lote termina en uno de estos estados.">
            <TablaEstados filas={ESTADOS_FILA} />
            <Sub>Cómo reacciona el ejecutor</Sub>
            <ul className="list-disc pl-5 space-y-1.5 text-tenue">
              <li>
                Si el operador dice <b className="text-texto">«máximo de transferencias»</b>, ese chip queda agotado por hoy y el envío se reintenta en otro chip.
              </li>
              <li>
                Si en el menú <b className="text-texto">no aparece el monto</b>, el chip se marca sin saldo y se prueba otro.
              </li>
              <li>
                <b className="text-texto">Timeout, ocupado o red</b> antes de confirmar: hasta 2 reintentos con 5 s de espera.
              </li>
              <li>
                Cualquier problema <b className="text-texto">después de confirmar</b> es incierto: nunca se reintenta.
              </li>
            </ul>
          </Seccion>

          <Seccion id="verificar" titulo="Verificar saldos" intro="Cuadra el dinero: compara lo que bajó el saldo de cada chip con lo que el lote dice que envió.">
            <p>
              Consulta el saldo actual de cada chip que envió algo y lo compara con el saldo inicial guardado al último inicio:
              <br />
              <Codigo>bajó = saldo inicial − saldo actual</Codigo> contra <Codigo>esperado = Σ (monto + comisión)</Codigo> de los envíos OK.
            </p>
            <TablaEstados filas={ESTADOS_CUADRE} />
            <ul className="list-disc pl-5 space-y-1.5 text-tenue">
              <li>
                <b className="text-texto">Confirmado</b> = lote completo <b className="text-texto">y</b> todos los chips cuadran. Es el indicador de «lote bien hecho».
              </li>
              <li>Solo vale si nadie usó esos chips por fuera durante el lote (recargas, envíos a mano): cualquier movimiento ajeno aparece como diferencia.</li>
              <li>Solo compara desde el último inicio o reanudación.</li>
              <li>No aplica a simulaciones (no mueven dinero) ni a lotes que nunca se iniciaron.</li>
              <li>El último resultado queda guardado; se borra si el lote vuelve a cambiar (iniciar, reanudar o reintentar).</li>
            </ul>
          </Seccion>

          <Seccion id="resultado" titulo="CSV de resultados" intro="Se descarga desde el detalle del lote. Usa «;» para abrirse en columnas en Excel.">
            <div className="overflow-x-auto">
              <table className="w-full text-left">
                <tbody>
                  {(
                    [
                      ["fila", "Número de fila en tu CSV original (cuenta el encabezado)."],
                      ["numero · operador · accion · monto · referencia", "Lo que pediste."],
                      ["estado", "Estado de la fila (tabla de «Estados de cada fila»)."],
                      ["enviado_bs", "Bs realmente enviados (puede ser menos que monto si fue parcial)."],
                      ["comision_bs", "Comisión cobrada por el operador."],
                      ["envios_ok · envios_total", "Piezas confirmadas sobre piezas totales de la fila."],
                      ["chips", "Números de los chips que enviaron, separados por «|»."],
                      ["detalle", "Motivo si no salió bien."],
                      ["hora", "Hora local del último envío."],
                    ] as const
                  ).map(([c, d]) => (
                    <tr key={c} className="border-t border-borde/50 align-top first:border-t-0">
                      <td className="py-2.5 pr-4 whitespace-nowrap">
                        <Codigo>{c}</Codigo>
                      </td>
                      <td className="py-2.5">{d}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </Seccion>

          <Seccion id="chips" titulo="Cupos, saldos y chips dealer" intro="Todo esto es del chip, no de la ranura ni del teléfono.">
            <ul className="list-disc pl-5 space-y-2">
              <li>
                <b>Cupo de hoy</b>: máximo de envíos por chip y por día (lo fija la receta, p. ej. 5 en Tigo). Se cuentan los envíos OK de hoy de ese chip, esté donde esté.
              </li>
              <li>
                <b>Saldo</b>: la última consulta. Baja solo tras cada envío OK (monto + comisión). Si hubo movimientos por fuera, consulta de nuevo en Teléfonos.
              </li>
              <li>
                <b>Agotado hoy</b>: el operador respondió «máximo de transferencias». Se libera al día siguiente.
              </li>
              <li>
                <b>Sin verificar</b>: entró un chip distinto a esa ranura y aún no se consultó su saldo. No entra al reparto (sus filas quedan «sin saldo verificado») hasta que el control lo consulte solo, cada 10 s hasta 2 minutos.
              </li>
              <li>
                <b>Cambiar chips</b> es normal: el control reconoce el chip por su ICCID o su número. Un chip nuevo arranca en cero; si vuelve uno ya visto, conserva su historial.
              </li>
              <li>
                <Insignia tono="info">Dealer</Insignia> marca chips especiales con USSD y límites propios. Android no dice cuáles son: se marcan a mano con «Marcar dealer» en la tarjeta del chip. Usan la receta dealer si existe y, si no, la normal del operador. Un chip nuevo siempre entra como normal. Al marcar dealer un chip Tigo se ofrece guardar su PIN de dealer («Guardar PIN» / «Cambiar PIN»): queda en la base del control, sigue al chip y nunca sale por la API ni en el historial.
              </li>
            </ul>
          </Seccion>

          <Seccion id="catalogo" titulo="Catálogo de recetas" intro="Las recetas que existen hoy. Cada una es un archivo YAML en control/recetas/.">
            <div className="grid md:grid-cols-2 gap-3">
              {RECETAS.map((r) => (
                <div key={r.nombre} className="rounded-2xl bg-panel-2 p-4 space-y-2.5">
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="font-semibold">{r.titulo}</span>
                    <Insignia tono="neutro">{r.operador}</Insignia>
                    <Insignia tono={r.tipo === "Consulta" ? "ok" : "aviso"}>{r.tipo}</Insignia>
                    {r.dealer && <Insignia tono="info">Solo dealer</Insignia>}
                  </div>
                  <p className="text-xs">
                    <Codigo>{r.nombre}</Codigo> · <span className="font-mono text-tenue">{r.codigo}</span>
                  </p>
                  <dl className="text-xs space-y-1.5">
                    <div><dt className="inline text-tenue">Qué hace: </dt><dd className="inline">{r.pasos}</dd></div>
                    <div><dt className="inline text-tenue">Parámetros: </dt><dd className="inline">{r.params}</dd></div>
                    <div><dt className="inline text-tenue">Devuelve: </dt><dd className="inline">{r.salida}</dd></div>
                  </dl>
                  <p className="text-xs text-tenue">{r.nota}</p>
                </div>
              ))}
            </div>
            <Sub>Cómo leerlas</Sub>
            <ul className="list-disc pl-5 space-y-1.5 text-tenue">
              <li>
                <b className="text-texto">Consulta</b>: gratis, no mueve dinero. <b className="text-texto">Mueve dinero</b>: tiene un paso <Codigo>confirmar</Codigo>, que nunca se reintenta.
              </li>
              <li>
                Los lotes usan por operador la receta <Codigo>transferir</Codigo> (envío) y <Codigo>saldo</Codigo> (consultar saldos). <b className="text-texto">Hoy solo Tigo tiene receta de transferir</b>: una fila ENTEL o VIVA sale con Error «no hay receta».
              </li>
              <li>
                Un chip dealer usa la receta dealer de la misma acción si existe (Entel y Tigo: recargar); para lo demás cae a la receta normal del operador.
              </li>
              <li>
                Las recetas nuevas se agregan como archivo YAML; el formato está en <Codigo>docs/protocolo.md</Codigo>.
              </li>
            </ul>
          </Seccion>

          <Seccion id="ussd" titulo="USSD, recetas y SMS">
            <ul className="list-disc pl-5 space-y-2">
              <li>
                <b>Consola USSD</b>: marca un código en un chip y responde los menús con botones. <i>Menús</i> usa la pantalla del teléfono; <i>Un paso</i> usa la API de Android (Tigo la deja abierta y Android la da por fallida, así que ahí conviene Menús). Al salir se cancela la sesión.
              </li>
              <li>
                <b>Recetas</b>: flujos guardados por operador (saldo, movimientos, transferir). <b>Simular</b> viene activado: llega hasta la confirmación y responde «No». Lo real pide un segundo clic. Las que pueden mover dinero se marcan.
              </li>
              <li>
                Resultado de una receta: <Insignia tono="ok">ok</Insignia> terminó bien · <Insignia tono="info">simulado</Insignia> · <Insignia tono="aviso">abortado</Insignia> no se movió dinero · <Insignia tono="error">incierto</Insignia> se confirmó pero no hubo respuesta clara · <Insignia tono="error">error</Insignia>.
              </li>
              <li>
                <b>SMS</b>: mensajes por teléfono; los nuevos llegan en vivo. <b>Actividad</b>: eventos en vivo más el historial guardado.
              </li>
              <li>
                Atajo: <kbd className="rounded-md bg-panel-2 px-1.5 py-0.5 text-[11px]">⌘K</kbd> / <kbd className="rounded-md bg-panel-2 px-1.5 py-0.5 text-[11px]">Ctrl K</kbd> abre la búsqueda para saltar a una vista o a la consola de un chip.
              </li>
            </ul>
          </Seccion>

          <Seccion
            id="editar"
            titulo="Editar recetas"
            intro="Las recetas se pueden crear y cambiar desde aquí. Siguen siendo archivos YAML en control/recetas: lo que guardes queda en ese archivo."
          >
            <Pasos
              items={[
                <>
                  En <b>Recetas</b> abre una y pulsa <b>Editar</b>. Hay dos modos sobre el mismo archivo: <b>Visual</b> (un formulario por paso) y{" "}
                  <b>YAML</b> (el texto). Los comentarios del archivo se conservan al editar en Visual.
                </>,
                <>
                  Cada paso tiene una acción: <b>Marcar</b> (el código), <b>Enviar</b> (un texto), <b>Elegir</b> (una opción del menú),{" "}
                  <b>Verificar</b> (exige que la pantalla diga algo, sin enviar nada) y <b>Confirmar</b> (la opción que mueve dinero). Con «+» agregas
                  Esperar, Error si, Éxito o un tiempo máximo. Los parámetros se escriben entre llaves: <Codigo>{"{monto}"}</Codigo>.
                </>,
                <>
                  Mientras escribes, el control revisa la receta: los <b className="text-error">errores</b> impiden guardar; los{" "}
                  <b className="text-aviso">avisos</b> no, pero conviene leerlos (p. ej. «el paso que mueve dinero no tiene éxito»).
                </>,
                <>
                  <b>Probar</b> (en cada paso): pega la pantalla del operador, o elige una ejecución reciente, y verás qué contestaría el motor, qué
                  opción elegiría y si esperar/error/éxito coinciden. No marca nada.
                </>,
                <>
                  Al guardar, la versión anterior queda en el historial. Si la receta mueve dinero, aparece <Insignia tono="error">Sin probar</Insignia>{" "}
                  hasta que una ejecución (aunque sea simulada) termine bien: <b>simúlala</b> antes de usarla en real.
                </>,
              ]}
            />
            <Sub>Versiones</Sub>
            <p className="text-tenue">
              En la pestaña <b className="text-texto">Versiones</b> de cada receta están las copias anteriores (en <Codigo>recetas/viejo/</Codigo>). Puedes ver qué
              cambió respecto de la actual y <b className="text-texto">restaurar</b> una: la actual también queda guardada. <b className="text-texto">Retirar</b> una
              receta mueve su archivo a <Codigo>recetas/viejo/</Codigo>; nunca se borra. Los lotes que ya corren siguen con su copia.
            </p>
            <Sub>Crear una receta desde la consola</Sub>
            <p className="text-tenue">
              Haz el recorrido a mano en la <b className="text-texto">Consola USSD</b> (mejor con un menú de solo consulta) y pulsa{" "}
              <b className="text-texto">Guardar como receta</b>. Marca qué respuestas eran un PIN: se guardan como <Codigo>{"{pin}"}</Codigo> y el PIN nunca se
              guarda. Se abre el editor con un borrador: ponle nombre, revisa cada paso y marca el que mueve dinero, si lo hay.
            </p>
            <Sub>Si alguien más la cambia</Sub>
            <p className="text-tenue">
              Si la receta se guarda en otro lado mientras la editas, verás un aviso. Al guardar podrás ver las diferencias, quedarte con la guardada o guardar
              la tuya encima.
            </p>
          </Seccion>

          <Seccion id="problemas" titulo="Si algo sale mal">
            <div className="overflow-x-auto">
              <table className="w-full text-left">
                <thead>
                  <tr className="text-[10px] uppercase tracking-[0.14em] text-tenue">
                    <th className="pb-2 pr-4 font-semibold">Pasa</th>
                    <th className="pb-2 font-semibold">Qué hacer</th>
                  </tr>
                </thead>
                <tbody>
                  {(
                    [
                      ["El lote se pausó solo", "Lee el motivo arriba (p. ej. no se pudo guardar en la base, o se reinició el control) y reanúdalo cuando esté resuelto."],
                      ["Fila Incierta", "Revisa el saldo del chip y el SMS del operador. Si no llegó, «Reintentar inciertas»; si llegó, déjala así."],
                      ["Muchas filas «Para mañana»", "Faltó cupo o saldo. Recarga los chips o espera al día siguiente y vuelve a Iniciar."],
                      ["Fila Fallida", "Mira el detalle. Si fue transitorio (red, ocupado), Reintentar fallidas. Si el dato es malo (número inválido), corrígelo en un lote nuevo."],
                      ["Verificar da «Diferencia»", "Alguien usó el chip por fuera, o hay un envío incierto. Mira el historial del chip en Actividad."],
                      ["El teléfono sale desconectado", "Revisa el cable USB, que la depuración esté aceptada y que la app agente esté abierta."],
                      ["No se leen los menús", "La accesibilidad de la app agente está apagada. No uses «uiautomator dump» con la app corriendo: la deja ciega."],
                    ] as const
                  ).map(([p, q]) => (
                    <tr key={p} className="border-t border-borde/50 align-top">
                      <td className="py-3 pr-4 font-medium whitespace-nowrap">{p}</td>
                      <td className="py-3 text-tenue">{q}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </Seccion>
        </div>
      </div>
    </div>
  );
}
