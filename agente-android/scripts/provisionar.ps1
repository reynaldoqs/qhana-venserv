# Deja un teléfono listo para el agente, por USB (Windows / PowerShell).
#   .\provisionar.ps1 -Serial <serial> [-Url ws://127.0.0.1:47800/agente] [-Token dev] [-Apk ruta] [-Adb adb.exe] [-PantallaBajoDemanda]
param(
    [Parameter(Mandatory = $true)][string]$Serial,
    [string]$Url = "ws://127.0.0.1:47800/agente",
    [string]$Token = "dev",
    [string]$Apk = "",
    [string]$Adb = "adb",
    [switch]$PantallaBajoDemanda   # experimental: el agente enciende la pantalla solo al usar USSD
)
$ErrorActionPreference = "Stop"
$Pkg = "bo.qhana.agente"
$Servicio = "$Pkg/$Pkg.ussd.ServicioAccesibilidad"
if (-not $Apk) { $Apk = Join-Path $PSScriptRoot "..\app\build\outputs\apk\debug\app-debug.apk" }
if (-not (Test-Path $Apk)) { throw "No existe $Apk. Compila primero: .\gradlew.bat :app:assembleDebug" }

function A { param([Parameter(ValueFromRemainingArguments = $true)][string[]]$Args)
    $out = & $Adb -s $Serial @Args 2>&1
    if ($LASTEXITCODE -ne 0) { throw "adb $($Args -join ' ') falló: $out" }
    return ($out | Out-String).Trim()
}

$modelo = A shell getprop ro.product.model
Write-Host "== $Serial : $modelo"

Write-Host "== instalando $Apk"
A install -r -g $Apk | Out-Null

Write-Host "== permisos"
foreach ($p in "CALL_PHONE","READ_PHONE_STATE","READ_PHONE_NUMBERS","RECEIVE_SMS","READ_SMS","POST_NOTIFICATIONS") {
    try { A shell pm grant $Pkg "android.permission.$p" | Out-Null } catch { Write-Host "   (no se pudo conceder $p)" }
}

Write-Host "== accesibilidad"
$actuales = A shell settings get secure enabled_accessibility_services
if ($actuales -like "*$Servicio*") {
    Write-Host "   ya estaba activa"
} else {
    if (-not $actuales -or $actuales -eq "null") { $nuevos = $Servicio } else { $nuevos = "$actuales`:$Servicio" }
    A shell settings put secure enabled_accessibility_services $nuevos | Out-Null
}
A shell settings put secure accessibility_enabled 1 | Out-Null

# Por defecto la pantalla queda encendida con USB (probado en el Infinix X678B). Con la pantalla apagada XOS congela el proceso
# del agente y el USSD vence. -PantallaBajoDemanda es experimental: el agente la enciende solo al usar USSD.
if ($PantallaBajoDemanda) {
    Write-Host "== EXPERIMENTAL: el agente enciende la pantalla solo al usar USSD y se apaga a los 15 s"
    A shell svc power stayon false | Out-Null
    A shell settings put system screen_off_timeout 15000 | Out-Null
} else {
    Write-Host "== pantalla siempre encendida con USB (baja el brillo y usa fondo negro para cuidar el OLED)"
    A shell svc power stayon usb | Out-Null
}
Write-Host "== sin optimización de batería"
A shell dumpsys deviceidle whitelist "+$Pkg" | Out-Null
try { A shell cmd appops set $Pkg RUN_ANY_IN_BACKGROUND allow | Out-Null } catch {}
try { A shell cmd appops set $Pkg RUN_IN_BACKGROUND allow | Out-Null } catch {}

if ($Url -match '^ws://(127\.0\.0\.1|localhost):(\d+)') {
    $puerto = $Matches[2]
    Write-Host "== adb reverse tcp:$puerto -> PC:$puerto"
    A reverse "tcp:$puerto" "tcp:$puerto" | Out-Null
}

Write-Host "== lanzando el agente -> $Url"
A shell input keyevent KEYCODE_WAKEUP | Out-Null
A shell am start -n "$Pkg/.ui.MainActivity" --es url $Url --es token $Token --es adb_serial $Serial | Out-Null
Write-Host "Listo: $Serial -> $Url"
