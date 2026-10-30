---
type: Investigacion
title: Módem Huawei E303 + chip Tigo en la magic PC
description: Cómo usar el módem USB para consultar saldo, mandar USSD y transferir saldo Tigo, y todo lo aprendido al hacerlo.
tags: [general]
estado: probando
timestamp: 2026-10-26
---

# Módem Huawei E303 + Tigo (magic PC)

## Documentos

- [2026-10-27_hub-modems-analisis.md](../docs/2026-10-27_hub-modems-analisis.md): resumen de todo lo aprendido y
  análisis del producto (hub de 10+ módems en Windows, Rust + Tauri, recetas por operador, IA como extra).

## Dónde vive

| | |
|---|---|
| Fuente (esta carpeta) | `~/Projects/qhana-venserv/modem` en la Mac |
| Donde corre | `~/modem` en la magic PC (`aud@192.168.1.9`), que tiene el módem enchufado |
| Registro vivo | `~/modem/logs/tigo.jsonl` en la magic PC. `registro/` aquí guarda copias |

```
qhana-venserv/
├── README.md        este documento
├── CLAUDE.md        guía corta para agentes
├── tigo             lanzador (python3 directo o Docker)
├── tigo.py          la herramienta
├── registro/        copias del log de la magic PC
└── viejo/           scripts de la primera prueba (at.py, ussd.py, gsm7.py, transfer.py) y su log
```

Desplegar cambios a la magic PC:

```bash
scp ~/Projects/qhana-venserv/modem/{tigo,tigo.py,sms.py,README.md} aud@192.168.1.9:modem/
```

Traer el registro actualizado:

```bash
scp aud@192.168.1.9:modem/logs/tigo.jsonl ~/Projects/qhana-venserv/modem/registro/$(date +%F)_tigo.jsonl
```

Correr desde la Mac sin entrar a la magic PC:

```bash
ssh aud@192.168.1.9 '~/.local/bin/tigo saldo'
ssh -t aud@192.168.1.9 '~/.local/bin/transferir 69869869 12'   # -t para poder confirmar
```

## Uso rápido

```bash
saldo                                  # = tigo saldo  (*105#)
transferir 69869869 12                 # muestra el plan y pide confirmar
transferir 69869869 12 --si            # sin preguntar
transferir 69869869 7 --simular        # recorre el menú y en "Confirma?" responde No
tigo movimientos                       # últimas transferencias hechas
tigo ussd "*105#"                      # cualquier código USSD
tigo ussd "*66#1#" 2                   # código + respuestas a los menús
tigo log -n 20                         # registro
```

`tigo`, `transferir` y `saldo` son symlinks en `~/.local/bin` hacia `~/modem/tigo`
(entran al PATH al iniciar sesión; desde un `ssh host 'cmd'` usa `~/.local/bin/tigo`).

### Qué hace `transferir 69869869 12`

1. Valida el número (8 dígitos, empieza en 6 o 7) y el monto (entero, ≤ `TIGO_MAX`, por defecto 100).
2. Consulta el saldo (`*105#`).
3. **Parte el monto en los montos que ofrece el menú** con la menor cantidad de envíos,
   porque cada envío cobra 0.20: `12 → 10 + 2`, `37 → 20 + 10 + 5 + 2`.
4. Frena antes de empezar si el **saldo principal** no alcanza para monto + comisiones, o si
   el **límite diario** (5 envíos) no deja completar todos los envíos. Así nunca queda a medias por esas causas.
5. Pide confirmación (salvo `--si`).
6. Por cada envío recorre el menú **leyendo las opciones por su texto, no por el número de posición**,
   y verifica que la pantalla "Confirma transferir … X Bs a NÚMERO" diga exactamente el monto
   y el número. Si algo no cuadra, cancela la sesión: nada se movió.
7. Después de cada envío vuelve a consultar el saldo y comprueba que bajó en monto + 0.20.
   Si no cuadra, se detiene.
8. Todo queda en `~/modem/logs/tigo.jsonl` (hora de Bolivia, UTC−4).

## En Windows

El producto es el control en Rust (`control/`, crate `modem`); `tigo.py` queda como herramienta de diagnóstico.

**Primer día, con el control (lo normal):**

```powershell
cd control
cargo run --release --bin qhana-control -- --sin-adb
# módem no aparece solo:  --modem-puertos COM5      (o --modem-todos-usb)
curl.exe http://127.0.0.1:47800/api/dispositivos       # el id es el IMEI del módem
curl.exe -X POST http://127.0.0.1:47800/api/ussd/iniciar -H "content-type: application/json" `
  -d '{\"dispositivo\":\"<IMEI>\",\"slot\":0,\"codigo\":\"*105#\"}'
```

Receta de solo consulta: `POST /api/recetas/<nombre>/ejecutar` con `simular: true` (ver `control/recetas`).

**Diagnóstico con Python** (si algo no cuadra; requiere `py -m pip install pyserial`):

```powershell
py modem\tigo.py puertos       # lista COM con VID:PID y cuál se probaría
py modem\tigo.py saldo         # *105#
set TIGO_PORTS=COM5             # fuerza el puerto
```

Antes de enchufar: instalar **solo el driver** del módem (no el programa de conexión de la marca, que se apodera
de los puertos). El E303 debe verse como `12d1:1506` con un puerto "PC UI Interface". Si Windows lo muestra como
CD-ROM (`12d1:1446`), falta cambiarlo a modo módem una vez (`AT^SETPORT`).
Desactivar la suspensión selectiva USB. Detalle de los problemas con muchos módems: `docs/2026-10-27_hub-modems-analisis.md`, sección 5.

**Sin probar con hardware todavía:** todo el camino en Windows (COM reales), el perfil de texto plano para
Quectel/SIMCom, y la lectura de `AT+CNUM`/ICCID en otros modelos. Los tests del crate (`cargo test -p modem`) cubren
GSM7, el protocolo AT contra un módem simulado y las sesiones de menú.

## Cómo se habla con el módem

| Dato | Valor |
|---|---|
| Modelo | Huawei **E303** (HW `CH1E3531SM`), solo 3G/2G |
| Firmware | `21.318.25.01.677` (de operador; venía con APN de Viva `3g.nuevatel.com`) |
| IMEI | 862567020947870 |
| USB | `12d1:1506`, ya en modo módem; no necesita `usb_modeswitch` |
| Puertos | `ttyUSB0` y `ttyUSB2` responden AT; `ttyUSB1` no (es el de datos). Red: `wwx001e101f0000` |
| Bloqueo | `AT^CARDLOCK?` → `2,0,0` = **liberado**. Acepta Tigo aunque venía de Viva |
| Chip | Tigo, línea **69759675** |
| Señal | `AT+CSQ` 24/31 (≈ −65 dBm), 3G WCDMA, red local |

### Cosas que costaron descubrir

- **El USSD en texto plano no funciona**: `AT+CUSD=1,"*105#",15` responde `+CME ERROR: UNKNOWN`.
  Hay que mandarlo **empaquetado en GSM de 7 bits y en hex**: `*105#` → `AA18AC3602`.
  Las respuestas también llegan así (aunque digan DCS 8 o 15) y hay que desempaquetarlas.
  Funciones `pack()` / `unpack()` en `tigo.py`.
- La respuesta del USSD **llega sola, unos segundos después** del `OK` (`+CUSD: <estado>,"<hex>",<dcs>`).
  Hay que esperarla leyendo el puerto. Estado `1` = Tigo espera respuesta (menú), `0` = sesión terminada.
  Para seguir en un menú se manda otro `AT+CUSD=1,"<respuesta empaquetada>",15`.
  Al terminar conviene `AT+CUSD=2` para cerrar la sesión.
- **El chip no se detectaba** (`+CPIN?` → `SIM failure`, `^SYSINFOEX` con estado de SIM 255) hasta que
  se reacomodó físicamente. `AT+CFUN=1,1` (reinicio del módem) **no** lo arregló: era contacto.
- **Justo después de enchufar el módem, `ttyUSB0` aparece "ocupado"** (`Device or resource busy`):
  ModemManager lo está revisando. `ttyUSB2` sigue libre; por eso la herramienta prueba primero ese.
  ModemManager igual termina diciendo `No modems were found`.
- **Permisos**: los puertos son `root:dialout` y `aud` no está en `dialout` (sudo pide contraseña).
  El lanzador `tigo` usa entonces un contenedor Docker (`freqtradeorg/freqtrade:2026.2`, ya descargado)
  que solo recibe los `/dev/ttyUSB*`, corriendo como `aud` con el grupo dialout.
  Para no depender de Docker: `sudo usermod -aG dialout aud` y volver a entrar por SSH; el lanzador lo
  detecta y corre `python3` directo.
- La magic PC está en **UTC**; Bolivia es UTC−4 sin horario de verano. La herramienta usa hora de Bolivia fija.

## Lo que aprendimos de Tigo (medido el 26-oct-2026)

### Saldo `*105#`

```
Linea:69759675
Saldo Total: 24 Bs vence el 25/12/2026 23:59:59
Saldo Principal: 14 Bs vence el 25/12/2026 23:59:59
Saldo Doble Carga: 10 Bs vence el 25/12/2026 00:00:00
```

- Una recarga **corre el vencimiento** (pasó del 19/12 al 25/12 con una recarga de 1 Bs).
- Hubo promo **Doble Carga**: una recarga de 10 Bs dio 10 Bs extra en una bolsa aparte, que vence unas horas antes.
- La consulta tarda 3–6 s y no cuesta nada (el saldo no bajó al consultar).

### Transferencias `*66#1#`

Menú, paso a paso (lo que respondió Tigo):

| Paso | Envío | Respuesta de Tigo | Tiempo |
|---|---|---|---|
| 1 | `*66#1#` | `Bienvenido al servicio de transferencias` · `1.-Transferir por unica vez` · `2.-Ultimos movimientos` | 2.1–2.5 s |
| 2 | `1` | `Ingrese numero de destino` | 1.1–1.3 s |
| 3 | `69869869` | `Ingrese monto` · `1.-1 Bs` `2.-2 Bs` `3.-5 Bs` `4.-10 Bs` `5.-20 Bs` | 1.1–1.3 s |
| 4 | `1` | `Confirma transferir por unica vez 1.00 Bs a 69869869 con un costo de 0.20?` · `1.-Si` `2.-No` | 1.3 s |
| 5 | `1` | `Le ha transferido 1.00 a 69869869` (sesión cerrada) | 1.3 s |

Una transferencia completa dura **≈ 7 s**. Se hicieron 5 seguidas, con 3 s de pausa entre ellas, sin errores.

Reglas observadas:

- **Comisión fija de 0.20 Bs por envío**, sin importar el monto. En 1 Bs es el 20%; en 20 Bs, el 1%.
  Conviene siempre pocos envíos grandes.
- **Montos fijos**: solo 1, 2, 5, 10 y 20 Bs. Montos como 12 Bs hay que partirlos en varios envíos.
- **Solo se transfiere el Saldo Principal.** La bolsa Doble Carga no se tocó.
- **El menú de montos solo muestra lo que el saldo principal alcanza a pagar** (monto + 0.20).
  Con 12.80 Bs de principal desapareció la opción de 20 Bs.
- **Límite: 5 transferencias por día.** La sexta abre así:
  `Bienvenido al servicio de transferencias / Usted alcanzo el maximo numero de transferecias diarias / 1.-Ultimos movimientos`.
  Falta confirmar si se reinicia a medianoche de Bolivia y si además hay un tope en Bs.
- **Últimos movimientos** (`*66#1#` → opción por texto) lista los envíos: `1.-1.00 Bs a 69869869`, etc.
  No muestra fecha ni hora.

### Historial de saldo del 26-oct

| Momento | Total | Principal | Doble Carga |
|---|---|---|---|
| Primera consulta | 3 | 3 | — |
| Tras recarga 1 Bs | 4 | 4 | — |
| Tras recarga 10 Bs (+promo) | 24 | 14 | 10 |
| Tras 5 × 1 Bs a 69869869 | 18 | 8 | 10 |

## Análisis: `transfer.py` (prueba) vs `tigo.py` (herramienta)

| `transfer.py` (prueba del 26-oct) | `tigo.py` |
|---|---|
| Elegía opciones por posición fija (`1`, `1`, `1`) | Busca la opción por su texto ("Transferir por unica vez", "10 Bs", "Si") |
| Solo 1 Bs, N veces | Cualquier monto entero, partido en la menor cantidad de envíos |
| No miraba el saldo antes | Comprueba saldo principal y límite diario **antes** de empezar |
| El chequeo falló con "1.00 Bs" | Compara el monto como número y exige número destino + opción "Si" |
| Esperas fijas (`sleep`) | Lee hasta que llega el `+CUSD`; con tiempo máximo |
| Un solo puerto | Prueba `ttyUSB2` y luego `ttyUSB0` |
| Sin candado | `flock`: dos comandos no pueden usar el módem a la vez |
| Dependía de la imagen del bot vía `docker inspect` | Lanzador con imagen fija, o `python3` directo si hay permisos |
| Log al final en un JSON | Registro JSONL evento por evento (sobrevive a un corte) |
| — | **Nunca reintenta el "Si"**: si no llega respuesta clara, decide por la diferencia de saldo (`ok` / `no_paso` / `revisar`) |
| — | Reconoce el mensaje de límite diario |

Los scripts de prueba (`transfer.py`, `at.py`, `ussd.py`, `gsm7.py`, `transfer_log.json`) quedaron en `~/modem/viejo/`.

## Pendiente

- Probar `transferir 69869869 7 --simular` cuando se reinicie el cupo diario: el camino hasta "No" no se
  pudo recorrer completo el 26-oct porque el límite ya estaba agotado.
- Confirmar a qué hora se reinicia el límite diario y si el "No" de la confirmación cobra algo.
- `enviados_hoy()` solo cuenta lo que pasó por esta herramienta; transferencias hechas desde un
  celular con este chip no se ven (Tigo frena igual y el script lo reconoce).
- Datos móviles: el APN sigue en `3g.nuevatel.com` (Viva). Para Tigo sería `internet.tigo.bo`.
  Si se configura, que sea como **respaldo** con métrica alta, para no mandar el tráfico de los bots por el chip.
