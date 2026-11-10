import React from "react";
import ReactDOM from "react-dom/client";
import { App } from "./app/App";
import { DesktopLyricsMenu } from "./features/lyrics/DesktopLyricsMenu";
import { DesktopLyrics } from "./features/lyrics/DesktopLyrics";
import { DesktopIsland } from "./features/island/DesktopIsland";
import { isDesktopIslandWindow } from "./lib/api";
import "./styles.css";
import { TextContextMenu } from "./components/TextContextMenu";
import { initializeTheme, ThemeProvider } from "./app/ThemeProvider";
import { initializeTypography, TypographyProvider } from "./app/TypographyProvider";

initializeTheme();
initializeTypography();

const isLyrics = new URLSearchParams(window.location.search).get("window") === "desktop-lyrics";
const isLyricsMenu = new URLSearchParams(window.location.search).get("window") === "desktop-lyrics-menu";
document.documentElement.classList.toggle("desktop-lyrics-window", isLyrics || isLyricsMenu);
document.documentElement.classList.toggle("desktop-lyrics-menu-window", isLyricsMenu);
const isIsland = isDesktopIslandWindow();
if (!isIsland && !isLyrics && !isLyricsMenu && "__TAURI_INTERNALS__" in window) {
  void import("./lib/supabaseDesktop").then(module => module.listenForSupabaseSync());
}
document.documentElement.classList.toggle("island-window", isIsland);

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ThemeProvider>
      <TypographyProvider>
      <TextContextMenu />
      {isLyricsMenu ? <DesktopLyricsMenu /> : isLyrics ? <DesktopLyrics /> : isIsland ? <DesktopIsland /> : <App />}
      </TypographyProvider>
    </ThemeProvider>
  </React.StrictMode>
);
