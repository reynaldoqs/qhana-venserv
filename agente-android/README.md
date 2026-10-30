---
type: Proyecto
title: agente-android — app que ejecuta USSD y lee SMS para el control
description: App Android (Kotlin) que se conecta al control por WebSocket y ejecuta USSD (API o pantalla con accesibilidad), lee SMS y reporta SIMs y estado.
tags: [general]
estado: probando
timestamp: 2026-10-29
---

# agente-android

App que convierte un teléfono Android en una "línea" del control. Se conecta al control (servidor Rust,
puerto **47800**) y obedece el protocolo de [`docs/protocolo.md`](../docs/protocolo.md).

```
[teléfono: agente] ── WebSocket ──► [control :47800]
   USB:   ws://127.0.0.1:47800/agente   + adb reverse tcp:47800 tcp:47800
   Wi-Fi: ws://<IP del PC>:47800/agente (solo cambia la URL)
```

## Qué hace

- **USSD modo `pantalla`**: marca con `ACTION_CALL` por la SIM pedida y un servicio de accesibilidad lee
  la ventana de USSD del sistema, escribe la respuesta (`ACTION_SET_TEXT`) y presiona Enviar/Cancelar por
  id. Sesiones de varios pasos. **Nunca usa coordenadas.**
- **USSD modo `api`**: `TelephonyManager.sendUssdRequest`. Un solo paso. **No sirve con Tigo** (ver abajo).
- **SMS**: avisa cada SMS entrante (`sms_recibido`) y lista la bandeja (`sms_listar`).
- **SIMs y estado**: ranuras, operador, número, estado del chip y de voz, red; batería, pantalla,
  accesibilidad y permisos faltantes cada 30 s.
- Corre como servicio en primer plano, arranca con el teléfono y se reconecta solo (1, 2, 4… 30 s).
- **No hace falta tener la app abierta.** Lo mantienen vivo `AgenteService` (primer plano, `START_STICKY`) y el servicio de
  accesibilidad, que al reconectarse también arranca el agente. Para ver por qué murió el proceso:
  `adb shell dumpsys activity exit-info bo.qhana.agente` (en las pruebas todos los cierres eran `PACKAGE UPDATED`, por reinstalar).

## Compilar

Requisitos: JDK 17 y Android SDK (platform 34). `local.properties` con `sdk.dir`.

```bash
cd agente-android
./gradlew :protocolo:test :app:assembleDebug
# APK: app/build/outputs/apk/debug/app-debug.apk
```

## Provisionar un teléfono (USB)

En el teléfono, una sola vez: **Opciones de desarrollador → Depuración USB**, y aceptar la huella del PC.

```bash
adb devices                                   # anotar el serial
./scripts/provisionar.sh <serial>             # URL por defecto ws://127.0.0.1:47800/agente, token dev
./scripts/provisionar.sh <serial> ws://192.168.1.50:47800/agente <token>   # Wi-Fi
```

Windows: `.\scripts\provisionar.ps1 -Serial <serial> [-Url …] [-Token …] [-Adb C:\platform-tools\adb.exe]`.

Qué hace el script: instala el APK, concede permisos (`CALL_PHONE`, `READ_PHONE_STATE`, `READ_PHONE_NUMBERS`,
`RECEIVE_SMS`, `READ_SMS`, `POST_NOTIFICATIONS`), **agrega** el servicio de accesibilidad a los ya activos,
deja la pantalla encendida mientras haya USB (`svc power stayon usb`) (por defecto; `PANTALLA=auto` es el modo experimental en que el agente la enciende solo al usar USSD), saca la app de la optimización de
batería, hace `adb reverse` si la URL es local y lanza la app con `--es url … --es token … --es adb_serial …`.

Cambiar la URL o el token sin tocar el teléfono:

```bash
adb -s <serial> shell am start -n bo.qhana.agente/.ui.MainActivity --es url ws://… --es token …
```

## Pasar a Wi-Fi

1. El control escucha en `0.0.0.0:47800` con un token que no sea `dev`.
2. `./scripts/provisionar.sh <serial> ws://<IP del PC>:47800/agente <token>` (no hace `adb reverse`).
3. El cable queda solo para cargar. Nada del código cambia.

## Estructura

```
agente-android/
├── protocolo/   Kotlin/JVM puro: mensajes del contrato (kotlinx.serialization, "tipo" como discriminador) + tests
├── app/src/main/java/bo/qhana/agente/
│   ├── transporte/  Transporte (interfaz) + TransporteWebSocket (OkHttp, reconexión, ping 15 s)
│   ├── ussd/        MotorUssd (interfaz), UssdApi, UssdPantalla, ServicioAccesibilidad,
│   │                PerfilDialogo (ids de la ventana por fabricante), GestorUssd (1 sesión, 60 s, enfriamiento)
│   ├── telefonia/   SIMs, slot ↔ sub_id, PhoneAccount para marcar por un chip concreto
│   ├── sms/         ReceptorSms (entrantes), LectorSms (bandeja), BusSms (cola hasta reconectar)
│   ├── servicio/    AgenteService (une todo), Despachador (petición → módulo → respuesta), EstadoDispositivo, arranque
│   ├── config/      Ajustes (url, token, adb_serial)
│   └── ui/          MainActivity: estado, SIMs, registro; editar URL/token
└── scripts/     provisionar.sh, provisionar.ps1
```

Para agregar otro canal (MQTT, otro motor USSD) se implementa `Transporte` o `MotorUssd` sin tocar el resto.
Para otro modelo de teléfono: con la ventana de USSD abierta, `adb shell uiautomator dump` (¡con el agente
detenido, ver abajo!) y agregar un `PerfilDialogo` con sus ids.

## Medido en el Infinix X678B (Android 14, XOS, 2 chips Tigo, VoLTE) — 29-oct-2026

| Operación | Tiempo |
|---|---|
| `*105#` pantalla (abre, lee, queda abierta) | 2.3–3.2 s (6 s la primera tras instalar) |
| Cancelar sesión | ~0.9 s (espera 0.8 s sin ventanas para confirmar) |
| `*66#1#` pantalla → menú | 1.3–1.5 s |
| Responder "2" → respuesta | 0.8–0.9 s |
| `ping` ida y vuelta por USB | 20–100 ms |
| `sms_listar` | 50–140 ms |

La SIM correcta se usa siempre: slot 0 → línea 78815232, slot 1 → 69759675 (verificado en el texto de `*105#`).

## Limitaciones y hallazgos

- **Tigo + modo `api` = no.** `sendUssdRequest` devuelve código −1 en ambos chips: Tigo deja la sesión
  "esperando respuesta" incluso para `*105#`, y Android lo trata como falla. Peor: **la red queda ocupada
  11–30 s** (log del módem: "cannot accept second ussd request") y el siguiente USSD falla con
  "Connection problem". El gestor aplica 30 s de enfriamiento tras un fallo de API y, si no alcanza, devuelve
  `ocupado` con los segundos restantes. **Con Tigo, usar siempre `pantalla`.**
- **No usar `uiautomator dump` con el agente funcionando**: Android desconecta los servicios de accesibilidad
  mientras corre y el agente queda ciego unos segundos.
- **Ventanas apiladas**: si el operador vence la sesión (Tigo la vence antes de 60 s), aparece
  "Connection problem or invalid MMI code" encima del menú. El cierre recorre todas hasta que no quede ninguna.
- **Mismo texto dos veces**: si el operador contesta exactamente el mismo texto que el paso anterior y la
  ventana no llega a desaparecer ni mostrar "ejecutando…", el paso termina en `timeout`. No pasó en las pruebas.
- **Pantalla apagada + bloqueo = USSD vence por tiempo.** El Infinix X678B tiene **patrón** (`dumpsys lock_settings` → `CredentialType: PATTERN`):
  con la pantalla apagada la ventana de USSD queda detrás del bloqueo y la operación termina en `timeout` tras 30 s. El agente ya
  enciende la pantalla solo al usar USSD (`servicio/Pantalla.kt`, wake lock) pero eso únicamente sirve con el bloqueo en "Ninguno";
  `adb shell locksettings set-disabled true` y `input swipe` no lo quitan en XOS. Mientras haya bloqueo se deja `stayon usb`
  (desgasta el OLED: bajar el brillo y fondo negro). El provisionar elige solo según `CredentialType`.
- **XOS congela el agente con la pantalla apagada** (Infinix X6728, Android 15, sin bloqueo): el log muestra
  `Hiber/stateManager: freeze uid … bo.qhana.agente`; aunque el servicio esté en primer plano y tenga `PARTIAL_WAKE_LOCK`, el WebSocket
  deja de responder y el USSD falla con "se cortó la conexión" a los 2–20 s. `dumpsys hiber` no es accesible por adb. Por eso el
  default es `stayon usb`. Pendiente: encontrar cómo excluir la app en Phone Master (ajustes de Transsion) para poder apagar la pantalla.
- **Pantalla**: el modo `pantalla` necesita la pantalla encendida y sin bloqueo (`svc power stayon usb`;
  sin PIN de bloqueo de pantalla).
- **ICCID**: Android solo lo entrega si la app es dueña del dispositivo (Device Owner); por ahora va `null`.
- **ANDROID_ID**: desde Android 8 es distinto por app, así que **no coincide** con `adb shell settings get secure
  android_id`. Por eso el `hola` incluye `adb_serial` (extensión al contrato v1; ver abajo).
- **Reconexiones al arrancar**: instalar + lanzar con extras produce 2–3 `hola` seguidos. El control debe
  reemplazar la conexión anterior del mismo `dispositivo_id`.
- **SMS entrante** (`sms_recibido`): implementado pero no probado con un SMS real todavía.
- **El slot 2 vacío** aparece porque el Infinix reporta 3 módems (2 físicos + eSIM): va con `estado: ausente`.

## Extensiones al protocolo v1

- `hola.adb_serial` (string, opcional): serial de ADB que el script de provisión le pasa a la app. Sirve para
  cruzar la conexión del agente con el dispositivo que ve `adb devices`.
