/**
 * 组装结构化指南 ReactNode，供 SettingGroup / Item 的 guide= 使用。
 * 分节标签：概述 / 关联 / 位置 / 提示（见 i18n guideSection*）。
 * bindGuide(path, entry) 同时返回 guidePath，用于搜索跳转锚点。
 */

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
  /** 目录路径，如 advanced.proxyEnable → DOM data-guide-path / id */
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

  /**
   * 绑定指南路径 + 正文。展开到 SettingGroup / Item：
   * `{...bindGuide('advanced.proxyEnable', g.advanced.proxyEnable)}`
   */
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
