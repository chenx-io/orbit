// Layout constants and starter examples for the AI drawer (kept under lib: components/** may only export components).

import type { AiMode } from "@/data/aiTypes";

/** Drawer width bounds and default (px). */
export const AI_DRAWER_WIDTH = {
  min: 360,
  max: 760,
  default: 440,
} as const;

/** Width persistence key. */
export const AI_DRAWER_WIDTH_KEY = "orbit:ai-drawer-width";

/** Empty-state starter examples (i18n keys: clicking one sends it as the first message). */
export const AI_EXAMPLE_KEYS = [
  "ai.example.createRequest",
  "ai.example.addScripts",
  "ai.example.runAndExplain",
  "ai.example.loadTest",
  "ai.example.scenario",
] as const;

/** Work-mode cycle order (Shift+Tab cycles through them, matching mainstream agent tools). */
export const AI_MODE_ORDER = ["ask", "agent", "plan"] as const;

/** The next mode (used by the keyboard shortcut cycle). */
export function nextAiMode(mode: AiMode): AiMode {
  const idx = AI_MODE_ORDER.indexOf(mode);
  return AI_MODE_ORDER[(idx + 1) % AI_MODE_ORDER.length];
}
