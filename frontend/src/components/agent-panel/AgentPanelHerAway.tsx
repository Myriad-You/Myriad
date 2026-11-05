import type { AgentPanelMode } from './agentPanelMode'
import React, { useEffect, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { agentService } from '../../services/agent/agentApi'
import { getPublicConfigDeduped } from '../../utils/requestDedup'
import { setAgentPanelMode } from './agentPanelMode'

/**
 * Where she is when not on this site, as the public config says it (H3e-9);
 * `mind_elsewhere`: she lives here, but her mind is on another of the
 * owner's devices now (the mind lease).
 */
export type HerAway = 'moving' | 'elsewhere' | 'vacant' | 'moved_out' | 'mind_elsewhere'

const AWAY: readonly HerAway[] = ['moving', 'elsewhere', 'vacant', 'moved_out', 'mind_elsewhere']

/**
 * Whether she is away from this site, read while the panel is open; for the
 * owner, the name of the device she lives on (`/agent/host` is theirs only),
 * or that her mind is on (`/agent/mind/where`), and whether they are the
 * owner (`owner`): only they are told which device.
 */
export function useHerAway(open: boolean): {
  away: HerAway | null
  device: string | null
  owner: boolean
} {
  const [away, setAway] = useState<HerAway | null>(null)
  const [device, setDevice] = useState<string | null>(null)
  const [owner, setOwner] = useState(false)
  useEffect(() => {
    if (!open) return
    let alive = true
    void (async () => {
      const config = await getPublicConfigDeduped().catch(() => null)
      const said = config?.herAway
      const next = AWAY.includes(said) ? (said as HerAway) : null
      if (!alive) return
      setAway(next)
      if (next === 'mind_elsewhere') {
        const mind = await agentService.getMindPlace().catch(() => null)
        if (!alive) return
        setOwner(mind !== null)
        setDevice(mind?.holderName ?? null)
        return
      }
      setOwner(false)
      if (next !== 'elsewhere') {
        setDevice(null)
        return
      }
      const host = await agentService.getHost().catch(() => null)
      if (alive) setDevice(host?.host?.holderName ?? null)
    })()
    return () => {
      alive = false
    }
  }, [open])
  return { away, device, owner }
}

/**
 * In place of the chat composer while she is not on this site: she cannot be
 * talked to here. Work mode keeps its composer (the plain assistant).
 */
export const AgentPanelHerAway: React.FC<{
  away: HerAway
  device: string | null
  owner?: boolean
  mode: AgentPanelMode
}> = ({ away, device, owner = false, mode }) => {
  const { t, format } = useI18n()
  if (mode !== 'chat') return null
  const words = t.agentPanel.away
  const text =
    away === 'mind_elsewhere'
      ? !owner
        ? words.member
        : device
          ? format(words.mindElsewhere, { name: device })
          : words.mindElsewhereUnknown
      : away === 'elsewhere'
        ? device
          ? format(words.elsewhere, { name: device })
          : words.elsewhereUnknown
        : away === 'moving'
          ? words.moving
          : away === 'moved_out'
            ? words.movedOut
            : words.vacant
  return (
    <div className="agent-panel-away" role="status">
      <p>{text}</p>
      {away === 'elsewhere' || away === 'vacant' ? (
        <button
          type="button"
          className="agent-panel-tag"
          data-tone="primary"
          onClick={() => setAgentPanelMode('work')}
        >
          <span className="agent-panel-tag-text">{words.toWork}</span>
        </button>
      ) : null}
    </div>
  )
}
