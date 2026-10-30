#!/usr/bin/env bash
# Deja un teléfono listo para el agente, por USB.
#   ./provisionar.sh <serial> [url] [token]
#   url   por defecto ws://127.0.0.1:47800/agente (USB con adb reverse)
#   token por defecto dev
# Variables: APK (ruta del apk), ADB (binario adb), PUERTO (47800),
#            PANTALLA=auto para el modo experimental en que el agente enciende la pantalla solo al usar USSD (ver abajo).
set -euo pipefail

SERIAL="${1:?uso: provisionar.sh <serial> [url] [token]}"
URL="${2:-ws://127.0.0.1:47800/agente}"
TOKEN="${3:-dev}"
PKG="bo.qhana.agente"
PUERTO="${PUERTO:-47800}"
DIR="$(cd "$(dirname "$0")/.." && pwd)"
APK="${APK:-$DIR/app/build/outputs/apk/debug/app-debug.apk}"
ADB="${ADB:-adb}"
SERVICIO="$PKG/$PKG.ussd.ServicioAccesibilidad"
a() { "$ADB" -s "$SERIAL" "$@"; }

[[ -f "$APK" ]] || { echo "No existe $APK. Compila primero: ./gradlew :app:assembleDebug" >&2; exit 1; }
echo "== $SERIAL: $(a shell getprop ro.product.manufacturer | tr -d '\r') $(a shell getprop ro.product.model | tr -d '\r'), Android $(a shell getprop ro.build.version.release | tr -d '\r')"

echo "== instalando $APK"
a install -r -g "$APK" >/dev/null

echo "== permisos"
for p in CALL_PHONE READ_PHONE_STATE READ_PHONE_NUMBERS RECEIVE_SMS READ_SMS POST_NOTIFICATIONS; do
  a shell pm grant "$PKG" "android.permission.$p" 2>/dev/null || echo "   (no se pudo conceder $p)"
done

echo "== accesibilidad"
ACTUALES="$(a shell settings get secure enabled_accessibility_services | tr -d '\r')"
if [[ "$ACTUALES" == *"$SERVICIO"* ]]; then
  echo "   ya estaba activa"
else
  if [[ -z "$ACTUALES" || "$ACTUALES" == "null" ]]; then NUEVOS="$SERVICIO"; else NUEVOS="$ACTUALES:$SERVICIO"; fi
  a shell settings put secure enabled_accessibility_services "$NUEVOS"
fi
a shell settings put secure accessibility_enabled 1

# Por defecto la pantalla queda encendida con USB: probado en el Infinix X678B. Con la pantalla apagada XOS congela el proceso del
# agente ("Hiber/stateManager: freeze uid…", visto en el X6728) y el USSD vence; además con PIN/patrón la ventana queda tras el bloqueo.
# PANTALLA=auto: experimental, la enciende el agente solo al usar USSD (solo si no hay bloqueo y XOS no lo congela).
if [[ "${PANTALLA:-siempre}" == "auto" ]]; then
  echo "== EXPERIMENTAL: el agente enciende la pantalla solo al usar USSD y se apaga a los 15 s"
  a shell svc power stayon false
  a shell settings put system screen_off_timeout 15000
else
  echo "== pantalla siempre encendida con USB (baja el brillo y usa fondo negro para cuidar el OLED)"
  a shell svc power stayon usb
fi
echo "== sin optimización de batería"
a shell dumpsys deviceidle whitelist +"$PKG" >/dev/null
a shell cmd appops set "$PKG" RUN_ANY_IN_BACKGROUND allow 2>/dev/null || true
a shell cmd appops set "$PKG" RUN_IN_BACKGROUND allow 2>/dev/null || true

if [[ "$URL" == ws://127.0.0.1:* || "$URL" == ws://localhost:* ]]; then
  P="${URL#ws://*:}"; P="${P%%/*}"
  echo "== adb reverse tcp:$P → PC:$P"
  a reverse "tcp:$P" "tcp:$P"
fi

echo "== lanzando el agente → $URL"
a shell input keyevent KEYCODE_WAKEUP
a shell am start -n "$PKG/.ui.MainActivity" --es url "$URL" --es token "$TOKEN" --es adb_serial "$SERIAL" >/dev/null
echo "Listo: $SERIAL → $URL"
