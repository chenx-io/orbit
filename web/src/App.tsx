import { useEffect } from "react";
import { Toaster } from "sonner";
import { AppShell } from "@/components/layout/AppShell";
import { AppModuleShell } from "@/components/layout/AppModuleShell";
import { WorkspacePicker } from "@/components/layout/WorkspacePicker";
import { GlobalOverlays } from "@/components/layout/GlobalOverlays";
import { useAppStore } from "@/store/useStore";
import { initPersistence, saveNow } from "@/lib/persistence";

function useTheme() {
  const theme = useAppStore((s) => s.theme);

  useEffect(() => {
    const root = document.documentElement;
    if (theme === "dark") {
      root.classList.add("dark");
    } else if (theme === "light") {
      root.classList.remove("dark");
    } else {
      // system: follow OS preference
      const mq = window.matchMedia("(prefers-color-scheme: dark)");
      const apply = () => {
        if (mq.matches) root.classList.add("dark");
        else root.classList.remove("dark");
      };
      apply();
      mq.addEventListener("change", apply);
      return () => mq.removeEventListener("change", apply);
    }
  }, [theme]);
}

export default function App() {
  const activeModule = useAppStore((s) => s.activeModule);
  const activeWorkspaceId = useAppStore((s) => s.activeWorkspaceId);

  useTheme();

  // Local snapshot persistence: load on startup → restore store → subscribe to autosave (debounced)
  useEffect(() => {
    void initPersistence(useAppStore);
    // Clean up leftover online auth cache from the past (the account system was removed)
    try {
      localStorage.removeItem("orbit.online.auth");
    } catch {
      // Ignore when localStorage is unavailable
    }
  }, []);

  // Ctrl/Cmd+S: save immediately
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "s") {
        e.preventDefault();
        saveNow();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // Disable the webview's default context menu (Inspect / Reload and other debug items);
  // Components with their own context menus (collection tree, etc.) call preventDefault themselves, so we don't intercept here
  useEffect(() => {
    const onContextMenu = (e: MouseEvent) => {
      if (!e.defaultPrevented) e.preventDefault();
    };
    document.addEventListener("contextmenu", onContextMenu);
    return () => document.removeEventListener("contextmenu", onContextMenu);
  }, []);

  // App-level modules (distributed / plugins): don't depend on a workspace, peer to AppShell / WorkspacePicker
  const isAppModule =
    activeModule === "distributed" || activeModule === "plugins";

  return (
    <>
      {isAppModule ? (
        <AppModuleShell />
      ) : activeWorkspaceId ? (
        <AppShell />
      ) : (
        <WorkspacePicker />
      )}
      {/* Global overlays (Mock / import / environment management) */}
      <GlobalOverlays />
      {/* Global toast (visible in both Tauri and the browser) */}
      <Toaster richColors position="bottom-right" />
    </>
  );
}
