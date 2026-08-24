import type { ComponentType, SVGProps } from 'react'
import type { PersonaGender } from '../onboardingTypes'
import {
  LuCircleDashed,
  LuMars,
  LuTransgender,
  LuVenus,
} from '@lib/icons'
import { GENDER_OPTIONS } from '../onboardingTypes'

type Glyph = ComponentType<SVGProps<SVGSVGElement>>

const GENDER_ICONS: Record<PersonaGender, Glyph> = {
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
  value: PersonaGender | null
  labels: Record<PersonaGender, string>
  disabled?: boolean
  onChange: (value: PersonaGender) => void
}) {
  return (
    <div className="merope-gender" role="radiogroup" aria-label={label}>
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
            className={`merope-gender__opt${selected ? ' is-on' : ''}`}
            data-gender={option}
            disabled={disabled}
            onClick={() => onChange(option)}
          >
            <Icon className="merope-gender__icon" aria-hidden />
            <span className="merope-gender__text">{labels[option]}</span>
          </button>
        )
      })}
    </div>
  )
}
