#!/usr/bin/env bash
# Junta en src-tauri/recursos/ lo que la app lleva dentro: control (release), interfaz, recetas, APK y adb.
#   scripts/preparar.sh
set -euo pipefail
AQUI="$(cd "$(dirname "$0")/.." && pwd)"
CONTROL="$(cd "$AQUI/.." && pwd)"
RAIZ="$(cd "$CONTROL/.." && pwd)"
R="$AQUI/src-tauri/recursos"
APK="${APK:-$RAIZ/agente-android/app/build/outputs/apk/debug/app-debug.apk}"

echo "== control (release)"
(cd "$CONTROL" && cargo build --release --bin qhana-control)
echo "== interfaz"
(cd "$CONTROL/ui" && { [ -d node_modules ] || npm install --no-audit --no-fund; } && npm run build >/dev/null)
[ -f "$APK" ] || { echo "Falta el APK: cd agente-android && ./gradlew :app:assembleDebug" >&2; exit 1; }

rm -rf "$R"; mkdir -p "$R/ui"
cp "$CONTROL/target/release/qhana-control" "$R/qhana-control"
cp -R "$CONTROL/ui/dist" "$R/ui/dist"
cp -R "$CONTROL/recetas" "$R/recetas"
cp "$APK" "$R/agente.apk"

ADB="$(command -v adb || true)"
[ -n "$ADB" ] || ADB="${ANDROID_HOME:-$HOME/Library/Android/sdk}/platform-tools/adb"
if [ -x "$ADB" ]; then cp "$ADB" "$R/adb"; echo "== adb incluido ($ADB)"; else echo "== sin adb: la app lo buscará en el PATH"; fi
echo "Listo: $R"
