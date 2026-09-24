import {
  DEFAULT_UI_LOCALE,
  localeDicts,
  normalizeLocale,
  translateIn,
  type UiLocale,
} from "@/lib/localeDict";
import zhCN from "@/locales/zh-CN";
import { useAppStore } from "@/store/useStore";

export type TKey = keyof typeof zhCN;

/**
 * Translation hook. Usage:
 *   const { t } = useT();
 *   <span>{t("common.save")}</span>
 */
export function useT() {
  const locale = useAppStore((s) => s.locale);
  const dict =
    localeDicts[normalizeLocale(locale)] ?? localeDicts[DEFAULT_UI_LOCALE];

  const t = (key: TKey, fallback?: string): string => {
    return dict[key] ?? fallback ?? key;
  };

  const format = (key: TKey, ...args: (string | number)[]): string => {
    let s = dict[key] ?? key;
    args.forEach((arg, i) => {
      s = s.replace(`{${i}}`, String(arg));
    });
    return s;
  };

  return { t, format, locale };
}

/** Non-hook version, for use in utility functions */
export function translate(
  key: TKey,
  locale: UiLocale = DEFAULT_UI_LOCALE,
): string {
  return translateIn(locale, key);
}
