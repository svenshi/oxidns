/*
 * SPDX-FileCopyrightText: 2026 Sven Shi
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

export const LOCALES = ["zh-CN", "en-US"] as const;

export type Locale = (typeof LOCALES)[number];

export type TranslationParams = Record<
  string,
  string | number | boolean | null | undefined
>;

export type TranslationTree = {
  readonly [key: string]: string | TranslationTree;
};

export type LocaleResourceShape<T> = {
  readonly [K in keyof T]: T[K] extends string
    ? string
    : T[K] extends object
      ? LocaleResourceShape<T[K]>
      : never;
};

export interface I18nContextValue {
  locale: Locale;
  setLocale: (locale: Locale) => void;
  toggleLocale: () => void;
  t: (key: string, params?: TranslationParams) => string;
  formatNumber: (value: number, options?: Intl.NumberFormatOptions) => string;
  formatDateTime: (
    value: Date | number | string,
    options?: Intl.DateTimeFormatOptions,
  ) => string;
}
