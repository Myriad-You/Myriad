import type { HerDoing, HerLazing } from './herTime'
import React, { useCallback, useEffect, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { dispatchMeropePerformance } from '../../features/merope/events/performanceEvents'
import { captureProductionRigStateSummary } from '../../features/merope/motion/runtimeHost'
import { listenTogether } from '../../features/merope/music/listenTogether'
import { getGlobalState } from '../../hooks/musicPlayer/globalState'
import { agentService } from '../../services/agent/agentApi'
import {
  listeningAlong,
  nextLookMs,
  UNAVAILABLE_RETRY_MS,
} from './herTime'
import { AgentPresence } from './useAgentPresence'

/** What she is doing on her own, asked while the panel is open. */
function useHerDoing(open: boolean): { doing: HerDoing | null; lazing: HerLazing | null } {
  const [doing, setDoing] = useState<HerDoing | null>(null)
  const [lazing, setLazing] = useState<HerLazing | null>(null)
  useEffect(() => {
    if (!open) return
    let timer: ReturnType<typeof setTimeout> | undefined
    let alive = true
    const look = async () => {
      const response = await agentService.getDoing().catch(() => null)
      if (!alive) return
      setDoing(response?.doing ?? null)
      setLazing(response?.lazing ?? null)
      timer = setTimeout(
        look,
        response ? nextLookMs(response) : UNAVAILABLE_RETRY_MS,
      )
    }
    void look()
    return () => {
      alive = false
      clearTimeout(timer)
    }
  }, [open])
  return open ? { doing, lazing } : { doing: null, lazing: null }
}

/** Whether this player is on her song, kept up as the player changes. */
function useListeningAlong(doing: HerDoing | null): boolean {
  const [along, setAlong] = useState(() =>
    listeningAlong(doing, getGlobalState()),
  )
  useEffect(() => {
    const update = () => setAlong(listeningAlong(doing, getGlobalState()))
    update()
    window.addEventListener('music-player-state-change', update)
    return () => window.removeEventListener('music-player-state-change', update)
  }, [doing])
  return along
}

/**
 * What she is doing, acted on her face rather than written beside it. The
 * director decides how, once each time what she does (or who listens along)
 * changes; the face plays it as long as nobody is talking with her.
 */
function useHerActing(doing: HerDoing | null, lazing: HerLazing | null, along: boolean): void {
  const what = doing ? `doing:${doing.started}` : lazing ? `lazing:${lazing.started}` : null
  useEffect(() => {
    if (!what) return
    let alive = true
    void agentService
      .getDoingActing({ rigState: captureProductionRigStateSummary(), together: along })
      .then(({ performance }) => {
        if (alive && performance) dispatchMeropePerformance({ text: '', source: 'interaction', performance })
      })
      .catch(() => {})
    return () => {
      alive = false
    }
  }, [what, along])
}

/** Listen together, beside the composer, while she is on a song. */
export const AgentPanelHerTime: React.FC<{ open: boolean }> = ({ open }) => {
  const { t } = useI18n()
  const { doing, lazing } = useHerDoing(open)
  const along = useListeningAlong(doing)
  useHerActing(doing, lazing, along)
  const [joining, setJoining] = useState(false)
  const join = useCallback(() => {
    setJoining(true)
    void listenTogether().finally(() => setJoining(false))
  }, [])
  const song = doing?.thing.kind === 'song'
  return (
    <>
      <AgentPresence open={song && !along} kind="chip" from="self">
        {song && !along ? (
          <button
            type="button"
            className="agent-panel-tag agent-panel-tag-strong"
            data-tone="primary"
            onClick={join}
            disabled={joining}
          >
            <span className="agent-panel-tag-text">
              {t.agentPanel.herTime.join}
            </span>
          </button>
        ) : null}
      </AgentPresence>
    </>
  )
}
