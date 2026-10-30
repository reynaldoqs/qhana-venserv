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
  bateria: number;
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
  /** `pda`: receta propia de los chips PDA (sin valor: vale para todos). */
  chip_tipo?: TipoChip | null;
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
  cupo_hoy: number;
  agotado_hoy?: boolean;
  /** Entró un chip distinto a esta ranura y aún no se consultó su saldo: no entra al reparto. */
  sin_verificar?: boolean;
  /** `pda`: chip especial con USSD y límites propios. Se marca a mano; los chips nuevos son `normal`. */
  tipo: TipoChip;
}

export type TipoChip = "normal" | "pda";

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

export interface ResumenOperador {
  operador: string;
  chips: number;
  cupo_hoy: number;
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
  planificados: number;
  sin_capacidad: number;
  operadores: ResumenOperador[];
}

export interface Validacion {
  filas: FilaValidada[];
  resumen: ResumenValidacion;
  chips: ChipInfo[];
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
  filas?: FilaLote[];
}

export interface RevisionLote {
  completo: boolean;
  motivos: string[];
}

export type EstadoCuadre = "cuadra" | "diferencia" | "sin_dato";

export interface CuadreChip {
  dispositivo: string;
  slot: number;
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
  completo: boolean;
  motivos: string[];
  cuadra: boolean;
  /** completo y cuadra */
  confirmado: boolean;
  chips: CuadreChip[];
  avisos: string[];
}
