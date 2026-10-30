#!/usr/bin/env bash
# Arma una carpeta autosuficiente con el control compilado (release): binario, interfaz, recetas y el APK del agente.
#   scripts/empaquetar.sh [carpeta-destino]      por defecto control/paquete
# Para usarla: entrar a la carpeta y correr ./iniciar.sh  (o ./qhana-control --apk agente.apk).
# En Windows: cargo build --release --bin qhana-control, y copiar a mano qhana-control.exe, ui/dist, recetas y el APK.
# Reempaquetar NO toca `datos/` (historial, saldos, cupos del día).
set -euo pipefail

CONTROL="$(cd "$(dirname "$0")/.." && pwd)"
RAIZ="$(cd "$CONTROL/.." && pwd)"
DEST="${1:-$CONTROL/paquete}"
APK_ORIGEN="$RAIZ/agente-android/app/build/outputs/apk/debug/app-debug.apk"

echo "== compilando el control (release)"
(cd "$CONTROL" && cargo build --release --bin qhana-control)

echo "== compilando la interfaz"
(cd "$CONTROL/ui" && { [ -d node_modules ] || npm ci; } && npm run build >/dev/null)

[ -f "$APK_ORIGEN" ] || { echo "== falta el APK; compilando el agente"; (cd "$RAIZ/agente-android" && ./gradlew :app:assembleDebug -q); }

echo "== armando $DEST"
mkdir -p "$DEST/ui" "$DEST/datos"
cp "$CONTROL/target/release/qhana-control" "$DEST/qhana-control"
rm -rf "$DEST/ui/dist" "$DEST/recetas"
cp -R "$CONTROL/ui/dist" "$DEST/ui/dist"
cp -R "$CONTROL/recetas" "$DEST/recetas"
cp "$APK_ORIGEN" "$DEST/agente.apk"

cat > "$DEST/iniciar.sh" <<'SH'
#!/usr/bin/env bash
# Arranca el control desde esta carpeta. Cualquier opción extra se pasa tal cual (./iniciar.sh --token otro).
cd "$(dirname "$0")"
command -v adb >/dev/null 2>&1 || echo "Aviso: no encontré 'adb' en el PATH; pásalo con --adb /ruta/a/adb (el control también lo busca en el SDK de Android)."
exec ./qhana-control --apk agente.apk "$@"
SH
chmod +x "$DEST/iniciar.sh" "$DEST/qhana-control"

echo "Listo: $DEST"
echo "  cd \"$DEST\" && ./iniciar.sh      → http://127.0.0.1:47800/"
