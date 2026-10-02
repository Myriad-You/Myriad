import type { WardrobeItem } from '../persona/wardrobe'
import { useI18n } from '../../../contexts/I18nContext'
import { wardrobeItemLabel } from '../persona/wardrobe'
import { FaceTabs } from './FaceTabs'

/** Switches the stage between the worn bust and any full-body set. */
export function FullBodyStageSwitch({
  sets,
  stagedId,
  onChange,
}: {
  sets: readonly WardrobeItem[]
  stagedId: string | null
  onChange: (id: string | null) => void
}) {
  const { t } = useI18n()
  const copy = t.merope.fullBody
  const styleNames = t.agentPersona.onboarding.clothingStyle
  return (
    <FaceTabs
      ariaLabel={copy.stage}
      value={stagedId ?? ''}
      options={[
        { value: '', label: copy.stageBust },
        ...sets.map((set) => ({
          value: set.id,
          label: `${wardrobeItemLabel(set, styleNames)} · ${copy.badge}`,
        })),
      ]}
      onChange={(value) => onChange(value || null)}
    />
  )
}
