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
| `--token` | `dev` | Token que el agente manda en `hola`. Vacío = sin token |
| `--recetas` | `recetas` | Carpeta de recetas YAML |
| `--db` | `datos/control.db` | Historial SQLite |
| `--adb` | PATH o SDK de Android | Ruta a `adb` / `adb.exe` |
| `--sin-adb` | — | Sin ADB (solo Wi-Fi) |
| `--sin-modems` | — | No buscar módems USB |
| `--modem-puertos` | automático | Puertos fijos, p. ej. `COM5,COM6` |
| `--modem-todos-usb` | — | Probar módems USB de cualquier fabricante (por defecto solo VID conocidos) |
| `--ui` | `ui/dist` | Interfaz compilada |
| `--apk` | — | APK del agente para `POST /api/adb/{serial}/provisionar` |
| `--paquete`, `--servicio-accesibilidad`, `--actividad` | `bo.qhana.agente`… | Identidad de la app agente (para provisionar) |

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
cargo run --bin agente-simulado -- --id sim-2 --chip2 entel --limite-diario 0  # Tigo topado + Entel
```

Entel (medido 30-oct-2026): `*105#` responde `Saldo:\nBS 35.00` con la sesión ya cerrada (receta `entel-saldo`); `*133#` es el menú de transferencias y en un chip de dealer inexistente responde "El numero de dealer no existe". Los chips Entel de ese teléfono no traen su número grabado (Android lo da vacío); `*#66#` responde `MSISDN:59163053831` con la sesión cerrada y la receta `entel-saldo` lo lee en un segundo `marcar`.

Entel **PDA (dealer)** (medido 30-oct-2026, chip FutbolSinLimites 63053831): `*133#` abre `1 Saldo para transferencia / 2 Saldo Pre-pago / 3 Consulta de transferencia`, sin opción de recargar. La recarga es el atajo `*133*número*monto#` con el monto en **centavos de 3 dígitos** (`050` = Bs 0.50; 1 Bs sería `100`, sin verificar). Pide `Esta seguro de recargar Bs. 0.50 al telefono 73090695? 1 Si 2 No` y al confirmar responde `Se ha transferido Bs. 0.50 a la linea 73090695. Su saldo actual para transferencia es de Bs. 9.02` (sesión cerrada). Receta `entel-pda-recargar` (`accion: recargar`, `chip_tipo: pda`): no usa la `accion: transferir` de los lotes porque el motor no convierte Bs a centavos y el monto va en el formato del USSD. En un chip Entel normal `*133#` responde "El numero de dealer no existe".

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
- **Wi-Fi:** `qhana-control --escuchar 0.0.0.0:47800 --token <secreto> --sin-adb`, y en la app poner
  `ws://<IP del PC>:47800/agente` con el mismo token. Nada más cambia.
  Ojo: la API `/api/*` todavía no pide token. En la red local, cualquiera que llegue al puerto puede usarla
  (pendiente).

## Agregar una receta

Crear `recetas/<nombre>.yaml` (formato en `docs/protocolo.md`) y llamar a `POST /api/recetas/recargar`, o
reiniciar. Las recetas inválidas se reportan y se ignoran. Antes de usarla de verdad, probarla con
`"simular": true`: recorre el menú hasta la confirmación y responde "No".

Conviene agregar un test en `crates/nucleo/src/motor/tests.rs`, y si es de otro operador, sus pantallas en
`nucleo::simulacion`.

## Lotes (CSV)

Contrato en `docs/protocolo.md` ("Lotes (CSV) — v1"). Código en `crates/lotes`:

| Módulo | Qué hace |
|---|---|
| `normalizar` (operador) | El operador de un chip sale de su nombre de red; si no lo reconoce, del código de red (`73601` Viva, `73602` Entel, `73603` Tigo). Así "FutbolSinLimites" (marca sobre la red de Entel) cuenta como ENTEL |
| `csv` + `normalizar` | Separador `,`/`;`, BOM, comillas, encabezados con alias ("Teléfono", "Monto (Bs)", "Operadora"…) o sin encabezado. `+591 7881-5232` → `78815232`; `Bs 12`, `12,00` → 12; `1,000` es **error** (ambiguo: puede ser mil o uno) |
| `descomponer` | 12 → [10, 2] con la menor cantidad de envíos (programación dinámica) |
| `validacion` | Errores y avisos por fila contra la receta `(operador, accion)`, y el plan: reparte cada envío al chip con más saldo y menos uso que tenga cupo |
| `contexto` | Único camino para correr recetas (API, lotes, saldos): marca ocupado, guarda la ejecución y actualiza el **saldo conocido** del chip. Cupo del día = `limites.por_dia` − envíos `ok` de hoy (hora local) |
| `ejecutor` | Corre el lote: un envío por teléfono a la vez, teléfonos en paralelo; reintenta solo lo `abortado`; **`incierto` pausa el lote** |
| `persistencia` | Tablas `lotes`, `lote_filas`, `lote_envios`, `chips_saldo`, `chips_agotado`, `lote_saldo_inicial`, `lote_verificacion` |

Las recetas que usa un lote declaran `accion` y `limites`:

```yaml
accion: transferir
limites: { por_dia: 5, comision: 0.20 }
```

Reglas de seguridad del ejecutor:
- Antes de confirmar, cualquier falla es `abortado` y se reintenta en otro chip: "máximo de transferencias" →
  chip agotado por hoy; el monto no aparece en el menú → saldo 0; `ocupado`/`desconectado` → espera; `timeout`/`red`
  → hasta 2 reintentos con 5 s de espera.
- `incierto` nunca se reintenta y pausa todo el lote. Al reanudar sigue con lo pendiente.
- Pausar y cancelar esperan a que terminen los envíos en vuelo.
- Si el control se corta con envíos en curso, al arrancar quedan `incierto` y el lote `pausado`.
- Un solo lote en curso a la vez.
- `terminado` **no** es éxito: `Lote.revision.completo` solo es `true` si todas las filas quedaron `ok`; si no, `motivos` dice por qué.
- `reintentar-fallidas` re-encola solo los envíos `fallido` (nunca pasaron por `confirmar`, no movieron dinero), no los `ok`
  de una fila `parcial`; se rechaza si hay un `incierto` sin revisar. Después hay que pulsar Iniciar.
- Al iniciar un lote real se consultan los saldos de los chips del operador (los de más de 5 min) y se guardan en
  `lote_saldo_inicial`. `verificar` compara ese saldo con el actual: descontado real vs Σ (monto + comisión) de los `ok`.
  Cualquier movimiento ajeno en el chip (recarga, envío a mano) aparece como diferencia.
- El último resultado de `verificar` se guarda (tabla `lote_verificacion`) y viaja en `Lote.verificacion`; se borra si el
  lote cambia (`iniciar`, `reintentar-fallidas`). No es un estado del lote.

Agregados al contrato: `FilaValidada.motivo_sin_capacidad`, `ChipInfo.agotado_hoy`, ids internos (`id`, `n`,
`intentos`, `numero_origen`, `receta`) en filas y envíos, eventos `lote` con fase `creado` y `alerta`, y
`?separador=;` en `resultado.csv` (para Excel en español).

## Cambiar chips

Se pueden sacar y poner chips con el teléfono conectado, sin reiniciar nada. El control identifica el **chip**
(ICCID, o número si el teléfono no entrega el ICCID), no la ranura: saldo, cupo del día y "agotado" son del chip.
Al entrar uno distinto a una ranura que ya tuvo otro, se descarta el saldo anterior, queda `sin_verificar` (fuera
del reparto) y el control consulta su saldo solo. Contrato y reglas: `docs/protocolo.md`, sección "Cambio de chips".
Límite: un chip que no entrega ni ICCID ni número no se distingue de otro en la misma ranura.
Esperar a que la pantalla Teléfonos lo muestre "Lista" antes de lanzar un lote; no hace falta desconectar el USB.
Código: `crates/lotes/src/persistencia.rs` (`reconciliar_sims`) y `contexto.rs` (`reconciliar`, `vigilar_chips`).

## Chips PDA

Un chip es `normal` o `pda` (USSD y límites propios). Se marca a mano: botón "Marcar PDA" en la tarjeta del chip
en el Panel, o `PATCH /api/chips/{dispositivo}/{slot}` con `{"tipo":"pda"}`. El tipo es del chip, no de la ranura.

Para darle un USSD propio, crea una receta con `chip_tipo: pda` y la misma `operador` + `accion` que la de la
compañía (por ejemplo `tigo-pda-transferir.yaml`, `accion: transferir`). Un chip PDA usa esa receta si existe y, si
no, la de la compañía; un chip normal nunca usa recetas PDA. Es por acción: se puede tener solo `recargar` PDA y
que el saldo siga con `tigo-saldo`. Detalle en `docs/protocolo.md`, "Chips PDA".

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
| `POST /api/adb/{serial}/provisionar` | Instala el APK (`{"apk": "ruta"}` o `--apk`), da permisos, activa la accesibilidad, `stayon usb`, quita la optimización de batería, `reverse` y abre la app |
| `POST /api/recetas/recargar` | Relee la carpeta de recetas |
| `GET /api/ejecuciones?limite=50` | Ejecuciones de recetas guardadas |
| `GET /api/historial?tipo=ussd&limite=100` | Filtra por tipo de evento |
| `GET /api/lotes/{id}/resultado.csv?separador=;` | Resultado con `;` (Excel en español) |
| `POST /api/lotes/{id}/reintentar-fallidas` | Vuelve a `pendiente` los envíos `fallido` (luego `iniciar`) |
| `POST /api/lotes/{id}/verificar` | Cuadra los saldos reales de los chips contra lo enviado |

Códigos de error agregados: `sin_dispositivo` (404), `desconectado` (503), `sin_receta` (404),
`ruta_inexistente` (404), `sin_adb` (503). Lotes: `sin_lote` (404), `en_curso`, `otro_lote`, `cancelado`,
`nada_pendiente`, `no_en_curso`, `terminado`, `hay_inciertos`, `nada_que_reintentar`, `simulado`, `sin_base` (todos 409).

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
