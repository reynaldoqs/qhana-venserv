import type { Sms } from "../api/tipos";

/** Código de verificación en el texto ("123456 es tu código", "verification code: 123456"), si lo hay. */
export function codigoDe(texto: string): string | null {
  const m =
    texto.match(/(?:c[oó]digo|code|clave|pin|otp|verificaci[oó]n|verification)[^\d\n]{0,40}(\d{4,8})\b/i) ??
    texto.match(/\b(\d{4,8})\b[^\d\n]{0,30}(?:es tu|es su|is your|para verificar)/i);
  return m ? m[1] : null;
}

const SERVICIO = /saldo|transferid|recibiste|recibido|pago|compra realizada|retiro|d[eé]bito|cr[eé]dito|vigencia/i;
const PUBLICIDAD = /oferta|promo|paquet|ilimitad|descuento|regalo|gana |exclusiv|aprovecha|evita el corte|no dejes de/i;

export type Clase = "codigo" | "promo" | "normal";

/** Clasifica un SMS para filtrar: códigos de verificación, publicidad del operador, o el resto. */
export function claseDe(m: Sms): Clase {
  if (codigoDe(m.texto)) return "codigo";
  if (PUBLICIDAD.test(m.texto) && !SERVICIO.test(m.texto)) return "promo";
  return "normal";
}

export interface Conversacion {
  de: string;
  mensajes: Sms[]; // del más viejo al más nuevo
  sinLeer: number;
  ultimo: Sms;
}

export function agrupar(sms: Sms[]): Conversacion[] {
  const mapa = new Map<string, Sms[]>();
  for (const m of sms) mapa.set(m.de, [...(mapa.get(m.de) ?? []), m]);
  return [...mapa.entries()]
    .map(([de, l]) => {
      const mensajes = [...l].sort((a, b) => a.fecha_ms - b.fecha_ms);
      return { de, mensajes, sinLeer: mensajes.filter((m) => !m.leido).length, ultimo: mensajes[mensajes.length - 1] };
    })
    .sort((a, b) => b.ultimo.fecha_ms - a.ultimo.fecha_ms);
}
