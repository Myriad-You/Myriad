import type { ReactNode } from 'react'
import type { SettingGuideEntry } from './types'
import React, { useCallback, useEffect, useMemo, useState } from 'react'
import { useI18n } from '../../../contexts/I18nContext'
import {
  getSettingGuidesCatalog,
  loadSettingGuidesCatalog,
} from './catalog'
import { SettingGuideBody } from './SettingGuideBody'

export interface GuideBinding {
  guide: ReactNode
  guidePath: string
}

export function useSettingGuide() {
  const { t, locale } = useI18n()
  const [catalog, setCatalog] = useState(() => getSettingGuidesCatalog(locale))

  useEffect(() => {
    let cancelled = false
    setCatalog(getSettingGuidesCatalog(locale))
    void loadSettingGuidesCatalog(locale).then((next) => {
      if (!cancelled) setCatalog(next)
    })
    return () => {
      cancelled = true
    }
  }, [locale])

  const labels = useMemo(
    () => ({
      what: t.config.guideSectionWhat,
      chain: t.config.guideSectionChain,
      frontend: t.config.guideSectionFrontend,
      notes: t.config.guideSectionNotes,
    }),
    [t],
  )

  const renderGuide = useCallback(
    (entry: SettingGuideEntry | undefined | null): ReactNode => {
      if (
        !entry?.what &&
        !entry?.chain &&
        !entry?.frontend &&
        !entry?.notes
      ) {
        return null
      }
      return <SettingGuideBody entry={entry!} labels={labels} />
    },
    [labels],
  )

  const bindGuide = useCallback(
    (
      path: string,
      entry: SettingGuideEntry | undefined | null,
    ): GuideBinding => ({
      guidePath: path,
      guide: renderGuide(entry),
    }),
    [renderGuide],
  )

  return { catalog, labels, renderGuide, bindGuide }
}
