// Espejo de docs/protocolo.md. Si cambia el contrato, cambia aquí.

export type EstadoSim = "listo" | "ausente" | "pin" | "puk" | "bloqueada" | "desconocido";
export type Voz = "en_servicio" | "sin_servicio" | "solo_emergencia" | "apagado" | null;

export interface Sim {
  slot: number;
  sub_id: number;
  operador: string;
  mcc_mnc: string;
  iccid: string | null;
  numero: string | null;
  estado: EstadoSim;
  voz: Voz;
  red: string | null;
}

export interface EstadoDispositivo {
  /** 0–100, o null si Android no la informa. */
  bateria: number | null;
  cargando: boolean;
  pantalla_encendida: boolean;
  accesibilidad_activa: boolean;
  permisos_faltantes: string[];
}

export interface Dispositivo {
  id: string;
  conectado: boolean;
  adb_serial: string | null;
  fabricante: string;
  modelo: string;
  android: string;
  app_version: string;
  sims: Sim[];
  estado: EstadoDispositivo | null;
  ultima_vez_ms: number;
  ocupado: boolean;
}

export interface AdbDispositivo {
  serial: string;
  estado: string;
  modelo: string | null;
  reverse_ok: boolean;
  dispositivo_id: string | null;
}

/** `POST /api/dispositivos/reconectar` (botón "Reconectar"). */
export interface Reconexion {
  adb: { serial: string; reverse_ok: boolean; app_abierta: boolean; detalle: string | null }[];
  refrescados: string[];
  sin_respuesta: string[];
  ocupados: string[];
}

export type ModoUssd = "api" | "pantalla";

export interface RespuestaUssd {
  sesion: string;
  texto: string;
  abierta: boolean;
  ms: number;
}

export interface Sms {
  id: string;
  slot: number | null;
  de: string;
  texto: string;
  fecha_ms: number;
  leido: boolean;
}

export interface ParametroReceta {
  nombre: string;
  patron?: string | null;
  opciones?: string[] | null;
  descripcion?: string | null;
  /** Valor secreto (PIN): no se guarda en el historial. Un `pin` secreto se toma del PIN guardado del chip. */
  secreto?: boolean;
}

export interface Receta {
  nombre: string;
  titulo: string;
  operador: string;
  descripcion?: string | null;
  parametros: ParametroReceta[];
  modo?: ModoUssd;
  pasos?: number;
  /** true si tiene un paso `confirmar` (puede mover saldo). */
  mueve_dinero?: boolean;
  /** `dealer`: receta propia de los chips dealer (sin valor: vale para todos). */
  chip_tipo?: TipoChip | null;
  /** Qué hace (`transferir`, `recargar`, `saldo`…). */
  accion?: string | null;
  /** Nombres que tuvo antes la receta: el control los resuelve a esta (lotes y enlaces viejos). */
  nombres_anteriores?: string[];
}

export interface PasoResultado {
  accion: string;
  enviado: string | null;
  texto: string | null;
  abierta?: boolean | null;
  ms?: number | null;
}

export type EstadoResultado = "ok" | "simulado" | "abortado" | "incierto" | "error";

export interface Resultado {
  ok: boolean;
  estado: EstadoResultado;
  detalle: string | null;
  pasos: PasoResultado[];
  salida: Record<string, string>;
}

export interface Evento {
  tipo:
    | "dispositivo_conectado"
    | "dispositivo_desconectado"
    | "dispositivo_actualizado"
    | "chip_cambiado"
    | "chip_tipo"
    | "ussd"
    | "sms"
    | "adb"
    | "receta"
    | "lote"
    /** El control descartó eventos para esta conexión (`n`): hay que releer todo, como al reconectar. */
    | "perdidos"
    | string;
  ts_ms: number;
  [k: string]: unknown;
}

export interface ErrorApi {
  codigo: string;
  detalle: string;
}

// ───────────── Lotes (CSV) ─────────────

export interface ChipInfo {
  dispositivo: string;
  slot: number;
  operador: string;
  numero: string | null;
  saldo: number | null;
  saldo_ms: number | null;
  enviados_hoy: number;
  /** Envíos que le quedan hoy (de `accion`; sin `accion`, de transferir). 0 y sin sentido si `sin_limite`. */
  cupo_hoy: number;
  /** Sin límite diario para esa acción (la receta no tiene `por_dia`). */
  sin_limite?: boolean;
  /** En `Validacion.chips`: la acción del lote a la que se refieren `enviados_hoy`, `cupo_hoy` y `sin_limite`. */
  accion?: string;
  agotado_hoy?: boolean;
  /** Entró un chip distinto a esta ranura y aún no se consultó su saldo: no entra al reparto. */
  sin_verificar?: boolean;
  /** `dealer`: chip especial con USSD y límites propios. Se marca a mano; los chips nuevos son `normal`. */
  tipo: TipoChip;
  /** El chip tiene un PIN guardado (el PIN en sí nunca sale de la API). */
  tiene_pin?: boolean;
  /** Saldo dealer conocido: el que gastan las recargas. No es el principal. */
  saldo_dealer?: number | null;
  saldo_dealer_ms?: number | null;
}

export type TipoChip = "normal" | "dealer";

export interface PlanEnvio {
  monto: number;
  dispositivo: string;
  slot: number;
}

export type EstadoFilaValidada = "valida" | "aviso" | "invalida";

export interface FilaValidada {
  fila: number;
  numero: string | null;
  monto: number | null;
  operador: string | null;
  accion: string;
  referencia: string | null;
  receta: string | null;
  estado: EstadoFilaValidada;
  errores: string[];
  avisos: string[];
  envios: number[];
  plan: PlanEnvio[];
  sin_capacidad: number;
  motivo_sin_capacidad?: string | null;
  original: Record<string, string>;
}

/** Una entrada por (operador, acción) de las filas válidas. */
export interface ResumenOperador {
  operador: string;
  accion: string;
  chips: number;
  /** Suma del cupo de los chips con límite diario para esa acción. */
  cupo_hoy: number;
  /** Algún chip no tiene límite diario: el cupo no limita el plan. */
  cupo_sin_limite: boolean;
  /** Saldo principal: solo con `transferir` (con otra acción, 0). */
  saldo_conocido: number;
  saldo_desconocido_chips: number;
  necesario_bs: number;
  envios: number;
}

export interface ResumenValidacion {
  filas: number;
  validas: number;
  con_avisos: number;
  invalidas: number;
  total_bs: number;
  envios: number;
  comision_total: number;
  /** Avisos del lote entero (p. ej. "la receta X cambió y no se ha probado"). */
  avisos?: string[];
  planificados: number;
  sin_capacidad: number;
  operadores: ResumenOperador[];
}

/** Un chip de origen tal como lo devuelve el control (`Validacion.origen.chips`, `Lote.origen.chips`). */
export interface ChipOrigen {
  dispositivo: string;
  slot: number;
  operador?: string | null;
  numero?: string | null;
  tipo?: TipoChip | null;
  /** La receta que correrá ese chip. */
  receta?: string | null;
}

/**
 * Origen del lote como lo devuelve el control. Dos formas (ver "Lote de un chip" y "Lote desde chips elegidos"):
 * - nueva: `{ chips: [ChipOrigen], accion }` (para un origen viejo, además `dispositivo`, `slot` y `receta`);
 * - vieja: `{ dispositivo, slot, receta, operador?, numero? }` (un solo chip).
 * Todo opcional para leer las dos sin romper: usar `chipsDeOrigen` / `accionDeOrigen` de `vistas/lotes/comunes`.
 */
export interface OrigenLote {
  dispositivo?: string;
  slot?: number;
  receta?: string;
  /** Los completa el control (origen viejo). */
  operador?: string | null;
  numero?: string | null;
  chips?: ChipOrigen[];
  accion?: string | null;
}

/** Origen que pide la interfaz: los chips elegidos y la acción para todas las filas. */
export interface OrigenChipsPedido {
  chips: { dispositivo: string; slot: number }[];
  accion: string;
}

/** Lo que se manda en `validar` y `POST /api/lotes`: el origen viejo de un chip sigue valiendo. */
export type OrigenPedido = OrigenChipsPedido | { dispositivo: string; slot: number; receta: string };

export interface Validacion {
  filas: FilaValidada[];
  resumen: ResumenValidacion;
  chips: ChipInfo[];
  /** Los chips elegidos (con operador, número, tipo y receta completados). */
  origen?: OrigenLote | null;
  /** Chips elegidos que no sirven y quedaron fuera (el resto alcanza), con el motivo. */
  avisos_origen?: string[];
}

/** Plantilla CSV de ejemplo (`GET /api/lotes/plantilla.csv?tipo=`). */
export type TipoPlantilla = "transferencias" | "recargas" | "chips";

// ───────────── Carriles (ejecución en paralelo) ─────────────

export type EstadoCarril = "enviando" | "esperando" | "libre" | "sin_capacidad" | "desconectado" | "excluido";

/** `GET /api/lotes/{id}/carriles`: un carril por chip que participa (o puede participar) en el lote. */
export interface Carril {
  dispositivo: string;
  slot: number;
  /** Nombre del teléfono ("Infinix X678B"). */
  telefono?: string | null;
  operador: string | null;
  numero: string | null;
  tipo?: TipoChip | null;
  receta?: string | null;
  estado: EstadoCarril;
  /** El envío en curso (solo con `enviando`). `desde_ms`: cuándo empezó (null si no se sabe). */
  actual?: { fila: number; numero: string | null; monto: number; desde_ms: number | null } | null;
  ok: number;
  fallidos: number;
  inciertos: number;
  enviado_bs: number;
  /** null: sin límite (`cupo_sin_limite`) o sin dato. */
  cupo_restante: number | null;
  /** La receta del chip no tiene límite diario. */
  cupo_sin_limite?: boolean;
  /** null: sin dato. */
  saldo: { tipo: "principal" | "dealer"; bs: number; ms: number | null } | null;
  ms_promedio: number | null;
  motivo: string | null;
}

export interface Carriles {
  carriles: Carril[];
  pendientes: number;
  /** null si el lote no está en curso o aún no hay ritmo. */
  eta_ms: number | null;
}

export type EstadoLote = "listo" | "en_curso" | "pausado" | "terminado" | "cancelado";
export type EstadoFilaLote =
  | "omitida"
  | "pendiente"
  | "en_curso"
  | "ok"
  | "simulado"
  | "parcial"
  | "fallida"
  | "incierta"
  | "sin_capacidad";

export interface EnvioLote {
  monto: number;
  numero_origen?: string | null;
  intentos?: number;
  dispositivo: string | null;
  slot: number | null;
  estado: string;
  ejecucion_id: number | null;
  detalle: string | null;
  ts_ms: number | null;
}

export interface FilaLote {
  fila: number;
  numero: string | null;
  monto: number | null;
  operador: string | null;
  accion: string;
  referencia: string | null;
  estado: EstadoFilaLote;
  enviado_bs: number;
  detalle: string | null;
  envios: EnvioLote[];
}

export interface ProgresoLote {
  filas: number;
  ok: number;
  simulado: number;
  parcial: number;
  fallidas: number;
  inciertas: number;
  pendientes: number;
  sin_capacidad: number;
  omitidas: number;
  enviado_bs: number;
  comision_bs: number;
  total_bs: number;
}

export interface Lote {
  id: number;
  nombre: string;
  creado_ms: number;
  estado: EstadoLote;
  simular: boolean;
  motivo_pausa: string | null;
  /** `terminado` no es éxito: `completo` solo es true si todas las filas quedaron ok. */
  revision: RevisionLote;
  /** Último resultado de verificar saldos (null si nunca, o si el lote cambió después). */
  verificacion?: Verificacion | null;
  progreso: ProgresoLote;
  /** Chips elegidos o lote de un chip (sin valor: reparto por operador entre todos los chips). Puede venir en la forma vieja. */
  origen?: OrigenLote | null;
  /** Archivado: no aparece en la lista por defecto. Solo lotes terminados o cancelados. */
  archivado?: boolean;
  /** Ausente con `?filas=false` (o en la lista): las filas se piden por páginas. */
  filas?: FilaLote[];
}

/** `GET /api/salud`. `requiere_token`: esta petición necesitaría token (viene de otra máquina). */
export interface Salud {
  version: string;
  agentes_conectados: number;
  requiere_token?: boolean;
}

/** Números de las pestañas de la lista de lotes (no dependen del filtro). */
export interface ConteosLotes {
  activos: number;
  problemas: number;
  terminados: number;
  simulaciones: number;
  archivados: number;
}

/** `GET /api/lotes` (lista paginada). */
export interface PaginaLotes {
  lotes: Lote[];
  /** Valor para `antes` de la próxima página, o null si no hay más. */
  siguiente: number | null;
  total: number;
  conteos: ConteosLotes | null;
}

export interface FiltroLotes {
  limite?: number;
  antes?: number | null;
  estado?: EstadoLote[];
  problemas?: boolean;
  simular?: boolean;
  q?: string;
  archivados?: boolean | "todos";
}

/** `GET /api/lotes/{id}/filas`. */
export interface PaginaFilas {
  filas: FilaLote[];
  /** Valor para `despues` de la próxima página, o null si no hay más. */
  siguiente: number | null;
  total: number;
}

export interface FiltroFilas {
  estado?: EstadoFilaLote[];
  q?: string;
  despues?: number | null;
  limite?: number;
}

export interface RevisionLote {
  completo: boolean;
  motivos: string[];
}

export type EstadoCuadre = "cuadra" | "diferencia" | "sin_dato";

export interface CuadreChip {
  dispositivo: string;
  slot: number;
  /** Qué saldo se cuadró en este chip (lote mixto: principal para lo transferido, dealer para lo recargado). */
  saldo?: "principal" | "dealer" | null;
  numero: string | null;
  estado: EstadoCuadre;
  saldo_inicial: number | null;
  saldo_actual: number | null;
  descontado_esperado: number;
  descontado_real: number | null;
  /** Positivo: se descontó de más. */
  diferencia_bs: number | null;
  envios_ok: number;
}

export interface Verificacion {
  lote_id: number;
  verificado_ms: number;
  /** Qué saldo se comparó: el principal (transferencias), el dealer (recargas desde chips dealer) o `mixto` (cada chip dice el suyo). */
  saldo?: "principal" | "dealer" | "mixto";
  completo: boolean;
  motivos: string[];
  cuadra: boolean;
  /** completo y cuadra */
  confirmado: boolean;
  chips: CuadreChip[];
  avisos: string[];
}

export type FaseLote = "inicio" | "envio" | "fila" | "alerta" | "pausa" | "fin";

/** Evento `lote`: todos llevan `progreso`; `envio` y `fila` llevan `fila` y `estado`. */
export interface EventoLote {
  tipo: "lote";
  ts_ms: number;
  lote_id: number;
  fase: FaseLote;
  progreso?: ProgresoLote;
  fila?: number;
  /** `fila`: estado de la fila. `envio`: estado del envío. */
  estado?: string;
  monto?: number;
  dispositivo_id?: string;
  slot?: number;
  /** `envio`: número de destino. */
  numero?: string | null;
  motivo?: string;
}

// ───────────── Editar recetas (ver "Editar recetas desde la interfaz") ─────────────

/** `uso` de un resumen: `ok_30d` cuenta `ok` y `simulado`. */
export interface UsoReceta {
  ejecuciones_30d: number;
  ok_30d: number;
  ultima_ms: number | null;
  ultimo_estado: EstadoResultado | "en_curso" | null;
  ultima_ok_ms: number | null;
}

/** `GET /api/recetas`. Los campos nuevos faltan en un control viejo. */
export interface ResumenReceta extends Receta {
  /** SHA-256 completo del archivo, en hex (mostrar los primeros 12). */
  version?: string;
  archivo?: string;
  /** null en `validar` si el texto no es el archivo guardado. */
  modificada_ms?: number | null;
  /** La versión actual nunca terminó `ok` ni `simulado`. Solo avisa. */
  sin_probar?: boolean;
  uso?: UsoReceta | null;
}

/** `GET /api/recetas/{nombre}`. */
export interface RecetaCompleta {
  resumen: ResumenReceta;
  yaml: string;
  receta: Record<string, unknown>;
  /** Los avisos de `validar`, como texto ("paso 6: …"). */
  avisos: string[];
}

/** Un problema de validación. `paso` es 1-based o null (la receta entera). */
export interface ProblemaReceta {
  paso: number | null;
  campo?: string | null;
  mensaje: string;
}

/** `POST /api/recetas/validar`. */
export interface ValidacionReceta {
  ok: boolean;
  errores: ProblemaReceta[];
  avisos: ProblemaReceta[];
  receta?: Record<string, unknown> | null;
  resumen?: ResumenReceta | null;
}

/** `POST /api/recetas/probar-paso`. */
export interface PruebaPaso {
  accion: string;
  enviaria: string | null;
  opciones?: { numero: string; texto: string; coincide: boolean }[];
  esperar: boolean | null;
  error_si: boolean | null;
  exito: boolean | null;
  verificar: boolean | null;
  salida?: Record<string, string>;
  /** `incierto`: un corte en o después del paso que mueve dinero. */
  resultado: "seguiria" | "exito" | "abortaria" | "incierto";
  /** null si sigue o terminó bien. */
  motivo: string | null;
  /** Los valores usados (los que faltaban, completados con un ejemplo; secretos tapados). */
  parametros?: Record<string, string>;
}

export interface EjecucionReceta {
  id: number;
  inicio_ms: number;
  estado: EstadoResultado | "en_curso";
  simular: boolean;
  dispositivo: string;
  slot: number;
  duracion_ms: number | null;
  detalle: string | null;
  version: string | null;
  pasos: PasoResultado[];
}

/** `GET /api/recetas/{nombre}/estadisticas?dias=30`. */
export interface EstadisticasReceta {
  dias: number;
  ejecuciones: number;
  por_estado: Partial<Record<EstadoResultado, number>>;
  ms_promedio: number | null;
  ms_p90: number | null;
  ultima_ok_ms: number | null;
  por_paso_fallido: { paso: number; veces: number; ultimo_detalle: string | null }[];
  ultimas: EjecucionReceta[];
}

/** `GET /api/recetas/{nombre}/versiones` (la más nueva primero; la primera es la actual). */
export interface VersionReceta {
  id: string;
  guardada_ms: number;
  version: string;
  actual: boolean;
}

export interface VersionRecetaDetalle {
  id: string;
  yaml: string;
  version: string;
}

/** Evento `receta_cambiada`. */
export interface EventoRecetaCambiada {
  tipo: "receta_cambiada";
  ts_ms: number;
  nombre: string;
  cambio: "creada" | "editada" | "retirada";
  /** null al retirar. */
  version: string | null;
}
