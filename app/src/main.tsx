import React from "react";
import ReactDOM from "react-dom/client";
// Fonts ship inside the bundle: the app must work offline in a booth.
import "@fontsource/inter/400.css";
import "@fontsource/inter/500.css";
import "@fontsource/inter/600.css";
import "@fontsource/jetbrains-mono/500.css";
import "@fontsource/jetbrains-mono/700.css";
import "@fontsource/fredoka/600.css";
import "@fontsource/fredoka/700.css";
import "./styles/tokens.css";
import "./styles/app.css";
import { App } from "./App";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
