import React from "react";
import ReactDOM from "react-dom/client";
import { App } from "./app/App";
import { DesktopLyricsMenu } from "./features/lyrics/DesktopLyricsMenu";
import { DesktopLyrics } from "./features/lyrics/DesktopLyrics";
import { DesktopIsland } from "./features/island/DesktopIsland";
import { isDesktopIslandWindow } from "./lib/api";
import "./styles.css";
import { TextContextMenu } from "./components/TextContextMenu";

const isLyrics = new URLSearchParams(window.location.search).get("window") === "desktop-lyrics";
const isLyricsMenu = new URLSearchParams(window.location.search).get("window") === "desktop-lyrics-menu";
document.documentElement.classList.toggle("desktop-lyrics-window", isLyrics || isLyricsMenu);
document.documentElement.classList.toggle("desktop-lyrics-menu-window", isLyricsMenu);
const isIsland = isDesktopIslandWindow();
document.documentElement.classList.toggle("island-window", isIsland);

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <TextContextMenu />
    {isLyricsMenu ? <DesktopLyricsMenu /> : isLyrics ? <DesktopLyrics /> : isIsland ? <DesktopIsland /> : <App />}
  </React.StrictMode>
);
