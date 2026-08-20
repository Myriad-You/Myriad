import type { ComponentType, SVGProps } from 'react'
import type { LifeGender } from '../onboardingTypes'
import {
  LuCircleDashed,
  LuMars,
  LuTransgender,
  LuVenus,
} from '@lib/icons'
import { GENDER_OPTIONS } from '../onboardingTypes'

type Glyph = ComponentType<SVGProps<SVGSVGElement>>

const GENDER_ICONS: Record<LifeGender, Glyph> = {
  female: LuVenus,
  male: LuMars,
  nonbinary: LuTransgender,
  unspecified: LuCircleDashed,
}

export default function GenderPicker({
  label,
  value,
  labels,
  disabled,
  onChange,
}: {
  label: string
  value: LifeGender | null
  labels: Record<LifeGender, string>
  disabled?: boolean
  onChange: (value: LifeGender) => void
}) {
  return (
    <div className="life-gender" role="radiogroup" aria-label={label}>
      {GENDER_OPTIONS.map((option) => {
        const selected = value === option
        const Icon = GENDER_ICONS[option]
        return (
          <button
            key={option}
            type="button"
            role="radio"
            aria-checked={selected}
            aria-label={labels[option]}
            className={`life-gender__opt${selected ? ' is-on' : ''}`}
            data-gender={option}
            disabled={disabled}
            onClick={() => onChange(option)}
          >
            <Icon className="life-gender__icon" aria-hidden />
            <span className="life-gender__text">{labels[option]}</span>
          </button>
        )
      })}
    </div>
  )
}
