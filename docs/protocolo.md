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
· `sin_receta` · `tipo_chip` (la receta es de otro tipo de chip: ver "Chips dealer")
· `no_autorizado` (API sin token válido: ver "Autenticación de la API") · `duplicado` (ver `POST /api/lotes`)
· `receta_invalida` · `existe` · `conflicto` (ver "Editar recetas desde la interfaz")

Solo el control emite `sin_receta`, `tipo_chip`, `no_autorizado`, `duplicado`, `receta_invalida`, `existe` y
`conflicto`. El agente emite los demás. `receta_invalida` lleva además `errores` dentro de `error`:
`{"error": {"codigo": "receta_invalida", "detalle": "…", "errores": [{paso, campo, mensaje}]}}`.

## Agente → control

| tipo | Campos | Cuándo |
|---|---|---|
| `hola` | `protocolo` (1), `token`, `dispositivo_id`, `fabricante`, `modelo`, `android` ("14"), `sdk` (34), `app_version`, `sims: [Sim]`, `capacidades: ["ussd_api","ussd_pantalla","sms"]`, `adb_serial` (opcional) | **Primer mensaje** al conectar |
| `sims` | `sims: [Sim]` | Al cambiar los chips o su estado |
| `estado` | `bateria` (0–100, o `null` si Android no la informa), `cargando`, `pantalla_encendida`, `accesibilidad_activa`, `permisos_faltantes: [string]` | Cada 30 s y cuando cambia |
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

Un mensaje con `tipo` desconocido pero con `req` se responde `error` con `codigo: "parametro"`, para que el control no
espere hasta su timeout.

### Validaciones del agente antes de marcar

- `codigo` debe cumplir `^[*#][0-9*#]+#$` (un USSD, nunca un número de teléfono): si no, `error parametro`. Así una
  orden mal formada no termina en una llamada de voz.
- Si Android no da la cuenta telefónica (`PhoneAccountHandle`) del chip del `slot`, `error sim_invalida`. Nunca se
  marca sin ella: Android usaría la SIM por defecto, o sea, podría mover dinero desde el chip equivocado.
- `ussd_responder` solo escribe si la ventana de USSD visible es la de esa sesión (su texto coincide con el último
  que el agente devolvió). Si es otra (un USSD que mandó la red, una promo), responde `error red` con
  `detalle: "la ventana cambió"` y no escribe nada. Después de escribir, relee el campo; si no tiene el texto
  esperado, responde `error red` sin presionar Enviar.
- El agente nunca anota en su registro visible el texto que envía (puede ser el PIN): solo su largo.

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
- La configuración por ADB (`url`, `token`, `adb_serial`, `reconectar`) entra **solo** por la actividad
  `bo.qhana.agente/.ui.ActividadConfig`. Está protegida con el permiso `android.permission.DUMP`, que tiene
  `adb shell` y no las apps comunes: así ninguna otra app del teléfono puede apuntar el agente a otro servidor. La
  actividad del lanzador (`.ui.MainActivity`) ignora esos extras. `url` debe empezar con `ws://` o `wss://`; si no,
  se ignora y queda la anterior.
- `am start -n bo.qhana.agente/.ui.ActividadConfig --es adb_serial <serial> --ez reconectar true` hace que el agente
  corte su conexión y vuelva a conectar ya, sin esperar su pausa. Lo usa `POST /api/dispositivos/reconectar`.
- Al reconectar (también el forzado) el agente cierra antes cualquier sesión USSD abierta.
- Si el PC duerme, el túnel `adb reverse` puede perderse. El gestor ADB detecta la pausa (el reloj saltó más de 15 s
  entre dos vueltas) y revisa el túnel de todos los teléfonos en la vuelta siguiente; el de un teléfono sin agente
  conectado se revisa en cada vuelta (3 s), no cada 30 s.

## API del control (para la interfaz)

Base: `http://127.0.0.1:47800`. Los errores responden HTTP 4xx/5xx con `{"error": {"codigo", "detalle"}}`.

### Autenticación de la API

- Si el control escucha solo en `127.0.0.1` (lo normal, y la app de escritorio), la API no pide token.
- Si escucha en otra dirección (`--escuchar 0.0.0.0:47800`, modo Wi-Fi):
  - Exige `--api-token` / `QHANA_API_TOKEN` de **16 caracteres o más**; si falta, el control no arranca.
  - Tampoco arranca si el token de los agentes (`--token`) está vacío o es `dev`.
  - Las peticiones a `/api/*` que no vengan de la misma máquina (loopback) deben llevar
    `Authorization: Bearer <token>`. La única excepción es el WebSocket `/api/eventos`, donde el navegador no deja
    poner cabeceras: ahí vale `?token=<token>`, y **solo ahí**. El control nunca anota la URL con su `?token=` en
    los logs. Las descargas `.csv` también van con la cabecera: la interfaz las baja con `fetch` y las guarda como
    archivo, sin poner el token en un enlace.
  - Sin token válido: `401` con `codigo: "no_autorizado"`.
- Los tokens se comparan en tiempo constante.
- `GET /api/salud` y los archivos de la interfaz no piden token. `salud` responde
  `{version, agentes_conectados, requiere_token}`: `requiere_token` es `true` cuando esta petición necesitaría token
  (viene de otra máquina y el control tiene token). La interfaz lo pide entonces una vez y lo guarda en el navegador.

| Método y ruta | Cuerpo | Respuesta |
|---|---|---|
| `GET /api/salud` | — | `{version, agentes_conectados, requiere_token}` |
| `GET /api/dispositivos` | — | `[Dispositivo]` |
| `GET /api/adb` | — | `[{serial, estado, modelo, reverse_ok, dispositivo_id}]` |
| `POST /api/adb/{serial}/provisionar` | `{apk?}` (por defecto el `--apk` del control) | `{ok, pasos: [{paso, ok, salida}]}`. Deja el teléfono listo por USB: instala el APK, concede permisos, quita el límite de batería, `stayon usb`, activa la accesibilidad, `adb reverse` y abre la app. Equivale a `agente-android/scripts/provisionar.sh`. `404 sin_dispositivo` si ADB no lo ve en estado `device`; `503 sin_adb` si el control corre con `--sin-adb`; `400 sin_apk` si no hay APK (ni en el cuerpo ni `--apk`/`QHANA_APK`) y la app no está instalada en el teléfono (con la app ya instalada, sin APK solo reconfigura) |
| `POST /api/dispositivos/reconectar` | `{}` | `{adb: [{serial, reverse_ok, app_abierta, detalle}], refrescados: [id], sin_respuesta: [id], ocupados: [id]}`. Botón "Reconectar" (p. ej. tras dormir el PC). Por ADB: vuelve a aplicar `adb reverse` a cada teléfono en estado `device` y, si su agente no está conectado, abre la app con `reconectar` (`app_abierta`). Agentes conectados: les pide `refrescar`; el que no responde en 15 s se da por caído (se cierra su conexión y él reconecta) y va en `sin_respuesta`. Los `ocupados` (corriendo una receta) no se tocan. Sin ADB, `adb` va vacío |
| `POST /api/ussd/iniciar` | `{dispositivo, slot, codigo, modo?}` | `{sesion, texto, abierta, ms}` |
| `POST /api/ussd/responder` | `{dispositivo, sesion, texto}` | `{sesion, texto, abierta, ms}` |
| `POST /api/ussd/cancelar` | `{dispositivo, sesion}` | `{ok: true}` |
| `GET /api/dispositivos/{id}/sms?limite=50` | — | `[Sms]` |
| `GET /api/recetas` | — | `[{nombre, titulo, operador, descripcion, parametros}]` |
| `POST /api/recetas/{nombre}/ejecutar` | `{dispositivo, slot, parametros: {}, simular}` | `Resultado`. Un parámetro `pin` secreto que no venga se toma del PIN guardado del chip; si no hay, `400 parametro` |
| `PUT /api/chips/{dispositivo}/{slot}/pin` | `{pin}` (4 a 8 dígitos; `null` o `""` lo borra) | `ChipInfo`. Ver "PIN del chip" |
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
`dispositivo_actualizado`, `chip_cambiado`, `ussd`, `sms`, `adb`, `receta`, `receta_cambiada`, `lote`.
`receta_cambiada`: ver "Editar recetas desde la interfaz" (también se emite al recargar si cambió algún archivo).
`chip_cambiado`: ver "Cambio de chips".

Si una conexión de `/api/eventos` se atrasa y el control descarta eventos para ella, le manda
`{ "tipo": "perdidos", "ts_ms", "n": 12 }` (`n`: cuántos se perdieron). La interfaz debe recargar todo lo que muestra,
como al reconectar. `perdidos` no se guarda en el historial.

Sesión USSD manual (`/api/ussd/iniciar`): mientras está abierta, el teléfono figura `ocupado` y los lotes no lo usan.
Si pasan 60 s sin `responder`, el control la da por cerrada y libera el teléfono (el agente también la cierra a los 60 s).
`responder` y `cancelar` solo actúan sobre esa sesión manual: con el teléfono ocupado por una receta o un lote responden
`409 ocupado`, y con una sesión manual que ya cerró, `404 sesion_inexistente`.

Cada ejecución de receta queda en el historial **antes** de marcar, en estado `en_curso`, y se actualiza al terminar.
Si el control se cae en medio, al arrancar las ejecuciones que quedaron `en_curso` pasan a `incierto`.

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

Cuando el paso que mueve dinero no es elegir una opción sino escribir algo (p. ej. el PIN del dealer), se usa
`enviar` con `mueve_dinero: true`: nunca se reintenta, desde ahí todo corte es `incierto` y en `simular` **no se
envía** (la receta termina `simulado` en la pantalla anterior). Vale en `enviar` y en `marcar`: un código que hace la
operación de una vez, sin pantalla de confirmación (p. ej. `*601*PIN*número*monto#` de Viva dealer), va como
`marcar` con `mueve_dinero: true`; en `simular` no se marca nada. Sin confirmación no hay qué `verificar`: el código
se arma solo con parámetros validados.

Modificadores de un paso:

| Modificador | Qué hace |
|---|---|
| `esperar: regex` | La respuesta de ese paso debe coincidir o se aborta |
| `error_si: regex` | Si la respuesta coincide, aborta con ese texto como motivo |
| `exito: regex` | Si coincide, la receta terminó bien. Si no hay `exito` en ningún paso, basta con llegar al final |
| `timeout_ms` | Tiempo máximo de ese paso |
| `mueve_dinero: true` | Solo en `enviar` o `marcar`: ese paso es el que mueve dinero (ver arriba) |

Parámetros: `{ nombre, patron?, opciones?, descripcion?, secreto? }`. `secreto: true` (un PIN) hace que el valor se
muestre como `••••` en `Resultado.pasos[].enviado`, en el historial (`parametros` de la ejecución) y en todo evento
(`receta`, `ussd`) mientras corre la receta. Un parámetro secreto llamado `pin` se completa solo con el PIN
guardado del chip (ver "PIN del chip").

Campos de la receta (además de `pasos`, `parametros` y `salida`):

| Campo | Qué es |
|---|---|
| `nombre` | Obligatorio, `[a-z0-9-]+`. Es también el nombre del archivo (`<nombre>.yaml`) |
| `titulo`, `descripcion`, `operador`, `modo` | Lo de siempre (`modo`: `pantalla` por defecto, o `api`) |
| `accion`, `limites` | Para lotes y saldos: ver "Recetas: campos para lotes" |
| `chip_tipo` | `dealer` (opcional): receta propia de los chips dealer. Ver "Chips dealer" |
| `nombres_anteriores` | Opcional, `[string]`: nombres que tuvo la receta antes de renombrarla (ver "Renombrar una receta") |

Reglas:
- `{param}` se reemplaza por el valor del parámetro. Dentro de una regex el valor se escapa.
- `salida` define regex con un grupo; se aplican sobre todos los textos recibidos y van a `Resultado.salida`.
- Al terminar, o ante cualquier aborto, el motor **siempre** cancela la sesión si quedó abierta.
- Si después de `confirmar` no hay respuesta o no coincide con `exito`, el estado es `incierto`, nunca `ok`.
- `modo: api` solo admite un paso `marcar`.

### Montos: formato base y valores calculados

El monto siempre entra y sale en **Bs** (el formato base): en el CSV de un lote, en el panel de ejecución, en
`parametros` de `ejecutar`, en el historial y en los reportes. Cada receta traduce ese monto al formato que pide su
USSD con **valores calculados**, que el motor agrega solos cuando la receta declara el parámetro `monto`:

| Valor | Para Bs 0.50 | Para Bs 5 | Para Bs 12.30 |
|---|---|---|---|
| `{monto}` | `0.50` | `5` | `12.30` |
| `{monto_cent}` (centavos) | `50` | `500` | `1230` |
| `{monto_cent3}` (centavos, al menos 3 dígitos) | `050` | `500` | `1230` |
| `{monto_entero}` (Bs sin decimales) | — | `5` | — |
| `{monto_mixto}` (Bs enteros; menos de 1 Bs, centavos con 3 dígitos) | `050` | `5` | — |

- `{monto}` es el formato canónico: entero sin decimales y, con centavos, con dos (`1.5` → `1.50`). El control
  normaliza a ese formato lo que llegue (`0,5`, `Bs 0.50`, `.50`) **antes** de validar `patron` y `opciones`.
- `{monto_entero}` en un monto con centavos, o `{monto_mixto}` en uno de 1 Bs o más con centavos (`1.50`): la
  receta no corre (`400 parametro`, "esta receta solo acepta montos enteros…"), sin marcar nada.
- Se usan igual que cualquier `{param}`: en `marcar`/`enviar` van tal cual; dentro de una regex, escapados.
- `monto_cent`, `monto_cent3`, `monto_entero` y `monto_mixto` son nombres reservados: no se pueden declarar como
  parámetros, y el validador no avisa "no declarado" por usarlos.
- `opciones` de `monto` están en Bs y aceptan centavos: `["0.50", "1", "5"]`. `patron` se aplica al `{monto}` ya
  normalizado.
- Ejemplo (Entel dealer): el código lleva Bs enteros (`*133*N*1#` = Bs 1) y la única fracción es Bs 0.50 (`050`).
  La receta usa `marcar: "*133*{numero}*{monto_mixto}#"` con `patron: '^(0\.50|[1-9]\d*)$'` (montos libres: un
  lote manda cada fila en un solo envío), y el `verificar` comprueba el monto que muestra la confirmación con
  `{monto}`.
- `mueve_dinero` de `GET /api/recetas` es true si algún paso es `confirmar` o tiene `mueve_dinero: true`.

## Editar recetas desde la interfaz

Las recetas siguen siendo archivos YAML en `control/recetas/` (un archivo por receta, `<nombre>.yaml`). La interfaz
las lee y las guarda por la API; editar el archivo a mano sigue valiendo (y se ve con "recargar").

**Versión** de una receta = SHA-256 del texto del archivo, en hex (los primeros 12 caracteres bastan para mostrar).
Cada ejecución guarda la versión de la receta con que corrió (`ejecuciones.receta_version`). Las ejecuciones de
antes de esta columna no tienen versión: hasta que corran una vez (`ok` o `simulado`), todas las recetas salen
`sin_probar`.

### Lectura

`GET /api/recetas` agrega a cada resumen:
- `version`, `archivo` (nombre del archivo, p. ej. `tigo-saldo.yaml`), `modificada_ms` (fecha del archivo).
- `sin_probar` (bool): la versión actual nunca terminó `ok` ni `simulado` en una ejecución. Solo **avisa**: la
  receta se puede usar igual (en real y en lotes). La interfaz lo muestra en la lista, en la confirmación de una
  ejecución real y en la validación de un lote (aviso de lote: "la receta X cambió y no se ha probado").
- `uso`: `{ "ejecuciones_30d", "ok_30d", "ultima_ms", "ultimo_estado", "ultima_ok_ms" }` (`null` si nunca corrió).
  `ok_30d` y `ultima_ok_ms` cuentan `ok` y `simulado`.

`nombres_anteriores` (`[]` si nunca se renombró) también va en cada resumen.

`GET /api/recetas/{nombre}` → la receta completa (también por un nombre anterior: responde la receta renombrada):
```json
{ "resumen": ResumenReceta, "yaml": "nombre: tigo-saldo\n…", "receta": { …la receta como JSON, mismos campos que el YAML… },
  "avisos": ["…"] }
```
`avisos` son los mismos de `validar`, como texto (`"paso 6: el paso que mueve dinero no tiene 'exito'…"`).

`GET /api/recetas/{nombre}/estadisticas?dias=30` (cuenta también las ejecuciones guardadas con sus
`nombres_anteriores`; por un nombre anterior responde las de la receta renombrada) →
```json
{ "dias": 30, "ejecuciones": 41, "por_estado": { "ok": 30, "simulado": 6, "abortado": 3, "incierto": 0, "error": 2 },
  "ms_promedio": 3400, "ms_p90": 5200, "ultima_ok_ms": 0,
  "por_paso_fallido": [ { "paso": 3, "veces": 2, "ultimo_detalle": "no coincide 'monto'" } ],
  "ultimas": [ { "id": 812, "inicio_ms": 0, "estado": "ok", "simular": false, "dispositivo": "…", "slot": 0,
                 "duracion_ms": 3300, "detalle": null, "version": "a1b2c3d4e5f6",
                 "pasos": [ { "accion": "marcar", "enviado": "*105#", "texto": "…", "ms": 2400 } ] } ] }
```
`ultimas` trae las 20 más recientes con sus pasos (los secretos ya tapados). Sirve también para "probar contra la
última pantalla" en el editor. `por_paso_fallido`: en qué paso abortaron las ejecuciones no exitosas (si se sabe).

### Validar sin guardar

`POST /api/recetas/validar` con `{ "yaml": "…" }` →
```json
{ "ok": false, "errores": [ { "paso": 3, "campo": "esperar", "mensaje": "regex inválida '…': …" } ],
  "avisos": [ { "paso": 6, "mensaje": "el paso que mueve dinero no tiene 'exito': todo resultado quedará incierto" } ],
  "receta": { …como JSON, si se pudo leer… }, "resumen": ResumenReceta | null }
```
`errores` impiden guardar (los mismos de la carga: YAML mal formado, regex inválida, primer paso no es `marcar`,
un nombre de `nombres_anteriores` inválido, igual al `nombre` o que choca con otra receta…).
`paso` es 1-based o `null` si es de la receta entera; `campo` puede ser `null`. En `resumen`, `modificada_ms` es
`null` si ese texto no es el archivo guardado. Los `avisos` no impiden guardar:
- receta que mueve dinero sin un paso `verificar` antes del paso que mueve dinero;
- paso que mueve dinero sin `exito`;
- `{param}` usado en un paso pero no declarado en `parametros`, o parámetro declarado y nunca usado;
- `accion: transferir` o `recargar` sin `limites`;
- `accion: saldo` sin `salida.saldo_principal`;
- dos recetas con el mismo (`operador`, `accion`, `chip_tipo`) (el lote no sabría cuál usar);
- campo desconocido en el YAML (un error de tipeo como `esprar:` se ignoraría en silencio);
- paso `verificar` con `esperar`, `error_si` o `exito` (el motor no los mira en ese paso).

### Probar un paso contra una pantalla

`POST /api/recetas/probar-paso` con `{ "yaml": "…", "paso": 2, "texto": "Tigo Money\n1.-Recarga Tigo\n…",
"parametros": { "monto": "5" } }` → evalúa ese paso **con la misma lógica del motor**, sin marcar nada:
```json
{ "accion": "elegir", "enviaria": "1", "opciones": [ { "numero": "1", "texto": "Recarga Tigo", "coincide": true } ],
  "esperar": true, "error_si": false, "exito": null, "verificar": null,
  "salida": { "saldo_principal": "10.5" }, "resultado": "seguiria", "motivo": null,
  "parametros": { "monto": "5", "numero": "70000000" } }
```
- `enviaria`: lo que el motor contestaría (`elegir`/`confirmar`: el número de la opción; `enviar`: el texto con los
  parámetros sustituidos; secretos tapados). `null` si no hay opción que coincida.
- `esperar` / `error_si` / `exito` / `verificar`: `true`/`false` si el paso lo tiene, `null` si no.
- `texto` es la pantalla que llegó **después** de este paso (contra ella se evalúan `esperar`, `error_si` y `exito`);
  para `elegir`, `confirmar` y `verificar`, la pantalla **antes** del paso va en `texto_antes` (opcional; si falta,
  se usa `texto`). `opciones` es `[]` en los pasos que no eligen.
- `resultado`: `seguiria` · `exito` · `abortaria` · `incierto` (un corte en o después del paso que mueve dinero), con
  `motivo` (`null` si sigue o terminó bien). En el último paso, sin `exito` en la receta, es `exito` (como el motor).
- Parámetros que falten se completan con un valor de ejemplo que cumpla su `patron` u `opciones` cuando se pueda.
  `parametros` devuelve los usados (secretos tapados).

### Guardar, crear, retirar

- `POST /api/recetas` con `{ "yaml" }` crea una receta nueva. `409 existe` si ya hay una con ese `nombre`.
- `PUT /api/recetas/{nombre}` con `{ "yaml", "version_base" }` reemplaza la receta. `version_base` es la `version`
  que se leyó: si el archivo cambió desde entonces (otro usuario o a mano), `409 conflicto` y no se guarda nada.
  El `nombre` del YAML debe ser el mismo de la ruta (`400 parametro`): para renombrar, ver "Renombrar una receta".
- `DELETE /api/recetas/{nombre}` **retira** la receta: mueve el archivo a `recetas/viejo/` (nunca se borra).
- Cualquiera de las tres: valida primero (`400 receta_invalida` con `detalle` y la lista `errores` como en
  `validar`); escribe en un archivo temporal y lo renombra (nunca queda un archivo a medias); y vuelve a cargar las
  recetas. Responden el `ResumenReceta` (o `{ok: true}` al retirar) y emiten el evento
  `{ "tipo": "receta_cambiada", "nombre", "cambio": "creada" | "editada" | "retirada", "version" }` (`version`
  `null` al retirar).
- `nombre` debe ser `[a-z0-9-]+` (es el nombre del archivo): si no, `400 parametro`. Un `PUT` con el mismo texto
  que ya está no guarda copia. `404 sin_receta` si no existe.
- Un lote que ya está corriendo no cambia: usa la copia de la receta tomada al iniciar.

### Versiones

Antes de reemplazar o retirar, el control copia el archivo actual a
`recetas/viejo/AAAA-MM-DD_HHMMSS_<nombre>.yaml`. Esas copias son el historial.

- `GET /api/recetas/{nombre}/versiones` → `[ { "id": "2026-11-05_143210_tigo-saldo.yaml", "guardada_ms": 0,
  "version": "…", "actual": false } ]`, la más nueva primero. La primera fila es la versión actual (`actual: true`,
  `id` = el archivo vigente).
- `GET /api/recetas/{nombre}/versiones/{id}` → `{ "id", "yaml", "version" }`.
- Restaurar = `PUT` con el `yaml` de esa versión (así también queda la actual en el historial).
- Las versiones de una receta retirada siguen en `viejo/`.
- Una receta renombrada lista también las copias guardadas con sus `nombres_anteriores`
  (`2026-11-05_084133_tigo-pda-recargar.yaml` aparece en las versiones de `tigo-dealer-recargar`), y
  `GET /api/recetas/{nombre anterior}/versiones` da las mismas.

### Renombrar una receta

Se crea la receta con el nombre nuevo y `nombres_anteriores: [<nombre viejo>]`, y se retira la vieja (su archivo
pasa a `viejo/`, así su historial queda). Con eso, **buscar por un nombre anterior da la receta renombrada**:
- `POST /api/recetas/{nombre anterior}/ejecutar` corre la renombrada (y `Resultado.receta` trae el nombre nuevo);
- un lote guardado con el nombre viejo (en `filas[].receta`, `origen.receta` u `origen.chips[].receta`) corre la
  renombrada al iniciar o reanudar; un `origen` pedido con el nombre viejo vale igual y se devuelve con el nuevo;
- `GET /api/recetas/{nombre anterior}`, `/versiones` y `/estadisticas` responden la renombrada;
- `uso`, `estadisticas`, `sin_probar` y el cupo de hoy (`por_dia`) cuentan también las ejecuciones guardadas con
  los nombres anteriores (el historial guarda el nombre que tenía la receta al correr; no se reescribe).

Reglas: cada nombre anterior es `[a-z0-9-]+`, distinto del `nombre` y sin repetir; no puede ser el nombre (ni un
nombre anterior) de otra receta, ni una receta nueva puede llamarse como el nombre anterior de otra (`400
parametro` al guardar; error en `validar`). Si al cargar la carpeta igual hay un choque, se avisa en el log y el
nombre actual manda.

`sin_probar` mira la **versión** (el texto del archivo): renombrar cambia el texto, así que la receta renombrada
sale `sin_probar` hasta que corra una vez (`ok` o `simulado`), aunque sus pasos sean los mismos.

### Crear una receta desde una sesión de la consola

`POST /api/recetas/desde-sesion` con
`{ "operador": "TIGO", "pasos": [ { "enviado": "*555#", "texto": "…pantalla que llegó…" }, { "enviado": "3", "texto": "…" } ] }`
(lo que se marcó y se respondió en la consola USSD, en orden; el primero es el código) → `{ "yaml": "…" }`, un
**borrador** que no se guarda:
- el primer paso es `marcar` con el código;
- una respuesta que es el número de una opción del menú anterior se convierte en `elegir: '<texto de la opción>'`
  (escapado como regex); cualquier otra, en `enviar: '<texto>'`;
- un texto que parece número de teléfono (8 dígitos que empiezan con 6 o 7) se reemplaza por `{numero}`, y uno que
  parece monto por `{monto}`, y se declaran esos parámetros; un PIN no se graba nunca (la consola manda `"enviado":
  "{pin}"` si el operador lo marcó como PIN);
- cada paso lleva un `esperar` sugerido con una frase de la pantalla que llegó;
- `nombre` sugerido `<operador>-nueva`, `titulo` vacío, sin `accion`; ningún paso se marca como que mueve dinero
  (lo decide quien edita).

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
| `monto` | sí | Bs con hasta 2 decimales: `12`, `12.00`, `12,00`, `Bs 12`, `1.50`, `1,5`. Con centavos solo vale si la receta no tiene montos fijos (`parametros[monto].opciones`): con `tigo-transferir` (1, 2, 5, 10, 20) `12,50` es **error**; con `tigo-dealer-recargar` vale `1.50`. Más de 2 decimales o `1.000` (ambiguo) es error |
| `operador` | sí, salvo en un lote de un chip | `TIGO`, `ENTEL`, `VIVA` (alias `NUEVATEL`), sin importar mayúsculas ni acentos. No se deduce del número, porque hay portabilidad |
| `accion` | no | Por defecto `transferir`. En un lote de un chip se ignora (manda la receta elegida) |
| `referencia` | no | Texto libre que vuelve igual en el CSV de resultados |

- Separador `,` o `;` (Excel en español usa `;`), detectado solo. UTF-8, con o sin BOM.
- Encabezados flexibles: `numero|número|telefono|teléfono|celular|linea|línea`, `monto|cantidad|importe|bs|carga`,
  `operador|operadora|empresa|compania|compañía`, `accion|acción|operacion|operación`, `referencia|ref|nota|id`.
- Si la primera fila no parece un encabezado, se asume el orden `numero,monto,operador,accion,referencia`
  (en un lote de un chip, `numero,monto,referencia`).
- Máximo 5 000 filas.
- El monto va a la receta como texto: entero sin decimales (`5`) y, con centavos, con dos (`1.50`).

## Lote de un chip (`origen`)

Por defecto cada fila dice su operador y los envíos se reparten entre todos los chips de ese operador. Con
`origen: { "dispositivo", "slot", "receta" }` en `validar` y en `POST /api/lotes`, **todos los envíos salen de
ese chip y corren esa receta** (por ejemplo `tigo-dealer-recargar` desde el chip dealer). Entonces el CSV basta con
`numero,monto`: el operador es el del chip y la acción la de la receta.

- El control revisa el chip y la receta antes de mirar el CSV (`400 parametro` si algo no sirve): el chip debe estar
  en línea y listo; la receta, existir y ser del operador del chip; una receta `chip_tipo: dealer` exige un chip `dealer`
  (y una receta de la compañía en un chip dealer corre su variante dealer, como siempre); la receta no puede pedir más
  parámetros que `numero`, `monto` y el `pin` secreto; si pide `pin`, el chip debe tener PIN guardado.
- Una fila con `operador` distinto del del chip es `error`. La columna `accion` se ignora.
- Cupo y saldo: con `accion: transferir` cuentan como siempre (los del chip). Con otra acción (recargar), el cupo es
  `limites.por_dia` de esa receta (sin límite si no tiene) y el saldo principal **no** se usa para repartir ni para
  verificar: una recarga de dealer gasta otro saldo. Si el chip es `dealer`, hay receta `accion: saldo_dealer`
  y tiene PIN guardado si esa receta lo pide, el lote cuadra con el **saldo dealer**: al `iniciar` (real) se consulta y se guarda como saldo
  inicial, y `verificar` lo compara con el saldo dealer de ese momento (ver `verificar`). Si no (chip normal, sin PIN
  cuando la receta lo pide, o sin receta), no se guarda saldo inicial y `verificar` responde `409 no_aplica`.
- `Validacion.origen` y `Lote.origen` devuelven el origen con `operador` y `numero` del chip completados.

## Lote desde chips elegidos (`origen.chips`)

Generaliza el lote de un chip: el operador elige **varios chips** (de uno o varios operadores, p. ej. todos los
chips dealer) y el lote se reparte solo entre ellos, en paralelo.

`origen: { "chips": [ { "dispositivo", "slot" }, … ], "accion": "recargar" }` (en `validar` y `POST /api/lotes`).
- `accion` vale para todas las filas (la columna `accion` se ignora). Cada envío corre la receta que le toca a su chip
  según "Selección de receta" (`operador` del chip, `accion`, `chip_tipo`).
- Columna `operador`: obligatoria si los chips elegidos son de más de un operador; si son de uno solo, se puede
  omitir (CSV `numero,monto,referencia`). Una fila de un operador sin ningún chip elegido es `error` ("ninguno de los
  chips elegidos es ENTEL").
- El control revisa cada chip como en el lote de un chip (en línea, listo, receta para esa acción, PIN si la receta
  lo pide). Un chip que no sirve es `400 parametro` con el motivo, salvo que el resto alcance: entonces va en
  `Validacion.avisos_origen` y se excluye.
- Cupo, saldo y verificación por chip, con las mismas reglas del lote de un chip (saldo principal para transferir;
  saldo dealer para recargar desde un chip dealer con receta `saldo_dealer` (y PIN si la pide); si no, sin saldo para cuadrar ese chip).
- El origen viejo `{ dispositivo, slot, receta }` sigue valiendo y equivale a un solo chip con esa receta.
- `Validacion.origen` y `Lote.origen` devuelven `{ "chips": [ { dispositivo, slot, operador, numero, tipo, receta } ],
  "accion" }` (con `receta` = la que correrá ese chip). Para el origen viejo, además `dispositivo`, `slot` y `receta`
  como antes.
- En la respuesta de `validar`, `chips[]` trae cada chip elegido con el cupo de la acción del lote (`accion`,
  `sin_limite`) y `resumen.operadores[]` una entrada por (operador, acción), como en "Lote por operador con recetas
  solo dealer".

## Lote por operador con recetas solo dealer

En el modo por operador (sin `origen`), si para (`operador`, `accion`) no hay receta de la compañía pero sí una
`chip_tipo: dealer`, la fila se valida con esa receta y **solo los chips dealer** de ese operador pueden hacerla (los
normales no corren recetas dealer). Así un CSV con `accion: recargar` de varios operadores reparte cada fila entre los
chips dealer de su operador. Si la receta dealer pide `pin`, un chip sin PIN guardado no recibe envíos (queda fuera del
plan con el motivo "sin PIN guardado"). Cupo y saldo de esas filas siguen las reglas de "recargar" (cupo
`limites.por_dia`, sin usar el saldo principal).

En la respuesta de `validar`, el resumen y los chips de esas filas muestran **ese** cupo, no el de transferir: la
entrada (`TIGO`, `recargar`) de `resumen.operadores[]` suma el cupo de la receta que correría cada chip dealer, y si
alguno no tiene `por_dia`, `cupo_sin_limite: true` (cada chip en `chips[]` con `accion: "recargar"` y
`sin_limite`). Ver `POST /api/lotes/validar`.

## Ejecución en paralelo

Cada **teléfono** hace un envío a la vez (Android permite una sola sesión USSD por teléfono) y los teléfonos trabajan
en paralelo, sin importar el operador ni el modo del lote. Dos chips del mismo teléfono se turnan. Con un solo chip
de origen, el lote va en serie.

### `GET /api/lotes/{id}/carriles`

Un carril por chip que participa (o puede participar) en el lote, para ver en vivo quién hace qué:
```json
{ "carriles": [ {
    "dispositivo": "6f44…", "slot": 0, "telefono": "Infinix X678B", "operador": "TIGO", "numero": "77716151",
    "tipo": "dealer", "receta": "tigo-dealer-recargar",
    "estado": "enviando",
    "actual": { "fila": 12, "numero": "71234567", "monto": 5, "desde_ms": 0 },
    "ok": 14, "fallidos": 1, "inciertos": 0, "enviado_bs": 70,
    "cupo_restante": 36, "cupo_sin_limite": false, "saldo": { "tipo": "dealer", "bs": 412.5, "ms": 0 },
    "ms_promedio": 9800, "motivo": null } ],
  "pendientes": 40, "eta_ms": 196000 }
```
- `estado`: `enviando` · `esperando` (el teléfono está ocupado con el otro chip u otra cosa) · `libre` ·
  `sin_capacidad` (sin cupo o saldo; `motivo` dice cuál) · `desconectado` · `excluido` (no sirve para el lote;
  `motivo`).
- `cupo_restante` y `saldo` pueden ser `null` (sin límite / sin dato). `cupo_sin_limite: true` distingue "sin
  límite diario" (la receta del chip no tiene `por_dia`) de "sin dato" (el chip no hace nada en el lote).
  `saldo.tipo`: `principal` o `dealer`.
- `ok` cuenta también los `simulado` (en un lote `simular: true`).
- `ms_promedio`: de los envíos de este lote en ese chip (`null` sin datos).
- `eta_ms`: estimación de lo que falta con el ritmo actual (envíos pendientes ÷ envíos por ms de todos los teléfonos
  en paralelo); `null` si el lote no está `en_curso` o aún no hay ritmo.
- Los eventos `lote` `fase: envio` ya traen `dispositivo_id`, `slot` y `estado`: la interfaz actualiza los carriles con
  ellos y vuelve a pedir `carriles` cada pocos segundos mientras el lote corre.
- Si al ejecutar el chip deja de servir (se desconectó, perdió el PIN…), lo pendiente queda `sin_capacidad` con ese
  motivo. Un envío que falla porque falta un parámetro (p. ej. el PIN) queda `fallido`, sin reintento.

## Recetas: campos para lotes

```yaml
accion: transferir          # qué hace la receta; el lote busca la receta con (operador, accion)
limites:
  por_dia: 5                # envíos por chip y por día
  comision: 0.20            # costo por envío, en Bs
```

- `chip_tipo: dealer` (opcional) marca una receta propia de los chips dealer; sin él vale para todos los chips. Ver
  "Chips dealer".
- Los montos permitidos salen de `parametros[monto].opciones` (en Bs, con centavos si hace falta: `0.50`), y el
  formato del número de `parametros[numero].patron`. La descomposición de un monto en envíos (12 → 10 + 2) se hace en
  centavos.
- La receta con `accion: saldo` de cada operador sirve para consultar el saldo de los chips; su `salida` debe
  incluir `saldo_principal`.

## Validación (sin ejecutar nada)

Por cada fila:

| Chequeo | Si falla |
|---|---|
| Número con el formato de la receta | `error` |
| Monto > 0 (hasta 2 decimales) y que se pueda formar con los montos permitidos (12 → 10 + 2, 1.50 → 0.50 × 3; la menor cantidad de envíos) | `error` |
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

Cuerpo `{ "csv": "<texto>", "consultar_saldos": true, "origen"?: { "chips", "accion" } | { "dispositivo", "slot", "receta" } }`
(ver "Lote desde chips elegidos" y "Lote de un chip"). Respuesta `Validacion` (con `origen` si se pidió):

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
                      "saldo_desconocido_chips": 0, "necesario_bs": 87.2, "envios": 11,
                      "accion": "transferir", "cupo_sin_limite": false } ]
  },
  "chips": [ { "dispositivo": "6f44…", "slot": 0, "operador": "TIGO", "numero": "+59178815232",
               "saldo": 10.0, "saldo_ms": 1793330000000, "enviados_hoy": 0, "cupo_hoy": 5,
               "accion": "transferir", "sin_limite": false } ]
}
```

**Cupo por acción.** El cupo de un chip depende de la acción que haría en el lote:
- `transferir`: el de siempre (`limites.por_dia` de la receta de transferir menos los envíos `ok` de hoy; 0 si el
  operador dijo "máximo de transferencias").
- otra acción (`recargar`…): el de la receta que correría ese chip para esa acción (su variante dealer si es un chip
  dealer): su `limites.por_dia` menos los envíos `ok` de hoy con las recetas de esa acción. Sin `por_dia` → sin límite.

`resumen.operadores[]`: **una entrada por (operador, acción)** de las filas válidas (un CSV con transferencias y
recargas de TIGO trae dos entradas de TIGO), ordenadas por operador y acción.
- `accion`: la acción de esas filas. `envios` y `necesario_bs` (montos + comisiones) son solo de esas filas.
- `chips`: con `transferir`, los chips en línea del operador (como siempre); con otra acción, los que pueden
  hacerla (los que quedan fuera, p. ej. "sin PIN guardado", no cuentan).
- `cupo_hoy`: suma del cupo de esos chips **con límite**. `cupo_sin_limite: true` si alguno no tiene límite diario
  (entonces el cupo no limita el plan y `cupo_hoy` cuenta solo los chips con límite; puede ser 0).
- `saldo_conocido` y `saldo_desconocido_chips`: saldo **principal**, solo con `transferir`. Con otra acción valen
  0: esas filas no gastan el saldo principal y `necesario_bs` no se compara con él.

`chips[]`: los chips en línea (o los elegidos, con `origen`). Un chip que hace una acción del lote aparece **una vez
por acción**, con `accion` y los números de esa acción (`enviados_hoy`, `cupo_hoy`, `sin_limite`; con una acción
que no es `transferir`, `saldo` es `null` porque no se usa). Un chip que no hace ninguna (otro operador, o quedó
fuera) aparece una vez sin `accion`, con los números de transferir. `sin_limite: true` = sin límite diario; entonces
`cupo_hoy` vale 0 y no significa nada. La API nunca devuelve un cupo "infinito" como número.

`estado` de una fila: `valida` · `aviso` (válida con advertencias) · `invalida`.

`resumen.avisos: [string]`: avisos del lote entero, que no cambian el estado de ninguna fila. Hoy: una receta que
usaría el lote (también la variante dealer si el plan usa un chip dealer) está `sin_probar` → `"la receta X cambió y no
se ha probado"`.

## Ejecución

### `POST /api/lotes`

Cuerpo `{ "nombre": "Pedidos 29-oct", "csv": "…", "simular": true, "origen"?: {…}, "forzar"?: false }`. Vuelve a
validar en el servidor (**sin** consultar saldos), guarda el lote con **solo las filas válidas** (las inválidas quedan
como `omitida`) y lo deja en estado `listo`. Responde el `Lote`.

**Duplicados.** El control guarda un hash (SHA-256) del CSV normalizado (sin espacios al final de las líneas ni líneas
vacías) junto con `origen`. Si se pide un lote **real** (`simular: false`) igual a otro lote real creado en las últimas
24 h que no esté `cancelado`, responde `409 duplicado` con `detalle` "Igual al lote #12 «Pedidos 29-oct», creado
14:32" y no crea nada. Con `forzar: true` lo crea igual. Las simulaciones no se revisan (simular y luego ejecutar el
mismo CSV es lo normal).

Crear y arrancar son dos llamadas. Si `iniciar` falla después de crear, el lote ya existe en `listo`: la interfaz
debe abrirlo y no volver a crearlo.

Filas con **más campos que el encabezado** (p. ej. `69869869,12,50` con la coma decimal sin comillas) son inválidas:
"la fila tiene 3 campos y el encabezado 2 (¿un monto con coma sin comillas?)". Nunca se descartan campos en silencio.

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
- `incierto` **no detiene el lote**: el envío queda `incierto`, se emite una alerta (`fase: "alerta"`) y el lote sigue
  con los demás envíos. El ejecutor nunca reintenta solo un envío incierto (pudo haberse hecho); solo el operador puede
  volver a encolarlo con `reintentar` + `inciertas: true`, después de revisar el saldo.
- Si no queda ningún chip con capacidad, el lote termina y lo pendiente queda `sin_capacidad` ("para mañana"). Un
  lote se puede volver a `iniciar` otro día y retoma solo lo pendiente.
- Lote `simular: true`: cada envío corre la receta en modo simulación (llega a la confirmación y responde No).
  No descuenta saldo ni cupo.
- Lote real: al iniciar o reanudar, el ejecutor consulta el saldo de los chips de los operadores del lote (los que no
  tengan un dato de menos de 5 minutos) y guarda ese **saldo inicial** de cada chip. Sirve para `verificar`.
- Al reiniciar el control, un lote `en_curso` pasa a `pausado`.
- **Un envío solo se despacha si su paso a `en_curso` quedó guardado** en la base. Si esa escritura falla, el envío no
  sale y el lote se pausa con `motivo_pausa` "no se pudo guardar en la base: …".
- La base usa `synchronous=FULL`: un corte de luz no deshace un envío ya marcado `en_curso`.
- Al arrancar el control, todo envío que quedó `en_curso` (en cualquier lote, sin importar el estado del lote) pasa a
  `incierto` (en un lote `simular: true` vuelve a `pendiente`: no mueve dinero). También si la tarea de un envío se cae: el envío queda `incierto` y su teléfono se libera.
- `ocupado` o error de canal antes de confirmar (no se movió dinero) se reintenta, pero como máximo 10 veces por
  envío; después el envío queda `fallido` ("el teléfono siguió ocupado").
- La receta se toma **al iniciar** (o reanudar) el lote y se usa esa copia hasta que pare: recargar las recetas a
  mitad de un lote no cambia su flujo.

Estados de un lote: `listo` · `en_curso` · `pausado` · `terminado` · `cancelado`.

Estados de una fila: `omitida` (inválida) · `pendiente` · `en_curso` · `ok` · `simulado` · `parcial` (algunos envíos
ok) · `fallida` · `incierta` · `sin_capacidad`.

### `POST /api/lotes/{id}/reintentar-fallidas`

Cuerpo opcional: `{ "inciertas": false }`.

Vuelve a `pendiente` todos los envíos `fallido` del lote para que `iniciar` los intente de nuevo. Es seguro: un envío
`fallido` nunca pasó por `confirmar` (cualquier problema después de confirmar es `incierto`), así que no movió dinero.
De una fila `parcial` solo se reintentan las piezas fallidas; las `ok` no se repiten.

Con `inciertas: true` también vuelve a `pendiente` los envíos `incierto`. **No es seguro por sí solo**: un incierto
pudo haberse hecho, y reintentarlo puede enviar dos veces. Solo después de revisar el saldo o el historial del chip y
ver que no llegó. La interfaz pide confirmación aparte. Sin `inciertas` (o `false`), los inciertos se quedan como están.

- Responde el `Lote`. Si estaba `terminado` pasa a `listo`; si estaba `pausado` se queda `pausado`.
- Después hay que llamar a `iniciar` (así el operador decide cuándo).
- Errores `409`: `en_curso` (el lote está corriendo), `cancelado`, `nada_que_reintentar` (no hay envíos `fallido`, ni
  `incierto` si se pidió `inciertas`).
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
`Lote` lleva además `archivado` (bool).

`GET /api/lotes/{id}?filas=false` devuelve el lote sin `filas` (más liviano; para pedirlas por páginas, ver abajo).

#### `GET /api/lotes` (lista paginada)

Parámetros, todos opcionales:

| Parámetro | Qué hace |
|---|---|
| `limite` | Cuántos lotes (por defecto 50, máximo 200) |
| `antes` | Cursor: solo lotes con `id` menor a este (usar el `siguiente` de la página anterior) |
| `estado` | Uno o varios separados por coma: `listo,en_curso,pausado,terminado,cancelado` |
| `problemas` | `true`: solo lotes con alguna fila `fallida`, `incierta` o `parcial` |
| `simular` | `true` (solo simulaciones) o `false` (solo reales) |
| `q` | Busca en el nombre (sin distinguir mayúsculas). `#12` o `12` también encuentra el lote 12 |
| `archivados` | `false` (por defecto: los oculta), `true` (solo archivados), `todos` |

Respuesta, más recientes primero (por `id` descendente):
```json
{ "lotes": [Lote sin filas], "siguiente": 117, "total": 340,
  "conteos": { "activos": 2, "problemas": 5, "terminados": 320, "simulaciones": 40, "archivados": 12 } }
```
- `siguiente`: el valor para `antes` de la próxima página, o `null` si no hay más. Es estable aunque lleguen lotes
  nuevos mientras se pagina.
- `total`: cuántos lotes cumplen el filtro (todas las páginas).
- `conteos` no depende del filtro (sirve para los números de las pestañas): `activos` = `listo` + `en_curso` +
  `pausado`; `problemas` = con filas fallidas, inciertas o parciales; `terminados` = `terminado` + `cancelado`;
  `simulaciones`. Los tres primeros y `simulaciones` no cuentan los archivados; `archivados` los cuenta aparte.
- El progreso de cada lote se guarda en la tabla de lotes al cambiar: listar **no** lee las filas de los lotes.

#### `GET /api/lotes/{id}/filas` (filas por páginas)

| Parámetro | Qué hace |
|---|---|
| `estado` | Uno o varios estados de fila separados por coma |
| `q` | Busca en el número y en la referencia |
| `despues` | Cursor: solo filas con número de fila mayor a este |
| `limite` | Por defecto 200, máximo 1000 |

Responde `{ "filas": [FilaLote], "siguiente": 412 | null, "total": 1530 }`, ordenadas por número de fila.

#### `POST /api/lotes/{id}/archivar`

Cuerpo `{ "archivado": true }` (o `false` para sacarlo del archivo). Solo lotes `terminado` o `cancelado`; si no,
`409 no_aplica`. Un lote archivado no cambia en nada más: solo deja de aparecer en la lista por defecto. Responde el
`Lote`.

`revision` responde "¿terminó todo bien?" sin interpretar números: `completo` es `true` solo si el lote está
`terminado` y **todas** las filas a ejecutar quedaron `ok` (o `simulado` en una simulación). Si no, `motivos` dice por
qué ("lote pausado", "2 filas fallidas", "1 fila parcial", "3 filas para mañana", "1 fila incierta"…). `terminado`
por sí solo **no** significa éxito: solo que el ejecutor ya no tiene nada más que intentar.

### `POST /api/lotes/{id}/verificar`

Con varios chips o acciones mezcladas, la verificación se hace **por chip y por tipo de saldo**: cada fila de `chips`
lleva `saldo` (`principal` para lo transferido, `dealer` para lo recargado desde un chip dealer con receta
`saldo_dealer`, y PIN si la pide). Un chip que recargó sin forma de consultar su saldo dealer sale `sin_dato` con un
aviso ("no hay receta de saldo dealer para VIVA"). El `saldo` de arriba es `principal`, `dealer` o `mixto`. `409 no_aplica` solo si ningún chip
se puede cuadrar.

Lotes de transferencias: cuadra con el saldo principal. Lote de un chip con otra acción (recargas) desde un chip dealer:
cuadra con el **saldo dealer** de ese chip (`saldo: "dealer"` en la respuesta; consultarlo envía el PIN guardado). Un lote
de un chip con otra acción que no es dealer, o sin PIN guardado, responde `409 no_aplica`.

Cuadra el dinero contra los saldos reales. Consulta (solo lectura, ~3.5 s por chip) el saldo actual de cada chip que
envió algo y lo compara con el **saldo inicial** guardado al último `iniciar`:

`descontado_real = saldo_inicial − saldo_actual` contra `descontado_esperado = Σ (monto + comisión)` de los envíos `ok`
hechos desde ese inicio. Cuadra si la diferencia es de ±0.10 Bs o menos.

```json
{ "lote_id": 3, "verificado_ms": 0, "completo": true, "motivos": [],
  "cuadra": true, "confirmado": true, "saldo": "principal",
  "chips": [ { "dispositivo": "6f44…", "slot": 1, "numero": "78815232", "estado": "cuadra",
               "saldo_inicial": 120.0, "saldo_actual": 87.4,
               "descontado_esperado": 32.6, "descontado_real": 32.6, "diferencia_bs": 0.0, "envios_ok": 3 } ],
  "avisos": [] }
```

- `chips[].estado`: `cuadra` · `diferencia` (se descontó más o menos de lo esperado) · `sin_dato` (no hubo saldo
  inicial o no se pudo consultar el actual).
- `saldo`: qué saldo se comparó, `principal` o `dealer`. Con `dealer`, `saldo_inicial`/`saldo_actual` son saldos de dealer.
  Si el operador le descuenta al dealer menos que el monto recargado (comisión a favor), sale como `diferencia`
  negativa.
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
Un texto que empieza con `=`, `+`, `-` o `@` (p. ej. una `referencia`) va precedido de `'` para que Excel no lo
ejecute como fórmula. Los montos y números no se tocan.

### `GET /api/lotes/plantilla.csv`

Devuelve un CSV de ejemplo con el encabezado y 3 filas. `?tipo=` elige el ejemplo:
- `transferencias` (por defecto): `numero,monto,operador,accion,referencia` con `transferir`;
- `recargas`: varios operadores con `accion` `recargar` y montos con centavos;
- `chips`: para "desde chips elegidos": `numero,monto,operador,referencia`.
Acepta también `?separador=;`.

### Eventos

`{ "tipo": "lote", "lote_id", "fase": "inicio" | "envio" | "fila" | "alerta" | "pausa" | "fin", … }`:
- `envio` lleva `fila`, `monto`, `dispositivo_id`, `slot`, `estado`;
- `fila` lleva `fila` y `estado`;
- `alerta` (un envío quedó incierto; el lote sigue) lleva `motivo` y `fila`;
- `pausa` lleva `motivo`;
- todos llevan `progreso`.

## Saldos de los chips

`GET /api/chips` → la lista de chips en línea con su `saldo`, `saldo_ms`, `enviados_hoy`, `cupo_hoy` (de
transferir; `sin_limite: true` y `cupo_hoy: 0` si su receta de transferir no tiene `por_dia`), `sin_verificar`, `tipo` (`normal` | `dealer`) y `tiene_pin` (el chip tiene PIN guardado). `POST /api/chips/saldos` → consulta el saldo de todos los chips en línea (o `{ "operadores": ["TIGO"] }`)
y devuelve la lista actualizada. Cada ejecución de una receta `accion: saldo` actualiza el saldo conocido de ese chip.

**Saldo dealer.** Un chip dealer tiene además el saldo que gastan sus recargas, que no es el principal:
`ChipInfo.saldo_dealer` y `saldo_dealer_ms` (`null` si nunca se consultó). Lo fija cada ejecución `ok` de una receta
`accion: saldo_dealer` (su `salida` debe incluir `saldo_dealer`), y es **del chip** (lo sigue si cambia de ranura).
`POST /api/chips/saldos` también corre, después del saldo principal, la receta `saldo_dealer` de cada chip `dealer` en
línea que la tenga (si la receta pide PIN, solo los que lo tienen guardado; `entel-dealer-saldo` no lo pide). Ninguna consulta automática (cambio de
chip, validar o iniciar un lote) corre `saldo_dealer`: envía el PIN y solo se hace cuando alguien lo pide.

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

## Chips dealer

Un chip es `normal` (por defecto) o `dealer`: chips especiales con USSD propio y límites distintos. Android no dice
cuál es cuál, así que se marca a mano. El tipo es **del chip** (como el saldo y el cupo): lo sigue si cambia de
teléfono o de ranura, y un chip nuevo entra como `normal`.

**Marcar.** `PATCH /api/chips/{dispositivo}/{slot}` con `{ "tipo": "dealer" }` marca el chip que ocupa esa ranura
ahora. Responde el `ChipInfo` actualizado y emite el evento `chip_tipo`: `{ dispositivo_id, slot, tipo, anterior }`.
`404 sin_dispositivo` si el chip no está en línea.

**Selección de receta.** Una receta se busca por `(operador, accion)` y, además, por `chip_tipo`. Para un chip
de tipo `dealer`:
1. la receta con `accion`, `operador` y `chip_tipo: dealer`, si existe;
2. si no, la receta de la compañía (`accion` y `operador`, sin `chip_tipo`);
3. si tampoco, `sin_receta`.

Un chip `normal` solo ve las recetas sin `chip_tipo`; nunca usa una receta dealer. La especialización es por acción:
si solo existe `tigo-dealer-recargar`, el chip dealer recarga con ella y consulta el saldo con `tigo-saldo`.

Eso rige en todo lugar donde se elige receta por chip: la consulta de saldos, el cupo de hoy (`limites.por_dia` de
la receta que usaría ese chip) y la ejecución de los envíos de un lote. `Resultado.receta` es siempre la receta que
realmente corrió.

**Receta por nombre** (`POST /api/recetas/{nombre}/ejecutar`): si el chip es `dealer` y la receta nombrada es de la
compañía, se sustituye por su variante dealer (misma `accion` y `operador`) cuando existe. Si la receta nombrada es
`chip_tipo: dealer` y el chip es `normal`, responde `409 tipo_chip` y no marca nada.

**PIN del chip.** Algunos chips dealer (Tigo: `*555#`) piden el PIN del dealer para recargar. El PIN es **del chip**
(como el tipo) y se guarda con `PUT /api/chips/{dispositivo}/{slot}/pin` `{ "pin": "1234" }` (4 a 8 dígitos;
`null` o `""` lo borra; `400 parametro` si no tiene ese formato o no hay chip en la ranura). Queda en la base
local del control (tabla `chips`, columna `pin`) y **nunca sale**: `ChipInfo.tiene_pin` solo dice si hay uno. Al
ejecutar una receta con un parámetro `pin` secreto, si el pedido no trae `pin`, el control usa el guardado; si
tampoco hay, responde `400 parametro` sin marcar nada. Un `pin` en el pedido manda sobre el guardado.

**Lotes.** (Si la acción solo existe como receta dealer, ver "Lote por operador con recetas solo dealer".) La validación
del CSV y los montos/formato usan la receta de la compañía (los mismos para todos los
chips). Un chip dealer corre su variante dealer, si existe, al ejecutar; si esa receta rechaza el monto o el número, el
envío falla de forma normal y se reintenta en otro chip.

### Compatibilidad (antes "PDA")

Hasta el 2026-11-05 los chips dealer se llamaban "PDA". El control sigue aceptando los valores viejos como
**entrada** y siempre responde con los nuevos:

| Viejo | Nuevo | Dónde se acepta el viejo |
|---|---|---|
| `pda` | `dealer` | `chip_tipo` del YAML de una receta; `tipo` de `PATCH /api/chips/{dispositivo}/{slot}`; `origen.chips[].tipo` de un pedido |
| `saldo_pda` | `saldo_dealer` | `accion` del YAML de una receta |
| `saldo_pda`, `saldo_pda_ms` | `saldo_dealer`, `saldo_dealer_ms` | al leer un `ChipInfo` guardado (la API ya no los manda) |
| `tigo-pda-recargar`, `tigo-pda-saldo`, `entel-pda-recargar` | `tigo-dealer-recargar`, `tigo-dealer-saldo`, `entel-dealer-recargar` | todo lugar donde va un nombre de receta (por `nombres_anteriores`, ver "Renombrar una receta") |

**Migración de la base** (al arrancar, idempotente: solo toca lo que todavía dice `pda`; el PIN y las demás
columnas no cambian): `chips.tipo` y `lote_saldo_inicial.tipo` `pda` → `dealer`; las columnas
`chips.saldo_pda`/`saldo_pda_ms` se renombran a `saldo_dealer`/`saldo_dealer_ms` (con sus datos); en el JSON
guardado de los lotes (`origen`, `progreso` y la verificación) `"tipo"`/`"saldo"` `pda` → `dealer` y "PDA" →
"dealer" en los textos (avisos); y en los eventos `chip_tipo` guardados, `tipo`/`anterior`. Los nombres de receta
guardados en el historial y en los lotes no se reescriben. Al leer un lote guardado, un `pda` que hubiera quedado
también se lee como `dealer`.
