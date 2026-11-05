---
type: Proyecto
title: qhana-venserv — hub de terminales para USSD y SMS
description: Control en Windows que maneja teléfonos Android (y más adelante módems) para ejecutar USSD, recetas por operador y leer SMS.
tags: [general]
estado: probando
timestamp: 2026-10-29
---

# qhana-venserv

Sistema para manejar muchos chips desde un PC: marcar USSD, contestar menús, ejecutar flujos guardados
("recetas") como consultar saldo o transferir, y leer SMS. Con reportes y registro de todo.

```
┌──────────── PC (Windows en producción; se desarrolla en Mac) ───────────┐
│  control/  servidor Rust :47800                                          │
│   ├─ agentes por WebSocket   ├─ ADB (USB)     ├─ motor de recetas        │
│   ├─ SQLite (historial)      └─ API + eventos ─► control/ui (React)      │
└──────────────▲──────────────────────────────────────────────────────────┘
               │ WebSocket (hoy por USB con `adb reverse`; mañana por Wi-Fi: solo cambia la URL)
┌──────────────┴─────────────┐
│ agente-android/ (Kotlin)   │  × N teléfonos, 2 chips cada uno
│ USSD por pantalla o API,   │
│ SMS, estado de los chips   │
└────────────────────────────┘
```

| Carpeta | Qué es |
|---|---|
| [docs/protocolo.md](docs/protocolo.md) | **El contrato**: mensajes agente ↔ control, API para la interfaz, formato de recetas. Fuente única de verdad |
| [agente-android/](agente-android/README.md) | App Android "agente" (Kotlin, Gradle multi-módulo) y scripts de provisión |
| [control/](control/README.md) | Servidor de control (Rust, workspace de 8 crates), recetas YAML, lotes por CSV, simulador. CSV de ejemplo en `control/ejemplos/` |
| [control/ui/](control/ui/README.md) | Interfaz (React + Vite + Tailwind), servida por el control |
| [modem/](modem/README.md) | Prototipo anterior con módem USB Huawei por comandos AT (lo que se aprendió de Tigo está ahí) |
| [docs/2026-10-27_hub-modems-analisis.md](docs/2026-10-27_hub-modems-analisis.md) | Análisis del producto: escalar, Windows, IA |

## Instalador de escritorio (Mac y Windows)

`control/escritorio/` empaqueta todo (control, interfaz, recetas, APK y `adb`) en una app Tauri. Los instaladores se generan en
GitHub Actions (`.github/workflows/instalador.yml`): **Actions → Instalador → Run workflow**, y se descargan de la página del run
(`qhana-windows`: `.exe`, `qhana-mac`: `.dmg`, `agente-apk`). Detalles y compilación local en [control/README.md](control/README.md).

## Arranque rápido (desarrollo en Mac)

```bash
# 1. Control + interfaz
cd control/ui && npm install && npm run build && cd ..
cargo run --bin qhana-control            # http://127.0.0.1:47800

# 2. Teléfono por USB (depuración USB activada)
cd ../agente-android
./gradlew :app:assembleDebug
./scripts/provisionar.sh <serial>        # instala, da permisos, activa accesibilidad, adb reverse, abre la app

# Sin teléfono: un teléfono simulado (Tigo con saldo, límite diario, etc.)
cd ../control && cargo run --bin agente-simulado
```

Abrir `http://127.0.0.1:47800`.

## Estado (29-oct-2026)

Probado de punta a punta con un **Infinix X678B (Android 14) con dos chips Tigo**:

| Prueba | Resultado |
|---|---|
| Teléfono aparece en el panel con sus chips, batería y accesibilidad | ✅ vinculado a su serial USB |
| Receta `tigo-saldo` en chip 1 y chip 2 | ✅ 3.4 s cada una (2.4 s el USSD) |
| Receta `tigo-movimientos` (menú de 2 pasos) | ✅ 3.4 s |
| USSD manual con menús (iniciar/responder/cancelar) | ✅ 0.8–1.5 s por paso |
| Modo API (`sendUssdRequest`) con Tigo | ❌ Tigo deja la sesión abierta y Android lo da por fallido: se usa modo pantalla |
| Transferencias reales | No probadas en el teléfono (sí en el simulador). El motor nunca reintenta la confirmación |
| **Lotes por CSV**: validar con saldos reales de los chips | ✅ 7.1 s para 9 filas y 2 chips; detecta número inválido, centavos, operador desconocido, operador sin receta, repetidos, envío al propio chip, falta de saldo o cupo |
| Lotes: ejecución completa, reparto entre chips, CSV de resultado | ✅ contra teléfonos simulados (incluye un chip que llega a su límite y la fila se reintenta en otro) |

## Pendiente

- Probar en Windows: compilar el control, `provisionar.ps1`, correrlo como servicio.
- Probar un lote real (pequeño) con el teléfono. La validación y la consulta de saldos ya se probaron con él.
- Lectura de SMS con un SMS real; ICCID (requiere modo "dueño del dispositivo").
- Empaquetar la interfaz con Tauri.
- IA (ver el análisis): detectar cambios de menú del operador y proponer recetas.
