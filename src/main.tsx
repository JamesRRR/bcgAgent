import React from "react";
import ReactDOM from "react-dom/client";
import "@fontsource/inter/400.css";
import "@fontsource/inter/600.css";
import "@fontsource/noto-sans-sc/400.css";
import "@fontsource/noto-sans-sc/600.css";
import "@fontsource/caveat/400.css";
import "./i18n";
import "./styles/globals.css";
import App from "./App";

if (localStorage.getItem("theme") === "dark") document.documentElement.classList.add("dark");

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
