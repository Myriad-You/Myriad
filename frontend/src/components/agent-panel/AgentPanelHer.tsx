import type { MeropeHerResponse } from '../../services/agent/types'
import React, { useEffect, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { agentService } from '../../services/agent/agentApi'
import { HerLifeView } from './AgentPanelHerView'
import { relativeTimeBucket } from './agentRelativeTime'
import { hasLife } from './herLife'

/**
 * Her life as anyone here can see it, asked for when the view opens. Most
 * of her life happens on her own time; this is where it shows.
 */
export const AgentPanelHer: React.FC<{ onPlay: (text: string) => void }> = ({
  onPlay,
}) => {
  const { t, format } = useI18n()
  const copy = t.agentPanel.her
  const [life, setLife] = useState<MeropeHerResponse | null>(null)
  const [failed, setFailed] = useState(false)

  useEffect(() => {
    let alive = true
    agentService
      .getHer()
      .then((response) => {
        if (alive) setLife(response)
      })
      .catch(() => {
        if (alive) setFailed(true)
      })
    return () => {
      alive = false
    }
  }, [])

  const ago = (iso: string): string => {
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
  }

  const note = failed
    ? copy.loadFailed
    : !life
      ? t.common.loading
      : !hasLife(life)
        ? copy.empty
        : null
  if (note || !life) {
    return (
      <div className="agent-panel-her">
        <p
          className="agent-panel-her-note"
          role={failed ? 'alert' : !life ? 'status' : undefined}
        >
          {note}
        </p>
      </div>
    )
  }
  return (
    <HerLifeView
      life={life}
      copy={copy}
      format={format}
      ago={ago}
      onPlay={onPlay}
    />
  )
}
