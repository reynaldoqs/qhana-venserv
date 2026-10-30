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
| Consola USSD | Marca un código en un chip, muestra la conversación y convierte las opciones del menú en botones. Modo "Menús" (pantalla) o "Un paso" (API). Cancela la sesión al salir |
| Recetas | Ejecuta flujos guardados con parámetros validados. **Simular** activado por defecto; lo real pide un segundo clic. Muestra cada paso con su tiempo |
| SMS | Mensajes por teléfono; los nuevos llegan en vivo |
| Actividad | Eventos en vivo más el historial guardado en SQLite |
| Manual | Guía de usuario: formato del CSV, significado de cada estado (chip, lote, fila, verificación), cupos, chips PDA y qué hacer ante problemas. Enlazable por sección (`#manual/csv`). Si cambian los estados o el formato del CSV, actualizar `vistas/Manual.tsx` junto con `docs/protocolo.md` |

## Estructura

```
src/
├── api/          tipos.ts (espejo del contrato), cliente.ts (REST), eventos.ts (WebSocket único con reconexión)
├── componentes/  base.tsx (botones, campos, insignias…), dispositivo.tsx (tarjeta de teléfono y chip)
├── util/         datos.ts (hooks de datos, lectura de menús, formatos)
└── vistas/       Panel, ConsolaUssd, Recetas, Mensajes, Actividad, Manual
```

Colores como variables en `estilos.css`, con modo claro y oscuro automático.
