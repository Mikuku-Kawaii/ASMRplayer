import React from "react";
import ReactDOM from "react-dom/client";
import { App } from "./app/App";
import { DesktopIsland } from "./features/island/DesktopIsland";
import { isDesktopIslandWindow } from "./lib/api";
import "./styles.css";

const isIsland = isDesktopIslandWindow();
document.documentElement.classList.toggle("island-window", isIsland);

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {isIsland ? <DesktopIsland /> : <App />}
  </React.StrictMode>
);
