# qhana-venserv: reglas para agentes

Software a medida: hub de terminales (teléfonos Android hoy, módems después) para USSD, recetas por operador y
SMS, con control en Windows. No es parte del bot de trading ni del trabajo de VitaWallet.

## Orientarse
1. `README.md` (visión general y estado) → `docs/protocolo.md` (contrato: **fuente única de verdad**).
2. Luego solo la carpeta que toca: `agente-android/`, `control/`, `control/ui/`, `modem/` (prototipo viejo con AT).

## Reglas
- **Si cambias un mensaje, la API o el formato de recetas, cambia primero `docs/protocolo.md`** y después los dos
  lados (Kotlin en `agente-android/protocolo`, Rust en `control/crates/protocolo`, TS en `control/ui/src/api/tipos.ts`).
- **Nunca hagas transferencias reales ni elijas opciones que muevan dinero o contraten servicios**, salvo que Rey
  lo pida explícitamente con número y monto. Para probar: el simulador (`agente-simulado`), `simular: true`,
  `*105#` o menús de solo consulta.
- No uses `uiautomator dump` mientras la app agente corre (deja ciega a la accesibilidad).
- Teléfono de pruebas: Infinix X678B, serial `1026925361012456`, chips Tigo en slot 0 (78815232) y 1 (69759675).
- Lo viejo va a una carpeta `viejo/` (mover, nunca borrar). Archivos fechados: `AAAA-MM-DD_tema.ext`.
- Al aprender algo nuevo de un operador o del teléfono, agrégalo al README que corresponda.
