import type {
  ClothingStyle,
  OnboardingHeaderChrome,
  OnboardingStep,
  PersonaGender,
  StructuredPersona,
  UpperBodyVisualIdentity,
} from './onboardingTypes'
import { useCallback, useEffect, useRef, useState } from 'react'
import { useI18n } from '../../../contexts/I18nContext'
import { agentService } from '../../../services/agent'
import { invalidatePublicConfigCache } from '../../../utils/requestDedup'
import {
  clothingStyleFromProfile,
  emptyPersona,
  flattenPersona,
  genderFromProfile,
  onboardingSeedsFromProfile,
  personaFromApi,
  visualIdentityFromProfile,
} from './onboardingTypes'
import BasicsStep from './steps/BasicsStep'
import CharacterVisualDesignStep from './steps/CharacterVisualDesignStep'
import MasterPortraitStep from './steps/MasterPortraitStep'
import PersonaEditStep from './steps/PersonaEditStep'
import TagBubblesStep from './steps/TagBubblesStep'

interface Props {
  initialName?: string
  initialPersona?: StructuredPersona
  initialVisualProfile?: Record<string, unknown> | null
  step: OnboardingStep
  onStepChange: (step: OnboardingStep) => void
  onBusyChange: (busy: boolean) => void
  onHeaderChange: (chrome: OnboardingHeaderChrome) => void
  onFinished: () => void
}

export default function OnboardingWizard({
  initialName = '',
  initialPersona,
  initialVisualProfile,
  step,
  onStepChange,
  onBusyChange,
  onHeaderChange,
  onFinished,
}: Props) {
  const { t, locale } = useI18n()
  const o = t.agentPersona.onboarding
  const [busy, setBusy] = useState(false)
  const [selectedTags, setSelectedTags] = useState<string[]>(
    () => onboardingSeedsFromProfile(initialVisualProfile).sourceTags,
  )
  const [displayName, setDisplayName] = useState(initialName)
  const [gender, setGender] = useState<PersonaGender | null>(() => {
    return genderFromProfile(initialVisualProfile)
  })
  const [extraRequirements, setExtraRequirements] = useState(
    () =>
      onboardingSeedsFromProfile(initialVisualProfile).personaExtraRequirements,
  )
  const [persona, setPersona] = useState<StructuredPersona>(
    () => initialPersona ?? emptyPersona(),
  )
  const [visualIdentity, setVisualIdentity] =
    useState<UpperBodyVisualIdentity | null>(() =>
      visualIdentityFromProfile(initialVisualProfile),
    )
  const [clothingStyle, setClothingStyle] = useState<ClothingStyle | null>(() =>
    clothingStyleFromProfile(initialVisualProfile),
  )
  const [visualRequirements, setVisualRequirements] = useState(() => {
    const saved = initialVisualProfile?.extraRequirements
    return typeof saved === 'string' ? saved : ''
  })
  const runLock = useRef(false)
  const previousStep = useRef(step)
  const hasStepped = useRef(false)
  const claimedAuto = useRef({ persona: false })
  const personaWriteSeq = useRef(0)
  const invalidatePersonaAndVisual = useCallback(() => {
    claimedAuto.current.persona = false
    setPersona(emptyPersona())
    setVisualIdentity(null)
  }, [])
  const updateSelectedTags = useCallback(
    (next: string[]) => {
      if (
        next.length === selectedTags.length &&
        next.every((tag, index) => tag === selectedTags[index])
      ) {
        return
      }
      setSelectedTags(next)
      invalidatePersonaAndVisual()
    },
    [invalidatePersonaAndVisual, selectedTags],
  )
  const updateDisplayName = useCallback(
    (next: string) => {
      if (next === displayName) return
      setDisplayName(next)
      invalidatePersonaAndVisual()
    },
    [displayName, invalidatePersonaAndVisual],
  )
  const updateGender = useCallback(
    (next: PersonaGender) => {
      if (next === gender) return
      setGender(next)
      invalidatePersonaAndVisual()
    },
    [gender, invalidatePersonaAndVisual],
  )
  const updatePersonaRequirements = useCallback(
    (next: string) => {
      if (next === extraRequirements) return
      setExtraRequirements(next)
      invalidatePersonaAndVisual()
    },
    [extraRequirements, invalidatePersonaAndVisual],
  )
  const claimPersona = useCallback(() => {
    if (claimedAuto.current.persona) return false
    claimedAuto.current.persona = true
    return true
  }, [])
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
    invalidatePersonaAndVisual()
  }, [invalidatePersonaAndVisual, locale])

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
    const seq = ++personaWriteSeq.current
    const draft = await agentService.draftPersona({
      name: displayName.trim() || 'Arael',
      tags: selectedTags,
      gender: gender ?? 'unspecified',
      extraRequirements: extraRequirements.trim(),
      language: locale,
    })
    if (seq !== personaWriteSeq.current) return
    if (!draft.persona) throw new Error(o.regeneratePersonaFailed)
    setPersona(personaFromApi(draft.persona))
    setVisualIdentity(null)
  }

  const confirmedVisualProfile = (
    identity: UpperBodyVisualIdentity | null = visualIdentity,
  ) => ({
    gender: gender ?? 'unspecified',
    language: locale,
    ...(clothingStyle ? { clothingStyle } : {}),
    extraRequirements: visualRequirements.trim(),
    visualIdentity: identity,
    sourceTags: selectedTags,
    personaExtraRequirements: extraRequirements.trim(),
  })

  const stepTitle = [
    o.step1Title,
    o.step2Title,
    o.step3Title,
    o.step4Title,
    o.step5Title,
  ][step - 1]

  return (
    <section className="merope-ob" aria-label={stepTitle}>
      <div className="merope-ob__card">
        <div className="merope-ob__viewport">
          <div
            key={step}
            className="merope-ob__pane sm-pane"
            data-nav={paneNav}
          >
            {step === 1 && (
              <TagBubblesStep
                selected={selectedTags}
                onChange={updateSelectedTags}
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
                onDisplayName={updateDisplayName}
                onGender={updateGender}
                onExtra={updatePersonaRequirements}
                onHeaderChange={onHeaderChange}
                onSubmit={async () => {
                  onStepChange(3)
                }}
              />
            )}
            {step === 3 && (
              <PersonaEditStep
                persona={persona}
                name={displayName.trim() || 'Arael'}
                gender={gender ?? undefined}
                busy={busy}
                claimAutoGenerate={claimPersona}
                onHeaderChange={onHeaderChange}
                onRegenerate={draftPersona}
                onImported={(next) => {
                  claimedAuto.current.persona = true
                  personaWriteSeq.current += 1
                  setPersona(next)
                  setVisualIdentity(null)
                }}
                onSave={(next) =>
                  run(async () => {
                    const personaChanged =
                      JSON.stringify(next) !== JSON.stringify(persona)
                    const nextVisualIdentity = personaChanged
                      ? null
                      : visualIdentity
                    await agentService.putPersona({
                      name: displayName.trim(),
                      personality: flattenPersona(next),
                      persona: {
                        displayName: displayName.trim(),
                        ...next,
                        language: locale,
                        draftSource: 'owner-reviewed',
                      },
                      visualProfile: confirmedVisualProfile(nextVisualIdentity),
                    })
                    setPersona(next)
                    if (personaChanged) setVisualIdentity(null)
                    invalidatePublicConfigCache()
                    window.dispatchEvent(
                      new CustomEvent('arael-persona-updated'),
                    )
                    return 4
                  })
                }
              />
            )}
            {step === 4 && (
              <CharacterVisualDesignStep
                identity={visualIdentity}
                gender={gender}
                language={locale}
                clothingStyle={clothingStyle}
                requirements={visualRequirements}
                busy={busy}
                onClothingStyle={setClothingStyle}
                onIdentity={setVisualIdentity}
                onRequirements={(next) => {
                  if (next === visualRequirements) return
                  setVisualRequirements(next)
                  setVisualIdentity(null)
                }}
                onBusyChange={(next) => {
                  setBusy(next)
                  onBusyChange(next)
                }}
                onHeaderChange={onHeaderChange}
                onConfirm={() =>
                  run(async () => {
                    if (!visualIdentity) throw new Error(o.visualDesignFailed)
                    await agentService.putPersona({
                      name: displayName.trim(),
                      personality: flattenPersona(persona),
                      persona: {
                        displayName: displayName.trim(),
                        ...persona,
                        language: locale,
                        draftSource: 'owner-reviewed',
                      },
                      visualProfile: confirmedVisualProfile(),
                    })
                    invalidatePublicConfigCache()
                    return 5
                  })
                }
              />
            )}
            {step === 5 && (
              <MasterPortraitStep
                characterName={displayName.trim() || 'Arael'}
                busy={busy}
                onBusyChange={(next) => {
                  setBusy(next)
                  onBusyChange(next)
                }}
                onHeaderChange={onHeaderChange}
                onFinished={onFinished}
              />
            )}
          </div>
        </div>
      </div>
    </section>
  )
}
