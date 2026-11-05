---
type: Proyecto
title: qhana-control — servidor de control (Rust)
description: Recibe a los teléfonos agente por WebSocket, los gestiona por ADB, ejecuta recetas USSD y expone la API/eventos para la interfaz.
tags: [general]
estado: probando
timestamp: 2026-10-29
---

# qhana-control

El servidor que corre en el PC (Windows en producción). Contrato: [`docs/protocolo.md`](../docs/protocolo.md).

```
[Teléfono: app agente] ──WS /agente──►┐
[Teléfono: app agente] ──WS /agente──►│  qhana-control :47800  ◄── HTTP /api + WS /api/eventos ── [Interfaz]
[agente-simulado]      ──WS /agente──►┘        │
                                               ├── ADB: adb reverse, android_id, provisión
                                               └── SQLite: historial y ejecuciones
```

Un solo puerto, **47800**, para agentes, API e interfaz (`ui/dist`, si está compilada).

## Crates

| Crate | Qué hace | Depende de |
|---|---|---|
| `protocolo` | Mensajes agente ↔ control y tipos de la API. Tolera campos desconocidos | serde |
| `nucleo` | Dominio sin red: trait **`CanalUssd`**, registro de dispositivos, bus de eventos, **recetas** y **motor**, operadores simulados | protocolo |
| `agentes` | Endpoint WebSocket `/agente`: saludo y token, `req`↔respuesta, ping 15 s / caída 45 s. Cada conexión implementa `CanalUssd` | nucleo, axum |
| `adb` | Gestor ADB: cada 3 s lista teléfonos, aplica `adb reverse tcp:47800`, cruza serial ↔ `android_id`, provisiona | nucleo |
| `modem` | **Módems USB con comandos AT** (Huawei, Quectel, SIMCom…): detector por puertos, identidad por IMEI, GSM7 en hex y `CanalModem` (implementa `CanalUssd`). Contrato: "Módems USB" en `docs/protocolo.md` | nucleo |
| `almacen` | SQLite (`rusqlite` *bundled*: nada que instalar en Windows). Eventos, ejecuciones y sus pasos | protocolo |
| `lotes` | **Lotes desde CSV**: lector tolerante, validación, descomposición de montos, plan de reparto entre chips, saldo/cupo por chip y **ejecutor** seguro. Tablas SQLite propias | nucleo, almacen |
| `simulador` | Binario **`agente-simulado`**: teléfono falso con 2 chips (Tigo/Entel) para trabajar sin gastar saldo | nucleo |
| `servidor` | Binario **`qhana-control`**: axum, rutas del contrato, interfaz estática, CLI | todos |

El motor (`nucleo::motor`) solo conoce `CanalUssd`. Un teléfono por USB, uno por Wi-Fi o un módem AT son
implementaciones distintas del mismo trait: el motor y las recetas no cambian.

## Correr

```bash
cd control
cargo run --bin qhana-control                     # API en http://127.0.0.1:47800, ADB automático
cargo run --bin agente-simulado -- --id sim-1     # en otra terminal: un teléfono falso
curl -s localhost:47800/api/dispositivos
```

Opciones (`qhana-control --help`; también por variable de entorno `QHANA_*`):

| Opción | Por defecto | Para qué |
|---|---|---|
| `--escuchar` | `127.0.0.1:47800` | `0.0.0.0:47800` para agentes por Wi-Fi |
| `--token` | `dev` | Token que el agente manda en `hola`. Vacío = sin token. Fuera de `127.0.0.1` no puede ser vacío ni `dev` |
| `--api-token` (`QHANA_API_TOKEN`) | — | Token de la API para pedidos desde otra máquina (16+ caracteres). Obligatorio si `--escuchar` no es `127.0.0.1` |
| `--recetas` | `recetas` | Carpeta de recetas YAML |
| `--db` | `datos/control.db` | Historial SQLite (respaldos en `backup/` junto a la base) |
| `--adb` | PATH o SDK de Android | Ruta a `adb` / `adb.exe` |
| `--sin-adb` | — | Sin ADB (solo Wi-Fi) |
| `--sin-modems` | — | No buscar módems USB |
| `--modem-puertos` | automático | Puertos fijos, p. ej. `COM5,COM6` |
| `--modem-todos-usb` | — | Probar módems USB de cualquier fabricante (por defecto solo VID conocidos) |
| `--ui` | `ui/dist` | Interfaz compilada |
| `--apk` | — | APK del agente para `POST /api/adb/{serial}/provisionar` |
| `--paquete`, `--servicio-accesibilidad`, `--actividad` | `bo.qhana.agente`, …, `bo.qhana.agente/.ui.ActividadConfig` | Identidad de la app agente (para provisionar). `--actividad` es la actividad que recibe la configuración por ADB (`ActividadConfig`, protegida con `DUMP`) |

Logs: `RUST_LOG=debug cargo run --bin qhana-control`.

### Correr el ejecutable (sin `cargo run`)

```bash
scripts/empaquetar.sh            # compila release + interfaz y arma control/paquete/
cd paquete && ./iniciar.sh       # → http://127.0.0.1:47800/   (opciones extra se pasan: ./iniciar.sh --token otro)
```

`paquete/` es autosuficiente: `qhana-control`, `ui/dist`, `recetas/`, `agente.apk` y `datos/` (historial, saldos y cupos; reempaquetar no lo toca).
Necesita `adb` en el PATH o en el SDK de Android (`--adb ruta`). Con el APK incluido, un teléfono nuevo se conecta **desde la interfaz**:
enchufarlo, aceptar la depuración USB, y en Teléfonos pulsar **Instalar agente** (`POST /api/adb/{serial}/provisionar`, el equivalente de
`agente-android/scripts/provisionar.sh`). Probado el 30-oct-2026: instaló y configuró un Infinix X6728 con todos los pasos en `ok`.
Para Windows: `cargo build --release --bin qhana-control` y copiar `qhana-control.exe`, `ui/dist`, `recetas` y el APK a una carpeta.

### Agente simulado

```bash
cargo run --bin agente-simulado -- --id sim-1 --saldo 14                       # 2 chips Tigo
cargo run --bin agente-simulado -- --id sim-2 --chip2 entel --limite-diario 0 --numero1 71111111 --numero2 72222222  # Tigo topado + Entel
cargo run --bin agente-simulado -- --id sim-3 --chip2 entel --entel-dealer --numero1 71110003 --numero-entel 71110004  # chip 2 Entel dealer (*133*N*050#)
```

El ICCID del simulador sale del número del chip: dos simuladores con los mismos números (los de por defecto) son
**los mismos chips** para el control (saldo, cupo y "agotado" compartidos). Para dos simuladores, cambia los números.

Entel (medido 30-oct-2026): `*105#` responde `Saldo:\nBS 35.00` con la sesión ya cerrada (receta `entel-saldo`); `*133#` es el menú de transferencias y en un chip de dealer inexistente responde "El numero de dealer no existe". Los chips Entel de ese teléfono no traen su número grabado (Android lo da vacío); `*#66#` responde `MSISDN:59163053831` con la sesión cerrada y la receta `entel-saldo` lo lee en un segundo `marcar`.

Entel **dealer** (medido 30-oct-2026, chip FutbolSinLimites 63053831): `*133#` abre `1 Saldo para transferencia / 2 Saldo Pre-pago / 3 Consulta de transferencia`, sin opción de recargar. La recarga es el atajo `*133*número*monto#` con el monto en **Bs enteros** (`*133*N*1#` = Bs 1, según Rey el 2026-11-05) y una sola fracción: `050` = Bs 0.50 (medido). No hay otros montos con centavos. La receta lo arma con `{monto_mixto}` (no con `{monto_cent3}`, que para Bs 1 marcaría `100`). Falta medir cómo muestra la confirmación un entero ("Bs. 1.00" o "Bs. 1"; el `verificar` acepta los dos). Pide `Esta seguro de recargar Bs. 0.50 al telefono 73090695? 1 Si 2 No` y al confirmar responde `Se ha transferido Bs. 0.50 a la linea 73090695. Su saldo actual para transferencia es de Bs. 9.02` (sesión cerrada). Receta `entel-dealer-recargar` (`accion: recargar`, `chip_tipo: dealer`): es una acción aparte de `transferir` porque gasta el saldo de dealer. En un chip Entel normal `*133#` responde "El numero de dealer no existe".

Tigo **dealer** (medido 03-nov-2026, chip 77716151 en Infinix X6728): `*555#` es el menú de **Tigo Money**
(`1- Transferencias / 2- Paquetigos / 3- Recarga Tigo / 4- Pagar Facturas Tigo / 5- Pagar Servicios / 6- Olvide mi PIN /
7- Ubicar Punto Tigo Money / 8- Consultar saldo / 9- Otros`); el "saldo dealer" es el saldo Tigo Money.
- Recarga (`tigo-dealer-recargar`): `3- Recarga Tigo` → `1- Recarga mi numero / 2- Recarga a otro numero` → `2` →
  `Escribe el monto en Bs (desde 0.20 ctvos.):` → `Ingresa el numero Tigo al que quieres realizar la compra de recarga:`
  → `Deseas comprar recarga por Bs 0.20  para el 62478239. Introduce tu PIN para confirmar:` → PIN →
  `Recarga Tigo por Bs 0.20. Tu saldo total Bs 11.06. Factura: mifactura.tigo.net.bo … ID: 2790003884.` (sesión cerrada).
  La receta elige las opciones por texto, verifica monto y número en la confirmación antes de mandar el PIN (por eso el
  monto va entero o con 2 decimales: `2` se muestra `2.00`) y lee de la respuesta el saldo Tigo Money que queda.
- Saldo (`tigo-dealer-saldo`): `8- Consultar saldo` → `Ingresa tu PIN para confirmar:` → PIN →
  `03/11/26 11:07 ⏎ Tu saldo total Tigo Money es de: Bs 211.06 ⏎ Saldo Principal: Bs 211.06`. La salida se ancla en
  "saldo total": con "saldo" a secas leía el `9` de `9- Otros` del menú.
- Falta medir: la respuesta a un PIN errado y a saldo insuficiente.

Viva (medido 30-oct-2026): `*105#` responde `60118935\nSALDO TOTAL Bs:0.00\nPrincipal:0.00\nV:23-10-26\nDATOS:0\n1. Ver mas\n2. Salir` y deja el menú abierto (la receta `viva-saldo` lo cierra; sin eso el dispositivo queda `ocupado`). El número de línea viene en la primera línea del texto; el chip Viva no trae su número grabado en Android. Las regex de `salida` se compilan sin flags: para `^`/`$` por línea hay que poner `(?m)`.

Responde los flujos reales de Tigo (saldo `*105#`, transferencias `*66#1#` con montos según saldo,
comisión 0.20, límite diario, "Ultimos movimientos") y de Entel (`*133#`). Tras una transferencia manda un
`sms_recibido`. Reconecta solo, como el agente real.

### Pruebas

```bash
cargo test                       # motor, menús, recetas reales, protocolo, ADB, SQLite
scripts/prueba-humo.sh           # API completa contra 2 agentes simulados (ver cabecera del script)
```

`prueba-humo.sh` **hace transferencias**: úsalo solo contra `agente-simulado`, nunca contra un teléfono real.
`scripts/prueba-humo-recargas.sh` prueba montos con centavos, recargas dealer por operador, lotes desde chips elegidos,
carriles, verificación por tipo de saldo y plantillas (cabecera del script: cómo levantar un control aparte y dos
simuladores); se niega a correr si hay un dispositivo que no es simulado.

## Windows

- Compilar en el propio Windows: instalar Rust (MSVC) y `cargo build --release`. Salen
  `target\release\qhana-control.exe` y `agente-simulado.exe`. SQLite va dentro del binario.
- `adb.exe` se busca en el PATH y en `%LOCALAPPDATA%\Android\Sdk\platform-tools`. Si no, usa `--adb`.
  Las llamadas a adb no abren ventanas de consola.
- Desde Mac, compilar para Windows (`cargo build --target x86_64-pc-windows-gnu`) exige el target y mingw
  por el SQLite incluido: es solo una opción. Lo recomendado es compilar en Windows o en CI.
- Para correr como servicio de Windows: pendiente (crate `windows-service`). Hoy corre como programa.

## USB hoy, Wi-Fi mañana

- **USB:** el gestor ADB aplica `adb reverse tcp:47800 tcp:47800` a cada teléfono y el agente se conecta a
  `ws://127.0.0.1:47800/agente`.
- **PC dormido:** al dormir el PC el túnel `adb reverse` puede perderse y el teléfono, que reintenta con pausas de
  hasta 30 s, tarda en volver. El gestor ADB detecta el salto del reloj y revisa el túnel de todos; el de un teléfono
  sin agente se revisa cada 3 s. La interfaz reabre su conexión de eventos al despertar y relee todo. Si aun así
  queda alguno caído: botón **Reconectar** en Teléfonos (abrir la app con `reconectar` necesita el APK nuevo; uno
  viejo solo abre la app).
- **Wi-Fi:** `qhana-control --escuchar 0.0.0.0:47800 --token <secreto> --api-token <16+ caracteres> --sin-adb`, y en
  la app poner `ws://<IP del PC>:47800/agente` con el mismo `--token`. Sin `--api-token` (o con `--token` vacío o `dev`)
  el control no arranca fuera de `127.0.0.1`.
- **Token de la API:** los pedidos a `/api/*` que no vienen de la misma máquina llevan `Authorization: Bearer <api-token>`
  (sin él: `401 no_autorizado`). Solo el WebSocket `/api/eventos` acepta además `?token=`. `/api/salud` no lo pide y dice
  `requiere_token`. Desde `127.0.0.1` (la app de escritorio, la interfaz local) no hace falta. Los tokens se comparan en
  tiempo constante y el control no anota URLs en los logs. Ojo: por `adb reverse`, una app del teléfono también llega
  como `127.0.0.1`.

## Agregar una receta

Crear `recetas/<nombre>.yaml` (formato en `docs/protocolo.md`) y llamar a `POST /api/recetas/recargar`, o
reiniciar. Las recetas inválidas se reportan y se ignoran. Antes de usarla de verdad, probarla con
`"simular": true`: recorre el menú hasta la confirmación y responde "No".

Conviene agregar un test en `crates/nucleo/src/motor/tests.rs`, y si es de otro operador, sus pantallas en
`nucleo::simulacion`.

**Desde la interfaz** (contrato: "Editar recetas desde la interfaz" en `docs/protocolo.md`). `nucleo::carpeta`
guarda en un temporal y renombra, copia la versión anterior a `recetas/viejo/AAAA-MM-DD_HHMMSS_<nombre>.yaml` antes
de reemplazar, y retirar mueve el archivo ahí (nunca se borra); lo de `viejo/` no se carga. `version_base` evita
pisar un cambio ajeno (`409 conflicto`). `nucleo::probar` evalúa un paso contra una pantalla con las reglas del motor
y `nucleo::borrador` arma un borrador desde una sesión de la consola. Cada ejecución guarda `receta_version` (SHA-256
del archivo): una receta está `sin_probar` hasta que esa versión termina `ok` o `simulado`, y el lote lo avisa en
`resumen.avisos`. Recargar emite `receta_cambiada` por cada archivo que cambió.

## Lotes (CSV)

Contrato en `docs/protocolo.md` ("Lotes (CSV) — v1"). Código en `crates/lotes`:

| Módulo | Qué hace |
|---|---|
| `normalizar` (operador) | El operador de un chip sale de su nombre de red; si no lo reconoce, del código de red (`73601` Viva, `73602` Entel, `73603` Tigo). Así "FutbolSinLimites" (marca sobre la red de Entel) cuenta como ENTEL |
| `csv` + `normalizar` | Separador `,`/`;`, BOM, comillas, encabezados con alias ("Teléfono", "Monto (Bs)", "Operadora"…) o sin encabezado. `+591 7881-5232` → `78815232`; `Bs 12`, `12,00` → 12; `1,000` es **error** (ambiguo: puede ser mil o uno) |
| `descomponer` | 12 → [10, 2] con la menor cantidad de envíos (programación dinámica) |
| `validacion` | Errores y avisos por fila contra la receta `(operador, accion)`, y el plan: reparte cada envío al chip con más saldo y menos uso que tenga cupo |
| `contexto` | Único camino para correr recetas (API, lotes, saldos): marca ocupado, guarda la ejecución y actualiza el **saldo conocido** del chip. Cupo del día = `limites.por_dia` − envíos `ok` de hoy (hora local) |
| `ejecutor` | Corre el lote: un envío por teléfono a la vez, teléfonos en paralelo; reintenta solo lo `abortado`; **`incierto` no detiene el lote ni se reintenta solo** |
| `persistencia` | Tablas `lotes`, `lote_filas`, `lote_envios`, `chips_saldo`, `chips_agotado`, `lote_saldo_inicial`, `lote_verificacion`. `lotes` guarda además `progreso` (JSON), `problemas`, `archivado` y `hash`: la lista paginada no lee filas |

Las recetas que usa un lote declaran `accion` y `limites`:

```yaml
accion: transferir
limites: { por_dia: 5, comision: 0.20 }
```

Reglas de seguridad del ejecutor:
- Antes de confirmar, cualquier falla es `abortado` y se reintenta en otro chip: "máximo de transferencias" →
  chip agotado por hoy; el monto no aparece en el menú → saldo 0; `ocupado`/`desconectado` → espera; `timeout`/`red`
  → hasta 2 reintentos con 5 s de espera.
- `incierto` no detiene el lote: se marca, se avisa y sigue con lo demás. El ejecutor nunca lo reintenta solo;
  el operador puede re-encolarlo («Reintentar inciertas», `inciertas: true`) tras revisar que no llegó.
- Pausar y cancelar esperan a que terminen los envíos en vuelo.
- Si el control se corta con envíos en curso, al arrancar quedan `incierto` (en cualquier lote, esté como esté; en una
  simulación vuelven a `pendiente`) y el lote `pausado`. Igual si la tarea de un envío se cae: `incierto` y el teléfono libre.
- Un envío solo sale si su paso a `en_curso` quedó guardado en la base (`synchronous=FULL`); si no, el lote se pausa con
  "no se pudo guardar en la base: …".
- El teléfono se **reserva** antes de despachar (de forma atómica, `Contexto::reservar`): una consulta de saldo, una
  receta desde la API o una sesión USSD manual no pueden tomarlo a la vez. La reserva se suelta sola al terminar, también
  si la tarea entra en pánico.
- `ocupado` o error del canal antes de marcar: se reintenta hasta 10 veces por envío; después `fallido` ("el teléfono
  siguió ocupado").
- Las recetas (y el cupo que dan) se copian al iniciar o reanudar: recargarlas a mitad de un lote no cambia su flujo.
- Un lote real igual a otro real de las últimas 24 h (hash SHA-256 del CSV normalizado + origen, no cancelado) responde
  `409 duplicado`; con `forzar: true` se crea igual.
- Un solo lote en curso a la vez.
- `terminado` **no** es éxito: `Lote.revision.completo` solo es `true` si todas las filas quedaron `ok`; si no, `motivos` dice por qué.
- `reintentar-fallidas` re-encola solo los envíos `fallido` (nunca pasaron por `confirmar`, no movieron dinero), no los `ok`
  de una fila `parcial`. Con `{ "inciertas": true }` también re-encola los `incierto` (puede enviar dos veces: solo
  tras revisar el saldo). Después hay que pulsar Iniciar.
- Al iniciar un lote real se consultan los saldos de los chips del operador (los de más de 5 min) y se guardan en
  `lote_saldo_inicial`. `verificar` compara ese saldo con el actual: descontado real vs Σ (monto + comisión) de los `ok`.
  Cualquier movimiento ajeno en el chip (recarga, envío a mano) aparece como diferencia.
- El último resultado de `verificar` se guarda (tabla `lote_verificacion`) y viaja en `Lote.verificacion`; se borra si el
  lote cambia (`iniciar`, `reintentar-fallidas`). No es un estado del lote.

Agregados al contrato: `FilaValidada.motivo_sin_capacidad`, `ChipInfo.agotado_hoy`, ids internos (`id`, `n`,
`intentos`, `numero_origen`, `receta`) en filas y envíos, eventos `lote` con fase `creado` y `alerta`, y
`?separador=;` en `resultado.csv` (para Excel en español).

## Base de datos

- SQLite en WAL con `synchronous=FULL` (un corte de luz no deshace un envío marcado `en_curso`).
- Cada ejecución de receta se anota **antes** de marcar (`ejecuciones.estado = en_curso`) y se completa al terminar.
  Al arrancar, las que quedaron `en_curso` pasan a `incierto`.
- **Mantenimiento** (al arrancar y cada 24 h, en un hilo aparte, sin frenar al ejecutor):
  - respaldo `VACUUM INTO <carpeta de la base>/backup/AAAA-MM-DD_control.db` (uno por día; quedan los últimos 14). Usa
    su propia conexión de lectura: con WAL no bloquea las escrituras;
  - borra los `eventos` de más de 90 días. Las ejecuciones y los lotes no se borran.
- Para restaurar: parar el control y copiar un respaldo sobre `control.db` (borrando `control.db-wal` y `-shm`).

## Cambiar chips

Se pueden sacar y poner chips con el teléfono conectado, sin reiniciar nada. El control identifica el **chip**
(ICCID, o número si el teléfono no entrega el ICCID), no la ranura: saldo, cupo del día y "agotado" son del chip.
Al entrar uno distinto a una ranura que ya tuvo otro, se descarta el saldo anterior, queda `sin_verificar` (fuera
del reparto) y el control consulta su saldo solo. Contrato y reglas: `docs/protocolo.md`, sección "Cambio de chips".
Límite: un chip que no entrega ni ICCID ni número no se distingue de otro en la misma ranura.
Esperar a que la pantalla Teléfonos lo muestre "Lista" antes de lanzar un lote; no hace falta desconectar el USB.
Código: `crates/lotes/src/persistencia.rs` (`reconciliar_sims`) y `contexto.rs` (`reconciliar`, `vigilar_chips`).

## Chips dealer

Un chip es `normal` o `dealer` (USSD y límites propios). Se marca a mano: botón "Marcar dealer" en la tarjeta del chip
en el Panel, o `PATCH /api/chips/{dispositivo}/{slot}` con `{"tipo":"dealer"}`. El tipo es del chip, no de la ranura.

Para darle un USSD propio, crea una receta con `chip_tipo: dealer` y la misma `operador` + `accion` que la de la
compañía (por ejemplo `tigo-dealer-transferir.yaml`, `accion: transferir`). Un chip dealer usa esa receta si existe y, si
no, la de la compañía; un chip normal nunca usa recetas dealer. Es por acción: se puede tener solo `recargar` dealer y
que el saldo siga con `tigo-saldo`. Detalle en `docs/protocolo.md`, "Chips dealer".

**Antes "PDA" (2026-11-05).** Los chips dealer se llamaban "PDA". El control sigue aceptando `pda` (tipo de chip,
`chip_tipo`) y `saldo_pda` (acción) como entrada y responde siempre `dealer` / `saldo_dealer`; al arrancar migra la
base (tipo de los chips, saldos iniciales, columnas `saldo_pda*` → `saldo_dealer*`, JSON de los lotes) sin tocar
el PIN. Las recetas `tigo-pda-recargar`, `tigo-pda-saldo` y `entel-pda-recargar` pasaron a `tigo-dealer-*` /
`entel-dealer-recargar` con `nombres_anteriores: [<nombre viejo>]`: los lotes guardados, `ejecutar`, las versiones
y las estadísticas encuentran la receta también por el nombre viejo. Las copias viejas están en `recetas/viejo/`.
Ver "Compatibilidad" y "Renombrar una receta" en `docs/protocolo.md`.

**Lote de un chip.** En "Nuevo lote" → "Desde un chip" se elige teléfono, chip y acción (receta); el CSV basta
con `numero,monto` (`1.50` vale si la receta no tiene montos fijos) y todo sale de ese chip. Es la forma de hacer
recargas dealer en lote. En la API: `origen: {dispositivo, slot, receta}` en `POST /api/lotes/validar` y `POST /api/lotes`.
Detalle en `docs/protocolo.md`, "Lote de un chip". Para probar sin teléfono: `agente-simulado --pin-dealer 4321
--saldo-dealer 50` hace del chip 1 un Tigo dealer con `*555#` (pantallas inventadas), y `--chip2 entel --entel-dealer`
hace del chip 2 un Entel dealer con `*133*número*monto#` (Bs enteros o `050`).

**Lote desde chips elegidos.** `origen: {chips: [{dispositivo, slot}, …], accion: "recargar"}`: el lote se reparte
solo entre esos chips (de uno o varios operadores), en paralelo entre teléfonos; cada chip corre la receta que le
toca. Un chip que no sirve (normal para una receta dealer, sin PIN…) se excluye con aviso (`avisos_origen`). La forma
vieja `{dispositivo, slot, receta}` sigue valiendo. **Por operador** (sin `origen`), si para (operador, acción) solo
hay receta dealer (`recargar`), las filas van solo a los chips dealer de ese operador. `GET /api/lotes/{id}/carriles`
muestra qué hace cada chip (en vivo y después). `verificar` cuadra por chip y por tipo de saldo (`principal`, `dealer` o
`mixto`). Detalle en `docs/protocolo.md`.

**Montos.** El monto siempre es en Bs (`0.50`, `5`, `12.30`): CSV, panel, `parametros` e historial. El control lo
normaliza (`0,5`, `Bs 0.50`, `.50` → `0.50`) y la receta lo traduce con valores calculados: `{monto_cent}` (50),
`{monto_cent3}` (050), `{monto_entero}` (solo sin centavos) y `{monto_mixto}` (Bs enteros; menos de 1 Bs, 050).
`entel-dealer-recargar` marca `*133*{numero}*{monto_mixto}#` con `patron: '^(0\.50|[1-9]\d*)$'`: Bs 100 es un solo
envío `*133*N*100#`. Los lotes calculan en centavos (con opciones fijas, 1.50 con opción 0.50 = 3 envíos).

**PIN del dealer.** Las recetas con un parámetro `pin` secreto (hoy `tigo-dealer-recargar`) toman el PIN guardado del
chip. Se guarda en el Panel ("Guardar PIN" en la tarjeta de un chip dealer; al marcar dealer un chip Tigo se ofrece
solo), en la pantalla Recetas ("Guardar el PIN en este chip") o con `PUT /api/chips/{dispositivo}/{slot}/pin`
`{"pin":"1234"}` (`null` lo borra). Queda en la tabla `chips` de la base local, sin cifrar (como el resto de la
base: protege la carpeta de datos), sigue al chip y nunca sale por la API, los eventos ni el historial (`••••`).

## Agregar otro tipo de canal (por ejemplo, módem AT)

1. Nuevo crate `crates/modem` que implemente `nucleo::CanalUssd`:
   - `iniciar`: `AT+CUSD=1,"<GSM7 empaquetado>",15` y esperar `+CUSD:` (ver `modem/tigo.py`).
   - `responder`: otro `AT+CUSD=1,…`.
   - `cancelar`: `AT+CUSD=2`.
2. Al detectar un módem, registrarlo en `Registro` con un `Hola` armado a mano (IMEI como `dispositivo_id`)
   y el canal: `registro.conectar(&hola, Arc::new(CanalModem { … }))`.
3. Listo: aparece en `/api/dispositivos` y el motor de recetas lo usa igual que a un teléfono.

## API (resumen)

Contrato completo en `docs/protocolo.md`. Agregados del control:

| Ruta | Para qué |
|---|---|
| `POST /api/dispositivos/{id}/refrescar` | Pide al agente `sims` y `estado` |
| `POST /api/dispositivos/reconectar` | Botón **Reconectar** en Teléfonos: rehace `adb reverse`, abre la app de los teléfonos sin agente (`--ez reconectar true`), pide `refrescar` a los conectados y corta al que no responde en 15 s. No toca a los ocupados |
| `POST /api/adb/{serial}/provisionar` | Instala el APK (`{"apk": "ruta"}` o `--apk`), da permisos, activa la accesibilidad, `stayon usb`, quita la optimización de batería, `reverse` y abre la app |
| `POST /api/recetas/recargar` | Relee la carpeta de recetas |
| `GET /api/ejecuciones?limite=50` | Ejecuciones de recetas guardadas |
| `GET /api/historial?tipo=ussd&limite=100` | Filtra por tipo de evento |
| `GET /api/lotes/{id}/resultado.csv?separador=;` | Resultado con `;` (Excel en español) |
| `POST /api/lotes/{id}/reintentar-fallidas` | Vuelve a `pendiente` los envíos `fallido` (luego `iniciar`) |
| `POST /api/lotes/{id}/verificar` | Cuadra los saldos reales de los chips contra lo enviado |
| `GET /api/lotes?limite=&antes=&estado=&problemas=&simular=&q=&archivados=` | Lista paginada sin filas: `{lotes, siguiente, total, conteos}` |
| `GET /api/lotes/{id}?filas=false` · `GET /api/lotes/{id}/filas?estado=&q=&despues=&limite=` | Lote sin filas · filas por páginas `{filas, siguiente, total}` |
| `POST /api/lotes/{id}/archivar` | `{archivado: true\|false}`, solo lotes terminados o cancelados |

Códigos de error agregados: `sin_dispositivo` (404), `desconectado` (503), `sin_receta` (404),
`ruta_inexistente` (404), `sin_adb` (503), `no_autorizado` (401). Lotes: `sin_lote` (404), `en_curso`, `otro_lote`,
`cancelado`, `nada_pendiente`, `no_en_curso`, `terminado`, `nada_que_reintentar`, `simulado`, `sin_base`, `no_aplica`,
`duplicado` (todos 409).

USSD manual: `/api/ussd/iniciar` reserva el teléfono (`409 ocupado` si un lote o una receta lo tiene) mientras la sesión
está abierta; se libera al cerrarse, al `cancelar` o a los 60 s sin `responder`. `responder`/`cancelar` solo actúan
sobre la sesión manual de ese teléfono, nunca sobre la de una receta en curso (`409 ocupado`).

### App de escritorio (Tauri, Mac y Windows)

`escritorio/` es una ventana Tauri v2 que lleva dentro el control, la interfaz, las recetas, el APK y `adb`. Al abrirla arranca el
control, espera a que responda y muestra la interfaz; al cerrarla lo detiene. Los datos (historial, saldos, cupos) quedan en la
carpeta de datos de la app (Mac: `~/Library/Application Support/bo.qhana.control/`; Windows: `%APPDATA%\bo.qhana.control\`).
Si ya hay un control en el puerto 47800, la app usa ese en vez de lanzar otro.

```bash
cd escritorio && npm install
npm run build -- --bundles app      # Mac → src-tauri/target/release/bundle/macos/Qhana.app
open src-tauri/target/release/bundle/macos/Qhana.app
```

`npm run build` sin `--bundles` genera también el `.dmg`. **En Windows** se compila en una PC Windows (Rust, Node y
las herramientas de Visual Studio C++), no desde el Mac: `npm run build` produce el instalador en `src-tauri/target/release/bundle/`.
`scripts/preparar.sh` es bash y copia el `adb` de Mac; en Windows hay que copiar a `src-tauri/recursos/` el `adb.exe` y sus DLL
(platform-tools) y el `qhana-control.exe`. Sin firmar: en otro Mac, la primera vez abrir con clic derecho → Abrir.
El icono es provisional (cuadrado naranja de marca).
