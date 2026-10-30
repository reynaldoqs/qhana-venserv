---
type: Proyecto
title: Protocolo agente ↔ control, API del control y formato de recetas (v1)
description: Contrato entre la app Android (agente) y el control (Rust). Fuente única de verdad; los dos proyectos se construyen contra este documento.
tags: [general]
estado: borrador
timestamp: 2026-10-29
---

# Protocolo v1

```
[Teléfono Android: app agente] ── WebSocket ──► [control (Rust): servidor :47800] ◄── HTTP/WS ── [Interfaz]
```

- **El agente es el cliente** y se conecta al control. Así funciona igual por USB y por Wi-Fi:
  - **USB (ahora):** `adb reverse tcp:47800 tcp:47800`. El agente usa `ws://127.0.0.1:47800/agente`
    y ADB lleva la conexión por el cable hasta el PC.
  - **Wi-Fi (después):** el agente usa `ws://<IP del PC>:47800/agente`. Nada más cambia.
- Un solo puerto (**47800**) para el agente, la API y la interfaz.
- Mensajes: frames de texto **JSON UTF-8**. Cada mensaje tiene `"tipo"`.
- Las peticiones llevan `"req"`, un id único elegido por quien pregunta, y la respuesta lo repite.
- Nombres de campos en `snake_case` y en español. Tiempos en milisegundos (`*_ms`); horas en epoch ms.
- Campos desconocidos se **ignoran**, para poder agregar campos sin romper versiones viejas.
  Si cambia el significado de un campo, sube `protocolo`.

## Tipos compartidos

### `Sim`

```json
{
  "slot": 0,                    // ranura física: 0, 1…
  "sub_id": 4,                  // id de suscripción de Android (cambia si cambias el chip)
  "operador": "TIGO",           // nombre que muestra la red
  "mcc_mnc": "73603",
  "iccid": "8959103000797642548", // null si Android no lo entrega (requiere dueño del dispositivo)
  "numero": "78815232",         // null si el chip no lo tiene guardado
  "estado": "listo",            // listo | ausente | pin | puk | bloqueada | desconocido
  "voz": "en_servicio",         // en_servicio | sin_servicio | solo_emergencia | apagado | null
  "red": "LTE"                  // tecnología de voz o null
}
```

### `Sms`

```json
{ "id": "1234", "slot": 0, "de": "300", "texto": "…", "fecha_ms": 1793300000000, "leido": false }
```

### Códigos de error

`ocupado` (ya hay una sesión USSD en curso) · `sin_permiso` · `sin_accesibilidad` · `sim_invalida`
· `timeout` · `red` (el operador rechazó o cortó) · `sesion_inexistente` · `parametro` · `desconocido`
· `sin_receta` · `tipo_chip` (la receta es de otro tipo de chip: ver "Chips PDA")

## Agente → control

| tipo | Campos | Cuándo |
|---|---|---|
| `hola` | `protocolo` (1), `token`, `dispositivo_id`, `fabricante`, `modelo`, `android` ("14"), `sdk` (34), `app_version`, `sims: [Sim]`, `capacidades: ["ussd_api","ussd_pantalla","sms"]`, `adb_serial` (opcional) | **Primer mensaje** al conectar |
| `sims` | `sims: [Sim]` | Al cambiar los chips o su estado |
| `estado` | `bateria` (0–100), `cargando`, `pantalla_encendida`, `accesibilidad_activa`, `permisos_faltantes: [string]` | Cada 30 s y cuando cambia |
| `ussd` | `req`, `sesion`, `texto`, `abierta` (bool), `ms` | Respuesta a `ussd_iniciar` / `ussd_responder` |
| `ok` | `req` | Respuesta a `ussd_cancelar` / `refrescar` |
| `error` | `req`, `codigo`, `detalle` | Cualquier petición que falla |
| `sms_recibido` | `sms: Sms` | Llega un SMS (evento, sin `req`) |
| `sms_lista` | `req`, `sms: [Sms]` | Respuesta a `sms_listar` |
| `pong` | `req` | Respuesta a `ping` |

`dispositivo_id` = `Settings.Secure.ANDROID_ID` de la app. Es estable mientras no se resetee el teléfono ni se
reinstale la app con otra firma.

`adb_serial`: desde Android 8 cada app ve un `ANDROID_ID` distinto al de `adb shell`, así que **no** sirve para
cruzar teléfono ↔ USB. El script de provisión le pasa a la app su serial de ADB (`--es adb_serial <serial>`), la
app lo guarda y lo manda en `hola`, y el control vincula con ese dato. Por Wi-Fi sin ADB va `null`.

Al instalar o abrir la app pueden llegar 2–3 `hola` seguidos del mismo `dispositivo_id`: el control se queda con
la conexión más nueva y cierra la anterior.

## Módems USB (sin agente)

Un módem USB con comandos AT no tiene app ni WebSocket: el propio control abre el puerto serie (crate `modem`)
y lo registra como un dispositivo más. Para la API, las recetas y los lotes es indistinguible de un teléfono.

| Campo del `Dispositivo` | Valor en un módem |
|---|---|
| `id` | **IMEI** del módem (nunca el número de COM: cambia al reenchufar) |
| `fabricante`, `modelo` | `AT+CGMI` / `AT+CGMM` (p. ej. `HUAWEI`, `E303`) |
| `android` | `""` |
| `app_version` | `modem` |
| `adb_serial` | `null` |
| `capacidades` | `["ussd_api","modem"]` (sin `ussd_pantalla`; `sms` se suma cuando se implemente) |
| `sims` | un solo `Sim` con `slot: 0` (`sub_id: 0`). `iccid` por `AT^ICCID?` (o `AT+QCCID`/`AT+CCID`), `operador` y `mcc_mnc` por `AT+COPS?`, `numero` por `AT+CNUM` o `null` |

- El módem solo tiene el **slot 0**: cualquier otro `slot` en `ussd_iniciar` responde `sim_invalida`.
- Soporta una sesión USSD a la vez; una segunda petición responde `ocupado`.
- `modo` se ignora (siempre es "api": el módem devuelve el menú como texto, sin ventana).
- El control detecta los puertos cada 3 s, prueba `AT` y agrupa los puertos del mismo módem por IMEI.
  Si el puerto desaparece se emite `dispositivo_desconectado`, y al volver, `dispositivo_conectado`.
- Formato del USSD según el fabricante: Huawei exige el código empaquetado en GSM de 7 bits en hex
  (`*105#` → `AA18AC3602`) y devuelve la respuesta igual; Quectel/SIMCom/otros usan texto plano con
  `AT+CSCS="GSM"` (sin probar con hardware todavía).

## Control → agente

| tipo | Campos | Respuesta |
|---|---|---|
| `bienvenido` | `servidor` ("qhana-control 0.1.0"), `hora_ms` | — (tras un `hola` válido) |
| `rechazado` | `motivo` | — (luego cierra; token inválido o protocolo incompatible) |
| `ussd_iniciar` | `req`, `slot`, `codigo` ("*105#"), `modo` ("api" \| "pantalla"), `timeout_ms` (30000) | `ussd` o `error` |
| `ussd_responder` | `req`, `sesion`, `texto`, `timeout_ms` | `ussd` o `error` |
| `ussd_cancelar` | `req`, `sesion` | `ok` o `error` |
| `sms_listar` | `req`, `slot` (opcional), `desde_ms` (opcional), `limite` (50) | `sms_lista` |
| `refrescar` | `req` | envía `sims` y `estado`, luego `ok` |
| `ping` | `req` | `pong` |

### Sesiones USSD

- **`modo: "api"`** usa `TelephonyManager.sendUssdRequest`: un solo paso, sin ventana en pantalla.
  La respuesta llega siempre con `abierta: false` y no se puede contestar.
  ⚠️ **Con Tigo no funciona** (medido el 29-oct-2026 en un Infinix con Android 14): Tigo deja la sesión abierta
  incluso para `*105#`, Android lo trata como falla (código −1), y la red queda ocupada 11–25 s ("cannot accept
  second ussd request"). El agente aplica 30 s de enfriamiento tras una falla del modo api. **Las recetas de Tigo
  usan `modo: pantalla`.**
- **`modo: "pantalla"`** marca con `ACTION_CALL` (por el chip del `slot`) y el servicio de accesibilidad lee
  la ventana de USSD del sistema. Si la ventana tiene campo de respuesta, `abierta: true` y se sigue con
  `ussd_responder`.
- **Una sesión por teléfono a la vez.** Un `ussd_iniciar` con otra abierta devuelve `ocupado`.
- Mientras el agente corre, **no usar `uiautomator dump`** desde ADB: Android desconecta los servicios de
  accesibilidad durante el volcado y el agente queda ciego unos segundos.
- Quien abre una sesión la cierra con `ussd_cancelar`. El agente cierra solo las sesiones sin actividad por 60 s.
- `sesion` es un id que genera el agente (UUID). `ms` es lo que tardó ese paso.

### Conexión

- El control manda `ping` cada 15 s. Sin `pong` en 45 s, da al agente por caído.
- El agente reconecta con espera creciente (1, 2, 4… hasta 30 s), para siempre.
- Al reconectar se descartan las sesiones abiertas: la ventana de USSD se cancela.

## API del control (para la interfaz)

Base: `http://127.0.0.1:47800`. Los errores responden HTTP 4xx/5xx con `{"error": {"codigo", "detalle"}}`.

| Método y ruta | Cuerpo | Respuesta |
|---|---|---|
| `GET /api/salud` | — | `{version, agentes_conectados}` |
| `GET /api/dispositivos` | — | `[Dispositivo]` |
| `GET /api/adb` | — | `[{serial, estado, modelo, reverse_ok, dispositivo_id}]` |
| `POST /api/adb/{serial}/provisionar` | `{apk?}` (por defecto el `--apk` del control) | `{ok, pasos: [{paso, ok, salida}]}`. Deja el teléfono listo por USB: instala el APK, concede permisos, quita el límite de batería, `stayon usb`, activa la accesibilidad, `adb reverse` y abre la app. Equivale a `agente-android/scripts/provisionar.sh`. `404 sin_dispositivo` si ADB no lo ve en estado `device`; `503 sin_adb` si el control corre con `--sin-adb`; `400 sin_apk` si no hay APK (ni en el cuerpo ni `--apk`/`QHANA_APK`) y la app no está instalada en el teléfono (con la app ya instalada, sin APK solo reconfigura) |
| `POST /api/ussd/iniciar` | `{dispositivo, slot, codigo, modo?}` | `{sesion, texto, abierta, ms}` |
| `POST /api/ussd/responder` | `{dispositivo, sesion, texto}` | `{sesion, texto, abierta, ms}` |
| `POST /api/ussd/cancelar` | `{dispositivo, sesion}` | `{ok: true}` |
| `GET /api/dispositivos/{id}/sms?limite=50` | — | `[Sms]` |
| `GET /api/recetas` | — | `[{nombre, titulo, operador, descripcion, parametros}]` |
| `POST /api/recetas/{nombre}/ejecutar` | `{dispositivo, slot, parametros: {}, simular}` | `Resultado` |
| `GET /api/historial?limite=100` | — | `[Evento]` (guardados en SQLite) |
| `WS /api/eventos` | — | flujo de `Evento` |

`Dispositivo`:
```json
{ "id": "a1b2…", "conectado": true, "adb_serial": "1026925361012456", "fabricante": "INFINIX",
  "modelo": "Infinix X678B", "android": "14", "app_version": "0.1.0", "sims": [Sim],
  "estado": { "bateria": 67, "cargando": true, … } , "ultima_vez_ms": 0, "ocupado": false }
```

`Resultado` de una receta:
```json
{ "ok": true, "estado": "ok", "detalle": null,
  "pasos": [ { "accion": "marcar", "enviado": "*105#", "texto": "…", "abierta": false, "ms": 2900 } ],
  "salida": { "saldo_principal": "10" } }
```
`estado`: `ok` · `simulado` · `abortado` (se cortó antes de mover dinero) · `incierto` (sin respuesta clara tras
`confirmar`: hay que revisar el saldo) · `error`.

`Evento`: `{ "tipo", "ts_ms", …datos }` con `tipo` ∈ `dispositivo_conectado`, `dispositivo_desconectado`,
`dispositivo_actualizado`, `chip_cambiado`, `ussd`, `sms`, `adb`, `receta`, `lote`.
`chip_cambiado`: ver "Cambio de chips".

## Recetas (YAML, en `control/recetas/`)

Una receta describe un flujo USSD de un operador. El motor la ejecuta contra cualquier canal (teléfono hoy,
módem mañana).

```yaml
nombre: tigo-transferir
titulo: Transferir saldo Tigo
operador: TIGO
descripcion: Transferencia por única vez. Comisión 0.20 Bs, máximo 5 por día.
modo: pantalla
parametros:
  - { nombre: numero, patron: '^[67]\d{7}$' }
  - { nombre: monto, opciones: ["1", "2", "5", "10", "20"] }
pasos:
  - marcar: "*66#1#"
    error_si: 'maximo numero de transfer'
    esperar: 'transferencias'
  - elegir: 'Transferir por unica vez'
    esperar: 'numero de destino'
  - enviar: '{numero}'
    esperar: 'monto'
  - elegir: '{monto}\s*Bs'
    esperar: 'Confirma'
  - verificar: 'transferir.*\b{monto}(\.00)?\s*Bs a {numero}'
  - confirmar: 'Si'
    exito: 'transferido\s+{monto}(\.0+)?\s+a\s+{numero}'
salida:
  costo: 'costo de ([\d.]+)'
```

| Acción | Qué hace |
|---|---|
| `marcar: código` | Inicia la sesión (`ussd_iniciar`). Solo el primer paso debe ser `marcar`, pero puede repetirse más adelante **si la sesión anterior ya cerró** (p. ej. `entel-saldo`: `*105#` y luego `*#66#`); con la sesión abierta es un error |
| `enviar: texto` | Contesta ese texto literal |
| `elegir: regex` | Busca en el menú la opción cuyo texto coincide **completo** con la regex (sin importar mayúsculas) y contesta su número. Entiende `1.-Texto`, `1. Texto`, `1) Texto` y `1 Texto` |
| `verificar: regex` | No envía nada: exige que el texto actual coincida. Si no, aborta |
| `confirmar: regex` | Igual que `elegir`, pero es **el paso que mueve dinero**: nunca se reintenta, y en `simular` elige la opción `No` |

Modificadores de un paso:

| Modificador | Qué hace |
|---|---|
| `esperar: regex` | La respuesta de ese paso debe coincidir o se aborta |
| `error_si: regex` | Si la respuesta coincide, aborta con ese texto como motivo |
| `exito: regex` | Si coincide, la receta terminó bien. Si no hay `exito` en ningún paso, basta con llegar al final |
| `timeout_ms` | Tiempo máximo de ese paso |

Reglas:
- `{param}` se reemplaza por el valor del parámetro. Dentro de una regex el valor se escapa.
- `salida` define regex con un grupo; se aplican sobre todos los textos recibidos y van a `Resultado.salida`.
- Al terminar, o ante cualquier aborto, el motor **siempre** cancela la sesión si quedó abierta.
- Si después de `confirmar` no hay respuesta o no coincide con `exito`, el estado es `incierto`, nunca `ok`.
- `modo: api` solo admite un paso `marcar`.

---

# Lotes (CSV) — v1

Un **lote** es una lista de operaciones cargada desde un CSV, por ejemplo "transferir X Bs a tal número". El flujo
tiene tres etapas separadas: **validar** (no toca ningún teléfono, salvo la consulta opcional de saldos),
**ejecutar** (simulado o real) y **reportar** (CSV de resultados).

## CSV de entrada

```csv
numero,monto,operador,accion,referencia
69869869,12,TIGO,transferir,Pedido 1001
+591 7881-5232,5,tigo,,Pedido 1002
```

| Columna | Obligatoria | Acepta |
|---|---|---|
| `numero` | sí | 8 dígitos. Se limpian espacios, guiones, puntos y el prefijo `+591` / `591` |
| `monto` | sí | Entero en Bs: `12`, `12.00`, `12,00`, `Bs 12`. Con decimales distintos de cero es **error** |
| `operador` | sí | `TIGO`, `ENTEL`, `VIVA` (alias `NUEVATEL`), sin importar mayúsculas ni acentos. No se deduce del número, porque hay portabilidad |
| `accion` | no | Por defecto `transferir` |
| `referencia` | no | Texto libre que vuelve igual en el CSV de resultados |

- Separador `,` o `;` (Excel en español usa `;`), detectado solo. UTF-8, con o sin BOM.
- Encabezados flexibles: `numero|número|telefono|teléfono|celular|linea|línea`, `monto|cantidad|importe|bs|carga`,
  `operador|operadora|empresa|compania|compañía`, `accion|acción|operacion|operación`, `referencia|ref|nota|id`.
- Si la primera fila no parece un encabezado, se asume el orden `numero,monto,operador`.
- Máximo 5 000 filas.

## Recetas: campos para lotes

```yaml
accion: transferir          # qué hace la receta; el lote busca la receta con (operador, accion)
limites:
  por_dia: 5                # envíos por chip y por día
  comision: 0.20            # costo por envío, en Bs
```

- `chip_tipo: pda` (opcional) marca una receta propia de los chips PDA; sin él vale para todos los chips. Ver
  "Chips PDA".
- Los montos permitidos salen de `parametros[monto].opciones`, y el formato del número de `parametros[numero].patron`.
- La receta con `accion: saldo` de cada operador sirve para consultar el saldo de los chips; su `salida` debe
  incluir `saldo_principal`.

## Validación (sin ejecutar nada)

Por cada fila:

| Chequeo | Si falla |
|---|---|
| Número con el formato de la receta | `error` |
| Monto entero > 0 y que se pueda formar con los montos permitidos (12 → 10 + 2, la menor cantidad de envíos) | `error` |
| Existe receta para (operador, acción) | `error` ("no hay receta de transferir para ENTEL") |
| Número repetido en el lote | `aviso` (puede ser intencional) |
| Monto alto (> 100 Bs por fila) | `aviso` |

Y para el lote completo, el **plan**: se reparte cada envío entre los chips del operador que estén en línea y
listos, según el **cupo de hoy** (`por_dia` menos los envíos `ok` de hoy de ese chip, contando las ejecuciones
guardadas) y el **saldo principal conocido** (la última consulta; debe alcanzar para monto + comisión). Se prefiere
el chip con más saldo y menos envíos hoy. Lo que no entra queda `sin_capacidad`, con el motivo (cupo o saldo).

`consultar_saldos: true` primero consulta el saldo de los chips de los operadores del lote que no tengan un dato
de menos de 5 minutos. Es solo lectura, gratis, y tarda ~3.5 s por chip (en paralelo entre teléfonos).

### `POST /api/lotes/validar`

Cuerpo `{ "csv": "<texto>", "consultar_saldos": true }`. Respuesta `Validacion`:

```json
{
  "filas": [
    { "fila": 2, "numero": "69869869", "monto": 12, "operador": "TIGO", "accion": "transferir",
      "referencia": "Pedido 1001", "receta": "tigo-transferir", "estado": "valida",
      "errores": [], "avisos": [], "envios": [10, 2],
      "plan": [ { "monto": 10, "dispositivo": "6f44…", "slot": 1 }, { "monto": 2, "dispositivo": "6f44…", "slot": 0 } ],
      "sin_capacidad": 0, "original": { "numero": "69869869", "monto": "12", "operador": "TIGO" } }
  ],
  "resumen": {
    "filas": 10, "validas": 8, "con_avisos": 1, "invalidas": 2,
    "total_bs": 85, "envios": 11, "comision_total": 2.2,
    "planificados": 9, "sin_capacidad": 2,
    "operadores": [ { "operador": "TIGO", "chips": 2, "cupo_hoy": 7, "saldo_conocido": 17.23,
                      "saldo_desconocido_chips": 0, "necesario_bs": 87.2, "envios": 11 } ]
  },
  "chips": [ { "dispositivo": "6f44…", "slot": 0, "operador": "TIGO", "numero": "+59178815232",
               "saldo": 10.0, "saldo_ms": 1793330000000, "enviados_hoy": 0, "cupo_hoy": 5 } ]
}
```

`estado` de una fila: `valida` · `aviso` (válida con advertencias) · `invalida`.

## Ejecución

### `POST /api/lotes`

Cuerpo `{ "nombre": "Pedidos 29-oct", "csv": "…", "simular": true }`. Vuelve a validar en el servidor
(**sin** consultar saldos), guarda el lote con **solo las filas válidas** (las inválidas quedan como `omitida`)
y lo deja en estado `listo`. Responde el `Lote`.

### Control

`POST /api/lotes/{id}/iniciar` · `POST /api/lotes/{id}/pausar` · `POST /api/lotes/{id}/cancelar`

Reglas del ejecutor:
- Cada fila se parte en sus envíos. Cada envío se asigna **en el momento** al mejor chip disponible, con las reglas
  del plan y los datos actualizados. Cada teléfono ejecuta un envío a la vez y los teléfonos trabajan en paralelo.
- Tras un envío `ok`, el saldo conocido del chip baja en monto + comisión y su cupo baja en 1.
- `abortado` (no se movió dinero) se reintenta en otro chip:
  - si el operador dijo "máximo de transferencias", ese chip queda **agotado por hoy**;
  - si en el menú no aparece el monto, el chip queda **sin saldo** (saldo conocido 0);
  - `timeout`, `ocupado` o `red` antes de confirmar: hasta 2 reintentos, con 5 s de espera.
- **`incierto` pausa el lote completo** y emite una alerta: alguien debe revisar el saldo antes de seguir.
  Nunca se reintenta un envío incierto.
- Si no queda ningún chip con capacidad, el lote termina y lo pendiente queda `sin_capacidad` ("para mañana"). Un
  lote se puede volver a `iniciar` otro día y retoma solo lo pendiente.
- Lote `simular: true`: cada envío corre la receta en modo simulación (llega a la confirmación y responde No).
  No descuenta saldo ni cupo.
- Lote real: al iniciar o reanudar, el ejecutor consulta el saldo de los chips de los operadores del lote (los que no
  tengan un dato de menos de 5 minutos) y guarda ese **saldo inicial** de cada chip. Sirve para `verificar`.
- Al reiniciar el control, un lote `en_curso` pasa a `pausado`.

Estados de un lote: `listo` · `en_curso` · `pausado` · `terminado` · `cancelado`.

Estados de una fila: `omitida` (inválida) · `pendiente` · `en_curso` · `ok` · `simulado` · `parcial` (algunos envíos
ok) · `fallida` · `incierta` · `sin_capacidad`.

### `POST /api/lotes/{id}/reintentar-fallidas`

Vuelve a `pendiente` todos los envíos `fallido` del lote para que `iniciar` los intente de nuevo. Es seguro: un envío
`fallido` nunca pasó por `confirmar` (cualquier problema después de confirmar es `incierto`), así que no movió dinero.
De una fila `parcial` solo se reintentan las piezas fallidas; las `ok` no se repiten.

- Responde el `Lote`. Si estaba `terminado` pasa a `listo`; si estaba `pausado` se queda `pausado`.
- Después hay que llamar a `iniciar` (así el operador decide cuándo).
- Errores `409`: `en_curso` (el lote está corriendo), `cancelado`, `hay_inciertos` (hay un envío `incierto` sin
  resolver: revisa el saldo primero), `nada_que_reintentar` (no hay envíos `fallido`).
- Un envío que falló por una causa permanente (p. ej. "número de destino inválido") volverá a fallar: corrige el
  dato en un lote nuevo.

### `GET /api/lotes` · `GET /api/lotes/{id}`

`Lote`:
```json
{ "id": 3, "nombre": "Pedidos 29-oct", "creado_ms": 0, "estado": "en_curso", "simular": false,
  "motivo_pausa": null,
  "revision": { "completo": false, "motivos": ["2 filas fallidas", "1 fila para mañana"] },
  "verificacion": null,
  "progreso": { "filas": 8, "ok": 3, "simulado": 0, "parcial": 0, "fallidas": 0, "inciertas": 0,
                "pendientes": 5, "sin_capacidad": 0, "omitidas": 2,
                "enviado_bs": 30, "comision_bs": 0.8, "total_bs": 85 },
  "filas": [ { "fila": 2, "numero": "69869869", "monto": 12, "operador": "TIGO", "accion": "transferir",
               "referencia": "Pedido 1001", "estado": "ok", "enviado_bs": 12, "detalle": null,
               "envios": [ { "monto": 10, "dispositivo": "6f44…", "slot": 1, "estado": "ok",
                             "ejecucion_id": 41, "detalle": null, "ts_ms": 0 } ] } ] }
```
En `progreso`, `filas` cuenta solo las filas a ejecutar (sin las `omitidas`, que van aparte).
En `GET /api/lotes` (la lista) no van las `filas`; `revision` y `verificacion` sí.

`revision` responde "¿terminó todo bien?" sin interpretar números: `completo` es `true` solo si el lote está
`terminado` y **todas** las filas a ejecutar quedaron `ok` (o `simulado` en una simulación). Si no, `motivos` dice por
qué ("lote pausado", "2 filas fallidas", "1 fila parcial", "3 filas para mañana", "1 fila incierta"…). `terminado`
por sí solo **no** significa éxito: solo que el ejecutor ya no tiene nada más que intentar.

### `POST /api/lotes/{id}/verificar`

Cuadra el dinero contra los saldos reales. Consulta (solo lectura, ~3.5 s por chip) el saldo actual de cada chip que
envió algo y lo compara con el **saldo inicial** guardado al último `iniciar`:

`descontado_real = saldo_inicial − saldo_actual` contra `descontado_esperado = Σ (monto + comisión)` de los envíos `ok`
hechos desde ese inicio. Cuadra si la diferencia es de ±0.10 Bs o menos.

```json
{ "lote_id": 3, "verificado_ms": 0, "completo": true, "motivos": [],
  "cuadra": true, "confirmado": true,
  "chips": [ { "dispositivo": "6f44…", "slot": 1, "numero": "78815232", "estado": "cuadra",
               "saldo_inicial": 120.0, "saldo_actual": 87.4,
               "descontado_esperado": 32.6, "descontado_real": 32.6, "diferencia_bs": 0.0, "envios_ok": 3 } ],
  "avisos": [] }
```

- `chips[].estado`: `cuadra` · `diferencia` (se descontó más o menos de lo esperado) · `sin_dato` (no hubo saldo
  inicial o no se pudo consultar el actual).
- `cuadra`: todos los chips `cuadra`. `confirmado`: `completo` **y** `cuadra`; es el indicador "lote bien hecho".
- Solo compara desde el último inicio o reanudación: lo enviado antes ya está en el saldo inicial.
- Solo vale si nadie usó esos chips por fuera durante el lote (recargas, envíos a mano): cualquier movimiento ajeno
  aparece como `diferencia`. El motivo concreto se ve en el historial del chip.
- Errores `409`: `en_curso`, `simulado` (no mueve dinero, no hay nada que cuadrar), `sin_base` (el lote nunca se inició).

El último resultado se **guarda** y viene en `Lote.verificacion` (el mismo objeto que devuelve `verificar`, o `null`
si nunca se verificó), así que sigue ahí al recargar o al volver otro día. Cada `verificar` reemplaza al anterior.
No es un estado del lote: un lote `terminado` se puede verificar las veces que haga falta. Se **borra** cuando el
lote cambia (`iniciar`, `reanudar` o `reintentar-fallidas`), porque ya no describe lo que hay.

### `GET /api/lotes/{id}/resultado.csv`

```csv
fila,numero,operador,accion,monto,referencia,estado,enviado_bs,comision_bs,envios_ok,envios_total,chips,detalle,hora
```
`chips`: los números de origen separados por `|`. `hora`: la del último envío, en hora local.

### `GET /api/lotes/plantilla.csv`

Devuelve un CSV de ejemplo con el encabezado y 3 filas.

### Eventos

`{ "tipo": "lote", "lote_id", "fase": "inicio" | "envio" | "fila" | "pausa" | "fin", … }`:
- `envio` lleva `fila`, `monto`, `dispositivo_id`, `slot`, `estado`;
- `fila` lleva `fila` y `estado`;
- `pausa` lleva `motivo`;
- todos llevan `progreso`.

## Saldos de los chips

`GET /api/chips` → la lista de chips en línea con su `saldo`, `saldo_ms`, `enviados_hoy`, `cupo_hoy` (por
operador), `sin_verificar` y `tipo` (`normal` | `pda`). `POST /api/chips/saldos` → consulta el saldo de todos los chips en línea (o `{ "operadores": ["TIGO"] }`)
y devuelve la lista actualizada. Cada ejecución de una receta `accion: saldo` actualiza el saldo conocido de ese chip.

## Cambio de chips

Los chips se cambian seguido, así que el control identifica **el chip**, no la ranura. Todo lo que es del chip
(saldo, cupo del día, agotado) lo sigue, y nada de eso lo hereda otro chip que entre a la misma ranura.

**Identidad.** Se compara con `Sim.iccid` y `Sim.numero`: dos lecturas son el mismo chip si coincide el ICCID; si
alguna lectura no trae ICCID, si coincide el número. Si falta un dato en una lectura, no cuenta como diferencia (un
chip que pide PIN y no entrega ICCID ni número sigue siendo el mismo). Un chip sin ninguna de las dos señales no
se puede distinguir de otro en la misma ranura: ese caso no detecta cambios.

**Cuándo se revisa.** En cada `hola`, en cada `sims` y antes de calcular `GET /api/chips`. Una ranura vacía, con
`estado: "ausente"` o que ya no aparece en `sims` cierra la estadía del chip que tenía.

**Qué pasa cuando entra otro chip a una ranura que ya tuvo uno:**
1. Se borran el saldo conocido y su número de esa ranura.
2. El chip nuevo arranca con `sin_verificar: true` (saldo desconocido, aún no consultado). **No entra al reparto**: sus filas
   quedan `sin_capacidad` con el motivo "sin saldo verificado".
3. Se emite el evento `chip_cambiado`: `{ dispositivo_id, slot, anterior: { iccid, numero } | null,
   nuevo: { iccid, numero }, regreso: bool }`. `regreso` es true si ese chip ya se había visto antes.
4. El control consulta su saldo solo (receta `accion: saldo`, solo lectura), reintentando cada 10 s hasta 2 minutos
   mientras el chip no tenga servicio o el teléfono esté ocupado. Al fijarse el saldo, `sin_verificar` pasa a `false`.

La primera vez que se ve una ranura no cuenta como cambio: conserva el saldo y el historial que ya tenía y no queda
`sin_verificar`; igual se consulta su saldo si no lo tiene.

**Cupo y agotado son del chip.** El cupo de hoy (`por_dia`) cuenta los envíos `ok` de hoy que hizo *ese chip*, en
cualquier ranura o teléfono en que haya estado. Si el operador responde "máximo de transferencias", el chip queda
agotado hoy esté donde esté, y el chip que entre después a su ranura no hereda la marca.

## Chips PDA

Un chip es `normal` (por defecto) o `pda`: chips especiales con USSD propio y límites distintos. Android no dice
cuál es cuál, así que se marca a mano. El tipo es **del chip** (como el saldo y el cupo): lo sigue si cambia de
teléfono o de ranura, y un chip nuevo entra como `normal`.

**Marcar.** `PATCH /api/chips/{dispositivo}/{slot}` con `{ "tipo": "pda" }` marca el chip que ocupa esa ranura
ahora. Responde el `ChipInfo` actualizado y emite el evento `chip_tipo`: `{ dispositivo_id, slot, tipo, anterior }`.
`404 sin_dispositivo` si el chip no está en línea.

**Selección de receta.** Una receta se busca por `(operador, accion)` y, además, por `chip_tipo`. Para un chip
de tipo `pda`:
1. la receta con `accion`, `operador` y `chip_tipo: pda`, si existe;
2. si no, la receta de la compañía (`accion` y `operador`, sin `chip_tipo`);
3. si tampoco, `sin_receta`.

Un chip `normal` solo ve las recetas sin `chip_tipo`; nunca usa una receta PDA. La especialización es por acción:
si solo existe `tigo-pda-recargar`, el chip PDA recarga con ella y consulta el saldo con `tigo-saldo`.

Eso rige en todo lugar donde se elige receta por chip: la consulta de saldos, el cupo de hoy (`limites.por_dia` de
la receta que usaría ese chip) y la ejecución de los envíos de un lote. `Resultado.receta` es siempre la receta que
realmente corrió.

**Receta por nombre** (`POST /api/recetas/{nombre}/ejecutar`): si el chip es `pda` y la receta nombrada es de la
compañía, se sustituye por su variante PDA (misma `accion` y `operador`) cuando existe. Si la receta nombrada es
`chip_tipo: pda` y el chip es `normal`, responde `409 tipo_chip` y no marca nada.

**Lotes.** La validación del CSV y los montos/formato usan la receta de la compañía (los mismos para todos los
chips). Un chip PDA corre su variante PDA, si existe, al ejecutar; si esa receta rechaza el monto o el número, el
envío falla de forma normal y se reintenta en otro chip.
