import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// En desarrollo la UI corre en :5173 y habla con el control en :47800.
// En producción el control sirve `dist/` en el mismo puerto, así que las rutas son relativas.
const CONTROL = process.env.QHANA_CONTROL ?? "http://127.0.0.1:47800";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: {
    port: 5173,
    proxy: {
      "/api/eventos": { target: CONTROL.replace(/^http/, "ws"), ws: true },
      "/api": { target: CONTROL, changeOrigin: true },
    },
  },
  build: { outDir: "dist", sourcemap: true },
});
