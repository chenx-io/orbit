import zhCN from "@/locales/zh-CN";
import enUS from "@/locales/en-US";

/** UI locales supported by the app. */
export type UiLocale = "zh-CN" | "en-US";

/** Locale assumed when nothing else is known. */
export const DEFAULT_UI_LOCALE: UiLocale = "zh-CN";

/**
 * Pure locale dictionaries, kept in one place so `lib/i18n.ts` and data-layer modules agree.
 *
 * Deliberately free of any store import: `data/seed.ts` needs localized default names at
 * module/seed time, and importing `lib/i18n.ts` there would create a cycle through the
 * Zustand store (`store` → `data/seed` → `lib/i18n` → `store`).
 */
export const localeDicts: Record<UiLocale, Record<string, string>> = {
  "zh-CN": zhCN,
  "en-US": enUS,
};

/** Narrow an arbitrary locale string to a supported one (unknown values fall back to zh-CN). */
export function normalizeLocale(locale: string | undefined): UiLocale {
  return locale === "en-US" ? "en-US" : DEFAULT_UI_LOCALE;
}

/**
 * Look up a key without touching the store.
 *
 * Falls back to `fallback` and finally to the key itself, so a missing entry stays visible in
 * the UI (and easy to grep) instead of silently rendering as an empty string.
 */
export function translateIn(
  locale: string | undefined,
  key: string,
  fallback?: string,
): string {
  const dict = localeDicts[normalizeLocale(locale)];
  return dict[key] ?? fallback ?? key;
}

/** UI locale currently in effect, for helpers that cannot read the store. */
let currentLocale: UiLocale = DEFAULT_UI_LOCALE;

/** Record the active UI locale (called by the store on locale change and on snapshot restore). */
export function setUiLocale(locale: string | undefined): void {
  currentLocale = normalizeLocale(locale);
}

/** The UI locale currently in effect. */
export function getUiLocale(): UiLocale {
  return currentLocale;
}

/**
 * Translate using the locale currently in effect — for data-layer and utility modules that
 * cannot call the `useT()` hook.
 */
export function t(key: string, fallback?: string): string {
  return translateIn(currentLocale, key, fallback);
}

/**
 * Translate and substitute positional `{0}`, `{1}`... placeholders using the locale in effect,
 * mirroring `format()` from `useT()` for non-React callers.
 */
export function tFormat(key: string, ...args: (string | number)[]): string {
  let s = translateIn(currentLocale, key);
  args.forEach((arg, i) => {
    s = s.replace(`{${i}}`, String(arg));
  });
  return s;
}
