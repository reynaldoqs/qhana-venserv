import type {
  AdbDispositivo,
  ChipInfo,
  ErrorApi,
  Evento,
  FiltroFilas,
  FiltroLotes,
  Lote,
  ModoUssd,
  Carriles,
  OrigenPedido,
  TipoPlantilla,
  PaginaFilas,
  PaginaLotes,
  Reconexion,
  RespuestaUssd,
  Resultado,
  Salud,
  Sms,
  TipoChip,
  Validacion,
  Verificacion,
  Dispositivo,
  EstadisticasReceta,
  ProblemaReceta,
  PruebaPaso,
  RecetaCompleta,
  ResumenReceta,
  ValidacionReceta,
  VersionReceta,
  VersionRecetaDetalle,
} from "./tipos";

/** Error de la API con el código del contrato (ocupado, timeout, red…). Todo lo que lanza `pedir` es un FalloApi. */
export class FalloApi extends Error {
  constructor(
    public codigo: string,
    detalle: string,
    public http?: number,
    /** `receta_invalida`: los problemas que impiden guardar (como en `POST /api/recetas/validar`). */
    public errores?: ProblemaReceta[],
  ) {
    super(detalle);
  }
}

/**
 * El control es anterior a esta función: la ruta no existe (404/405 sin código del contrato).
 * Para mostrar "actualiza el control" en vez de un error raro.
 */
export const esControlViejo = (e: unknown) =>
  e instanceof FalloApi && e.codigo === "desconocido" && (e.http === 404 || e.http === 405);

export const MENSAJE_VIEJO = "Tu control no tiene esta función todavía: actualiza el control (qhana-control) y recarga la página.";

/** Qué significa cada código, dicho corto. `null`: el detalle del control ya lo explica solo. */
const CODIGOS: Record<string, string | null> = {
  sin_conexion: "No hay conexión con el control",
  sin_respuesta: "El control no respondió a tiempo",
  respuesta_invalida: "Respuesta inesperada del control",
  no_autorizado: "Falta el token de acceso o no es válido",
  duplicado: null,
  otro_lote: "Hay otro lote usando esos chips",
  en_curso: "El lote está corriendo: páusalo antes",
  cancelado: "El lote está cancelado",
  nada_que_reintentar: "No hay envíos que reintentar",
  no_aplica: "No aplica a este lote",
  simulado: "Es una simulación: no movió dinero",
  sin_base: "El lote nunca se inició",
  sin_lote: "El lote no existe",
  ocupado: "El teléfono está ocupado con otra sesión",
  sin_permiso: "Falta un permiso en el teléfono",
  sin_accesibilidad: "La accesibilidad del agente está apagada",
  sim_invalida: "Ese chip no está disponible",
  timeout: "El operador no respondió a tiempo",
  red: "El operador rechazó o cortó la sesión",
  sesion_inexistente: "La sesión USSD ya se cerró",
  sin_receta: "No hay receta para eso",
  tipo_chip: "La receta es de otro tipo de chip",
  sin_dispositivo: "El teléfono no está conectado",
  sin_adb: "El control corre sin ADB",
  sin_apk: "No hay APK para instalar",
  parametro: null,
  receta_invalida: "La receta tiene errores",
  existe: "Ya hay una receta con ese nombre",
  conflicto: "La receta cambió mientras la editabas",
  estado: null,
  desconocido: null,
};

/** Texto en español para mostrar un error de la API (o cualquier otro). Usar en vez de armar `${codigo}: ${detalle}`. */
export function mensajeError(e: unknown): string {
  if (esControlViejo(e)) return MENSAJE_VIEJO;
  if (e instanceof FalloApi) {
    const titulo = CODIGOS[e.codigo];
    const detalle = e.message?.trim();
    if (titulo === undefined) return detalle ? `${detalle} (${e.codigo})` : e.codigo;
    if (titulo === null) return detalle || e.codigo;
    return detalle && detalle !== titulo ? `${titulo}: ${detalle}` : titulo;
  }
  if (e instanceof Error) return e.message;
  return String(e);
}

/** El pedido se canceló a propósito (AbortController del que llama): no es un error que mostrar. */
export const esCancelado = (e: unknown) => e instanceof FalloApi && e.codigo === "cancelado";

// ───────────── Compatibilidad: control viejo con "pda" ─────────────

/** Valores que un control viejo manda con el nombre anterior del chip dealer, por clave. */
const VALOR_VIEJO: Record<string, [string, string]> = {
  tipo: ["pda", "dealer"],
  chip_tipo: ["pda", "dealer"],
  saldo: ["pda", "dealer"],
  accion: ["saldo_pda", "saldo_dealer"],
};
/** Claves que un control viejo manda con el nombre anterior. */
const CLAVE_VIEJA: Record<string, string> = { saldo_pda: "saldo_dealer", saldo_pda_ms: "saldo_dealer_ms" };

/**
 * El único lugar que conoce el nombre viejo: un control anterior llama "pda" al chip dealer (`tipo: "pda"`,
 * `saldo_pda`, acción `saldo_pda`…). Se traduce al nombre nuevo para que la interfaz nunca lo muestre. Solo toca
 * valores exactos de esas claves; los textos (YAML, pantallas USSD, nombres de receta) quedan como vienen.
 */
export function normalizarViejo<T>(x: T): T {
  if (Array.isArray(x)) {
    for (let i = 0; i < x.length; i++) x[i] = normalizarViejo(x[i]);
    return x;
  }
  if (x && typeof x === "object") {
    const o = x as Record<string, unknown>;
    for (const k of Object.keys(o)) {
      const nueva = CLAVE_VIEJA[k];
      if (nueva) {
        if (!(nueva in o)) o[nueva] = o[k];
        delete o[k];
        continue;
      }
      const v = o[k];
      const par = VALOR_VIEJO[k];
      if (par && v === par[0]) o[k] = par[1];
      else if (v && typeof v === "object") o[k] = normalizarViejo(v);
    }
  }
  return x;
}

// ───────────── Token de la API (modo Wi-Fi) ─────────────

const CLAVE_TOKEN = "qhana.token";

function leerToken(): string | null {
  try {
    return localStorage.getItem(CLAVE_TOKEN);
  } catch {
    return null;
  }
}

/** `pide`: hay que mostrar la pantalla de token. `rechazado`: el control rechazó el token que había. */
type EstadoAuth = { token: string | null; pide: boolean; rechazado: boolean };
let auth: EstadoAuth = { token: leerToken(), pide: false, rechazado: false };
const oyentesAuth = new Set<() => void>();
const avisarAuth = () => oyentesAuth.forEach((o) => o());

export const sesion = {
  estado: () => auth,
  suscribir(o: () => void) {
    oyentesAuth.add(o);
    return () => {
      oyentesAuth.delete(o);
    };
  },
  /** Guarda el token (en el navegador, si se puede) y deja de pedirlo. */
  fijar(token: string) {
    try {
      localStorage.setItem(CLAVE_TOKEN, token);
    } catch {
      /* sin almacenamiento: vale solo para esta pestaña */
    }
    auth = { token, pide: false, rechazado: false };
    avisarAuth();
  },
  /** El control pide token y no hay uno válido: se borra el guardado y la interfaz muestra la pantalla de token. */
  pedir() {
    try {
      localStorage.removeItem(CLAVE_TOKEN);
    } catch {
      /* nada */
    }
    if (auth.pide && auth.token === null) return;
    auth = { token: null, pide: true, rechazado: auth.token !== null };
    avisarAuth();
  },
};

/** Agrega `?token=` a la URL del WebSocket de eventos (no admite cabeceras). Solo para eso: las descargas van con cabecera. */
export function conToken(url: string): string {
  const t = auth.token;
  if (!t) return url;
  return `${url}${url.includes("?") ? "&" : "?"}token=${encodeURIComponent(t)}`;
}

// ───────────── Pedidos ─────────────

const ESPERA_MS = 60_000;
/** Operaciones que consultan saldos, instalan o corren recetas: pueden tardar minutos. */
const LARGO_MS = 5 * 60_000;

interface Opciones {
  method?: string;
  body?: unknown;
  signal?: AbortSignal;
  /** Tiempo máximo antes de dar el pedido por perdido (por defecto 60 s). */
  esperaMs?: number;
}

async function pedir<T>(ruta: string, o: Opciones = {}): Promise<T> {
  const ctl = new AbortController();
  let vencido = false;
  const tope = window.setTimeout(() => {
    vencido = true;
    ctl.abort();
  }, o.esperaMs ?? ESPERA_MS);
  const cortar = () => ctl.abort();
  if (o.signal) {
    if (o.signal.aborted) ctl.abort();
    else o.signal.addEventListener("abort", cortar, { once: true });
  }
  const headers: Record<string, string> = {};
  if (o.body !== undefined) headers["Content-Type"] = "application/json";
  if (auth.token) headers.Authorization = `Bearer ${auth.token}`;

  try {
    let r: Response;
    try {
      r = await fetch(ruta, {
        method: o.method ?? (o.body !== undefined ? "POST" : "GET"),
        headers,
        body: o.body !== undefined ? JSON.stringify(o.body) : undefined,
        signal: ctl.signal,
      });
    } catch {
      if (vencido) throw new FalloApi("sin_respuesta", "puede que la operación siga en curso en el control");
      if (o.signal?.aborted) throw new FalloApi("cancelado", "cancelado");
      throw new FalloApi("sin_conexion", "No hay conexión con el control");
    }
    let cuerpo: string;
    try {
      cuerpo = await r.text();
    } catch {
      if (o.signal?.aborted) throw new FalloApi("cancelado", "cancelado");
      throw new FalloApi("sin_conexion", "Se cortó la respuesta del control", r.status);
    }
    let datos: unknown = null;
    if (cuerpo) {
      try {
        datos = JSON.parse(cuerpo);
      } catch {
        if (r.ok) throw new FalloApi("respuesta_invalida", `HTTP ${r.status}, no es JSON`, r.status);
      }
    }
    if (!r.ok) {
      const e = (datos as { error?: ErrorApi & { errores?: ProblemaReceta[] } } | null)?.error;
      const codigo = e?.codigo ?? (r.status === 401 ? "no_autorizado" : "desconocido");
      if (codigo === "no_autorizado") sesion.pedir();
      throw new FalloApi(
        codigo,
        e?.detalle ?? `HTTP ${r.status}${cuerpo && !datos ? `: ${cuerpo.slice(0, 120)}` : ""}`,
        r.status,
        Array.isArray(e?.errores) ? e.errores : undefined,
      );
    }
    return normalizarViejo(datos) as T;
  } finally {
    window.clearTimeout(tope);
    o.signal?.removeEventListener("abort", cortar);
  }
}

/**
 * Descarga un archivo de la API con la cabecera de autorización (el token nunca va en la URL) y lo guarda con un
 * `<a download>` temporal. El nombre sale de `Content-Disposition` si viene; si no, `porDefecto`.
 */
export async function descargar(ruta: string, porDefecto: string): Promise<void> {
  const ctl = new AbortController();
  const tope = window.setTimeout(() => ctl.abort(), ESPERA_MS);
  let r: Response;
  try {
    r = await fetch(ruta, { headers: auth.token ? { Authorization: `Bearer ${auth.token}` } : {}, signal: ctl.signal });
  } catch {
    window.clearTimeout(tope);
    throw new FalloApi(ctl.signal.aborted ? "sin_respuesta" : "sin_conexion", "No pude descargar el archivo");
  }
  try {
    if (!r.ok) {
      let e: ErrorApi | undefined;
      try {
        e = (JSON.parse(await r.text()) as { error?: ErrorApi }).error;
      } catch {
        /* cuerpo no JSON */
      }
      const codigo = e?.codigo ?? (r.status === 401 ? "no_autorizado" : "desconocido");
      if (codigo === "no_autorizado") sesion.pedir();
      throw new FalloApi(codigo, e?.detalle ?? `HTTP ${r.status}`, r.status);
    }
    const blob = await r.blob();
    const cd = r.headers.get("Content-Disposition") ?? "";
    const m = cd.match(/filename\*=UTF-8''([^;]+)/i) ?? cd.match(/filename="?([^";]+)"?/i);
    let nombre = porDefecto;
    if (m) {
      try {
        nombre = decodeURIComponent(m[1].trim());
      } catch {
        nombre = m[1].trim();
      }
    }
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = nombre;
    a.style.display = "none";
    document.body.appendChild(a);
    a.click();
    a.remove();
    window.setTimeout(() => URL.revokeObjectURL(url), 1000);
  } catch (e) {
    if (e instanceof FalloApi) throw e;
    throw new FalloApi("sin_conexion", "Se cortó la descarga");
  } finally {
    window.clearTimeout(tope);
  }
}

const post = <T>(ruta: string, cuerpo: unknown, esperaMs?: number) => pedir<T>(ruta, { method: "POST", body: cuerpo, esperaMs });

function consulta(params: Record<string, string | number | boolean | null | undefined>): string {
  const q = new URLSearchParams();
  for (const [k, v] of Object.entries(params)) if (v !== undefined && v !== null && v !== "") q.set(k, String(v));
  const s = q.toString();
  return s ? `?${s}` : "";
}

/** Un servidor viejo responde `[Lote]` en vez de la página: se trata como una sola página. */
function normalizarPagina(r: PaginaLotes | Lote[]): PaginaLotes {
  if (Array.isArray(r)) {
    const lotes = [...r].sort((a, b) => b.id - a.id);
    return { lotes, siguiente: null, total: lotes.length, conteos: null };
  }
  return { lotes: r.lotes ?? [], siguiente: r.siguiente ?? null, total: r.total ?? r.lotes?.length ?? 0, conteos: r.conteos ?? null };
}

export const api = {
  salud: () => pedir<Salud>("/api/salud", { esperaMs: 10_000 }),
  dispositivos: () => pedir<Dispositivo[]>("/api/dispositivos"),
  adb: () => pedir<AdbDispositivo[]>("/api/adb"),
  /** Rehace el túnel USB, despierta la app de los desconectados y pide su estado a los conectados. */
  reconectar: () => post<Reconexion>("/api/dispositivos/reconectar", {}, LARGO_MS),
  provisionar: (serial: string) =>
    post<{ ok: boolean; pasos: { paso: string; ok: boolean; salida: string }[] }>(
      `/api/adb/${encodeURIComponent(serial)}/provisionar`,
      {},
      LARGO_MS,
    ),
  sms: (id: string, o: { limite?: number; slot?: number | null } = {}, signal?: AbortSignal) =>
    pedir<Sms[]>(`/api/dispositivos/${encodeURIComponent(id)}/sms${consulta({ limite: o.limite ?? 50, slot: o.slot })}`, { signal }),
  recetas: () => pedir<ResumenReceta[]>("/api/recetas"),
  historial: (limite = 100) => pedir<Evento[]>(`/api/historial?limite=${limite}`),

  ussdIniciar: (dispositivo: string, slot: number, codigo: string, modo: ModoUssd) =>
    post<RespuestaUssd>("/api/ussd/iniciar", { dispositivo, slot, codigo, modo }, 2 * ESPERA_MS),
  ussdResponder: (dispositivo: string, sesion: string, texto: string) =>
    post<RespuestaUssd>("/api/ussd/responder", { dispositivo, sesion, texto }, 2 * ESPERA_MS),
  ussdCancelar: (dispositivo: string, sesion: string) => post<{ ok: boolean }>("/api/ussd/cancelar", { dispositivo, sesion }),

  // Lotes
  validarLote: (csv: string, consultar_saldos: boolean, origen?: OrigenPedido | null, signal?: AbortSignal) =>
    pedir<Validacion>("/api/lotes/validar", { method: "POST", body: { csv, consultar_saldos, origen: origen ?? null }, esperaMs: LARGO_MS, signal }),
  /** `forzar`: crearlo aunque sea igual a otro lote real reciente (si no, `409 duplicado`). */
  crearLote: (nombre: string, csv: string, simular: boolean, origen?: OrigenPedido | null, forzar = false) =>
    post<Lote>("/api/lotes", { nombre, csv, simular, origen: origen ?? null, forzar }),
  lotes: async (f: FiltroLotes = {}, signal?: AbortSignal) =>
    normalizarPagina(
      await pedir<PaginaLotes | Lote[]>(
        `/api/lotes${consulta({
          limite: f.limite,
          antes: f.antes,
          estado: f.estado?.join(","),
          problemas: f.problemas,
          simular: f.simular,
          q: f.q?.trim(),
          archivados: f.archivados,
        })}`,
        { signal },
      ),
    ),
  /** Sin filas (más liviano): para la cabecera y el progreso. Las filas van por `filasLote`. */
  lote: (id: number, signal?: AbortSignal) => pedir<Lote>(`/api/lotes/${id}?filas=false`, { signal }),
  filasLote: async (id: number, f: FiltroFilas = {}, signal?: AbortSignal) => {
    const r = await pedir<PaginaFilas>(
      `/api/lotes/${id}/filas${consulta({ estado: f.estado?.join(","), q: f.q?.trim(), despues: f.despues, limite: f.limite })}`,
      { signal },
    );
    return { filas: r.filas ?? [], siguiente: r.siguiente ?? null, total: r.total ?? r.filas?.length ?? 0 };
  },
  iniciarLote: (id: number) => post<Lote>(`/api/lotes/${id}/iniciar`, {}, LARGO_MS),
  pausarLote: (id: number) => post<Lote>(`/api/lotes/${id}/pausar`, {}),
  cancelarLote: (id: number) => post<Lote>(`/api/lotes/${id}/cancelar`, {}),
  archivarLote: (id: number, archivado: boolean) => post<Lote>(`/api/lotes/${id}/archivar`, { archivado }),
  /** `inciertas: true` también re-encola los inciertos: solo tras revisar el saldo (pudieron haberse hecho). */
  reintentarFallidas: (id: number, inciertas = false) => post<Lote>(`/api/lotes/${id}/reintentar-fallidas`, { inciertas }),
  verificarLote: (id: number) => post<Verificacion>(`/api/lotes/${id}/verificar`, {}, LARGO_MS),
  // `;` para que Excel en español lo abra en columnas.
  descargarResultado: (id: number) => descargar(`/api/lotes/${id}/resultado.csv?separador=;`, `lote-${id}-resultado.csv`),
  /** Quién hace cada envío ahora mismo: un carril por chip (ver "Ejecución en paralelo"). */
  carrilesLote: (id: number, signal?: AbortSignal) => pedir<Carriles>(`/api/lotes/${id}/carriles`, { signal, esperaMs: 15_000 }),
  descargarPlantilla: (tipo: TipoPlantilla = "transferencias") =>
    descargar(`/api/lotes/plantilla.csv${consulta({ tipo })}`, tipo === "transferencias" ? "plantilla.csv" : `plantilla-${tipo}.csv`),
  chips: () => pedir<ChipInfo[]>("/api/chips"),
  consultarSaldos: () => post<ChipInfo[]>("/api/chips/saldos", {}, LARGO_MS),
  marcarTipoChip: (dispositivo: string, slot: number, tipo: TipoChip) =>
    pedir<ChipInfo>(`/api/chips/${encodeURIComponent(dispositivo)}/${slot}`, { method: "PATCH", body: { tipo } }),
  /** Guarda el PIN del chip (`null` lo borra). */
  fijarPinChip: (dispositivo: string, slot: number, pin: string | null) =>
    pedir<ChipInfo>(`/api/chips/${encodeURIComponent(dispositivo)}/${slot}/pin`, { method: "PUT", body: { pin } }),

  ejecutarReceta: (nombre: string, dispositivo: string, slot: number, parametros: Record<string, string>, simular: boolean) =>
    post<Resultado>(`/api/recetas/${encodeURIComponent(nombre)}/ejecutar`, { dispositivo, slot, parametros, simular }, LARGO_MS),

  // Editar recetas
  receta: (nombre: string, signal?: AbortSignal) => pedir<RecetaCompleta>(`/api/recetas/${encodeURIComponent(nombre)}`, { signal }),
  estadisticasReceta: (nombre: string, dias = 30, signal?: AbortSignal) =>
    pedir<EstadisticasReceta>(`/api/recetas/${encodeURIComponent(nombre)}/estadisticas?dias=${dias}`, { signal }),
  validarReceta: (yaml: string, signal?: AbortSignal) => pedir<ValidacionReceta>("/api/recetas/validar", { method: "POST", body: { yaml }, signal }),
  probarPaso: (c: { yaml: string; paso: number; texto: string; texto_antes?: string | null; parametros: Record<string, string> }) =>
    post<PruebaPaso>("/api/recetas/probar-paso", c),
  crearReceta: (yaml: string) => post<ResumenReceta>("/api/recetas", { yaml }),
  /** `version_base`: la versión leída. Si el archivo cambió desde entonces, `409 conflicto`. */
  guardarReceta: (nombre: string, yaml: string, version_base: string) =>
    pedir<ResumenReceta>(`/api/recetas/${encodeURIComponent(nombre)}`, { method: "PUT", body: { yaml, version_base } }),
  /** Mueve el archivo a `recetas/viejo/` (nunca se borra). */
  retirarReceta: (nombre: string) => pedir<{ ok: boolean }>(`/api/recetas/${encodeURIComponent(nombre)}`, { method: "DELETE" }),
  versionesReceta: (nombre: string, signal?: AbortSignal) =>
    pedir<VersionReceta[]>(`/api/recetas/${encodeURIComponent(nombre)}/versiones`, { signal }),
  versionReceta: (nombre: string, id: string) =>
    pedir<VersionRecetaDetalle>(`/api/recetas/${encodeURIComponent(nombre)}/versiones/${encodeURIComponent(id)}`),
  /** Borrador de receta a partir de una sesión de la consola. No guarda nada. */
  recetaDesdeSesion: (operador: string, pasos: { enviado: string; texto: string }[]) =>
    post<{ yaml: string }>("/api/recetas/desde-sesion", { operador, pasos }),
};
