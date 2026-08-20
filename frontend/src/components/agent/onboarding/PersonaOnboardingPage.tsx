import type {
  OnboardingHeaderChrome,
  OnboardingPageChrome,
  OnboardingStep,
  StructuredPersona,
} from './onboardingTypes'
import { useCallback, useEffect, useRef, useState } from 'react'
import { parseFlattenedPersona } from './onboardingTypes'
import { useAuth } from '../../../contexts/AuthContext'
import { useI18n } from '../../../contexts/I18nContext'
import { agentService } from '../../../services/agent'
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
  lifeOn: boolean
  gateLead: string
}

export default function PersonaOnboardingPage({
  onBack,
  onChromeChange,
  lifeOn,
  gateLead,
}: Props) {
  const { t } = useI18n()
  const o = t.life.onboarding
  const { user } = useAuth()
  const isOwner = user?.is_owner === true
  const [name, setName] = useState('')
  const [savedPersona, setSavedPersona] = useState<StructuredPersona | null>(
    null,
  )
  const [resumeSaved, setResumeSaved] = useState(false)
  const [ready, setReady] = useState(false)
  const [step, setStep] = useState<OnboardingStep>(1)
  const [wizardBusy, setWizardBusy] = useState(false)
  const [header, setHeader] = useState<OnboardingHeaderChrome>({
    description: '',
  })
  const actionClickRef = useRef<(() => void) | undefined>(undefined)
  const onChromeChangeRef = useRef(onChromeChange)
  onChromeChangeRef.current = onChromeChange
  const chromeRef = useRef<OnboardingPageChrome | null>(null)
  const pageTitle = lifeOn
    ? [o.step1Title, o.step2Title, o.step3Title][step - 1]
    : t.config.agentLife
  const pageLead =
    header.description ||
    (lifeOn ? [o.step1Lead, o.step2Lead, o.step3Lead][step - 1] : gateLead)

  const loadSaved = useCallback(async () => {
    if (!isOwner || !lifeOn) {
      setReady(true)
      return
    }
    try {
      const persona = await agentService.getPersona()
      const name = persona?.name?.trim() ?? ''
      const personality = persona?.personality?.trim() ?? ''
      const saved =
        persona?.hasCustomPersona === true ||
        name.length > 0 ||
        personality.length > 0
      setName(name)
      if (saved && personality) {
        setSavedPersona(parseFlattenedPersona(personality))
        setResumeSaved(true)
        setStep(3)
      } else {
        setSavedPersona(null)
        setResumeSaved(false)
      }
    } catch {
      /* 预填失败就从空称呼开始 */
    } finally {
      setReady(true)
    }
  }, [isOwner, lifeOn])

  useEffect(() => {
    void loadSaved()
  }, [loadSaved])

  const handleHeaderChange = useCallback((next: OnboardingHeaderChrome) => {
    actionClickRef.current = next.action?.onClick
    setHeader((prev) => {
      if (
        prev.description === next.description &&
        sameHeaderAction(prev.action, next.action)
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
      }
    })
  }, [])

  const handleBack = useCallback(() => {
    if (lifeOn && step > 1) {
      if (resumeSaved && step === 3) {
        if (!wizardBusy) onBack()
        return
      }
      if (!wizardBusy) setStep((current) => (current - 1) as OnboardingStep)
      return
    }
    onBack()
  }, [lifeOn, onBack, resumeSaved, step, wizardBusy])

  useEffect(() => {
    if (!isOwner) return
    const next: OnboardingPageChrome = {
      title: pageTitle,
      description: pageLead,
      detailTone: lifeOn ? 'default' : 'warning',
      action: header.action
        ? {
            label: header.action.label,
            busy: header.action.busy,
            disabled: header.action.disabled,
            onClick: () => actionClickRef.current?.(),
          }
        : undefined,
      backDisabled: wizardBusy && step > 1,
      backAria:
        lifeOn && step > 1
          ? o.backTo.replace(
              '{step}',
              [o.step1Short, o.step2Short, o.step3Short][step - 2] || '',
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
    isOwner,
    lifeOn,
    o.backTo,
    o.step1Short,
    o.step2Short,
    o.step3Short,
    pageLead,
    pageTitle,
    step,
    t.common.back,
    wizardBusy,
  ])

  if (!isOwner || !ready) return null

  return lifeOn ? (
    <OnboardingWizard
      initialName={name}
      initialPersona={savedPersona ?? undefined}
      step={step}
      onStepChange={setStep}
      onBusyChange={setWizardBusy}
      onHeaderChange={handleHeaderChange}
      onFinished={onBack}
    />
  ) : null
}
