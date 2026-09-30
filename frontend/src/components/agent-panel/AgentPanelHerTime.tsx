import type { HerDoing, HerLazing } from './herTime'
import React, { useCallback, useEffect, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { listenTogether } from '../../features/merope/listenTogether'
import { getGlobalState } from '../../hooks/musicPlayer/globalState'
import { agentService } from '../../services/agent/agentApi'
import {
  herTitle,
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

/** "Listening to X · Listen together", beside the composer. */
export const AgentPanelHerTime: React.FC<{ open: boolean }> = ({ open }) => {
  const { t, format } = useI18n()
  const { doing, lazing } = useHerDoing(open)
  const along = useListeningAlong(doing)
  const [joining, setJoining] = useState(false)
  const join = useCallback(() => {
    setJoining(true)
    void listenTogether().finally(() => setJoining(false))
  }, [])
  const label = doing
    ? format(
        doing.thing.kind === 'song'
          ? t.agentPanel.herTime.listening
          : t.agentPanel.herTime.reading,
        { title: herTitle(doing) },
      )
    : ''
  const song = doing?.thing.kind === 'song'
  return (
    <>
      <AgentPresence open={!doing && !!lazing} kind="chip" from="self">
        {!doing && lazing ? (
          <span className="agent-panel-tag">
            <span className="agent-panel-tag-text">
              {t.agentPanel.herTime[lazing.kind]}
            </span>
          </span>
        ) : null}
      </AgentPresence>
      <AgentPresence open={!!doing} kind="chip" from="self">
        {doing ? (
          <span className="agent-panel-tag" data-tone="primary">
            <span className="agent-panel-tag-text">
              {along ? t.agentPanel.herTime.together : label}
            </span>
          </span>
        ) : null}
      </AgentPresence>
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
