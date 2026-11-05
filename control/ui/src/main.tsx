import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
// Fuentes empaquetadas con la app (sin Google Fonts: el control puede correr sin internet).
import "@fontsource/manrope/400.css";
import "@fontsource/manrope/500.css";
import "@fontsource/manrope/600.css";
import "@fontsource/manrope/700.css";
import "@fontsource/manrope/800.css";
import "@fontsource/jetbrains-mono/400.css";
import "@fontsource/jetbrains-mono/500.css";
import "./estilos.css";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
