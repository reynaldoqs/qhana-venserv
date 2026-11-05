# Interfaz del control (React + Vite + TypeScript + Tailwind)

Panel web que habla con el control por la API del contrato ([docs/protocolo.md](../../docs/protocolo.md)).
En producción el control sirve `dist/` en el mismo puerto (47800). Más adelante se empaqueta como app de
escritorio con Tauri sin cambiar este código.

```bash
npm install
npm run dev      # http://localhost:5173 (pasa /api al control en 127.0.0.1:47800; otro: QHANA_CONTROL=http://host:puerto)
npm run build    # genera dist/, que sirve el control
```

## Vistas

| Vista | Qué hace |
|---|---|
| Teléfonos | Tarjetas por teléfono: chips con su estado (lista, pide PIN, sin voz…), batería, accesibilidad, serial USB. Lista aparte los teléfonos USB que aún no tienen la app conectada |
| Consola USSD | Marca un código en un chip, muestra la conversación y convierte las opciones del menú en botones. Modo "Menús" (pantalla) o "Un paso" (API). Cancela la sesión al salir. «Guardar como receta» arma un borrador con la sesión (los PIN marcados van como `{pin}`) |
| Lotes | Lista paginada con pestañas (activos, con problema, terminados, simulaciones, archivados), búsqueda y los lotes en marcha fijados arriba. Nuevo lote: primero el tipo (Transferencias · Recargas de varios operadores · Desde chips elegidos, cada uno con su plantilla CSV), luego el CSV; tras validar muestra el plan por operador → chip (envíos y Bs por chip, lo que no entra) y, si es real, pide confirmar con los operadores, los chips y cuántos teléfonos van en paralelo (y avisa si es igual a otro lote real reciente). Detalle: una acción principal (Iniciar/Pausar/Reanudar), menú «⋯» para cancelar o archivar, reintentos en la franja de problemas, «Carriles por teléfono» (qué hace cada chip en vivo, `GET /api/lotes/{id}/carriles` cada 3 s + eventos) y filas por páginas |
| Recetas | Lista con búsqueda, filtro por operador e insignias (mueve saldo, dealer, sin probar, último uso). Ficha por receta (`#recetas/<nombre>[/pasos\|uso\|versiones]`): **Ejecutar** (simular por defecto; lo real pide confirmar y avisa si la versión está sin probar), **Pasos** (solo lectura), **Uso** (estadísticas y últimas ejecuciones) y **Versiones** (historial, diferencias, restaurar). **Editor** (`#recetas/<nombre>/editar`, `#recetas/nueva[?desde=]`): visual y YAML sobre el mismo texto; cada cambio visual se aplica al documento YAML con la librería `yaml` (`parseDocument`/`setIn`…), así los comentarios y el estilo del archivo se conservan. Edita `nombres_anteriores` (fichas; un enlace `#recetas/<nombre viejo>` abre la receta de hoy). Valida en vivo, prueba un paso contra una pantalla, guarda con `version_base` (conflictos) y frena la salida con cambios sin guardar |
| SMS | Mensajes por teléfono, filtro por SIM, búsqueda y «Cargar más»; los nuevos llegan en vivo |
| Actividad | Eventos en vivo más el historial guardado en SQLite |
| Manual | Guía de usuario: formato del CSV, significado de cada estado (chip, lote, fila, verificación), cupos, chips dealer y qué hacer ante problemas. Enlazable por sección (`#manual/csv`). Si cambian los estados o el formato del CSV, actualizar `vistas/Manual.tsx` junto con `docs/protocolo.md` |

## Estructura

```
src/
├── api/          tipos.ts (espejo del contrato), cliente.ts (REST, token, `mensajeError`, descargas), eventos.ts (WebSocket único con reconexión)
├── componentes/  base.tsx (botones, campos, insignias, Dialogo, MenuAcciones, Segmentado…), dispositivo.tsx (tarjeta de teléfono y chip)
├── util/         datos.ts (hooks de datos, agrupar recargas, preferencias, formatos)
└── vistas/       Panel, ConsolaUssd, Mensajes, Actividad, Manual, lotes/ (Lotes, NuevoLote, DetalleLote + detalle/),
                  recetas/ (Recetas, ListaRecetas, Ficha, Ejecutar, PasosLectura, Uso, Versiones, diff, modelo.ts = YAML, editor/)
```

- Navegación con cambios sin guardar: `util/guardia.ts` (el editor la instala; `useVista` en App.tsx la consulta).
- Errores: todo lo que lanza `api` es un `FalloApi`; para mostrarlo, `mensajeError(e)` (traduce el código al español).
- Token: si el control escucha en la red y pide token (`GET /api/salud` → `requiere_token`), la interfaz lo pide una vez
  y lo guarda en el navegador. Va como `Authorization: Bearer` (y `?token=` solo en el WebSocket de eventos); las
  descargas CSV se piden con la cabecera y se guardan desde el navegador. Un `401` vuelve a pedirlo.
- Fuentes empaquetadas (`@fontsource/manrope`, `@fontsource/jetbrains-mono`): funciona sin internet.

Colores como variables en `estilos.css`. Solo hay tema oscuro.
