import type { Anime25DPlayback } from '@myriad/figure/anime25drig'
import { useEffect, useState } from 'react'
import { SettingGroup, SettingsButton } from '../../../components/settings'
import { useI18n } from '../../../contexts/I18nContext'
import { PartProfileSummary } from './PartProfileSummary'

/** Looking for the staged figure's moving parts on the server. */
export interface MovingPartsSearch {
  /** Starts a search (it runs in the background). */
  find: () => Promise<void>
  /** Whether one is running for this figure now. */
  searching: () => Promise<boolean>
  /** Takes up the package a finished search saved. */
  found: () => Promise<void>
}

/** How often a running search is asked after. */
const WATCH_MS = 5000

/**
 * The parts of the figure on stage that move on their own, as its package
 * has them, and a way to look for them again: for a package made before they
 * were looked for, or when that failed. The vision model runs in the
 * background; while it does, the page shows it (after a reload too, and for
 * the search an import starts by itself), and takes its package up when done.
 */
export function MovingPartsPanel({
  playback,
  target,
  search,
}: {
  playback: Readonly<Anime25DPlayback>
  target?: string
  search: MovingPartsSearch
}) {
  const { t } = useI18n()
  const labels = t.merope
  const [state, setState] = useState<'idle' | 'busy' | 'started' | 'failed'>('idle')
  const [error, setError] = useState<string | null>(null)
  const found = playback.partProfile?.source === 'ai-vision'
  // A search already running when the panel opens shows as running.
  useEffect(() => {
    let current = true
    search.searching().then((running) => {
      if (current && running) setState((now) => (now === 'idle' ? 'started' : now))
    }, () => {})
    return () => { current = false }
    // Asked once per package (the panel is keyed by it).
  }, [])
  // While one runs, its end is watched for; its package is then taken up.
  useEffect(() => {
    if (state !== 'started') return
    let current = true
    const timer = setInterval(() => {
      search.searching().then(async (running) => {
        if (!current || running) return
        setState('idle')
        await search.found()
      }, () => {})
    }, WATCH_MS)
    return () => {
      current = false
      clearInterval(timer)
    }
  }, [state])
  const find = async () => {
    setState('busy')
    setError(null)
    try {
      await search.find()
      setState('started')
    } catch (reason) {
      setState('failed')
      setError(reason instanceof Error ? reason.message : labels.rigPartsRefreshFailed)
    }
  }
  return (
    <SettingGroup
      title={labels.rigPartsTitle}
      description={target ? `${labels.rigPartsRefreshHint} ${target}` : labels.rigPartsRefreshHint}
      descriptionVisible
      collapsible
      defaultExpanded={false}
      id="merope-moving-parts"
    >
      {found ? <PartProfileSummary profile={playback.partProfile} titled={false} /> : <p className="merope-motion-rig__hint">{labels.rigPartsNone}</p>}
      <SettingsButton disabled={state === 'busy' || state === 'started'} onClick={() => void find()}>
        {state === 'busy' || state === 'started' ? labels.rigPartsRefreshing : labels.rigPartsRefresh}
      </SettingsButton>
      {state === 'started' ? <p className="merope-motion-rig__hint">{labels.rigPartsRefreshStarted}</p> : null}
      {state === 'failed' && error ? <p className="merope-motion-rig__hint" role="alert">{error}</p> : null}
    </SettingGroup>
  )
}
