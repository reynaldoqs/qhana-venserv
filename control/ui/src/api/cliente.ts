import type {
  AdbDispositivo,
  ChipInfo,
  Lote,
  Verificacion,
  Validacion,
  Dispositivo,
  ErrorApi,
  Evento,
  ModoUssd,
  Receta,
  RespuestaUssd,
  Resultado,
  Sms,
  TipoChip,
} from "./tipos";

/** Error de la API con el código del contrato (ocupado, timeout, red…). */
export class FalloApi extends Error {
  constructor(
    public codigo: string,
    detalle: string,
    public http?: number,
  ) {
    super(detalle);
  }
}

async function pedir<T>(ruta: string, init?: RequestInit): Promise<T> {
  let r: Response;
  try {
    r = await fetch(ruta, {
      ...init,
      headers: { "Content-Type": "application/json", ...(init?.headers ?? {}) },
    });
  } catch {
    throw new FalloApi("sin_conexion", "No hay conexión con el control");
  }
  const cuerpo = await r.text();
  const datos = cuerpo ? JSON.parse(cuerpo) : null;
  if (!r.ok) {
    const e: ErrorApi | undefined = datos?.error;
    throw new FalloApi(e?.codigo ?? "desconocido", e?.detalle ?? `HTTP ${r.status}`, r.status);
  }
  return datos as T;
}

const post = <T>(ruta: string, cuerpo: unknown) =>
  pedir<T>(ruta, { method: "POST", body: JSON.stringify(cuerpo) });

export const api = {
  salud: () => pedir<{ version: string; agentes_conectados: number }>("/api/salud"),
  dispositivos: () => pedir<Dispositivo[]>("/api/dispositivos"),
  adb: () => pedir<AdbDispositivo[]>("/api/adb"),
  provisionar: (serial: string) =>
    post<{ ok: boolean; pasos: { paso: string; ok: boolean; salida: string }[] }>(`/api/adb/${encodeURIComponent(serial)}/provisionar`, {}),
  sms: (id: string, limite = 50) =>
    pedir<Sms[]>(`/api/dispositivos/${encodeURIComponent(id)}/sms?limite=${limite}`),
  recetas: () => pedir<Receta[]>("/api/recetas"),
  historial: (limite = 100) => pedir<Evento[]>(`/api/historial?limite=${limite}`),

  ussdIniciar: (dispositivo: string, slot: number, codigo: string, modo: ModoUssd) =>
    post<RespuestaUssd>("/api/ussd/iniciar", { dispositivo, slot, codigo, modo }),
  ussdResponder: (dispositivo: string, sesion: string, texto: string) =>
    post<RespuestaUssd>("/api/ussd/responder", { dispositivo, sesion, texto }),
  ussdCancelar: (dispositivo: string, sesion: string) =>
    post<{ ok: boolean }>("/api/ussd/cancelar", { dispositivo, sesion }),

  // Lotes
  validarLote: (csv: string, consultar_saldos: boolean) =>
    post<Validacion>("/api/lotes/validar", { csv, consultar_saldos }),
  crearLote: (nombre: string, csv: string, simular: boolean) => post<Lote>("/api/lotes", { nombre, csv, simular }),
  lotes: () => pedir<Lote[]>("/api/lotes"),
  lote: (id: number) => pedir<Lote>(`/api/lotes/${id}`),
  iniciarLote: (id: number) => post<Lote>(`/api/lotes/${id}/iniciar`, {}),
  pausarLote: (id: number) => post<Lote>(`/api/lotes/${id}/pausar`, {}),
  cancelarLote: (id: number) => post<Lote>(`/api/lotes/${id}/cancelar`, {}),
  reintentarFallidas: (id: number) => post<Lote>(`/api/lotes/${id}/reintentar-fallidas`, {}),
  verificarLote: (id: number) => post<Verificacion>(`/api/lotes/${id}/verificar`, {}),
  // `;` para que Excel en español lo abra en columnas.
  urlResultado: (id: number) => `/api/lotes/${id}/resultado.csv?separador=;`,
  urlPlantilla: "/api/lotes/plantilla.csv",
  chips: () => pedir<ChipInfo[]>("/api/chips"),
  consultarSaldos: () => post<ChipInfo[]>("/api/chips/saldos", {}),
  marcarTipoChip: (dispositivo: string, slot: number, tipo: TipoChip) =>
    pedir<ChipInfo>(`/api/chips/${encodeURIComponent(dispositivo)}/${slot}`, { method: "PATCH", body: JSON.stringify({ tipo }) }),

  ejecutarReceta: (
    nombre: string,
    dispositivo: string,
    slot: number,
    parametros: Record<string, string>,
    simular: boolean,
  ) =>
    post<Resultado>(`/api/recetas/${encodeURIComponent(nombre)}/ejecutar`, {
      dispositivo,
      slot,
      parametros,
      simular,
    }),
};
