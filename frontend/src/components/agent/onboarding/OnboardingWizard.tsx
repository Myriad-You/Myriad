import type {
  LifeGender,
  OnboardingHeaderChrome,
  OnboardingStep,
  StructuredPersona,
} from './onboardingTypes'
import { useEffect, useRef, useState } from 'react'
import { useI18n } from '../../../contexts/I18nContext'
import { agentService } from '../../../services/agent'
import { invalidatePublicConfigCache } from '../../../utils/requestDedup'
import { emptyPersona, flattenPersona, personaFromApi } from './onboardingTypes'
import BasicsStep from './steps/BasicsStep'
import PersonaEditStep from './steps/PersonaEditStep'
import TagBubblesStep from './steps/TagBubblesStep'

interface Props {
  initialName?: string
  initialPersona?: StructuredPersona
  step: OnboardingStep
  onStepChange: (step: OnboardingStep) => void
  onBusyChange: (busy: boolean) => void
  onHeaderChange: (chrome: OnboardingHeaderChrome) => void
  onFinished: () => void
}

export default function OnboardingWizard({
  initialName = '',
  initialPersona,
  step,
  onStepChange,
  onBusyChange,
  onHeaderChange,
  onFinished,
}: Props) {
  const { t, locale } = useI18n()
  const o = t.life.onboarding
  const [busy, setBusy] = useState(false)
  const [selectedTags, setSelectedTags] = useState<string[]>([])
  const [displayName, setDisplayName] = useState(initialName)
  const [gender, setGender] = useState<LifeGender | null>(null)
  const [extraRequirements, setExtraRequirements] = useState('')
  const [persona, setPersona] = useState<StructuredPersona>(
    () => initialPersona ?? emptyPersona(),
  )
  const runLock = useRef(false)
  const previousStep = useRef(step)
  const hasStepped = useRef(false)
  const direction = step >= previousStep.current ? 1 : -1
  if (previousStep.current !== step) hasStepped.current = true
  previousStep.current = step
  const paneNav = !hasStepped.current
    ? undefined
    : direction > 0
      ? 'forward'
      : 'back'

  useEffect(() => {
    const saved = initialName.trim()
    if (!saved) return
    setDisplayName((current) => (current.trim() ? current : initialName))
  }, [initialName])

  const localeRef = useRef(locale)
  useEffect(() => {
    if (localeRef.current === locale) return
    localeRef.current = locale
    setSelectedTags([])
  }, [locale])

  const run = async (operation: () => Promise<OnboardingStep | void>) => {
    if (busy || runLock.current) return
    runLock.current = true
    setBusy(true)
    onBusyChange(true)
    try {
      const next = await operation()
      if (next) onStepChange(next)
    } finally {
      runLock.current = false
      setBusy(false)
      onBusyChange(false)
    }
  }

  const draftPersona = async () => {
    const draft = await agentService.draftPersona({
      name: displayName.trim() || 'Arael',
      tags: selectedTags,
      gender: gender ?? 'unspecified',
      extraRequirements: extraRequirements.trim(),
      language: locale,
    })
    if (!draft.persona) throw new Error(o.regeneratePersonaFailed)
    setPersona(personaFromApi(draft.persona))
  }

  const stepTitle =
    step === 1 ? o.step1Title : step === 2 ? o.step2Title : o.step3Title

  return (
    <section className="life-ob" aria-label={stepTitle}>
      <div className="life-ob__card">
        <div className="life-ob__viewport">
          <div
            key={step}
            className="life-ob__pane sm-pane"
            data-nav={paneNav}
          >
            {step === 1 && (
              <TagBubblesStep
                selected={selectedTags}
                onChange={setSelectedTags}
                onHeaderChange={onHeaderChange}
                onNext={() => onStepChange(2)}
              />
            )}
            {step === 2 && (
              <BasicsStep
                displayName={displayName}
                gender={gender}
                extraRequirements={extraRequirements}
                selectedTags={selectedTags}
                busy={busy}
                onDisplayName={setDisplayName}
                onGender={setGender}
                onExtra={setExtraRequirements}
                onHeaderChange={onHeaderChange}
                onSubmit={() =>
                  run(async () => {
                    await draftPersona()
                    return 3
                  })
                }
              />
            )}
            {step === 3 && (
              <PersonaEditStep
                persona={persona}
                busy={busy}
                onHeaderChange={onHeaderChange}
                onRegenerate={draftPersona}
                onSave={(next) =>
                  run(async () => {
                    await agentService.putPersona({
                      name: displayName.trim(),
                      personality: flattenPersona(next),
                    })
                    invalidatePublicConfigCache()
                    window.dispatchEvent(
                      new CustomEvent('arael-persona-updated'),
                    )
                    onFinished()
                  })
                }
              />
            )}
          </div>
        </div>
      </div>
    </section>
  )
}
