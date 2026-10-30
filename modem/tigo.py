#!/usr/bin/env python3
"""tigo.py: saldo, USSD y transferencias de saldo Tigo Bolivia por el módem Huawei E303.

Uso:
    tigo saldo
    tigo transferir 69869869 12            # pide confirmación
    tigo transferir 69869869 12 --si       # sin preguntar
    tigo transferir 69869869 12 --simular  # llega hasta "Confirma?" y responde No
    tigo movimientos
    tigo ussd "*105#"                      # cualquier código, con respuestas opcionales
    tigo ussd "*66#1#" 2
    tigo log [-n 20]

Linux: solo librería estándar (termios), o pyserial si está instalado.
Windows: necesita pyserial (`py -m pip install pyserial`). Ver README.md.
"""
import argparse
import datetime as dt
import json
import os
import re
import sys
import time
from dataclasses import dataclass, asdict
from pathlib import Path

WINDOWS = os.name == "nt"
if WINDOWS:
    import msvcrt
else:
    import fcntl
    import termios
try:
    import serial
    from serial.tools import list_ports
except ImportError:
    serial = list_ports = None

BASE = Path(os.environ.get("TIGO_DIR", Path(__file__).resolve().parent))
LOG = BASE / "logs" / "tigo.jsonl"
LOCK = BASE / ".tigo.lock"
# Linux: ttyUSB2 primero, porque ModemManager suele ocupar ttyUSB0 justo después de enchufar el módem.
# Windows: se detecta solo (ver puertos_candidatos); TIGO_PORTS=COM5,COM6 lo fuerza.
PORTS = [p for p in os.environ.get("TIGO_PORTS", "" if WINDOWS else "/dev/ttyUSB2,/dev/ttyUSB0").split(",") if p]
HUAWEI_VID = 0x12D1
DENOMS = [20, 10, 5, 2, 1]          # montos que ofrece el menú de Tigo (oct-2026)
COSTO_ESPERADO = 0.20               # comisión por cada envío, sin importar el monto
MAX_POR_COMANDO = float(os.environ.get("TIGO_MAX", "100"))
PAUSA_ENTRE_ENVIOS = 3.0
DEST_RE = re.compile(r"^[67]\d{7}$")  # celular boliviano: 8 dígitos, empieza en 6 o 7
# Tigo corta tras 5 transferencias en el día (medido 26-oct-2026). Se cuenta con el registro local.
LIMITE_DIARIO = int(os.environ.get("TIGO_LIMITE_DIARIO", "5"))
LIMITE_RE = re.compile(r"maximo numero de transfer\w*\s+diarias", re.I)


# ---------------------------------------------------------------- GSM 7 bits
# El E303 rechaza el USSD en texto plano (+CME ERROR: UNKNOWN); hay que mandarlo
# "empaquetado" en 7 bits y en hex. Las respuestas también vuelven así.
def pack(s: str) -> str:
    bits = n = 0
    out = bytearray()
    for ch in s.encode():
        bits |= (ch & 0x7F) << n
        n += 7
        while n >= 8:
            out.append(bits & 0xFF)
            bits >>= 8
            n -= 8
    if n:
        out.append(bits & 0xFF)
    return out.hex().upper()


def unpack(h: str) -> str:
    bits = n = 0
    out = []
    for x in bytes.fromhex(h):
        bits |= x << n
        n += 8
        while n >= 7:
            out.append(chr(bits & 0x7F))
            bits >>= 7
            n -= 7
    return "".join(out).rstrip("\r\x00@")


def decode(h: str, dcs: int) -> str:
    if dcs == 72:  # UCS2
        return bytes.fromhex(h).decode("utf-16-be", errors="replace")
    return unpack(h)


# ---------------------------------------------------------------- módem
class ModemError(Exception):
    pass


class Abort(Exception):
    """Se cortó antes de confirmar: no se movió dinero."""


@dataclass
class Resp:
    status: int   # 0 sesión cerrada, 1 espera respuesta, 2 cancelada, -1 error, -2 timeout
    text: str
    secs: float


CUSD_RE = re.compile(r'\+CUSD:\s*(\d)(?:,"([0-9A-Fa-f]*)",(\d+))?\r?\n')
ERR_RE = re.compile(r"\+CME ERROR:[^\r\n]*|\r\nERROR\r\n")


def puertos_candidatos() -> list[str]:
    """Puertos a probar. Si TIGO_PORTS está definido se usa tal cual; en Windows se buscan los del módem."""
    if PORTS:
        return PORTS
    if list_ports is None:
        raise ModemError("Falta pyserial: py -m pip install pyserial")
    todos = list(list_ports.comports())

    def clave(p):
        d = (p.description or "").lower()
        # "PC UI Interface" es el puerto AT del Huawei; "Diagnostics" no se toca (puede colgar el módem).
        return (p.vid != HUAWEI_VID, "pc ui" not in d, p.device)

    ok = [p for p in todos if "diag" not in (p.description or "").lower()]
    return [p.device for p in sorted(ok, key=clave)]


class Modem:
    def __init__(self, ports=None):
        self.fd = None      # Linux sin pyserial
        self.ser = None     # pyserial
        errs = []
        for p in ports or puertos_candidatos():
            try:
                self._abrir(p)
            except (OSError, ValueError) as e:
                errs.append(f"{p}: {getattr(e, 'strerror', None) or e}")
                continue
            self.port = p
            if "OK" in self.at("ATE0", 1.5):
                self.at("AT+CMEE=2", 1.0)
                return
            self.close()
            errs.append(f"{p}: no responde a AT")
        raise ModemError("No hay puerto del módem disponible → " + "; ".join(errs))

    def _abrir(self, p):
        if serial is not None:
            self.ser = serial.Serial(p, 115200, timeout=0, write_timeout=5)
            return
        fd = os.open(p, os.O_RDWR | os.O_NOCTTY | os.O_NONBLOCK)
        a = termios.tcgetattr(fd)
        a[0] = a[1] = a[3] = 0
        a[2] = termios.CS8 | termios.CREAD | termios.CLOCAL
        a[4] = a[5] = termios.B115200
        termios.tcsetattr(fd, termios.TCSANOW, a)
        self.fd = fd

    def _read(self) -> str:
        out = b""
        if self.ser is not None:
            try:
                n = self.ser.in_waiting
                out = self.ser.read(n) if n else b""
            except serial.SerialException as e:
                raise ModemError(f"se perdió el puerto {self.port}: {e}")
            return out.decode(errors="replace")
        while True:
            try:
                d = os.read(self.fd, 4096)
            except BlockingIOError:
                break
            if not d:
                break
            out += d
        return out.decode(errors="replace")

    def _write(self, data: bytes):
        if self.ser is not None:
            self.ser.write(data)
            self.ser.flush()
        else:
            os.write(self.fd, data)

    def at(self, cmd: str, timeout: float = 2.0) -> str:
        self._read()
        self._write((cmd + "\r").encode())
        buf, t0 = "", time.monotonic()
        while time.monotonic() - t0 < timeout:
            buf += self._read()
            if re.search(r"\r\n(OK|ERROR|\+CME ERROR:[^\r]*)\r\n", buf):
                break
            time.sleep(0.05)
        return buf

    def ussd(self, text: str, timeout: float = 30.0) -> Resp:
        self._read()
        t0 = time.monotonic()
        self._write(f'AT+CUSD=1,"{pack(text)}",15\r'.encode())
        buf = ""
        while time.monotonic() - t0 < timeout:
            buf += self._read()
            m = CUSD_RE.search(buf)
            if m:
                txt = decode(m.group(2), int(m.group(3))) if m.group(2) else ""
                return Resp(int(m.group(1)), txt.strip(), round(time.monotonic() - t0, 2))
            e = ERR_RE.search(buf)
            if e:
                return Resp(-1, e.group().strip(), round(time.monotonic() - t0, 2))
            time.sleep(0.05)
        return Resp(-2, "TIMEOUT", round(time.monotonic() - t0, 2))

    def cancel(self):
        self.at("AT+CUSD=2", 2.0)

    def close(self):
        if self.ser is not None:
            self.ser.close()
            self.ser = None
        if self.fd is not None:
            os.close(self.fd)
            self.fd = None

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.close()


# ---------------------------------------------------------------- utilidades
BO = dt.timezone(dt.timedelta(hours=-4), "BOT")  # Bolivia, sin horario de verano


def now() -> str:
    return dt.datetime.now(BO).isoformat(timespec="milliseconds")


def log(event: str, **data):
    LOG.parent.mkdir(parents=True, exist_ok=True)
    with LOG.open("a", encoding="utf-8") as f:
        f.write(json.dumps({"ts": now(), "evento": event, **data}, ensure_ascii=False) + "\n")


def menu(text: str) -> dict:
    """'1.-Transferir por unica vez' → {'1': 'Transferir por unica vez'}"""
    return {k: v.strip() for k, v in re.findall(r"^\s*(\d+)\s*\.-\s*(.+?)\s*$", text, re.M)}


def opcion(text: str, patron: str) -> str | None:
    for k, v in menu(text).items():
        if re.fullmatch(patron, v, re.I):
            return k
    return None


def parse_saldo(t: str) -> dict:
    d = {"linea": None, "bolsas": {}, "texto": t}
    m = re.search(r"Linea:\s*(\d+)", t)
    if m:
        d["linea"] = m.group(1)
    for nombre, monto, vence in re.findall(
            r"^(Saldo [^:]+):\s*([\d.]+)\s*Bs(?:\s*vence el\s*([\d/]+ [\d:]+))?", t, re.M):
        d["bolsas"][nombre] = {"bs": float(monto), "vence": vence or None}
    d["principal"] = d["bolsas"].get("Saldo Principal", {}).get("bs")
    d["total"] = d["bolsas"].get("Saldo Total", {}).get("bs")
    return d


def saldo(m: Modem) -> dict:
    r = m.ussd("*105#")
    m.cancel()
    if r.status < 0:
        raise ModemError(f"*105# falló: {r.text}")
    s = parse_saldo(r.text)
    if s["principal"] is None:
        raise ModemError(f"No entendí la respuesta de saldo:\n{r.text}")
    return s


def enviados_hoy() -> int:
    """Transferencias exitosas de hoy según el registro (solo las que pasaron por esta herramienta)."""
    if not LOG.exists():
        return 0
    hoy = dt.datetime.now(BO).date().isoformat()
    n = 0
    for line in LOG.read_text(encoding="utf-8").splitlines():
        try:
            d = json.loads(line)
        except ValueError:
            continue
        if d.get("evento") == "transferir_envio" and d.get("estado") == "ok" and d["ts"].startswith(hoy):
            n += 1
    return n


def plan(monto: int) -> list[int]:
    """Parte el monto en los billetes del menú usando la menor cantidad de envíos.
    Con {20,10,5,2,1} el método voraz da el óptimo. Cada envío cuesta 0.20, así que
    menos envíos = menos comisión."""
    out, resto = [], monto
    for d in DENOMS:
        while resto >= d:
            out.append(d)
            resto -= d
    return out


# ---------------------------------------------------------------- transferencia
def transferir_una(m: Modem, dest: str, monto: int, simular: bool) -> dict:
    pasos = []

    def paso(enviar, desc, check=None):
        r = m.ussd(enviar)
        pasos.append({"enviado": enviar, "desc": desc, **asdict(r)})
        if r.status < 0:
            raise Abort(f"{desc}: {r.text}")
        if r.status != 1:
            raise Abort(f"{desc}: Tigo cerró la sesión → {r.text!r}")
        if check and not check(r.text):
            raise Abort(f"{desc}: respuesta inesperada → {r.text!r}")
        return r

    res = {"dest": dest, "monto": monto, "pasos": pasos, "inicio": now()}
    t0 = time.monotonic()
    try:
        r = paso("*66#1#", "abrir menú")
        if LIMITE_RE.search(r.text):
            raise Abort(f"Tigo dice que ya llegaste al máximo de transferencias del día "
                        f"(~{LIMITE_DIARIO}/día). Reintenta mañana.")
        k = opcion(r.text, r"Transferir por unica vez")
        if not k:
            raise Abort(f"no encontré 'Transferir por unica vez' en el menú: {menu(r.text)}")
        paso(k, "elegir transferir", lambda t: re.search(r"numero de destino", t, re.I))
        r = paso(dest, "número destino", lambda t: re.search(r"monto", t, re.I))
        k = opcion(r.text, rf"{monto}\s*Bs")
        if not k:
            raise Abort(f"Tigo no ofrece {monto} Bs ahora (opciones: {list(menu(r.text).values())}). "
                        "El menú solo muestra montos que el saldo principal alcanza a pagar.")
        r = paso(k, "elegir monto")
        c = re.search(r"([\d.]+)\s*Bs a (\d+) con un costo de ([\d.]+)", r.text)
        if not c or float(c.group(1)) != monto or c.group(2) != dest:
            raise Abort(f"la confirmación no coincide con {monto} Bs a {dest}: {r.text!r}")
        res["costo"] = float(c.group(3))
        si, no = opcion(r.text, r"Si"), opcion(r.text, r"No")
        if not si:
            raise Abort(f"no encontré la opción 'Si': {menu(r.text)}")
        if simular:
            if no:
                rr = m.ussd(no)
                pasos.append({"enviado": no, "desc": "simulación: responder No", **asdict(rr)})
            res.update(estado="simulado", ok=True)
            return res
        # Sin reintentos de aquí en adelante: repetir podría transferir dos veces.
        r = m.ussd(si, timeout=45)
        pasos.append({"enviado": si, "desc": "confirmar", **asdict(r)})
        if re.search(rf"transferido\s+{monto}(\.0+)?\s+a\s+{dest}", r.text, re.I):
            res.update(estado="ok", ok=True)
        else:
            res.update(estado="incierto", ok=False, detalle=r.text)
        return res
    except Abort as e:
        res.update(estado="abortado", ok=False, detalle=str(e))
        return res
    finally:
        m.cancel()
        res["segundos"] = round(time.monotonic() - t0, 2)


def cmd_transferir(a):
    dest, monto = a.destino, a.monto
    if not DEST_RE.match(dest):
        sys.exit(f"Número inválido: {dest} (8 dígitos, empieza en 6 o 7)")
    if monto != int(monto) or monto <= 0:
        sys.exit("El monto debe ser un entero positivo en Bs (Tigo no transfiere centavos)")
    monto = int(monto)
    if monto > MAX_POR_COMANDO:
        sys.exit(f"Monto {monto} supera el tope de seguridad {MAX_POR_COMANDO:g} Bs (variable TIGO_MAX)")

    partes = plan(monto)
    comision = round(COSTO_ESPERADO * len(partes), 2)
    with Modem() as m:
        s = saldo(m)
        necesita = monto + comision
        print(f"Línea {s['linea']} · saldo principal {s['principal']:.2f} Bs "
              f"(total {s['total']:.2f}; lo demás no se puede transferir)")
        print(f"Plan: {monto} Bs a {dest} en {len(partes)} envío(s): "
              f"{' + '.join(map(str, partes))}  · comisión ≈ {comision:.2f} Bs · descuenta ≈ {necesita:.2f} Bs")
        if s["principal"] + 1e-9 < necesita:
            sys.exit(f"Saldo principal insuficiente: hay {s['principal']:.2f}, se necesitan {necesita:.2f}")
        hoy = enviados_hoy()
        print(f"Transferencias hoy: {hoy}/{LIMITE_DIARIO} (según el registro local)")
        if hoy + len(partes) > LIMITE_DIARIO and not a.simular:
            sys.exit(f"Este monto necesita {len(partes)} envío(s) y hoy quedan {max(LIMITE_DIARIO - hoy, 0)}. "
                     "No empiezo para no dejarlo a medias. Prueba mañana o un monto con menos envíos.")
        if a.simular:
            print("Modo simulación: llego hasta la confirmación y respondo No.")
        elif not a.si:
            if not sys.stdin.isatty():
                sys.exit("Sin terminal para confirmar: agrega --si")
            if input("¿Continuar? escribe 's' para confirmar: ").strip().lower() not in ("s", "si", "sí"):
                sys.exit("Cancelado, no se movió nada.")

        log("transferir_inicio", dest=dest, monto=monto, plan=partes, simular=a.simular, saldo=s)
        enviado = 0.0
        for i, p in enumerate(partes, 1):
            if i > 1:
                time.sleep(PAUSA_ENTRE_ENVIOS)
            antes = s["principal"]
            r = transferir_una(m, dest, p, a.simular)
            time.sleep(1.5)
            s = saldo(m)
            r["saldo_antes"], r["saldo_despues"] = antes, s["principal"]
            if not a.simular:
                bajo = round(antes - s["principal"], 2)
                r["descontado"] = bajo
                esperado = round(p + r.get("costo", COSTO_ESPERADO), 2)
                if r["estado"] == "incierto":
                    # Sin texto claro de Tigo: el saldo decide si pasó o no.
                    r["estado"] = "ok" if abs(bajo - esperado) < 0.01 else ("no_paso" if bajo == 0 else "revisar")
                    r["ok"] = r["estado"] == "ok"
                elif r["estado"] == "ok" and abs(bajo - esperado) >= 0.01:
                    r["estado"], r["ok"] = "revisar", False
            log("transferir_envio", n=i, de=len(partes), **r)
            marca = {"ok": "✅", "simulado": "🧪"}.get(r["estado"], "❌")
            extra = f" · descontó {r['descontado']:.2f}" if "descontado" in r else ""
            print(f"  {marca} envío {i}/{len(partes)}: {p} Bs → {r['estado']} "
                  f"({r['segundos']:.1f}s{extra}) · saldo principal {s['principal']:.2f}")
            if not r["ok"]:
                print(f"     detalle: {r.get('detalle', '')}")
                print("Me detengo. Revisa con `tigo saldo` / `tigo movimientos`.")
                log("transferir_fin", dest=dest, monto=monto, enviado=enviado, estado="detenido")
                sys.exit(2)
            if not a.simular:
                enviado += p
        estado = "simulado" if a.simular else "ok"
        log("transferir_fin", dest=dest, monto=monto, enviado=enviado, estado=estado, saldo=s)
        print(f"Listo: {'simulación completa' if a.simular else f'{enviado:g} Bs enviados a {dest}'}. "
              f"Saldo principal {s['principal']:.2f} Bs, total {s['total']:.2f} Bs.")


def cmd_saldo(a):
    with Modem() as m:
        s = saldo(m)
    log("saldo", **{k: v for k, v in s.items() if k != "texto"})
    print(s["texto"])


def cmd_ussd(a):
    with Modem() as m:
        try:
            for i, txt in enumerate([a.codigo] + a.respuestas):
                r = m.ussd(txt)
                print(f">>> {txt}   ({r.secs}s, estado {r.status})\n{r.text}\n")
                log("ussd", enviado=txt, **asdict(r))
                if r.status != 1 and i < len(a.respuestas):
                    print("(la sesión se cerró, no mando el resto)")
                    break
        finally:
            m.cancel()


def cmd_movimientos(a):
    with Modem() as m:
        try:
            r = m.ussd("*66#1#")
            k = opcion(r.text, r"Ultimos movimientos")
            if not k:
                sys.exit(f"No encontré 'Ultimos movimientos' en:\n{r.text}")
            r = m.ussd(k)
            print(r.text)
            log("movimientos", **asdict(r))
        finally:
            m.cancel()


def cmd_log(a):
    if not LOG.exists():
        sys.exit("Todavía no hay registro.")
    for line in LOG.read_text(encoding="utf-8").splitlines()[-a.n:]:
        print(line)


def cmd_puertos(a):
    if list_ports is None:
        sys.exit("Falta pyserial: py -m pip install pyserial")
    for p in list_ports.comports():
        print(f"{p.device:10} {p.vid or 0:04x}:{p.pid or 0:04x}  {p.description}")
    print("Se probarían, en orden:", ", ".join(puertos_candidatos()) or "(ninguno)")


def main():
    for f in (sys.stdout, sys.stderr):
        f.reconfigure(encoding="utf-8", errors="replace")  # consola de Windows suele ser cp1252
    nombre = Path(sys.argv[0]).name
    argv = sys.argv[1:]
    if nombre in ("transferir", "saldo") and (not argv or argv[0] != nombre):
        argv = [nombre] + argv
    ap = argparse.ArgumentParser(prog="tigo", description="Saldo y transferencias Tigo por el módem USB")
    sub = ap.add_subparsers(dest="cmd", required=True)
    t = sub.add_parser("transferir", help="transferir saldo: tigo transferir 69869869 12")
    t.add_argument("destino")
    t.add_argument("monto", type=float)
    t.add_argument("--si", action="store_true", help="no pedir confirmación")
    t.add_argument("--simular", action="store_true", help="llegar a la confirmación y responder No")
    t.set_defaults(f=cmd_transferir)
    sub.add_parser("saldo", help="consultar saldo (*105#)").set_defaults(f=cmd_saldo)
    sub.add_parser("movimientos", help="últimos movimientos de transferencias").set_defaults(f=cmd_movimientos)
    u = sub.add_parser("ussd", help='código USSD libre: tigo ussd "*66#1#" 2')
    u.add_argument("codigo")
    u.add_argument("respuestas", nargs="*")
    u.set_defaults(f=cmd_ussd)
    sub.add_parser("puertos", help="listar puertos serie y cuál se usaría").set_defaults(f=cmd_puertos)
    lg = sub.add_parser("log", help="últimas líneas del registro")
    lg.add_argument("-n", type=int, default=20)
    lg.set_defaults(f=cmd_log)
    a = ap.parse_args(argv)

    LOCK.parent.mkdir(parents=True, exist_ok=True)
    with LOCK.open("w") as lk:
        for _ in range(60):
            try:
                if WINDOWS:
                    msvcrt.locking(lk.fileno(), msvcrt.LK_NBLCK, 1)
                else:
                    fcntl.flock(lk, fcntl.LOCK_EX | fcntl.LOCK_NB)
                break
            except (BlockingIOError, OSError):
                time.sleep(1)
        else:
            sys.exit("Otro comando tigo está usando el módem; reintenta en un rato.")
        try:
            a.f(a)
        except ModemError as e:
            sys.exit(f"Módem: {e}")


if __name__ == "__main__":
    main()
