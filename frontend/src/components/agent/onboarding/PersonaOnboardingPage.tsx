import type {
  OnboardingHeaderChrome,
  OnboardingPageChrome,
  OnboardingStep,
  StructuredPersona,
} from './onboardingTypes'
import { useCallback, useEffect, useRef, useState } from 'react'
import { useAuth } from '../../../contexts/AuthContext'
import { useI18n } from '../../../contexts/I18nContext'
import { agentService } from '../../../services/agent'
import {
  completedPersonaResumeStep,
  parseFlattenedPersona,
  personaFromApi,
  structuredPersonaIsComplete,
} from './onboardingTypes'
import OnboardingWizard from './OnboardingWizard'
import '../PersonaOnboarding.css'

function sameHeaderAction(
  left?: OnboardingHeaderChrome['action'],
  right?: OnboardingHeaderChrome['action'],
) {
  return (
    Boolean(left) === Boolean(right) &&
    left?.label === right?.label &&
    left?.busy === right?.busy &&
    left?.disabled === right?.disabled
  )
}

function samePageChrome(left: OnboardingPageChrome, right: OnboardingPageChrome) {
  return (
    left.title === right.title &&
    left.description === right.description &&
    left.detailTone === right.detailTone &&
    left.backDisabled === right.backDisabled &&
    left.backAria === right.backAria &&
    sameHeaderAction(left.action, right.action)
  )
}

interface Props {
  onBack: () => void
  onChromeChange: (chrome: OnboardingPageChrome) => void
  meropeOn: boolean
  gateLead: string
}

export default function PersonaOnboardingPage({
  onBack,
  onChromeChange,
  meropeOn,
  gateLead,
}: Props) {
  const { t } = useI18n()
  const o = t.agentPersona.onboarding
  const { user } = useAuth()
  const isOwner = user?.is_owner === true
  const [name, setName] = useState('')
  const [savedPersona, setSavedPersona] = useState<StructuredPersona | null>(
    null,
  )
  const [savedVisualProfile, setSavedVisualProfile] = useState<Record<
    string,
    unknown
  > | null>(null)
  const [ready, setReady] = useState(false)
  const [step, setStep] = useState<OnboardingStep>(1)
  const [wizardBusy, setWizardBusy] = useState(false)
  const [header, setHeader] = useState<OnboardingHeaderChrome>({
    description: '',
  })
  const actionClickRef = useRef<(() => void) | undefined>(undefined)
  const stepBackRef = useRef<(() => boolean) | undefined>(undefined)
  const onChromeChangeRef = useRef(onChromeChange)
  onChromeChangeRef.current = onChromeChange
  const chromeRef = useRef<OnboardingPageChrome | null>(null)
  const pageTitle = meropeOn
    ? [o.step1Title, o.step2Title, o.step3Title, o.step4Title, o.step5Title][
        step - 1
      ]
    : t.config.agentPersona
  const pageLead =
    header.description ||
    (meropeOn
      ? [o.step1Lead, o.step2Lead, o.step3Lead, o.step4Lead, o.step5Lead][
          step - 1
        ]
      : gateLead)

  const loadSaved = useCallback(async () => {
    if (!isOwner || !meropeOn) {
      setReady(true)
      return
    }
    try {
      const persona = await agentService.getPersona()
      const name = persona?.name?.trim() ?? ''
      const personality = persona?.personality?.trim() ?? ''
      const structured = personaFromApi(persona?.persona)
      const hasStructuredPersona = structuredPersonaIsComplete(structured)
      const saved =
        persona?.hasCustomPersona === true ||
        name.length > 0 ||
        personality.length > 0 ||
        hasStructuredPersona
      setName(name)
      setSavedVisualProfile(persona?.visualProfile ?? null)
      if (saved && (hasStructuredPersona || personality)) {
        setSavedPersona(
          hasStructuredPersona
            ? structured
            : parseFlattenedPersona(personality),
        )
        setStep(
          hasStructuredPersona
            ? completedPersonaResumeStep(persona?.visualProfile)
            : 3,
        )
      } else {
        setSavedPersona(null)
        setSavedVisualProfile(null)
      }
    } catch {
      /* 预填失败就从空称呼开始 */
    } finally {
      setReady(true)
    }
  }, [isOwner, meropeOn])

  useEffect(() => {
    void loadSaved()
  }, [loadSaved])

  const handleHeaderChange = useCallback((next: OnboardingHeaderChrome) => {
    actionClickRef.current = next.action?.onClick
    stepBackRef.current = next.onBack
    setHeader((prev) => {
      if (
        prev.description === next.description &&
        sameHeaderAction(prev.action, next.action) &&
        Boolean(prev.onBack) === Boolean(next.onBack)
      ) {
        return prev
      }
      return {
        description: next.description,
        action: next.action
          ? {
              label: next.action.label,
              busy: next.action.busy,
              disabled: next.action.disabled,
              onClick: () => actionClickRef.current?.(),
            }
          : undefined,
        onBack: next.onBack
          ? () => stepBackRef.current?.() === true
          : undefined,
      }
    })
  }, [])

  const handleBack = useCallback(() => {
    if (header.onBack?.()) return
    if (meropeOn && step > 1) {
      if (!wizardBusy) setStep((current) => (current - 1) as OnboardingStep)
      return
    }
    onBack()
  }, [header.onBack, meropeOn, onBack, step, wizardBusy])

  useEffect(() => {
    if (!isOwner) return
    const next: OnboardingPageChrome = {
      title: pageTitle,
      description: pageLead,
      detailTone: meropeOn ? 'default' : 'warning',
      action: header.action
        ? {
            label: header.action.label,
            busy: header.action.busy,
            disabled: header.action.disabled,
            onClick: () => actionClickRef.current?.(),
          }
        : undefined,
      backDisabled: wizardBusy && step > 1,
      backAria: header.onBack
        ? o.visualBackToStyle
        : meropeOn && step > 1
          ? o.backTo.replace(
              '{step}',
              [
                o.step1Short,
                o.step2Short,
                o.step3Short,
                o.step4Short,
                o.step5Short,
              ][step - 2] || '',
            )
          : t.common.back,
      onBack: handleBack,
    }
    if (chromeRef.current && samePageChrome(chromeRef.current, next)) return
    chromeRef.current = next
    onChromeChangeRef.current(next)
  }, [
    handleBack,
    header.action,
    header.onBack,
    isOwner,
    o.visualBackToStyle,
    meropeOn,
    o.backTo,
    o.step1Short,
    o.step2Short,
    o.step3Short,
    o.step4Short,
    o.step5Short,
    pageLead,
    pageTitle,
    step,
    t.common.back,
    wizardBusy,
  ])

  if (!isOwner || !ready) return null

  return meropeOn ? (
    <OnboardingWizard
      initialName={name}
      initialPersona={savedPersona ?? undefined}
      initialVisualProfile={savedVisualProfile}
      step={step}
      onStepChange={setStep}
      onBusyChange={setWizardBusy}
      onHeaderChange={handleHeaderChange}
      onFinished={onBack}
    />
  ) : null
}
