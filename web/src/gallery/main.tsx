import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { Toaster } from "../components/ui/toast";
import { applyTheme, DEFAULT_THEME, type ThemePrefs } from "../theme/state";
import { ACCENT_PRESETS, GRAYS, type AccentPreset, type Gray } from "../theme/presets";
import { Gallery } from "./Gallery";
import "../styles/app.css";

const params = new URLSearchParams(window.location.search);
const gray = params.get("gray");
const accent = params.get("accent");
const radius = Number(params.get("radius") ?? "6");
const prefs: ThemePrefs = {
  ...DEFAULT_THEME,
  gray: GRAYS.includes(gray as Gray) ? (gray as Gray) : "zinc",
  radius: Number.isFinite(radius) ? radius : 6,
  accent: ACCENT_PRESETS.includes(accent as AccentPreset) ? { kind: "preset", name: accent as AccentPreset } : DEFAULT_THEME.accent,
};
applyTheme(prefs, params.get("mode") === "dark" ? "dark" : "light");

const container = document.getElementById("root");
if (container !== null) {
  createRoot(container).render(
    <StrictMode>
      <Gallery />
      <Toaster />
    </StrictMode>,
  );
}
