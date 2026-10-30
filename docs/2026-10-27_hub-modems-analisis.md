---
type: Investigacion
title: Hub de módems para transferencias de saldo — lo aprendido y el análisis del producto
description: Resumen de la conversación del 26–27 oct 2026, desde liberar el E303 hasta el diseño de un hub de 10+ módems en Windows con IA como extra.
tags: [general]
estado: borrador
timestamp: 2026-10-27
---

# Hub de módems para transferencias de saldo

Este documento resume la conversación del 26 y 27 de octubre de 2026. Empezó como "liberar un módem USB" y terminó
en el diseño de un producto: un hub de 10 o más módems en Windows que procesa CSV de transferencias o recargas.

- La parte práctica (módem, Tigo, herramienta `tigo`) está documentada con detalle en [README.md](../modem/README.md).
- Aquí van el resumen de los hallazgos, las decisiones y el análisis del producto.

---

## 1. Lo que se hizo y se aprendió con el módem real

### El módem

| Dato | Valor |
|---|---|
| Modelo | Huawei **E303**, solo 3G/2G, firmware `21.318.25.01.677` (de operador, venía de Viva) |
| Dónde | Magic PC (Linux, `aud@192.168.1.9`), `~/modem` |
| Bloqueo | **Ya estaba liberado**: `AT^CARDLOCK?` → `2,0,0`. No hizo falta código ni flashear |
| Chip | Tigo, línea 69759675 |

### Problemas encontrados, en orden

1. **El chip no se detectaba** (`+CPIN?` → `SIM failure`). No era un bloqueo, era contacto físico. Un reinicio
   por software (`AT+CFUN=1,1`) no lo arregló; reacomodar el chip sí.
2. **Permisos:** los puertos `/dev/ttyUSB*` son del grupo `dialout` y el usuario no lo tenía. Se resolvió con
   un contenedor Docker que solo ve esos puertos. Lo definitivo: `sudo usermod -aG dialout aud`.
3. **Puerto ocupado** justo después de enchufar: ModemManager toma `ttyUSB0`. `ttyUSB2` sigue libre.
4. **El USSD en texto plano falla** (`+CME ERROR: UNKNOWN`). El Huawei exige el código **empaquetado en GSM de
   7 bits y en hex** (`*105#` → `AA18AC3602`). Las respuestas llegan igual y hay que desempaquetarlas.
5. **Zona horaria:** la magic PC está en UTC. Para contar "el día" hay que usar la hora de Bolivia (UTC−4).

### Reglas de Tigo medidas (26-oct-2026)

| Regla | Detalle |
|---|---|
| Saldo | `*105#`: muestra Principal, Doble Carga (promo) y vencimientos. Consultar es gratis |
| Menú de transferencia | `*66#1#` → Transferir por única vez → número → monto → confirmar |
| Montos | Solo **1, 2, 5, 10 y 20 Bs** (montos como 12 se parten: 10 + 2) |
| Comisión | **0.20 Bs fijos por envío** |
| Qué se transfiere | Solo el **saldo Principal**; la Doble Carga no |
| Menú de montos | Solo muestra los montos que el Principal alcanza a pagar (monto + 0.20) |
| Límite | **5 transferencias por día por chip**. La sexta muestra "alcanzo el maximo numero de transferencias diarias" |
| Tiempo | Una transferencia completa dura **≈ 7 s** (el primer paso ≈ 2.3 s y cada paso siguiente ≈ 1.2 s) |
| Movimientos | Lista los envíos, sin fecha ni hora |

Prueba real: 5 transferencias de 1 Bs a 69869869, todas OK. El saldo pasó de 24 a 18 Bs (5 enviados + 1 de comisión).

### Herramienta resultante

`tigo` / `transferir` / `saldo` en la magic PC, con fuente en esta carpeta. Lo principal que hace:
- Parte el monto en la menor cantidad de envíos.
- Revisa saldo y límite diario **antes** de empezar.
- Elige las opciones por su texto, no por su posición.
- Verifica monto y número en la confirmación.
- **Nunca reintenta el "Sí"**.
- Comprueba el saldo después de cada envío.
- Tiene modo `--simular`, que llega a la confirmación y responde No.
- Guarda un registro en JSONL.

Detalle en [README.md](../modem/README.md).

---

## 2. ¿En qué sistema funciona el módem?

| Sistema | Resultado |
|---|---|
| Linux | ✅ Funciona (drivers incluidos en el sistema) |
| Windows | ✅ Drivers de Huawei y COM "PC UI Interface". `tigo.py` necesitaría pasar de `termios` a `pyserial` |
| Mac Apple Silicon | ❌ Sin driver. Solo con un experimento por USB directo (`libusb`) |

---

## 3. Decisiones del proyecto

| Decisión | Motivo |
|---|---|
| **Windows es obligatorio** | Es software a medida para un cliente |
| **Regulación resuelta** | Ya tienen permisos; no es parte del análisis |
| **Módems de distintas marcas**, no solo Huawei | Hay que soportar varias marcas desde el diseño |
| **Al menos 10 módems** en un hub | El software detecta cada uno, ve su saldo y elige con qué chip hacer cada envío |
| **Rust + Tauri + React** | Instalador chico, interfaz moderna, puerto serie estable, el compilador atrapa errores donde hay dinero |
| **IA como extra**, no como base | El producto debe funcionar sin IA |

### Lo que se pide al producto

1. Subir un CSV con números, montos y la acción a realizar.
2. Acciones configurables o seleccionables.
3. Una interfaz muy cuidada.
4. Reportes de qué salió bien y qué no, con un CSV de resultados.
5. Logs para ver qué pasó en cada caso.

---

## 4. Arquitectura propuesta (Windows)

```
┌───────────── PC Windows ─────────────────────────────────────┐
│  Servicio de Windows (Rust, corre aunque nadie inicie sesión) │
│   ├─ Detector: encuentra módems, se entera cuando enchufas    │
│   │  o desenchufas uno                                        │
│   ├─ Un "trabajador" por módem (dueño exclusivo de su COM)    │
│   ├─ Planificador: elige qué chip hace cada envío             │
│   ├─ Motor de recetas (las acciones por operador)             │
│   └─ SQLite: chips, trabajos, resultados, logs                │
│            ▲ API local (HTTP/WebSocket en 127.0.0.1)          │
│  App Tauri + React (la interfaz): CSV, panel, reportes        │
└───────────────────────────────────────────────────────────────┘
```

El **servicio** va separado de la **interfaz**: si se cierra o se cae la interfaz, los envíos siguen. Un solo MSI instala ambos.

### Tecnologías

| Pieza | Tecnología |
|---|---|
| Servicio | Rust: `tokio`, `windows-service`, `axum` |
| Módems | `serialport`, `windows` (SetupAPI y avisos de enchufar/desenchufar) |
| Datos | SQLite con `sqlx` |
| Recetas y perfiles | YAML con `serde_yaml` |
| Logs | `tracing` (archivos diarios) y tabla de eventos en SQLite |
| Interfaz | Tauri 2 + React + TypeScript + shadcn/ui + Tailwind + TanStack Table + Recharts |
| Instalador | MSI de Tauri (WiX) |

**Alternativa evaluada:** Python (Flet o PySide6). Es más rápido para empezar porque ya existe `tigo.py`, pero
el instalador es pesado (60–150 MB, a veces marcado por antivirus) y cuesta más llegar a una interfaz "increíble".

### Dos capas de configuración

**A. Perfil de módem** (según la marca y el modelo): cómo hablarle al aparato.

| | Huawei | ZTE | Quectel / SIMCom (4G) |
|---|---|---|---|
| Cuál COM acepta AT | "PC UI Interface" | "AT Port" (cuidado con "Diagnostics") | "AT Port" |
| USSD | Empaquetado GSM7 en hex | Varía según firmware | Texto plano con `AT+CSCS="GSM"` |
| ICCID | `AT^ICCID?` | `AT+ZGETICCID` | `AT+QCCID` / `AT+CICCID` |
| Modo CD virtual | Sí | Sí | No |

**B. Receta del operador** (Tigo, Entel, Viva): los pasos del menú USSD, con lo que se envía, la pantalla que se
espera y la verificación antes de confirmar. Ejemplo:

```yaml
nombre: Transferir saldo Tigo
pasos:
  - enviar: "*66#1#"
    elegir: "Transferir por unica vez"
  - esperar: "numero de destino"
    enviar: "{numero}"
  - elegir: "{monto} Bs"
  - verificar: "{monto}.00 Bs a {numero}"
    elegir: "Si"
  - exito_si: "transferido {monto}"
reglas: { comision: 0.20, montos: [1,2,5,10,20], limite_diario: 5 }
```

El software combina las dos según el módem y el chip. Agregar una marca o un operador es agregar un archivo, sin tocar el código.

### Detección de módems

1. Windows lista los COM con VID/PID y el nombre de la interfaz, y con eso se elige el perfil probable.
2. Se prueba `AT` **solo en los puertos seguros**: mandar texto a un puerto de diagnóstico puede colgar el módem.
3. `ATI` / `AT+CGMM` confirman el modelo. Se identifica el módem por su **IMEI** y el chip por su **ICCID**, nunca por el número de COM.
4. Se consulta el saldo con la receta del operador.
5. Un modelo desconocido queda en modo manual hasta crearle su perfil.

### Cómo elige el chip para cada envío

1. Solo chips del mismo operador que la receta.
2. Descarta los que no tienen saldo Principal ≥ monto + comisión, ya agotaron los envíos del día, tienen mala señal o están en error.
3. Entre los que quedan, elige por menor uso hoy y luego por mayor saldo.
4. Un monto partido (12 = 10 + 2) puede salir de dos chips distintos.
5. Lo que no entra hoy queda en cola para mañana, y se avisa en la vista previa.

Los 10 módems trabajan en paralelo. Capacidad con las reglas medidas de Tigo: **10 chips × 5 = 50 envíos/día**,
hasta 20 Bs cada uno.

### Lo que hay que mantener de la herramienta actual

- Nunca reintentar el "Sí". Si la respuesta no es clara, se decide por la diferencia de saldo.
- Elegir las opciones por su texto.
- Revisar saldo y límite antes de cada lote.
- Guardar cada estado en SQLite apenas cambia, para retomar sin duplicar si se corta la luz.

---

## 5. Problemas de Windows con muchos módems

| Problema | Solución |
|---|---|
| Límite de recursos del controlador USB (varias interfaces por módem) | Repartir en **2 o más controladores USB** (puertos traseros + tarjeta PCIe USB) |
| Modo CD virtual (Huawei, ZTE): Windows no trae `usb_modeswitch` | Dejar cada módem en **modo módem permanente** una sola vez (`AT^SETPORT` en Huawei, `AT+ZCDRUN=8` en ZTE), con un asistente "preparar módem nuevo" |
| Programas de conexión de cada marca que se apoderan de los puertos | Instalar solo los drivers |
| Windows Mobile Broadband (WwanSvc) conecta datos en los módems 4G | Desactivar la conexión automática o configurar los módems sin interfaz de datos |
| Ahorro de energía USB | Desactivar la suspensión selectiva USB y el ahorro en cada dispositivo |
| Windows Update cambia drivers | Fijar los drivers con políticas de grupo |
| Módem colgado | `AT+CFUN=1,1`; si no responde, cortar corriente con un hub con control por puerto (YKUSH, Acroname). Si no hay, avisar en la interfaz qué puerto desenchufar |
| Corriente (10 módems ≈ 5 A al transmitir) | Hubs con fuente propia de 60 W o más |
| Módems 3G (E303) | Riesgo de que el operador apague el 3G. Preferir 4G (Quectel EC25, SIMCom SIM7600) |

---

## 6. IA como extra

**Regla:** la IA aconseja y propone; **nunca mueve plata por su cuenta**. El motor de recetas ejecuta siempre con
sus verificaciones fijas, y todo cambio que propone la IA pasa por una prueba en modo simulación y por la aprobación de una persona.

### 6.1 Detectar y reparar cambios en el flujo del operador (la principal)

1. **Detección sin IA:** cada paso guarda una huella de la pantalla esperada (texto normalizado y opciones).
   Si lo que llega difiere, se registra un evento de cambio.
2. **Freno por operador:** se pausan todas las recetas de ese operador, en todos los chips, y se dispara la alerta.
   Los demás operadores siguen.
3. **Propuesta de la IA:** recibe la receta, las pantallas de siempre, la nueva y el objetivo. Devuelve una receta
   corregida como diferencia, con explicación y nivel de confianza.
4. **Validación segura:** la propuesta se prueba en un chip hasta la confirmación y responde "No". Solo pasa si
   la confirmación dice exactamente el monto y el número esperados.
5. **Aprobación:** una persona revisa diferencia, explicación y prueba, y aprueba. Las recetas tienen versiones,
   con vuelta atrás en un clic.
6. **Alertas:** en la interfaz, notificación de Windows y, opcionalmente, Telegram, WhatsApp o email.

Ejemplo: Tigo agrega "Seleccione: 1.-Saldo 2.-Megas" después del número. El detector pausa Tigo y alerta, la IA
propone insertar "elegir: Saldo", la simulación llega a la confirmación correcta, una persona aprueba y la cola sigue.

### 6.2 Otros usos

| Dónde | Qué hace la IA |
|---|---|
| Respuestas desconocidas | Clasifica textos nuevos (servicio no disponible, saldo insuficiente, promos) y sugiere la regla para reconocerlos |
| Lectura de saldos | Si cambia el formato, lo convierte a datos y propone el patrón nuevo |
| Nuevo operador o código | Explora el menú USSD en modo solo lectura y redacta la receta |
| Nuevo modelo de módem | Con `ATI` y `AT+CLAC`, arma un borrador del perfil |
| SMS de los chips | Lee los avisos del operador (cambios, suspensiones, promos) y alerta |
| Importar CSV | Reconoce columnas con otros nombres y normaliza números y montos. La validación final sigue siendo fija |
| Diagnóstico | Explica en lenguaje simple por qué falló una fila, leyendo los logs |
| Anomalías | Las estadísticas detectan y la IA explica: latencia que sube, chip que falla, operador caído |
| Reportes | Resumen diario en texto |
| Asistente | Consultas y acciones en lenguaje natural, siempre con confirmación |
| Recargas de chips | Sugiere qué chips recargar y cuánto según el consumo |

**Sin IA:** elegir el chip, verificar monto y número, y decidir si una transferencia pasó. Ahí el código fijo es
más seguro, más rápido y se puede auditar.

### 6.3 Principios

- El producto funciona sin IA. Sin conexión, solo se pierden las sugerencias.
- Una persona aprueba todo cambio de receta o perfil.
- Los números se ocultan (`6986****`) antes de enviarlos a la IA.
- La IA solo se llama ante eventos, no en cada transferencia: Claude Haiku para clasificar, Claude Sonnet u Opus para reparar.
- Opción local para clasificar (Ollama o llama.cpp en Windows).
- Guardar cada pantalla USSD vista (sin datos personales) arma un mapa de los menús de cada operador, que mejora la detección y las propuestas.

**Para la primera versión:** detección de cambios con propuesta de receta, clasificación de respuestas
desconocidas y lectura de SMS del operador.

---

## 7. Plan sugerido

1. **Prueba de compatibilidad** (lo más riesgoso primero): herramienta Rust para Windows que lista los módems,
   identifica modelo y puerto AT, lee IMEI, ICCID y señal, y consulta el saldo. Con eso se arma la lista de módems compatibles.
2. **Servicio:** perfiles, recetas, planificador, SQLite y **módem simulado** (responde como Tigo, incluyendo
   límite diario, timeouts y respuestas raras) para desarrollar sin gastar saldo.
3. **Interfaz:** panel de chips, subir CSV, vista previa (validación, montos partidos, comisiones, avisos de
   saldo y límite), ejecución en vivo, reportes y CSV de resultados, y el diálogo USSD de cada fila.
4. **Prueba de un día** con 10 módems reales: cuelgues, tiempos y reinicios.
5. **IA**, en el orden de 6.3.

Formato propuesto del CSV de resultados:
`numero, monto, accion, estado, enviado, comision, chip_origen, saldo_antes, saldo_despues, hora, detalle`

---

## 8. Preguntas abiertas

- ¿Qué marcas y modelos de módem se van a usar? Define el trabajo de la prueba de compatibilidad.
- ¿Qué operadores además de Tigo, y qué acciones (transferir, recargar, paquetes)?
- Tigo: ¿a qué hora se reinicia el límite diario? ¿Hay límite semanal, mensual o en Bs? ¿El número que recibe
  también tiene límite? ¿Responder "No" en la confirmación cobra algo?
- ¿Seguirá Tigo con 3G? Si no, los E303 no sirven.
- ¿Quién administra el sistema y por qué canal quiere las alertas?
