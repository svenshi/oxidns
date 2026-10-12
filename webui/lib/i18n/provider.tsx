/*
 * SPDX-FileCopyrightText: 2026 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

"use client";

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import {
  DEFAULT_LOCALE,
  LOCALE_STORAGE_KEY,
  WEBUI,
  getClientLocale,
  t as translate,
  type Locale,
  type TranslationParams,
} from "@/lib/i18n";
import type { I18nContextValue } from "./types";

const I18nContext = createContext<I18nContextValue | null>(null);

export function I18nProvider({ children }: { children: ReactNode }) {
  const [locale, setLocaleState] = useState<Locale>(DEFAULT_LOCALE);

  useEffect(() => {
    const storedLocale = getClientLocale();
    if (storedLocale === DEFAULT_LOCALE) return;
    const timer = window.setTimeout(() => setLocaleState(storedLocale), 0);
    return () => window.clearTimeout(timer);
  }, []);

  const setLocale = useCallback((nextLocale: Locale) => {
    setLocaleState(nextLocale);
    try {
      window.localStorage.setItem(LOCALE_STORAGE_KEY, nextLocale);
    } catch {
      // Keep the in-memory locale even if persistence is unavailable.
    }
  }, []);

  const toggleLocale = useCallback(() => {
    setLocale(locale === "zh-CN" ? "en-US" : "zh-CN");
  }, [locale, setLocale]);

  const t = useCallback(
    (key: string, params?: TranslationParams) => translate(locale, key, params),
    [locale],
  );

  const formatNumber = useCallback(
    (value: number, options?: Intl.NumberFormatOptions) =>
      new Intl.NumberFormat(locale, options).format(value),
    [locale],
  );

  const formatDateTime = useCallback(
    (value: Date | number | string, options?: Intl.DateTimeFormatOptions) =>
      new Intl.DateTimeFormat(locale, options).format(new Date(value)),
    [locale],
  );

  useEffect(() => {
    document.documentElement.lang = locale;
    document.title = t(WEBUI.metadata.title);
    const metaDescription = document.querySelector<HTMLMetaElement>(
      "meta[name='description']",
    );
    if (metaDescription) {
      metaDescription.content = t(WEBUI.metadata.description);
    }
  }, [locale, t]);

  const value = useMemo(
    () => ({
      locale,
      setLocale,
      toggleLocale,
      t,
      formatNumber,
      formatDateTime,
    }),
    [locale, setLocale, toggleLocale, t, formatNumber, formatDateTime],
  );

  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export function useI18n() {
  const context = useContext(I18nContext);
  if (!context) {
    throw new Error("useI18n must be used within I18nProvider");
  }
  return context;
}
