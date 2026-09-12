import type { Locale, TranslationKeys } from '../i18n'
import React, {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
} from 'react'
import { formatMessage, getDefaultLocale, htmlLang, saveLocale } from '../i18n'
import {
  getCachedLocale,
  loadLocale,
} from '../i18n/loadLocale'
import { persistLocaleToAccount } from '../i18n/localeAccount'
import { currentCopy } from '../i18n/localeCopy'

interface I18nContextType {
  locale: Locale
  setLocale: (locale: Locale, options?: { persist?: boolean }) => void
  t: TranslationKeys
  format: (template: string, params: Record<string, string | number>) => string
}

function createI18nContext() {
  return createContext<I18nContextType | null>(null)
}

// Survive Vite HMR: a new createContext() makes useI18n read a different object
// than the still-mounted Provider (HomeStickerCropTip then throws).
const I18nContext: React.Context<I18nContextType | null> = import.meta.hot
  ? ((import.meta.hot.data.i18nContext ??=
      createI18nContext()) as React.Context<I18nContextType | null>)
  : createI18nContext()

if (import.meta.hot) {
  import.meta.hot.data.i18nContext = I18nContext
}

interface LocaleBundle {
  locale: Locale
  t: TranslationKeys
}

export const I18nProvider: React.FC<{ children: React.ReactNode }> = ({
  children,
}) => {
  const [locale, setLocaleState] = useState<Locale>(getDefaultLocale)
  // Swap locale and t together so the UI never shows the wrong language.
  const [bundle, setBundle] = useState<LocaleBundle | null>(() => {
    const initial = getDefaultLocale()
    const cached = getCachedLocale(initial)
    return cached ? { locale: initial, t: cached } : null
  })

  useEffect(() => {
    let cancelled = false

    if (bundle?.locale === locale) return

    loadLocale(locale)
      .then((t) => {
        if (cancelled) return
        setBundle({ locale, t })
      })
      .catch((err) => {
        console.error('[I18n] Failed to load locale:', locale, err)
        if (locale !== 'en-US') {
          void import('../utils/toastManager').then(({ showError }) => {
            showError(currentCopy().errors.localeLoadFailed)
          })
          loadLocale('en-US').then((t) => {
            if (cancelled) return
            setLocaleState('en-US')
            setBundle({ locale: 'en-US', t })
          })
        }
      })

    return () => {
      cancelled = true
    }
  }, [locale, bundle?.locale])

  // Set target locale first; swap copy with the bundle (no flash).
  const setLocale = useCallback(
    (newLocale: Locale, options?: { persist?: boolean }) => {
      setLocaleState(newLocale)
      saveLocale(newLocale)
      if (options?.persist !== false) {
        persistLocaleToAccount(newLocale)
      }
      void import('../utils/analyticsEvents').then(
        ({ trackProductEvent, AnalyticsEvents }) => {
          trackProductEvent(AnalyticsEvents.LOCALE_SWITCH, {
            target: newLocale,
            throttleMs: 3000,
          })
        },
      )
    },
    [],
  )

  // html lang tracks the loaded bundle, not the target locale.
  useEffect(() => {
    if (bundle) {
      document.documentElement.lang = htmlLang(bundle.locale)
    }
  }, [bundle])

  const format = useCallback(
    (template: string, params: Record<string, string | number>) => {
      return formatMessage(bundle?.locale ?? locale, template, params)
    },
    [bundle?.locale, locale],
  )

  const value = useMemo(() => {
    if (!bundle) return null
    return {
      locale: bundle.locale,
      setLocale,
      t: bundle.t,
      format,
    }
  }, [bundle, setLocale, format])

  // Do not mount children until the first bundle is ready.
  if (!value) {
    return null
  }

  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>
}

function fallbackI18n(): I18nContextType {
  const locale = getDefaultLocale()
  const t = currentCopy()
  return {
    locale,
    setLocale: (newLocale, options) => {
      saveLocale(newLocale)
      if (options?.persist !== false) {
        persistLocaleToAccount(newLocale)
      }
    },
    t,
    format: (template, params) => formatMessage(locale, template, params),
  }
}

export function useI18n(): I18nContextType {
  return useContext(I18nContext) ?? fallbackI18n()
}

export type { Locale, TranslationKeys }
