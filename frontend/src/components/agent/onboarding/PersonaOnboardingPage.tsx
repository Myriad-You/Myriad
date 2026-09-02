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
  CHOICE_STEP,
  completedPersonaResumeStep,
  parseFlattenedPersona,
  personaFromApi,
  previousOnboardingStep,
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
  /**
   * 主立绘到手之后去哪。
   *
   * 后端的立绘流水线还没走完——出图之后还要分层、编译、激活，那一段在动作
   * 工作台里。引导原本走完就退回上一层，站长得自己知道另一个子页的存在，
   * 不知道的话拿到的是一张不会动的立绘。
   */
  onFinished: () => void
  onChromeChange: (chrome: OnboardingPageChrome) => void
  meropeOn: boolean
  gateLead: string
}

export default function PersonaOnboardingPage({
  onBack,
  onFinished,
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
  const [step, setStep] = useState<OnboardingStep>(CHOICE_STEP)
  const [wizardBusy, setWizardBusy] = useState(false)
  const [header, setHeader] = useState<OnboardingHeaderChrome>({
    description: '',
  })
  const actionClickRef = useRef<(() => void) | undefined>(undefined)
  const stepBackRef = useRef<(() => boolean) | undefined>(undefined)
  const onChromeChangeRef = useRef(onChromeChange)
  onChromeChangeRef.current = onChromeChange
  const chromeRef = useRef<OnboardingPageChrome | null>(null)
  // 按 step 取，不是 step - 1：0 是分岔口、1 是导入，生成链从 2 起。
  const pageTitle = meropeOn
    ? [
        o.choiceTitle,
        o.importTitle,
        o.step1Title,
        o.step2Title,
        o.step3Title,
        o.step4Title,
        o.step5Title,
      ][step]
    : t.config.agentPersona
  const pageLead =
    header.description ||
    (meropeOn
      ? [
          o.choiceLead,
          o.importLead,
          o.step1Lead,
          o.step2Lead,
          o.step3Lead,
          o.step4Lead,
          o.step5Lead,
        ][step]
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
            // 有正文但结构化人设不完整：回到「起草人设」那页让站长补齐。
            : 4,
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

  const previousStep = meropeOn ? previousOnboardingStep(step) : null

  const handleBack = useCallback(() => {
    if (header.onBack?.()) return
    // `step - 1` 会让生成链的第一页后退到导入页——它们是分岔口的两条路，
    // 不是前后关系。上一页由 previousOnboardingStep 说了算。
    if (previousStep !== null) {
      if (!wizardBusy) setStep(previousStep)
      return
    }
    onBack()
  }, [header.onBack, onBack, previousStep, wizardBusy])

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
      backDisabled: wizardBusy && previousStep !== null,
      backAria: header.onBack
        ? o.visualBackToStyle
        : previousStep !== null
          ? o.backTo.replace(
              '{step}',
              [
                o.choiceShort,
                o.importShort,
                o.step1Short,
                o.step2Short,
                o.step3Short,
                o.step4Short,
                o.step5Short,
              ][previousStep] || '',
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
    o.choiceShort,
    o.importShort,
    o.step1Short,
    o.step2Short,
    o.step3Short,
    o.step4Short,
    o.step5Short,
    pageLead,
    pageTitle,
    previousStep,
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
      onFinished={onFinished}
    />
  ) : null
}
