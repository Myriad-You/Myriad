import type { WardrobeItem } from '../persona/wardrobe'
import { LuRefreshCw } from '@lib/icons'
import { useI18n } from '../../../contexts/I18nContext'
import { wardrobeItemLabel } from '../persona/wardrobe'

/** Cycles the stage through the worn bust and each full-body set. */
export function FullBodyStageSwitch({
  sets,
  stagedId,
  onChange,
}: {
  sets: readonly WardrobeItem[]
  stagedId: string | null
  onChange: (id: string | null) => void
}) {
  const { t, format } = useI18n()
  const copy = t.merope.fullBody
  const styleNames = t.agentPersona.onboarding.clothingStyle
  const stages = [
    { id: null, label: copy.stageBust },
    ...sets.map((set) => ({
      id: set.id,
      label: `${wardrobeItemLabel(set, styleNames)} · ${copy.badge}`,
    })),
  ]
  const index = Math.max(
    0,
    stages.findIndex((stage) => stage.id === stagedId),
  )
  const next = stages[(index + 1) % stages.length]
  return (
    <button
      type="button"
      className="merope-motion-page__stage-cycle"
      aria-label={`${copy.stage}: ${stages[index].label}`}
      title={format(copy.stageNext, { name: next.label })}
      onClick={() => onChange(next.id)}
    >
      <LuRefreshCw aria-hidden="true" />
      <span>{stages[index].label}</span>
    </button>
  )
}
