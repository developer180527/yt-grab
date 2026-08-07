import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";

// Self-hosted, so the UI renders correctly with no network. These used to come
// from a Google Fonts @import, which meant a packaged desktop app fell back to
// system fonts whenever it was offline — and phoned home on every launch.
import "@fontsource-variable/dm-sans";
import "@fontsource-variable/jetbrains-mono";
import "@fontsource/bebas-neue";

import "./index.css";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
