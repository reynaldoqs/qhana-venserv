/**
 * La receta como documento YAML. El texto es la única fuente de verdad: cada cambio del editor visual lee el texto
 * (`parseDocument`), cambia ese nodo en el árbol (`setIn`, `deleteIn`, mover ítems…) y vuelve a escribirlo
 * (`toString`). Así los comentarios, las comillas y el estilo de cada nodo (p. ej. los parámetros en una línea
 * `{ nombre: …, patron: … }`) quedan como estaban; solo cambia lo que se tocó.
 */
import { Document, Pair, Scalar, YAMLMap, YAMLSeq, isMap, isPair, isScalar, isSeq, parseDocument } from "yaml";
import type { ParametroReceta } from "../../api/tipos";

export const ACCIONES = ["marcar", "enviar", "elegir", "verificar", "confirmar"] as const;
export type Accion = (typeof ACCIONES)[number];

export const MODIFICADORES = ["esperar", "error_si", "exito", "timeout_ms"] as const;
export type Modificador = (typeof MODIFICADORES)[number];

/** Un paso tal como está en el YAML, más su acción y valor ya separados. */
export interface PasoModelo {
  accion: Accion | null;
  valor: string;
  esperar?: string;
  error_si?: string;
  exito?: string;
  timeout_ms?: number;
  mueve_dinero?: boolean;
}

export interface RecetaModelo {
  nombre: string;
  titulo: string;
  operador: string;
  descripcion: string;
  modo: "pantalla" | "api";
  accion: string;
  chip_tipo: string;
  /** Nombres que tuvo antes la receta (el control los resuelve a esta). */
  nombres_anteriores: string[];
  limites: { por_dia?: number; comision?: number };
  parametros: ParametroReceta[];
  pasos: PasoModelo[];
  salida: [string, string][];
}

export type Lectura = { ok: true; doc: Document; modelo: RecetaModelo } | { ok: false; error: string; linea?: number };

const LECTURA_OPCIONES = { keepSourceTokens: false, prettyErrors: true } as const;
/** Sin cortar líneas largas (las descripciones y regex quedan en una línea, como las escribe la gente). */
const ESCRITURA = { lineWidth: 0, minContentWidth: 0 } as const;

const texto = (v: unknown) => (v == null ? "" : String(v));

function pasoDe(x: unknown): PasoModelo {
  const o = (x && typeof x === "object" ? x : {}) as Record<string, unknown>;
  const accion = (ACCIONES.find((a) => a in o) ?? null) as Accion | null;
  return {
    accion,
    valor: accion ? texto(o[accion]) : "",
    esperar: o.esperar != null ? texto(o.esperar) : undefined,
    error_si: o.error_si != null ? texto(o.error_si) : undefined,
    exito: o.exito != null ? texto(o.exito) : undefined,
    timeout_ms: typeof o.timeout_ms === "number" ? o.timeout_ms : o.timeout_ms != null ? Number(o.timeout_ms) : undefined,
    mueve_dinero: o.mueve_dinero === true,
  };
}

/** Lee el texto. Con error de sintaxis devuelve el mensaje y la línea (1-based). */
export function leer(yaml: string): Lectura {
  const doc = parseDocument(yaml, LECTURA_OPCIONES);
  if (doc.errors.length > 0) {
    const e = doc.errors[0];
    return { ok: false, error: e.message.split("\n")[0], linea: e.linePos?.[0]?.line };
  }
  const js = (doc.toJS() ?? {}) as Record<string, unknown>;
  if (typeof js !== "object" || Array.isArray(js)) return { ok: false, error: "La receta debe ser un mapa (nombre: …, pasos: …)" };
  const lim = (js.limites ?? {}) as Record<string, unknown>;
  const sal = js.salida && typeof js.salida === "object" ? (js.salida as Record<string, unknown>) : {};
  const modelo: RecetaModelo = {
    nombre: texto(js.nombre),
    titulo: texto(js.titulo),
    operador: texto(js.operador),
    descripcion: texto(js.descripcion),
    modo: js.modo === "api" ? "api" : "pantalla",
    accion: texto(js.accion),
    chip_tipo: texto(js.chip_tipo),
    nombres_anteriores: Array.isArray(js.nombres_anteriores) ? js.nombres_anteriores.map(texto).filter(Boolean) : [],
    limites: {
      por_dia: typeof lim.por_dia === "number" ? lim.por_dia : undefined,
      comision: typeof lim.comision === "number" ? lim.comision : undefined,
    },
    parametros: Array.isArray(js.parametros)
      ? js.parametros.map((p) => {
          const o = (p ?? {}) as Record<string, unknown>;
          return {
            nombre: texto(o.nombre),
            patron: o.patron != null ? texto(o.patron) : null,
            opciones: Array.isArray(o.opciones) ? o.opciones.map(texto) : null,
            descripcion: o.descripcion != null ? texto(o.descripcion) : null,
            secreto: o.secreto === true,
          };
        })
      : [],
    pasos: Array.isArray(js.pasos) ? js.pasos.map(pasoDe) : [],
    salida: Object.entries(sal).map(([k, v]) => [k, texto(v)]),
  };
  return { ok: true, doc, modelo };
}

/** true si algún paso mueve dinero (`confirmar` o `enviar` con `mueve_dinero`). Igual que `mueve_dinero` del control. */
export const mueveDinero = (m: RecetaModelo) => m.pasos.some((p) => p.accion === "confirmar" || p.mueve_dinero);

/** `{monto}`, `{numero}`… usados en los pasos y la salida. */
export function parametrosUsados(m: RecetaModelo): Set<string> {
  const usados = new Set<string>();
  const ver = (t?: string) => t && [...t.matchAll(/\{([a-z_][a-z0-9_]*)\}/gi)].forEach((x) => usados.add(x[1]));
  for (const p of m.pasos) [p.valor, p.esperar, p.error_si, p.exito].forEach(ver);
  return usados;
}

// ───────────── Cambios sobre el documento ─────────────

type Ruta = (string | number)[];
export type Cambio = (d: Document) => void;

/** Aplica un cambio al texto y devuelve el texto nuevo (con los comentarios intactos). Si no se puede leer, no cambia. */
export function aplicar(yaml: string, cambio: Cambio): string {
  const d = parseDocument(yaml, LECTURA_OPCIONES);
  if (d.errors.length > 0) return yaml;
  if (!isMap(d.contents)) d.contents = d.createNode({}) as unknown as typeof d.contents;
  cambio(d);
  return d.toString(ESCRITURA);
}

/** Valor escalar nuevo. Las cadenas van entre comillas simples: en YAML no interpretan `\`, ideal para regex. */
function escalar(v: string | number | boolean): Scalar {
  const s = new Scalar(v);
  if (typeof v === "string") s.type = Scalar.QUOTE_SINGLE;
  return s;
}

/**
 * Fija un valor escalar. Si el nodo ya existe se cambia su valor en el mismo nodo (conserva comillas y comentarios);
 * si no, se crea. `null`/`""` (con `borrarVacio`) quita la clave.
 */
export function fijar(ruta: Ruta, v: string | number | boolean | null | undefined, borrarVacio = false): Cambio {
  return (d) => {
    if (v == null || (borrarVacio && v === "")) {
      if (d.hasIn(ruta)) d.deleteIn(ruta);
      return;
    }
    const nodo = d.getIn(ruta, true);
    if (isScalar(nodo)) {
      // Una cadena plana que pasa a número (o al revés) cambia de tipo: se respeta el valor nuevo tal cual.
      nodo.value = v;
      if (typeof v !== "string" && nodo.type !== Scalar.PLAIN) nodo.type = Scalar.PLAIN;
      return;
    }
    asegurarPadre(d, ruta);
    d.setIn(ruta, escalar(v));
  };
}

/** Crea los mapas intermedios que falten (p. ej. `limites`). */
function asegurarPadre(d: Document, ruta: Ruta) {
  for (let i = 1; i < ruta.length; i++) {
    const r = ruta.slice(0, i);
    if (!d.hasIn(r)) d.setIn(r, new YAMLMap());
  }
}

export const quitar = (ruta: Ruta): Cambio => (d) => {
  if (d.hasIn(ruta)) d.deleteIn(ruta);
};

function seq(d: Document, clave: string): YAMLSeq {
  let s = d.get(clave, true);
  if (!isSeq(s)) {
    s = new YAMLSeq();
    d.set(clave, s);
  }
  const lista = s as YAMLSeq;
  // `parametros: []` vacío está en estilo flujo: al agregar el primero pasa a bloque, como el resto del archivo.
  if (lista.items.length === 0) lista.flow = false;
  return lista;
}

/** Mueve el ítem `i` a la posición `j` (sus comentarios lo acompañan). */
export const mover = (clave: string, i: number, j: number): Cambio => (d) => {
  const s = seq(d, clave);
  if (j < 0 || j >= s.items.length) return;
  const [x] = s.items.splice(i, 1);
  s.items.splice(j, 0, x);
};

export const borrarItem = (clave: string, i: number): Cambio => (d) => {
  seq(d, clave).items.splice(i, 1);
};

/** Copia del ítem `i` justo debajo (sin los comentarios de arriba, que describen al original). */
export const duplicarItem = (clave: string, i: number): Cambio => (d) => {
  const s = seq(d, clave);
  const orig = s.items[i];
  if (!orig) return;
  const copia = d.createNode((orig as YAMLMap).toJSON()) as YAMLMap;
  if (isMap(orig)) {
    copia.flow = orig.flow;
    copiarEstilos(orig, copia);
  }
  s.items.splice(i + 1, 0, copia);
};

function copiarEstilos(de: YAMLMap, a: YAMLMap) {
  for (const p of a.items) {
    const k = isScalar(p.key) ? p.key.value : p.key;
    const o = de.items.find((x) => (isScalar(x.key) ? x.key.value : x.key) === k);
    if (o && isScalar(o.value) && isScalar(p.value)) p.value.type = o.value.type;
  }
}

// ───────────── Pasos ─────────────

/** Paso nuevo de esa acción. */
export function nuevoPaso(accion: Accion, valor = ""): YAMLMap {
  const m = new YAMLMap();
  m.items.push(new Pair(new Scalar(accion), escalar(valor)));
  return m;
}

export const insertarPaso = (i: number, accion: Accion = "elegir", valor = ""): Cambio => (d) => {
  seq(d, "pasos").items.splice(i, 0, nuevoPaso(accion, valor));
};

/** Cambia la acción de un paso conservando el valor y su lugar en el mapa. `mueve_dinero` solo vale en `enviar`. */
export const cambiarAccion = (i: number, nueva: Accion): Cambio => (d) => {
  const paso = d.getIn(["pasos", i], true);
  if (!isMap(paso)) return;
  const par = paso.items.find((p) => isPair(p) && isScalar(p.key) && ACCIONES.includes(String(p.key.value) as Accion));
  if (par && isScalar(par.key)) par.key.value = nueva;
  else paso.items.unshift(new Pair(new Scalar(nueva), escalar("")));
  if (nueva !== "enviar") paso.delete("mueve_dinero");
};

/** Fija el valor de la acción del paso (`marcar: "*555#"` → el código). */
export const fijarValorPaso = (i: number, accion: Accion, v: string): Cambio => fijar(["pasos", i, accion], v);

/** Fija un modificador. `undefined` lo quita. Los nuevos se ponen después de la acción, en el orden de la tabla. */
export const fijarModificador = (i: number, campo: Modificador | "mueve_dinero", v: string | number | boolean | undefined): Cambio =>
  (d) => {
    if (v === undefined || v === false) return quitar(["pasos", i, campo])(d);
    fijar(["pasos", i, campo], v)(d);
  };

// ───────────── Parámetros ─────────────

export const agregarParametro = (nombre: string): Cambio => (d) => {
  const s = seq(d, "parametros");
  const m = new YAMLMap();
  // Como el resto de las recetas: un parámetro por línea, `{ nombre: …, patron: … }`.
  m.flow = s.items.length === 0 || s.items.some((x) => isMap(x) && x.flow);
  m.items.push(new Pair(new Scalar("nombre"), new Scalar(nombre)));
  s.items.push(m);
};

/** Fija `opciones` conservando el estilo de la lista (en una línea si ya lo estaba; nueva: en una línea). */
export const fijarOpciones = (i: number, opciones: string[] | null): Cambio => (d) => {
  const ruta = ["parametros", i, "opciones"];
  if (!opciones || opciones.length === 0) return quitar(ruta)(d);
  const antes = d.getIn(ruta, true);
  const s = new YAMLSeq();
  s.flow = isSeq(antes) ? antes.flow : true;
  for (const o of opciones) {
    const x = new Scalar(o);
    x.type = Scalar.QUOTE_DOUBLE;
    s.items.push(x);
  }
  d.setIn(ruta, s);
};

/** Fija `nombres_anteriores` (lista vacía: quita la clave). Conserva el estilo de la lista; nueva: en una línea. */
export const fijarNombresAnteriores = (nombres: string[]): Cambio => (d) => {
  if (nombres.length === 0) return quitar(["nombres_anteriores"])(d);
  const antes = d.get("nombres_anteriores", true);
  const s = new YAMLSeq();
  s.flow = isSeq(antes) ? antes.flow : true;
  for (const n of nombres) s.items.push(new Scalar(n));
  d.set("nombres_anteriores", s);
};

// ───────────── Salida ─────────────

function mapaSalida(d: Document): YAMLMap {
  let m = d.get("salida", true);
  if (!isMap(m)) {
    m = new YAMLMap();
    d.set("salida", m);
  }
  return m as YAMLMap;
}

export const agregarSalida = (clave: string): Cambio => (d) => {
  const m = mapaSalida(d);
  m.flow = false;
  m.items.push(new Pair(new Scalar(clave), escalar("")));
};

/** Renombra una clave de `salida` en su lugar (el orden y los comentarios no cambian). */
export const renombrarSalida = (i: number, nueva: string): Cambio => (d) => {
  const par = mapaSalida(d).items[i];
  if (par && isScalar(par.key)) par.key.value = nueva;
  else if (par) par.key = new Scalar(nueva);
};

export const fijarSalida = (i: number, regex: string): Cambio => (d) => {
  const par = mapaSalida(d).items[i];
  if (!par) return;
  if (isScalar(par.value)) par.value.value = regex;
  else par.value = escalar(regex);
};

export const borrarSalida = (i: number): Cambio => (d) => {
  const m = mapaSalida(d);
  m.items.splice(i, 1);
  if (m.items.length === 0) d.delete("salida");
};

// ───────────── Plantillas ─────────────

export function plantillaNueva(operador = "TIGO"): string {
  const op = operador.toLowerCase();
  return `nombre: ${op}-nueva
titulo: Nueva receta
operador: ${operador}
descripcion: ''
modo: pantalla
parametros: []
pasos:
  - marcar: '*105#'
`;
}

/** Copia de otra receta con nombre `<nombre>-copia` (lo demás, comentarios incluidos, igual). */
export function plantillaCopia(yaml: string, nombre: string): string {
  return aplicar(yaml, (d) => {
    fijar(["nombre"], `${nombre}-copia`)(d);
    const t = d.get("titulo");
    if (typeof t === "string" && t) fijar(["titulo"], `${t} (copia)`)(d);
  });
}
