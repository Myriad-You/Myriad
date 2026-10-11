import type { Anime25DPart, Anime25DPartProfile } from '../anime25drig/types'
import { useI18n } from '../../../contexts/I18nContext'

type Labels = ReturnType<typeof useI18n>['t']['merope']

/** The parts the import's vision inventory found moving on their own, and how each moves. */
export function PartProfileSummary({ profile, titled = true }: { profile: Anime25DPartProfile | undefined; titled?: boolean }) {
  const { t, format } = useI18n()
  const labels = t.merope
  if (!profile?.parts.length) return null
  return (
    <div className="merope-motion-rig__checks">
      {titled ? <b>{labels.rigPartsTitle}</b> : null}
      <p className="merope-motion-rig__hint">{labels.rigPartsHint}</p>
      <ul className="merope-motion-rig__issues">
        {profile.parts.map((part) => (
          <li key={`${part.name}:${part.anchor.join(',')}`}>
            {format(labels.rigPartsItem, { name: part.name, motion: motionLabel(labels, part.motion), material: part.material })}
          </li>
        ))}
      </ul>
    </div>
  )
}

function motionLabel(labels: Labels, motion: Anime25DPart['motion']): string {
  return {
    rigid: labels.rigPartMotionRigid,
    sway: labels.rigPartMotionSway,
    dangle: labels.rigPartMotionDangle,
    flutter: labels.rigPartMotionFlutter,
  }[motion]
}
