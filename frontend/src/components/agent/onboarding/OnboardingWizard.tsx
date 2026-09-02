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
  CHOICE_STEP,
  clothingStyleFromProfile,
  emptyPersona,
  flattenPersona,
  genderFromProfile,
  GUIDED_FIRST_STEP,
  IMPORT_STEP,
  onboardingSeedsFromProfile,
  personaFromApi,
  visualIdentityFromProfile,
} from './onboardingTypes'
import BasicsStep from './steps/BasicsStep'
import CharacterVisualDesignStep from './steps/CharacterVisualDesignStep'
import ChoiceStep from './steps/ChoiceStep'
import ImportStep from './steps/ImportStep'
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
  const [importedPortraitUrl, setImportedPortraitUrl] = useState<string | null>(
    null,
  )
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
  /**
   * 从分岔口进入一条路 = 那条路从头开始。
   *
   * 两条路分开的只是界面，底下共用同一份写作状态。不在换道时清场的话：
   * 生成链起草出来的人设会让导入页的「下一步」直接亮起来（什么都没导入），
   * 而导入进来的人设又会占住 `claimedAuto.persona`，让生成链跳过自动起草。
   *
   * 名字和性别不清：它们是身份，两条路都要，用户刚填的不该被抹掉。
   */
  const enterLane = useCallback(
    (lane: OnboardingStep) => {
      // 第 4 步的自动起草走它自己的 regenBusy，不是向导的 run()，所以起草
      // 在飞的时候后退并不被拦。推进这个序号，让那份晚到的草稿落不进新的
      // 一条路——`draftPersona` 的回包正是按它判断自己是否已经过期。
      personaWriteSeq.current += 1
      invalidatePersonaAndVisual()
      setImportedPortraitUrl(null)
      onStepChange(lane)
    },
    [invalidatePersonaAndVisual, onStepChange],
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
    o.choiceTitle,
    o.importTitle,
    o.step1Title,
    o.step2Title,
    o.step3Title,
    o.step4Title,
    o.step5Title,
  ][step]

  return (
    <section className="merope-ob" aria-label={stepTitle}>
      <div className="merope-ob__card">
        <div className="merope-ob__viewport">
          <div
            key={step}
            className="merope-ob__pane sm-pane"
            data-nav={paneNav}
          >
            {step === CHOICE_STEP && (
              <ChoiceStep
                onHeaderChange={onHeaderChange}
                onGuided={() => enterLane(GUIDED_FIRST_STEP)}
                onImport={() => enterLane(IMPORT_STEP)}
              />
            )}
            {step === IMPORT_STEP && (
              <ImportStep
                displayName={displayName}
                gender={gender}
                persona={persona}
                portraitUrl={importedPortraitUrl}
                busy={busy}
                // 导入页改的是身份，不是「起草的输入」。用生成链那两个
                // updater 的话，在名字框里敲一个字就会触发
                // invalidatePersonaAndVisual，把刚导入的人设静默清空。
                onDisplayName={setDisplayName}
                onGender={setGender}
                onHeaderChange={onHeaderChange}
                onImported={(next) => {
                  // 不动 claimedAuto：那是生成链「自动起草已认领」的闩，
                  // 换道时 enterLane 会把它重置，导入这边碰它只会让两条路
                  // 的行为取决于用户顺手点过什么。
                  // personaWriteSeq 要动：它作废可能在飞的 draftPersona 回包。
                  personaWriteSeq.current += 1
                  setPersona(next)
                }}
                onPortrait={setImportedPortraitUrl}
                onSubmit={() =>
                  run(async () => {
                    // 主立绘在选中文件时就已经落库了（upload_portrait），这里只
                    // 补人设本身。不传 portraitAssetId，后端按 Keep 处理。
                    await agentService.putPersona({
                      name: displayName.trim(),
                      personality: flattenPersona(persona),
                      persona: {
                        displayName: displayName.trim(),
                        ...persona,
                      },
                      // 显式写空，和生成链一致：后端 `merge_visual_profile`
                      // 会把「缺席」的键从旧值补上，于是上一次生成留下的视觉
                      // 设定、词条、补充要求会全部跟到导入的人设身上——那份
                      // 视觉设定描述的是另一个角色。
                      visualProfile: {
                        gender: gender ?? 'unspecified',
                        language: locale,
                        visualIdentity: null,
                        clothingStyle: null,
                        sourceTags: [],
                        personaExtraRequirements: '',
                      },
                    })
                    invalidatePublicConfigCache()
                    onFinished()
                  })
                }
              />
            )}
            {step === GUIDED_FIRST_STEP && (
              <TagBubblesStep
                selected={selectedTags}
                onChange={updateSelectedTags}
                onHeaderChange={onHeaderChange}
                onNext={() => onStepChange(3)}
              />
            )}
            {step === 3 && (
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
                  onStepChange(4)
                }}
              />
            )}
            {step === 4 && (
              <PersonaEditStep
                persona={persona}
                busy={busy}
                claimAutoGenerate={claimPersona}
                onHeaderChange={onHeaderChange}
                onRegenerate={draftPersona}
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
                    return 5
                  })
                }
              />
            )}
            {step === 5 && (
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
                    return 6
                  })
                }
              />
            )}
            {step === 6 && (
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
