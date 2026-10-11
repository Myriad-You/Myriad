import type { Anime25DPlayback } from '../anime25drig/types'
import { useState } from 'react'
import { SettingGroup, SettingsButton } from '../../../components/settings'
import { useI18n } from '../../../contexts/I18nContext'
import { PartProfileSummary } from './PartProfileSummary'

/**
 * The parts of the figure on stage that move on their own, as its package
 * has them, and a way to look for them again: for a package made before they
 * were looked for, or when that failed. The vision model runs in the
 * background; its package takes over when it is done.
 */
export function MovingPartsPanel({
  playback,
  target,
  onFind,
}: {
  playback: Readonly<Anime25DPlayback>
  target?: string
  onFind: () => Promise<void>
}) {
  const { t } = useI18n()
  const labels = t.merope
  const [state, setState] = useState<'idle' | 'busy' | 'started' | 'failed'>('idle')
  const [error, setError] = useState<string | null>(null)
  const found = playback.partProfile?.source === 'ai-vision'
  const find = async () => {
    setState('busy')
    setError(null)
    try {
      await onFind()
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
      <SettingsButton disabled={state === 'busy'} onClick={() => void find()}>
        {state === 'busy' ? labels.rigPartsRefreshing : labels.rigPartsRefresh}
      </SettingsButton>
      {state === 'started' ? <p className="merope-motion-rig__hint">{labels.rigPartsRefreshStarted}</p> : null}
      {state === 'failed' && error ? <p className="merope-motion-rig__hint" role="alert">{error}</p> : null}
    </SettingGroup>
  )
}
