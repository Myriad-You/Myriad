import type { MeropeHerResponse } from '../../../services/agent/types'
import type { HerFilter, HerKind } from './herItems'
import { LuRefreshCw, LuSparkles } from '@lib/icons'
import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useConfigI18n as useI18n } from '../../../contexts/I18nContext'
import { agentService } from '../../../services/agent/agentApi'
import { relativeTimeBucket } from '../../agent-panel/agentRelativeTime'
import { ManagedList, SettingGroup, SettingTitleTag } from '../../settings'
import { herItems } from './herItems'
import { hasLife } from './herLife'

const KINDS: readonly HerKind[] = ['puzzles', 'wants', 'lately']

/**
 * 「她最近」整组：Agent 设置里人设之后。刷新是标题旁的 tag。
 * Her life as anyone here can see it, asked for when the group shows. Most
 * of her life happens on her own time; this is where it shows.
 */
export const HerRecent: React.FC<{ onPlay: (text: string) => void }> = ({
  onPlay,
}) => {
  const { t, format } = useI18n()
  const copy = t.agentPanel.her
  const [life, setLife] = useState<MeropeHerResponse | null>(null)
  const [loading, setLoading] = useState(true)
  const [failed, setFailed] = useState(false)
  const [filter, setFilter] = useState<HerFilter>('all')

  // Only the latest ask may land; a refresh supersedes the one before it.
  const asked = useRef(0)
  const load = useCallback(() => {
    const ask = ++asked.current
    setLoading(true)
    setFailed(false)
    agentService
      .getHer()
      .then((response) => {
        if (ask === asked.current) setLife(response)
      })
      .catch(() => {
        if (ask === asked.current) setFailed(true)
      })
      .finally(() => {
        if (ask === asked.current) setLoading(false)
      })
  }, [])

  useEffect(() => {
    load()
    return () => {
      asked.current++
    }
  }, [load])

  const ago = useCallback(
    (iso: string): string => {
      const bucket = relativeTimeBucket(iso, Date.now())
      if (!bucket) return ''
      switch (bucket.kind) {
        case 'justNow':
          return t.agentPanel.sessions.justNow
        case 'minutes':
          return format(t.agentPanel.sessions.minutesAgo, { value: bucket.value })
        case 'hours':
          return format(t.agentPanel.sessions.hoursAgo, { value: bucket.value })
        case 'days':
          return format(t.agentPanel.sessions.daysAgo, { value: bucket.value })
        case 'date':
          return bucket.date.toLocaleDateString()
      }
    },
    [format, t.agentPanel.sessions],
  )

  const kinds = life ? KINDS.filter((kind) => life[kind].length > 0) : []
  // After a refresh the kind picked may have nothing left.
  const shown: HerFilter =
    filter !== 'all' && kinds.includes(filter) ? filter : 'all'

  const items = useMemo(
    () =>
      life ? herItems({ life, filter: shown, copy, format, ago, onPlay }) : [],
    [ago, copy, shown, format, life, onPlay],
  )

  return (
    <SettingGroup
      title={copy.title}
      icon={<LuSparkles />}
      description={t.config.agentHerDesc}
      titleExtra={
        <SettingTitleTag
          variant="muted"
          icon={<LuRefreshCw aria-hidden />}
          onClick={load}
          disabled={loading}
          title={t.agentPanel.manage.refreshDesc}
        >
          {t.common.refresh}
        </SettingTitleTag>
      }
    >
      <ManagedList
        loading={loading}
        emptyText={failed ? copy.loadFailed : copy.empty}
        queryChrome="plain"
        queryCollapsible={false}
        filters={
          hasLife(life) && kinds.length > 1
            ? {
                value: shown,
                onChange: (key) => setFilter(key as HerFilter),
                ariaLabel: copy.title,
                options: [
                  {
                    key: 'all',
                    label: t.config.mcpFilterAll,
                    count: kinds.reduce((sum, kind) => sum + life![kind].length, 0),
                  },
                  ...kinds.map((kind) => ({
                    key: kind,
                    label: copy[kind],
                    count: life![kind].length,
                  })),
                ],
              }
            : undefined
        }
        items={items}
      />
    </SettingGroup>
  )
}
