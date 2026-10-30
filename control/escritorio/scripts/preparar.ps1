# Junta en src-tauri\recursos\ lo que la app lleva dentro: control (release), interfaz, recetas, APK y adb de Windows.
# Equivalente de preparar.sh para Windows. Uso (PowerShell, desde control\escritorio):  .\scripts\preparar.ps1
# Variables: $env:APK (ruta al APK; por defecto el compilado de agente-android o el de la carpeta apk\)
$ErrorActionPreference = "Stop"
$Aqui = Split-Path -Parent $PSScriptRoot
$Control = Split-Path -Parent $Aqui
$Raiz = Split-Path -Parent $Control
$R = Join-Path $Aqui "src-tauri\recursos"

Write-Host "== control (release)"
Push-Location $Control; cargo build --release --bin qhana-control; if ($LASTEXITCODE) { throw "falló cargo" }; Pop-Location

Write-Host "== interfaz"
Push-Location (Join-Path $Control "ui")
if (-not (Test-Path node_modules)) { npm install --no-audit --no-fund; if ($LASTEXITCODE) { throw "falló npm install" } }
npm run build | Out-Null; if ($LASTEXITCODE) { throw "falló npm run build" }
Pop-Location

$Apk = $env:APK
if (-not $Apk) {
  $Apk = Join-Path $Raiz "agente-android\app\build\outputs\apk\debug\app-debug.apk"
  if (-not (Test-Path $Apk)) { $Apk = Join-Path $Raiz "apk\agente.apk" }
}
if (-not (Test-Path $Apk)) { throw "Falta el APK del agente ($Apk). Compílalo con gradlew :app:assembleDebug o ponlo en apk\agente.apk" }

if (Test-Path $R) { Remove-Item -Recurse -Force $R }
New-Item -ItemType Directory -Force (Join-Path $R "ui") | Out-Null
Copy-Item (Join-Path $Control "target\release\qhana-control.exe") $R
Copy-Item -Recurse (Join-Path $Control "ui\dist") (Join-Path $R "ui\dist")
Copy-Item -Recurse (Join-Path $Control "recetas") (Join-Path $R "recetas")
Copy-Item $Apk (Join-Path $R "agente.apk")

# adb de Windows (platform-tools de Google): se baja una vez y se copian adb.exe y sus DLL.
$Pt = Join-Path $env:TEMP "platform-tools"
if (-not (Test-Path (Join-Path $Pt "adb.exe"))) {
  Write-Host "== bajando platform-tools"
  $Zip = Join-Path $env:TEMP "platform-tools.zip"
  Invoke-WebRequest "https://dl.google.com/android/repository/platform-tools-latest-windows.zip" -OutFile $Zip
  Expand-Archive $Zip -DestinationPath $env:TEMP -Force
}
foreach ($f in "adb.exe", "AdbWinApi.dll", "AdbWinUsbApi.dll") { Copy-Item (Join-Path $Pt $f) $R }
Write-Host "Listo: $R"
