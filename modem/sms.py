#!/usr/bin/env python3
"""sms.py: lee SMS del módem y, opcionalmente, marca un USSD y espera el SMS que llega como respuesta.

    sms.py leer                 # lista todos los SMS (chip y memoria del módem)
    sms.py esperar "*605#" 90   # marca el USSD y espera hasta 90 s por SMS nuevos
"""
import re
import sys
import time

sys.path.insert(0, __file__.rsplit("/", 1)[0])
from tigo import Modem, log  # noqa: E402


def ucs2(s: str) -> str:
    """En modo UCS2 el módem entrega los campos en hex UTF-16; si no lo es, se deja igual."""
    if s and len(s) % 4 == 0 and re.fullmatch(r"[0-9A-Fa-f]+", s):
        try:
            return bytes.fromhex(s).decode("utf-16-be")
        except ValueError:
            pass
    return s


def preparar(m: Modem):
    m.at("AT+CMGF=1")                    # modo texto
    m.at('AT+CSCS="UCS2"')               # acentos y emojis sin pérdida
    m.at('AT+CPMS="SM","SM","SM"')
    m.at("AT+CNMI=2,1,0,0,0")            # guardar y avisar con +CMTI al llegar


def listar(m: Modem) -> list[dict]:
    out = []
    for mem in ("SM", "ME"):
        m.at(f'AT+CPMS="{mem}","{mem}","{mem}"')
        raw = m.at('AT+CMGL="ALL"', 6)
        for h, body in re.findall(r'\+CMGL: ([^\r\n]+)\r?\n([^\r\n]*)', raw):
            f = [x.strip('"') for x in re.findall(r'"[^"]*"|[^,]+', h)]
            out.append({"mem": mem, "idx": f[0], "estado": f[1], "de": ucs2(f[2]),
                        "fecha": f[4] if len(f) > 4 else "", "texto": ucs2(body)})
    m.at('AT+CPMS="SM","SM","SM"')
    return out


def mostrar(msgs):
    if not msgs:
        print("(sin SMS)")
    for s in msgs:
        print(f"[{s['mem']} #{s['idx']}] de {s['de']} · {s['fecha']} · {s['estado']}\n{s['texto']}\n")


def main():
    cmd = sys.argv[1] if len(sys.argv) > 1 else "leer"
    with Modem() as m:
        preparar(m)
        antes = listar(m)
        if cmd == "leer":
            mostrar(antes)
            return
        codigo, espera = sys.argv[2], float(sys.argv[3]) if len(sys.argv) > 3 else 90
        vistos = {(s["mem"], s["idx"], s["texto"]) for s in antes}
        print(f"SMS ya guardados: {len(antes)}")
        r = m.ussd(codigo)
        m.cancel()
        print(f">>> {codigo}  ({r.secs}s, estado {r.status})\n{r.text}\n")
        log("ussd", enviado=codigo, status=r.status, text=r.text, secs=r.secs)
        t0 = time.monotonic()
        nuevos = []
        while time.monotonic() - t0 < espera:
            time.sleep(5)
            nuevos = [s for s in listar(m) if (s["mem"], s["idx"], s["texto"]) not in vistos]
            if nuevos:
                time.sleep(5)  # por si llega en varias partes
                nuevos = [s for s in listar(m) if (s["mem"], s["idx"], s["texto"]) not in vistos]
                break
        print(f"SMS nuevos tras {time.monotonic() - t0:.0f}s: {len(nuevos)}")
        mostrar(nuevos)
        for s in nuevos:
            log("sms", **s)


if __name__ == "__main__":
    main()
