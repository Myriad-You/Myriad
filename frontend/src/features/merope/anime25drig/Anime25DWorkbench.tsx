import type { ReactNode, RefObject } from 'react'
import type { TranslationKeys } from '../../../i18n'
import type {
  RigAssetCompileEvent,
  RigAssetPreflight,
} from '../assets/pipeline'
import type { RigCharacterHandle } from '../rig/RigCharacter'
import type { Anime25DDriver } from './driver'
import type { Anime25DMotionEnvelopeProbeId } from './motionEnvelope'
import type { Anime25DDebugSnapshot } from './player'
import { useEffect, useMemo, useRef, useState, useSyncExternalStore } from 'react'
import { generationFailureMessage } from '../../../components/agent/onboarding/generationError'
import {
  GitHubProjectBadge,
  InfoActionCard,
  InputItem,
  SettingGroup,
  SettingsButton,
  SettingTitleTag,
  SliderItem,
  SwitchItem,
} from '../../../components/settings'
import {
  getTourSnapshot,
  subscribeTour,
} from '../../../components/tour/tourEngine'
import { personaTourPanel } from '../../../components/tour/tourLogic'
import { useI18n } from '../../../contexts/I18nContext'
import { formatMessage, getDefaultLocale } from '../../../i18n'
import { userFacingError } from '../../../utils/userFacingError'
import { PreviewMotionScope } from '../motion/previewScope'
import {
  SEE_THROUGH_PROJECT_NAME,
  SEE_THROUGH_PROJECT_URL,
} from '../seeThroughProject'
import {
  ANIME25D_PROJECT_NAME,
  ANIME25D_PROJECT_URL,
  PERSONA_UPSTREAM_THANKS,
} from './credit'
import { WORKBENCH_DRIVER } from './driver'
import {
  ANGRY_EXPRESSION_PRESET,
  CRY_EXPRESSION_PRESET,
  DIZZY_EXPRESSION_PRESET,
  LOVESTRUCK_EXPRESSION_PRESET,
  MANIAC_EXPRESSION_PRESET,
  SILLY_EXPRESSION_PRESET,
  SPEECHLESS_EXPRESSION_PRESET,
  SQUEEZE_EXPRESSION_PRESET,
  THINKING_EXPRESSION_PRESET,
} from './expressionPresets'
import { ANIME25D_MOTION_ENVELOPE_PROBES } from './motionEnvelope'

interface Props {
  characterRef: RefObject<RigCharacterHandle | null>
  sourceMasterAssetId: string
  sourceGenerationFingerprint?: string
  seeThroughTokenConfigured: boolean
  onSaveSeeThroughToken: (token: string) => Promise<void>
  onDecomposeRigPsd: () => Promise<File>
  onPreflightRigPsd: (
    file: File,
    onStage: (event: RigAssetCompileEvent) => void,
    signal?: AbortSignal,
  ) => Promise<RigAssetPreflight>
  onCommitRigPsd: (
    preflight: RigAssetPreflight,
    onStage: (event: RigAssetCompileEvent) => void,
  ) => Promise<{ partCount: number; score: number }>
  wardrobeLead?: ReactNode
  outfitLead?: ReactNode
  outfitRig?: boolean
  personaLead?: ReactNode
  overviewLead?: ReactNode
  motionEnabled?: boolean
}

const RIG_IMPORT_STEP_ORDER: RigAssetCompileEvent['stage'][] = [
  'validate-source',
  'pack-atlas',
  'compile-preview',
  'analyze-capabilities',
  'persist-manifest',
]

type RigImportStepStatus = RigAssetCompileEvent['status'] | 'pending'
type RigImportStepState = Partial<
  Record<RigAssetCompileEvent['stage'], RigAssetCompileEvent['status']>
>

interface RigImportSummary {
  partCount: number
  score: number
  activated: boolean
}

const PRESETS: Array<{ id: string; driver: Partial<Anime25DDriver> }> = [
  {
    id: 'neutral',
    driver: {
      eyeOpenL: 1,
      eyeOpenR: 1,
      brow: 0,
      mouthOpen: 0,
      mouthForm: 0,
      irisScale: 1,
    },
  },
  {
    id: 'smile',
    driver: {
      eyeOpenL: 0,
      eyeOpenR: 0,
      brow: 0.45,
      mouthOpen: 0,
      mouthForm: 0.9,
      irisScale: 1,
    },
  },
  {
    id: 'usume',
    driver: {
      eyeOpenL: 0.5,
      eyeOpenR: 0.5,
      brow: 0.35,
      mouthOpen: 1,
      mouthForm: 0.8,
      irisScale: 1,
    },
  },
  {
    id: 'surprise',
    driver: {
      eyeOpenL: 1,
      eyeOpenR: 1,
      brow: 1,
      mouthOpen: 0.75,
      mouthForm: -0.1,
      irisScale: 0.7,
    },
  },
  {
    id: 'jito',
    driver: {
      eyeOpenL: 0.4,
      eyeOpenR: 0.4,
      brow: -0.6,
      mouthOpen: 0,
      mouthForm: -0.4,
      irisScale: 1,
    },
  },
  {
    id: 'thinking',
    driver: { ...THINKING_EXPRESSION_PRESET },
  },
  {
    id: 'dizzy',
    driver: { ...DIZZY_EXPRESSION_PRESET },
  },
  {
    id: 'squeeze',
    driver: { ...SQUEEZE_EXPRESSION_PRESET },
  },
  {
    id: 'cry',
    driver: { ...CRY_EXPRESSION_PRESET },
  },
  {
    id: 'angry',
    driver: { ...ANGRY_EXPRESSION_PRESET },
  },
  {
    id: 'speechless',
    driver: { ...SPEECHLESS_EXPRESSION_PRESET },
  },
  {
    id: 'maniac',
    driver: { ...MANIAC_EXPRESSION_PRESET },
  },
  {
    id: 'silly',
    driver: { ...SILLY_EXPRESSION_PRESET },
  },
  {
    id: 'lovestruck',
    driver: { ...LOVESTRUCK_EXPRESSION_PRESET },
  },
  {
    id: 'winkL',
    driver: {
      eyeOpenL: 0,
      eyeOpenR: 1,
      brow: 0.2,
      mouthOpen: 0.4,
      mouthForm: 0.7,
      irisScale: 1,
    },
  },
  {
    id: 'winkR',
    driver: {
      eyeOpenL: 1,
      eyeOpenR: 0,
      brow: 0.2,
      mouthOpen: 0.4,
      mouthForm: 0.7,
      irisScale: 1,
    },
  },
]

export default function Anime25DWorkbench({
  characterRef,
  sourceMasterAssetId,
  sourceGenerationFingerprint,
  seeThroughTokenConfigured,
  onSaveSeeThroughToken,
  onDecomposeRigPsd,
  onPreflightRigPsd,
  onCommitRigPsd,
  wardrobeLead = null,
  outfitLead = null,
  outfitRig = true,
  personaLead = null,
  overviewLead = null,
  motionEnabled = false,
}: Props) {
  const { t, format } = useI18n()
  const labels = t.merope
  type FacePanel = 'overview' | 'persona' | 'wardrobe' | 'motion'
  type RigPath = 'upload' | 'seeThrough'
  const [userPanel, setUserPanel] = useState<FacePanel>('overview')
  const tourPanel = useSyncExternalStore(
    subscribeTour,
    () =>
      personaTourPanel(
        getTourSnapshot().tourId,
        getTourSnapshot().step?.id ?? null,
      ),
    () => undefined,
  )
  const panel = tourPanel ?? userPanel
  const [rigPath, setRigPath] = useState<RigPath>('upload')
  const panels: Array<{ value: FacePanel; label: string }> = [
    { value: 'overview', label: labels.overviewGroup },
    { value: 'persona', label: labels.personaGroup },
    { value: 'wardrobe', label: labels.wardrobeTitle },
    { value: 'motion', label: labels.anime25dDebug },
  ]
  const rigPaths: Array<{ value: RigPath; label: string }> = [
    { value: 'upload', label: labels.rigPathUpload },
    { value: 'seeThrough', label: labels.rigPathSeeThrough },
  ]
  const seeThroughErrors = {
    see_through_token_required: labels.motionSeeThroughTokenRequired,
    see_through_busy: labels.motionSeeThroughBusy,
    see_through_auth_failed: labels.motionSeeThroughAuthFailed,
    see_through_quota_unavailable: labels.motionSeeThroughQuota,
    see_through_timeout: labels.motionSeeThroughTimeout,
    see_through_upstream_failed: labels.motionSeeThroughUpstream,
    see_through_invalid_input: labels.motionSeeThroughUpstream,
  }
  const [driver, setDriver] = useState<Anime25DDriver>({ ...WORKBENCH_DRIVER })
  const driverRef = useRef(driver)
  const syncedDriverRef = useRef(false)
  const previewScopeRef = useRef<PreviewMotionScope | null>(null)
  if (previewScopeRef.current === null) {
    previewScopeRef.current = new PreviewMotionScope()
  }
  driverRef.current = driver

  const writePreview = (write: (rig: RigCharacterHandle) => void) => {
    const scope = previewScopeRef.current
    const rig = characterRef.current
    if (!scope) return
    scope.take()
    if (!rig) return
    rig.setMotionPolicy(scope.policy())
    write(rig)
  }
  const [snapshot, setSnapshot] = useState<Anime25DDebugSnapshot | null>(null)
  const rigPsdInputRef = useRef<HTMLInputElement>(null)
  const [rigImportStage, setRigImportStage] =
    useState<RigAssetCompileEvent | null>(null)
  const [rigImportSteps, setRigImportSteps] = useState<RigImportStepState>({})
  const [rigImportResult, setRigImportResult] =
    useState<RigImportSummary | null>(null)
  const [rigImportError, setRigImportError] = useState<string | null>(null)
  const [rigPreflight, setRigPreflight] = useState<RigAssetPreflight | null>(
    null,
  )
  const [seeThroughTokenDraft, setSeeThroughTokenDraft] = useState('')
  const [seeThroughTokenError, setSeeThroughTokenError] = useState<
    string | undefined
  >()
  const [rigImportOperation, setRigImportOperation] = useState<
    'decompose' | 'manual' | 'commit' | null
  >(null)
  const importingRig = rigImportOperation !== null
  const importAbortRef = useRef<AbortController | null>(null)

  useEffect(() => {
    importAbortRef.current?.abort()
    importAbortRef.current = null
    setRigImportOperation(null)
    setRigPreflight(null)
    setRigImportStage(null)
    setRigImportSteps({})
    setRigImportResult(null)
    setRigImportError(null)
    syncedDriverRef.current = false
    return () => { importAbortRef.current?.abort() }
  }, [sourceGenerationFingerprint, sourceMasterAssetId])

  useEffect(() => {
    if (panel !== 'motion' || !motionEnabled) {
      setSnapshot(null)
      return undefined
    }
    const refresh = () => {
      const next = characterRef.current?.debugSnapshot() ?? null
      setSnapshot(next)
      if (next && !syncedDriverRef.current) {
        syncedDriverRef.current = true
        writePreview((rig) => rig.replaceDriver(driverRef.current))
      }
    }
    refresh()
    const timer = window.setInterval(refresh, 200)
    return () => window.clearInterval(timer)
  }, [characterRef, motionEnabled, panel])

  const applyDriver = (next: Anime25DDriver) => {
    setDriver(next)
    writePreview((rig) => rig.replaceDriver(next))
  }

  const patchDriver = (partial: Partial<Anime25DDriver>) => {
    const next = { ...driver, ...partial }
    setDriver(next)
    writePreview((rig) => rig.setDriver(partial))
  }

  const applyPreset = (partial: Partial<Anime25DDriver>) => {
    applyDriver({
      ...WORKBENCH_DRIVER,
      ...partial,
      idle: false,
      rand: false,
      talk: false,
      mouse: false,
    })
  }

  const resetPose = () => {
    applyDriver({ ...WORKBENCH_DRIVER })
  }

  const saveSeeThroughToken = async (token: string) => {
    if (!token || token.includes('•') || token.includes('*')) return
    setSeeThroughTokenError(undefined)
    try {
      await onSaveSeeThroughToken(token)
      setSeeThroughTokenDraft('')
    } catch (reason) {
      setSeeThroughTokenError(
        userFacingError(
          generationFailureMessage(
            reason,
            labels.motionSeeThroughTokenFailed,
            labels.motionSeeThroughTimeout,
          ),
          labels.motionSeeThroughTokenFailed,
        ),
      )
      throw reason
    }
  }

  const recordRigImportStage = (event: RigAssetCompileEvent) => {
    setRigImportStage(event)
    setRigImportSteps((current) => ({
      ...current,
      [event.stage]: event.status,
    }))
  }

  const preflightRigPsd = async (file: File) => {
    if (importingRig || !sourceMasterAssetId) return
    const controller = new AbortController()
    importAbortRef.current = controller
    setRigImportOperation('manual')
    setRigImportStage(null)
    setRigImportSteps({})
    setRigImportResult(null)
    setRigImportError(null)
    try {
      const imported = await onPreflightRigPsd(file,
        (event) => {
          if (!controller.signal.aborted) recordRigImportStage(event)
        },
        controller.signal,
      )
      controller.signal.throwIfAborted()
      setRigPreflight(imported)
      setRigImportResult({
        partCount: imported.partCount,
        score: imported.report.score,
        activated: false,
      })
    } catch (reason) {
      if (controller.signal.aborted) return
      setRigImportError(
        userFacingError(
          generationFailureMessage(
            reason,
            labels.rigImportFailed,
            labels.motionSeeThroughTimeout,
            seeThroughErrors,
          ),
          labels.rigImportFailed,
        ),
      )
    } finally {
      if (importAbortRef.current === controller && !controller.signal.aborted) {
        importAbortRef.current = null
        setRigImportOperation(null)
    }
  }
  }

  const decomposeRigPsd = async () => {
    if (importingRig || !sourceMasterAssetId || !seeThroughTokenConfigured) {
      return
    }
    const controller = new AbortController()
    importAbortRef.current = controller
    setRigImportOperation('decompose')
    setRigImportStage(null)
    setRigImportSteps({})
    setRigImportResult(null)
    setRigImportError(null)
    try {
      const file = await onDecomposeRigPsd()
      controller.signal.throwIfAborted()
      const imported = await onPreflightRigPsd(file,
        (event) => {
          if (!controller.signal.aborted) recordRigImportStage(event)
        },
        controller.signal,
      )
      controller.signal.throwIfAborted()
      setRigPreflight(imported)
      setRigImportResult({
        partCount: imported.partCount,
        score: imported.report.score,
        activated: false,
      })
    } catch (reason) {
      if (controller.signal.aborted) return
      setRigImportError(
        userFacingError(
          generationFailureMessage(
            reason,
            labels.motionSeeThroughUpstream,
            labels.motionSeeThroughTimeout,
            seeThroughErrors,
          ),
          labels.motionSeeThroughUpstream,
        ),
      )
    } finally {
      if (importAbortRef.current === controller && !controller.signal.aborted) {
        importAbortRef.current = null
        setRigImportOperation(null)
    }
  }
  }

  const commitRigPsd = async () => {
    if (importingRig || !rigPreflight) return
    setRigImportOperation('commit')
    setRigImportStage(null)
    setRigImportError(null)
    try {
      const imported = await onCommitRigPsd(rigPreflight, recordRigImportStage)
      setRigImportResult({
        partCount: imported.partCount,
        score: imported.score,
        activated: true,
      })
      setRigPreflight(null)
    } catch (reason) {
      setRigImportError(
        userFacingError(
          generationFailureMessage(
            reason,
            labels.rigCommitFailed,
            labels.motionSeeThroughTimeout,
            seeThroughErrors,
          ),
          labels.rigCommitFailed,
        ),
      )
    } finally {
      setRigImportOperation(null)
    }
  }

  const sliders = useMemo(
    () => [
      {
        key: 'angleX',
        label: labels.anime25dHeadX,
        min: -1,
        max: 1,
        value: driver.angleX,
      },
      {
        key: 'angleY',
        label: labels.anime25dHeadY,
        min: -1,
        max: 1,
        value: driver.angleY,
      },
      {
        key: 'angleZ',
        label: labels.anime25dHeadZ,
        min: -1,
        max: 1,
        value: driver.angleZ,
      },
      {
        key: 'eyeOpenL',
        label: labels.anime25dEyeL,
        min: 0,
        max: 1,
        value: driver.eyeOpenL,
      },
      {
        key: 'eyeOpenR',
        label: labels.anime25dEyeR,
        min: 0,
        max: 1,
        value: driver.eyeOpenR,
      },
      {
        key: 'eyeX',
        label: labels.anime25dEyeX,
        min: -1,
        max: 1,
        value: driver.eyeX,
      },
      {
        key: 'eyeY',
        label: labels.anime25dEyeY,
        min: -1,
        max: 1,
        value: driver.eyeY,
      },
      {
        key: 'irisScale',
        label: labels.anime25dPupil,
        min: 0.5,
        max: 1.3,
        value: driver.irisScale,
      },
      {
        key: 'eyeScaleL',
        label: labels.anime25dEyeScaleL,
        min: 0.5,
        max: 1.5,
        value: driver.eyeScaleL,
      },
      {
        key: 'eyeScaleR',
        label: labels.anime25dEyeScaleR,
        min: 0.5,
        max: 1.5,
        value: driver.eyeScaleR,
      },
      {
        key: 'eyeEase',
        label: labels.anime25dEyeEase,
        min: 0,
        max: 1,
        value: driver.eyeEase,
      },
      {
        key: 'eyeCY',
        label: labels.anime25dEyeCY,
        min: -1,
        max: 1,
        value: driver.eyeCY,
      },
      {
        key: 'eyeCAng',
        label: labels.anime25dEyeCAng,
        min: -1,
        max: 1,
        value: driver.eyeCAng,
      },
      {
        key: 'brow',
        label: labels.anime25dBrow,
        min: -1,
        max: 1,
        value: driver.brow,
      },
      {
        key: 'browAngSym',
        label: labels.anime25dBrowAngSym,
        min: -1,
        max: 1,
        value: driver.browAngSym,
      },
      {
        key: 'browAngL',
        label: labels.anime25dBrowL,
        min: -1,
        max: 1,
        value: driver.browAngL,
      },
      {
        key: 'browAngR',
        label: labels.anime25dBrowR,
        min: -1,
        max: 1,
        value: driver.browAngR,
      },
      {
        key: 'mouthOpen',
        label: labels.anime25dMouth,
        min: 0,
        max: 1,
        value: driver.mouthOpen,
      },
      {
        key: 'mouthForm',
        label: labels.anime25dMouthForm,
        min: -1,
        max: 1,
        value: driver.mouthForm,
      },
      {
        key: 'mouthCY',
        label: labels.anime25dMouthCY,
        min: -1,
        max: 1,
        value: driver.mouthCY,
      },
      {
        key: 'mouthEase',
        label: labels.anime25dMouthEase,
        min: 0,
        max: 1,
        value: driver.mouthEase,
      },
      {
        key: 'mouthCAng',
        label: labels.anime25dMouthCAng,
        min: -1,
        max: 1,
        value: driver.mouthCAng,
      },
      {
        key: 'mouthScale',
        label: labels.anime25dMouthScale,
        min: 0.5,
        max: 1.5,
        value: driver.mouthScale,
      },
      {
        key: 'fhAmp',
        label: labels.anime25dFhAmp,
        min: 0,
        max: 3,
        value: driver.fhAmp,
      },
      {
        key: 'fhSoft',
        label: labels.anime25dFhSoft,
        min: 0,
        max: 2,
        value: driver.fhSoft,
      },
      {
        key: 'bangL',
        label: labels.anime25dBangL,
        min: -1,
        max: 1,
        value: driver.bangL,
      },
      {
        key: 'bangC',
        label: labels.anime25dBangC,
        min: -1,
        max: 1,
        value: driver.bangC,
      },
      {
        key: 'bangR',
        label: labels.anime25dBangR,
        min: -1,
        max: 1,
        value: driver.bangR,
      },
      {
        key: 'body',
        label: labels.anime25dLean,
        min: -1,
        max: 1,
        value: driver.body,
      },
      {
        key: 'bodyYaw',
        label: labels.anime25dBodyYaw,
        min: 0,
        max: 1,
        value: driver.bodyYaw,
      },
      {
        key: 'armY',
        label: labels.anime25dArmY,
        min: -1,
        max: 1,
        value: driver.armY,
      },
      {
        key: 'armPos',
        label: labels.anime25dArmPos,
        min: -1,
        max: 1,
        value: driver.armPos,
      },
      {
        key: 'bust',
        label: labels.anime25dBust,
        min: 0,
        max: 4,
        value: driver.bust,
      },
      {
        key: 'bustY',
        label: labels.anime25dBustY,
        min: -3,
        max: 3,
        value: driver.bustY,
      },
      {
        key: 'physAmp',
        label: labels.anime25dPhysAmp,
        min: 0,
        max: 3,
        value: driver.physAmp,
      },
      {
        key: 'soft',
        label: labels.anime25dSoft,
        min: 0,
        max: 3,
        value: driver.soft,
      },
    ],
    [driver, labels],
  )

  const renderSliderKeys = (keys: string[]) =>
    keys.flatMap((key) => {
      const slider = sliders.find((item) => item.key === key)
      if (!slider) return []
      return [
        <SliderItem
          key={slider.key}
          itemKey={slider.key}
          label={slider.label}
          value={slider.value}
          min={slider.min}
          max={slider.max}
          step={0.01}
          formatValue={(value) => value.toFixed(2)}
          disabled={!motionEnabled}
          onChange={(value) =>
            patchDriver({ [slider.key]: value } as Partial<Anime25DDriver>)
          }
          layout="vertical"
        />,
      ]
    })

  const sliderCluster = (title: string, keys: string[]) => (
    <div className="merope-motion-home__cluster">
      <h3 className="merope-motion-home__cluster-title">{title}</h3>
      {renderSliderKeys(keys)}
    </div>
  )

  return (
    <>
      <div data-tour="config-persona-tabs">
      {panel === 'wardrobe' && outfitLead ? null : (
        <FaceTabs
          ariaLabel={labels.assetGroup}
          value={panel}
          options={panels}
          onChange={setUserPanel}
        />
      )}
      </div>
      <div data-tour="config-persona-overview">
      {panel === 'overview' ? (
        <SettingGroup
          title={labels.overviewGroup}
          description={labels.overviewGroupDescription}
          id="merope-motion-overview"
        >
          {overviewLead}
        </SettingGroup>
      ) : null}
      </div>
      <div data-tour="config-persona-identity">
      {panel === 'persona' ? (
        <SettingGroup
          title={labels.personaGroup}
          description={labels.personaGroupDescription}
          id="merope-motion-persona"
        >
          {personaLead}
        </SettingGroup>
      ) : null}
      </div>
      <div data-tour="config-persona-wardrobe">
      {panel === 'wardrobe' && !outfitLead ? (
        <SettingGroup
          title={labels.wardrobeTitle}
          description={labels.wardrobeGroupDescription}
          id="merope-motion-wardrobe"
        >
          {wardrobeLead}
        </SettingGroup>
      ) : null}
      {panel === 'wardrobe' && outfitLead ? outfitLead : null}
      {panel === 'wardrobe' && outfitLead && outfitRig ? (
        <SettingGroup
          title={labels.rigGroup}
          description={labels.rigGroupDescription}
          id="merope-motion-asset"
        >
          {!sourceMasterAssetId ? (
            <p className="merope-motion-home__help">
              {labels.assetNeedsPortrait}
            </p>
          ) : (
            <div className="merope-motion-rig">
              <FaceTabs
                className="merope-motion-rig__tabs"
                ariaLabel={labels.rigGroup}
                value={rigPath}
                options={rigPaths}
                onChange={setRigPath}
              />
              <div className="merope-character-home__credit">
                <p className="merope-character-home__credit-line">
                  <GitHubProjectBadge
                    url={SEE_THROUGH_PROJECT_URL}
                    name={SEE_THROUGH_PROJECT_NAME}
                  />
                  {labels.anime25dSeeThroughCredit}
                </p>
                <p className="merope-character-home__credit-line">
                  <GitHubProjectBadge
                    url={ANIME25D_PROJECT_URL}
                    name={ANIME25D_PROJECT_NAME}
                  />
                  {labels.anime25dRuntimeCredit}
                </p>
                <p className="merope-character-home__thanks">
                  {labels.anime25dProjectThanks}
                  {PERSONA_UPSTREAM_THANKS.map((person) => (
                    <a
                      key={person.handle}
                      href={person.url}
                      target="_blank"
                      rel="noopener noreferrer"
                    >
                      @{person.handle}
                    </a>
                  ))}
                </p>
              </div>
              {rigPath === 'upload' ? (
                <section className="merope-motion-rig__path">
                  <SettingsButton
                    type="button"
                    size="sm"
                    disabled={importingRig}
                    loading={rigImportOperation === 'manual'}
                    onClick={() => rigPsdInputRef.current?.click()}
                  >
                    {rigImportOperation === 'manual'
                      ? labels.motionPsdUploading
                      : labels.motionPsdUpload}
                  </SettingsButton>
                </section>
              ) : (
                <section className="merope-motion-rig__path">
                  {seeThroughTokenConfigured ? (
                    <p className="merope-motion-rig__token-ready">
                      {labels.rigTokenReady}
                    </p>
                  ) : null}
                  <InputItem
                    itemKey="see-through-hf-token"
                    label={labels.motionSeeThroughToken}
                    labelAccessory={
                      <SettingTitleTag
                        onClick={() =>
                          window.open(
                            'https://huggingface.co/settings/tokens',
                            '_blank',
                            'noopener,noreferrer',
                          )
                        }
                      >
                        {labels.motionSeeThroughTokenCreate}
                      </SettingTitleTag>
                    }
                    description={labels.motionSeeThroughTokenDescription}
                    value={
                      seeThroughTokenDraft ||
                      (seeThroughTokenConfigured ? '••••••••' : '')
                    }
                    onChange={(value) => {
                      setSeeThroughTokenDraft(value)
                      setSeeThroughTokenError(undefined)
                    }}
                    inputType="password"
                    autoComplete="off"
                    placeholder="hf_…"
                    variant="clickToEdit"
                    emptyLabel={labels.motionSeeThroughTokenMissing}
                    editLabel={labels.motionSeeThroughTokenEdit}
                    saveLabel={labels.motionSeeThroughTokenSave}
                    cancelLabel={labels.motionSeeThroughTokenCancel}
                    onCommit={saveSeeThroughToken}
                    error={seeThroughTokenError}
                    clearable={false}
                  />
                  <SettingsButton
                    type="button"
                    size="sm"
                    disabled={importingRig || !seeThroughTokenConfigured}
                    loading={rigImportOperation === 'decompose'}
                    onClick={() => void decomposeRigPsd()}
                  >
                    {rigImportOperation === 'decompose'
                      ? labels.motionSeeThroughGenerating
                      : labels.motionSeeThroughGenerate}
                  </SettingsButton>
                </section>
              )}
              <input
                ref={rigPsdInputRef}
                type="file"
                accept=".psd,image/vnd.adobe.photoshop"
                hidden
                onChange={(event) => {
                  const file = event.currentTarget.files?.[0]
                  event.currentTarget.value = ''
                  if (file) void preflightRigPsd(file)
                }}
              />
              {rigImportOperation === 'manual' && (
                  <SettingsButton
                    type="button"
                    size="sm"
                    onClick={() => {
                      importAbortRef.current?.abort()
                      importAbortRef.current = null
                      setRigImportOperation(null)
                      setRigImportStage(null)
                      setRigImportSteps({})
                    }}
                  >
                    {labels.motionSeeThroughTokenCancel}
                  </SettingsButton>
                )}
                {rigImportStage ||
              rigImportResult ||
              rigImportError ||
              rigPreflight ? (
                <section
                  className="merope-motion-rig__status"
                  aria-live="polite"
                >
                  <strong>{labels.rigPreflightTitle}</strong>
                  <ol className="merope-motion-rig__steps">
                    {RIG_IMPORT_STEP_ORDER.map((stage, index) => {
                      const status = rigImportSteps[stage] ?? 'pending'
                      const copy = rigImportStepCopy(labels, stage)
                      return (
                        <li
                          key={stage}
                          className={`merope-motion-rig__step is-${status}`}
                          aria-current={
                            status === 'started' ? 'step' : undefined
                          }
                        >
                          <span
                            className="merope-motion-rig__step-marker"
                            aria-hidden="true"
                          >
                            {status === 'completed' ? '✓' : index + 1}
                          </span>
                          <span className="merope-motion-rig__step-copy">
                            <b>{copy.title}</b>
                            <span>{copy.description}</span>
                          </span>
                          <span className="merope-motion-rig__step-state">
                            {rigImportStatusLabel(labels, status)}
                          </span>
                        </li>
                      )
                    })}
                  </ol>
                  {rigImportResult ? (
                    <div className="merope-motion-rig__summary" role="status">
                      <b>
                        {format(labels.rigPreflightSummary, {
                          parts: rigImportResult.partCount,
                          score: rigImportResult.score,
                        })}
                      </b>
                      <span>
                        {rigImportResult.activated
                          ? labels.rigPreflightActivated
                          : labels.rigPreflightReady}
                      </span>
                    </div>
                  ) : null}
                  {rigPreflight ? (
                    <div className="merope-motion-rig__checks">
                      <b>{labels.rigPreflightIssuesTitle}</b>
                      {rigPreflight.report.issues.length > 0 ? (
                        <ul className="merope-motion-rig__issues">
                          {rigPreflight.report.issues
                            .slice(0, 6)
                            .map((item) => (
                              <li
                                key={`${item.code}:${item.clipId || item.boneId || ''}`}
                              >
                                <span
                                  className={`merope-motion-rig__severity is-${item.severity}`}
                                >
                                  {rigDiagnosticSeverityLabel(
                                    labels,
                                    item.severity,
                                  )}
                                </span>{' '}
                                {rigDiagnosticMessage(
                                  labels,
                                  item.code,
                                  item.message,
                                )}
                              </li>
                            ))}
                        </ul>
                      ) : (
                        <p className="merope-motion-rig__hint">
                          {labels.rigPreflightNoIssues}
                        </p>
                      )}
                    </div>
                  ) : null}
                  {rigImportError ? (
                    <p className="merope-motion-home__help" role="alert">
                      {rigImportError}
                    </p>
                  ) : null}
                  {rigPreflight ? (
                    <SettingsButton
                      type="button"
                      size="sm"
                      disabled={
                        importingRig ||
                        rigPreflight.report.issues.some(
                          (item) => item.severity === 'error',
                        )
                      }
                      loading={rigImportOperation === 'commit'}
                      onClick={() => void commitRigPsd()}
                    >
                      {labels.motionPsdCommit}
                    </SettingsButton>
                  ) : null}
                </section>
              ) : null}
              {motionEnabled ? (
                <section className="merope-motion-rig__status">
                  <strong>{labels.rigReadyTitle}</strong>
                  <p className="merope-motion-rig__hint">
                    {labels.rigReadyHint}
                  </p>
                </section>
              ) : null}
            </div>
          )}
        </SettingGroup>
      ) : null}
      </div>
      <div data-tour="config-persona-motion">
      {panel === 'motion' ? (
        <>
          <SettingGroup
            title={labels.expressionGroup}
            description={
              motionEnabled
                ? labels.expressionGroupDescription
                : labels.motionNeedsRig
            }
            id="merope-motion-expression"
          >
            <div className="merope-motion-home__chips">
              <div>
                {PRESETS.map((preset) => (
                  <SettingsButton
                    key={preset.id}
                    type="button"
                    size="sm"
                    disabled={!motionEnabled}
                    onClick={() => applyPreset(preset.driver)}
                  >
                    {presetLabel(labels, preset.id)}
                  </SettingsButton>
                ))}
                <SettingsButton
                  type="button"
                  size="sm"
                  disabled={!motionEnabled}
                  onClick={() => characterRef.current?.blinkNow()}
                >
                  {labels.anime25dBlinkNow}
                </SettingsButton>
                <SettingsButton
                  type="button"
                  size="sm"
                  disabled={!motionEnabled}
                  onClick={resetPose}
                >
                  {labels.anime25dResetPose}
                </SettingsButton>
              </div>
            </div>
            <SwitchItem
              itemKey="anime25d-idle"
              label={labels.anime25dIdle}
              value={driver.idle}
              disabled={!motionEnabled}
              onChange={(idle) => patchDriver({ idle })}
            />
            <SwitchItem
              itemKey="anime25d-blink"
              label={labels.anime25dAutoBlink}
              value={driver.blink}
              disabled={!motionEnabled}
              onChange={(blink) => patchDriver({ blink })}
            />
            <SwitchItem
              itemKey="anime25d-rand"
              label={labels.anime25dRand}
              value={driver.rand}
              disabled={!motionEnabled}
              onChange={(rand) => patchDriver({ rand })}
            />
            <SwitchItem
              itemKey="anime25d-talking"
              label={labels.anime25dTalking}
              value={driver.talk}
              disabled={!motionEnabled}
              onChange={(talk) => patchDriver({ talk })}
            />
            <SwitchItem
              itemKey="anime25d-mouse"
              label={labels.anime25dMouse}
              value={driver.mouse}
              disabled={!motionEnabled}
              onChange={(mouse) => patchDriver({ mouse })}
            />
          </SettingGroup>
          <SettingGroup
            title={labels.poseGroup}
            description={
              motionEnabled
                ? labels.poseGroupDescription
                : labels.motionNeedsRig
            }
            id="merope-motion-pose"
          >
            <div className="merope-motion-home__chips">
              <div>
                {ANIME25D_MOTION_ENVELOPE_PROBES.map((probe) => (
                  <SettingsButton
                    key={probe.id}
                    type="button"
                    size="sm"
                    disabled={!motionEnabled}
                    onClick={() => applyPreset(probe.driver)}
                  >
                    {envelopeProbeLabel(labels, probe.id)}
                  </SettingsButton>
                ))}
              </div>
            </div>
            {sliderCluster(labels.clusterHead, ['angleX', 'angleY', 'angleZ'])}
            {sliderCluster(labels.clusterEyes, [
              'eyeOpenL',
              'eyeOpenR',
              'eyeX',
              'eyeY',
              'irisScale',
              'eyeScaleL',
              'eyeScaleR',
              'eyeEase',
              'eyeCY',
              'eyeCAng',
            ])}
            {sliderCluster(labels.clusterBrows, [
              'brow',
              'browAngSym',
              'browAngL',
              'browAngR',
            ])}
            {sliderCluster(labels.clusterMouth, [
              'mouthOpen',
              'mouthForm',
              'mouthCY',
              'mouthEase',
              'mouthCAng',
              'mouthScale',
            ])}
          </SettingGroup>
          <SettingGroup
            title={labels.hairBodyGroup}
            description={
              motionEnabled
                ? labels.hairBodyGroupDescription
                : labels.motionNeedsRig
            }
            id="merope-motion-hair-body"
          >
            <SwitchItem
              itemKey="anime25d-phys"
              label={labels.anime25dPhys}
              value={driver.phys}
              disabled={!motionEnabled}
              onChange={(phys) => patchDriver({ phys })}
            />
            {sliderCluster(labels.clusterHair, [
              'fhAmp',
              'fhSoft',
              'bangL',
              'bangC',
              'bangR',
              'physAmp',
              'soft',
            ])}
            {sliderCluster(labels.clusterBody, [
              'body',
              'bodyYaw',
              'armY',
              'armPos',
              'bust',
              'bustY',
            ])}
          </SettingGroup>
          <SettingGroup
            title={labels.anime25dInspect}
            description={labels.inspectGroupDescription}
            id="merope-motion-inspect"
          >
            <InfoActionCard
              copyable={false}
              fields={
                snapshot
                  ? [
                      {
                        key: 'layers',
                        label: labels.anime25dInspectLayers,
                        value: fillInspect(labels.anime25dInspectLayersValue, {
                          count: snapshot.layerCount,
                        }),
                        copyable: false,
                      },
                      {
                        key: 'strands',
                        label: labels.anime25dInspectStrands,
                        value: fillInspect(labels.anime25dInspectStrandsValue, {
                          strands: snapshot.strandCount,
                          layers: snapshot.hairLayerCount,
                        }),
                        copyable: false,
                      },
                      {
                        key: 'eyes',
                        label: labels.anime25dInspectEyes,
                        value: fillInspect(labels.anime25dInspectEyesValue, {
                          open: snapshot.eyeOpenLayers,
                          close: snapshot.eyeCloseLayers,
                        }),
                        copyable: false,
                      },
                      {
                        key: 'mouth',
                        label: labels.anime25dInspectMouth,
                        value: fillInspect(labels.anime25dInspectMouthValue, {
                          open:
                            snapshot.mouthOpenLayers +
                            snapshot.mouthWideLayers +
                            snapshot.mouthRoundLayers +
                            snapshot.mouthNarrowLayers,
                          close: snapshot.mouthCloseLayers,
                        }),
                        copyable: false,
                      },
                      {
                        key: 'canvas',
                        label: labels.anime25dInspectCanvas,
                        value: fillInspect(labels.anime25dInspectCanvasValue, {
                          width: Math.round(snapshot.canvas.width),
                          height: Math.round(snapshot.canvas.height),
                        }),
                        copyable: false,
                      },
                      {
                        key: 'motion-envelope',
                        label: labels.anime25dInspectEnvelope,
                        value: fillInspect(
                          labels.anime25dInspectEnvelopeValue,
                          {
                            pitch: Math.round(
                              snapshot.motionEnvelope.pitchLimit * 100,
                            ),
                            torso: Math.round(
                              snapshot.motionEnvelope.torsoLimit * 100,
                            ),
                            arm: Math.round(
                              snapshot.motionEnvelope.armLimit * 100,
                            ),
                            transfer: Math.round(
                              snapshot.motionEnvelope.transferredEnergy * 100,
                            ),
                          },
                        ),
                        copyable: false,
                      },
                      {
                        key: 'performance',
                        label: labels.anime25dInspectPerformance,
                        value: fillInspect(
                          labels.anime25dInspectPerformanceValue,
                          {
                            frame: snapshot.performance.frameCpuMs.toFixed(2),
                            deform: snapshot.performance.deformMs.toFixed(2),
                            upload:
                              snapshot.performance.uploadSubmitMs.toFixed(2),
                          },
                        ),
                        copyable: false,
                      },
                      {
                        key: 'workload',
                        label: labels.anime25dInspectWorkload,
                        value: fillInspect(
                          labels.anime25dInspectWorkloadValue,
                          {
                            vertices: Math.round(
                              snapshot.performance.deformedVertices,
                            ),
                            skipped: Math.round(
                              snapshot.performance.skippedVertices,
                            ),
                            kilobytes: Math.round(
                              snapshot.performance.uploadedBytes / 1024,
                            ),
                            saved: Math.round(
                              snapshot.performance.savedUploadBytes / 1024,
                            ),
                            draws: Math.round(snapshot.performance.drawCalls),
                          },
                        ),
                        copyable: false,
                      },
                    ]
                  : undefined
              }
              empty={!snapshot}
              emptyText={labels.anime25dInspectEmpty}
            />
          </SettingGroup>
        </>
      ) : null}
      </div>
    </>
  )
}

function fillInspect(
  template: string,
  vars: Record<string, string | number>,
): string {
  return formatMessage(getDefaultLocale(), template, vars)
}

function FaceTabs<T extends string>({
  ariaLabel,
  value,
  options,
  onChange,
  className,
}: {
  ariaLabel: string
  value: T
  options: Array<{ value: T; label: string }>
  onChange: (value: T) => void
  className?: string
}) {
  return (
    <div
      className={['merope-motion-page__tabs', className]
        .filter(Boolean)
        .join(' ')}
      role="tablist"
      aria-label={ariaLabel}
    >
      {options.map((item) => (
        <button
          key={item.value}
          type="button"
          role="tab"
          aria-selected={value === item.value}
          className={`merope-motion-page__tab${value === item.value ? ' is-active' : ''}`}
          onClick={() => onChange(item.value)}
        >
          {item.label}
        </button>
      ))}
    </div>
  )
}

function rigImportStepCopy(
  labels: TranslationKeys['merope'],
  stage: RigAssetCompileEvent['stage'],
): { title: string; description: string } {
  if (stage === 'validate-source') {
    return {
      title: labels.rigPreflightStepValidate,
      description: labels.rigPreflightStepValidateDescription,
    }
  }
  if (stage === 'pack-atlas') {
    return {
      title: labels.rigPreflightStepPack,
      description: labels.rigPreflightStepPackDescription,
    }
  }
  if (stage === 'compile-preview') {
    return {
      title: labels.rigPreflightStepPreview,
      description: labels.rigPreflightStepPreviewDescription,
    }
  }
  if (stage === 'analyze-capabilities') {
    return {
      title: labels.rigPreflightStepAnalyze,
      description: labels.rigPreflightStepAnalyzeDescription,
    }
  }
  return {
    title: labels.rigPreflightStepActivate,
    description: labels.rigPreflightStepActivateDescription,
  }
}

function rigImportStatusLabel(
  labels: TranslationKeys['merope'],
  status: RigImportStepStatus,
): string {
  if (status === 'started') return labels.rigPreflightStatusRunning
  if (status === 'completed') return labels.rigPreflightStatusCompleted
  if (status === 'failed') return labels.rigPreflightStatusFailed
  return labels.rigPreflightStatusPending
}

function rigDiagnosticSeverityLabel(
  labels: TranslationKeys['merope'],
  severity: 'error' | 'warning' | 'info',
): string {
  if (severity === 'error') return labels.rigDiagnosticSeverityError
  if (severity === 'warning') return labels.rigDiagnosticSeverityWarning
  return labels.rigDiagnosticSeverityInfo
}

function rigDiagnosticMessage(
  labels: TranslationKeys['merope'],
  code: string,
  fallback: string,
): string {
  if (code === 'missing-presentation-fallback') {
    return labels.rigDiagnosticMissingPresentationFallback
  }
  if (code === 'unknown-presentation-variant') {
    return labels.rigDiagnosticUnknownPresentationVariant
  }
  if (code === 'missing-head') return labels.rigDiagnosticMissingHead
  if (code === 'missing-body') return labels.rigDiagnosticMissingBody
  if (code === 'missing-mouth') return labels.rigDiagnosticMissingMouth
  if (code === 'missing-gaze') return labels.rigDiagnosticMissingGaze
  if (code === 'missing-facial-variants') {
    return labels.rigDiagnosticMissingFacialVariants
  }
  if (code === 'missing-secondary-motion') {
    return labels.rigDiagnosticMissingSecondaryMotion
  }
  if (code === 'missing-outfit-profile') {
    return labels.rigDiagnosticMissingOutfitProfile
  }
  if (code === 'missing-spatial-profile') {
    return labels.rigDiagnosticMissingSpatialProfile
  }
  if (code === 'rigid-part-deformation') {
    return labels.rigDiagnosticRigidPartDeformation
  }
  return fallback
}

function presetLabel(labels: TranslationKeys['merope'], id: string): string {
  if (id === 'neutral') return labels.anime25dPresetIdle
  if (id === 'smile') return labels.anime25dPresetSmile
  if (id === 'usume') return labels.anime25dPresetUsume
  if (id === 'surprise') return labels.anime25dPresetShock
  if (id === 'jito') return labels.anime25dPresetDeadpan
  if (id === 'thinking') return labels.anime25dPresetThinking
  if (id === 'dizzy') return labels.anime25dPresetDizzy
  if (id === 'squeeze') return labels.anime25dPresetSqueeze
  if (id === 'cry') return labels.anime25dPresetCry
  if (id === 'angry') return labels.anime25dPresetAngry
  if (id === 'speechless') return labels.anime25dPresetSpeechless
  if (id === 'maniac') return labels.anime25dPresetManiac
  if (id === 'silly') return labels.anime25dPresetSilly
  if (id === 'lovestruck') return labels.anime25dPresetLovestruck
  if (id === 'winkL') return labels.anime25dPresetWinkLeft
  if (id === 'winkR') return labels.anime25dPresetWinkRight
  return id
}

function envelopeProbeLabel(
  labels: TranslationKeys['merope'],
  id: Anime25DMotionEnvelopeProbeId,
): string {
  if (id === 'turn-left') return labels.anime25dProbeTurnLeft
  if (id === 'turn-right') return labels.anime25dProbeTurnRight
  if (id === 'pitch-up') return labels.anime25dProbePitchUp
  if (id === 'pitch-down') return labels.anime25dProbePitchDown
  if (id === 'full-left') return labels.anime25dProbeFullLeft
  return labels.anime25dProbeFullRight
}
