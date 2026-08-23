import type { ReactNode, RefObject } from 'react'
import type { TranslationKeys } from '../../../i18n'
import type {
  RigAssetCompileEvent,
  RigAssetPreflight,
} from '../assets/pipeline'
import type { RigCharacterHandle } from '../rig/RigCharacter'
import type { Anime25DDebugSnapshot, Anime25DDriver } from './player'
import { useEffect, useMemo, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import {
  InfoActionCard,
  InputItem,
  SettingGroup,
  SettingsButton,
  SettingTitleTag,
  SliderItem,
  SwitchItem,
} from '../../../components/settings'
import { generationFailureMessage } from '../../../components/agent/onboarding/generationError'
import { useI18n } from '../../../contexts/I18nContext'
import { WORKBENCH_DRIVER } from './player'

interface Props {
  reviewMode: boolean
  onReviewModeChange: (reviewing: boolean) => void
  characterRef: RefObject<RigCharacterHandle | null>
  sourceMasterAssetId: string
  sourceGenerationFingerprint?: string
  seeThroughTokenConfigured: boolean
  onSaveSeeThroughToken: (token: string) => Promise<void>
  onDecomposeRigPsd: () => Promise<File>
  onPreflightRigPsd: (
    file: File,
    onStage: (event: RigAssetCompileEvent) => void,
  ) => Promise<RigAssetPreflight>
  onCommitRigPsd: (
    preflight: RigAssetPreflight,
    onStage: (event: RigAssetCompileEvent) => void,
  ) => Promise<{ partCount: number; score: number }>
  reviewDock?: HTMLElement | null
  essentialsLead?: ReactNode
  personaLead?: ReactNode
  overviewLead?: ReactNode
  motionEnabled?: boolean
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
  reviewMode,
  onReviewModeChange,
  characterRef,
  sourceMasterAssetId,
  sourceGenerationFingerprint,
  seeThroughTokenConfigured,
  onSaveSeeThroughToken,
  onDecomposeRigPsd,
  onPreflightRigPsd,
  onCommitRigPsd,
  reviewDock = null,
  essentialsLead = null,
  personaLead = null,
  overviewLead = null,
  motionEnabled = false,
}: Props) {
  const { t } = useI18n()
  const labels = t.companion
  type FacePanel = 'overview' | 'persona' | 'portrait' | 'rig' | 'motion'
  type RigPath = 'upload' | 'seeThrough'
  const [panel, setPanel] = useState<FacePanel>('overview')
  const [rigPath, setRigPath] = useState<RigPath>('upload')
  const panels: Array<{ value: FacePanel; label: string }> = [
    { value: 'overview', label: labels.overviewGroup },
    { value: 'persona', label: labels.personaGroup },
    { value: 'portrait', label: labels.portraitGroup },
    { value: 'rig', label: labels.rigGroup },
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
  driverRef.current = driver
  const [snapshot, setSnapshot] = useState<Anime25DDebugSnapshot | null>(null)
  const rigPsdInputRef = useRef<HTMLInputElement>(null)
  const [rigImportStage, setRigImportStage] =
    useState<RigAssetCompileEvent | null>(null)
  const [rigImportResult, setRigImportResult] = useState<string | null>(null)
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

  useEffect(() => {
    setRigPreflight(null)
    setRigImportResult(null)
    setRigImportError(null)
    syncedDriverRef.current = false
  }, [sourceGenerationFingerprint, sourceMasterAssetId])

  useEffect(() => {
    const timer = window.setInterval(() => {
      const next = characterRef.current?.debugSnapshot() ?? null
      setSnapshot(next)
      if (next && motionEnabled && !syncedDriverRef.current) {
        syncedDriverRef.current = true
        characterRef.current?.replaceDriver(driverRef.current)
      }
    }, 200)
    return () => window.clearInterval(timer)
  }, [characterRef, motionEnabled])

  const applyDriver = (next: Anime25DDriver) => {
    setDriver(next)
    characterRef.current?.replaceDriver(next)
  }

  const patchDriver = (partial: Partial<Anime25DDriver>) => {
    const next = { ...driver, ...partial }
    setDriver(next)
    characterRef.current?.setDriver(partial)
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
        generationFailureMessage(
          reason,
          labels.motionSeeThroughTokenFailed,
          labels.motionSeeThroughTimeout,
        ),
      )
      throw reason
    }
  }

  const preflightRigPsd = async (file: File) => {
    if (importingRig || !sourceMasterAssetId) return
    setRigImportOperation('manual')
    setRigImportStage(null)
    setRigImportResult(null)
    setRigImportError(null)
    try {
      setRigPreflight(null)
      const imported = await onPreflightRigPsd(file, setRigImportStage)
      setRigPreflight(imported)
      setRigImportResult(
        `${imported.partCount} · ${imported.report.score}/100`,
      )
    } catch (reason) {
      setRigImportError(
        generationFailureMessage(
          reason,
          'Rig PSD import failed',
          labels.motionSeeThroughTimeout,
          seeThroughErrors,
        ),
      )
    } finally {
      setRigImportOperation(null)
    }
  }

  const decomposeRigPsd = async () => {
    if (importingRig || !sourceMasterAssetId || !seeThroughTokenConfigured) {
      return
    }
    setRigImportOperation('decompose')
    setRigImportStage(null)
    setRigImportResult(null)
    setRigImportError(null)
    try {
      setRigPreflight(null)
      const file = await onDecomposeRigPsd()
      const imported = await onPreflightRigPsd(file, setRigImportStage)
      setRigPreflight(imported)
      setRigImportResult(
        `${imported.partCount} · ${imported.report.score}/100`,
      )
    } catch (reason) {
      setRigImportError(
        generationFailureMessage(
          reason,
          labels.motionSeeThroughUpstream,
          labels.motionSeeThroughTimeout,
          seeThroughErrors,
        ),
      )
    } finally {
      setRigImportOperation(null)
    }
  }

  const commitRigPsd = async () => {
    if (importingRig || !rigPreflight) return
    setRigImportOperation('commit')
    setRigImportStage(null)
    setRigImportError(null)
    try {
      const imported = await onCommitRigPsd(rigPreflight, setRigImportStage)
      setRigImportResult(`${imported.partCount} · ${imported.score}/100`)
      setRigPreflight(null)
    } catch (reason) {
      setRigImportError(
        generationFailureMessage(
          reason,
          'Rig PSD commit failed',
          labels.motionSeeThroughTimeout,
          seeThroughErrors,
        ),
      )
    } finally {
      setRigImportOperation(null)
    }
  }

  const sliders = useMemo(
    () => [
      { key: 'angleX', label: labels.anime25dHeadX, min: -1, max: 1, value: driver.angleX },
      { key: 'angleY', label: labels.anime25dHeadY, min: -1, max: 1, value: driver.angleY },
      { key: 'angleZ', label: labels.anime25dHeadZ, min: -1, max: 1, value: driver.angleZ },
      { key: 'eyeOpenL', label: labels.anime25dEyeL, min: 0, max: 1, value: driver.eyeOpenL },
      { key: 'eyeOpenR', label: labels.anime25dEyeR, min: 0, max: 1, value: driver.eyeOpenR },
      { key: 'eyeX', label: labels.anime25dEyeX, min: -1, max: 1, value: driver.eyeX },
      { key: 'eyeY', label: labels.anime25dEyeY, min: -1, max: 1, value: driver.eyeY },
      { key: 'irisScale', label: labels.anime25dPupil, min: 0.5, max: 1.3, value: driver.irisScale },
      { key: 'eyeScaleL', label: labels.anime25dEyeScaleL, min: 0.5, max: 1.5, value: driver.eyeScaleL },
      { key: 'eyeScaleR', label: labels.anime25dEyeScaleR, min: 0.5, max: 1.5, value: driver.eyeScaleR },
      { key: 'eyeEase', label: labels.anime25dEyeEase, min: 0, max: 1, value: driver.eyeEase },
      { key: 'eyeCY', label: labels.anime25dEyeCY, min: -1, max: 1, value: driver.eyeCY },
      { key: 'eyeCAng', label: labels.anime25dEyeCAng, min: -1, max: 1, value: driver.eyeCAng },
      { key: 'brow', label: labels.anime25dBrow, min: -1, max: 1, value: driver.brow },
      { key: 'browAngSym', label: labels.anime25dBrowAngSym, min: -1, max: 1, value: driver.browAngSym },
      { key: 'browAngL', label: labels.anime25dBrowL, min: -1, max: 1, value: driver.browAngL },
      { key: 'browAngR', label: labels.anime25dBrowR, min: -1, max: 1, value: driver.browAngR },
      { key: 'mouthOpen', label: labels.anime25dMouth, min: 0, max: 1, value: driver.mouthOpen },
      { key: 'mouthForm', label: labels.anime25dMouthForm, min: -1, max: 1, value: driver.mouthForm },
      { key: 'mouthCY', label: labels.anime25dMouthCY, min: -1, max: 1, value: driver.mouthCY },
      { key: 'mouthEase', label: labels.anime25dMouthEase, min: 0, max: 1, value: driver.mouthEase },
      { key: 'mouthCAng', label: labels.anime25dMouthCAng, min: -1, max: 1, value: driver.mouthCAng },
      { key: 'mouthScale', label: labels.anime25dMouthScale, min: 0.5, max: 1.5, value: driver.mouthScale },
      { key: 'fhAmp', label: labels.anime25dFhAmp, min: 0, max: 3, value: driver.fhAmp },
      { key: 'fhSoft', label: labels.anime25dFhSoft, min: 0, max: 2, value: driver.fhSoft },
      { key: 'bangL', label: labels.anime25dBangL, min: -1, max: 1, value: driver.bangL },
      { key: 'bangC', label: labels.anime25dBangC, min: -1, max: 1, value: driver.bangC },
      { key: 'bangR', label: labels.anime25dBangR, min: -1, max: 1, value: driver.bangR },
      { key: 'body', label: labels.anime25dLean, min: -1, max: 1, value: driver.body },
      { key: 'armY', label: labels.anime25dArmY, min: -1, max: 1, value: driver.armY },
      { key: 'armPos', label: labels.anime25dArmPos, min: -1, max: 1, value: driver.armPos },
      { key: 'bust', label: labels.anime25dBust, min: 0, max: 4, value: driver.bust },
      { key: 'bustY', label: labels.anime25dBustY, min: -3, max: 3, value: driver.bustY },
      { key: 'physAmp', label: labels.anime25dPhysAmp, min: 0, max: 3, value: driver.physAmp },
      { key: 'soft', label: labels.anime25dSoft, min: 0, max: 3, value: driver.soft },
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
    <div className="life-motion-home__cluster">
      <h3 className="life-motion-home__cluster-title">{title}</h3>
      {renderSliderKeys(keys)}
    </div>
  )

  const reviewBar =
    reviewMode && reviewDock
      ? createPortal(
          <div className="life-motion-home__dock-bar">
            <SettingsButton
              type="button"
              size="sm"
              onClick={() => characterRef.current?.blinkNow()}
            >
              {labels.anime25dBlinkNow}
            </SettingsButton>
            <SettingsButton
              type="button"
              size="sm"
              onClick={() =>
                patchDriver({ talk: true, mouthOpen: 0.5 })
              }
            >
              {labels.anime25dPresetTalk}
            </SettingsButton>
            <SettingsButton type="button" size="sm" onClick={resetPose}>
              {labels.anime25dResetPose}
            </SettingsButton>
            <SettingsButton
              type="button"
              size="sm"
              onClick={() => onReviewModeChange(false)}
            >
              {labels.motionReviewExit}
            </SettingsButton>
          </div>,
          reviewDock,
        )
      : null

  return (
    <>
      <FaceTabs
        ariaLabel={labels.assetGroup}
        value={panel}
        options={panels}
        onChange={setPanel}
      />
      {panel === 'overview' ? (
      <SettingGroup
        title={labels.overviewGroup}
        description={labels.overviewGroupDescription}
        id="life-motion-overview"
      >
        {overviewLead}
      </SettingGroup>
      ) : null}
      {panel === 'persona' ? (
      <SettingGroup
        title={labels.personaGroup}
        description={labels.personaGroupDescription}
        id="life-motion-persona"
      >
        {personaLead}
      </SettingGroup>
      ) : null}
      {panel === 'portrait' ? (
      <SettingGroup
        title={labels.portraitGroup}
        description={labels.portraitGroupDescription}
        id="life-motion-portrait"
      >
        {essentialsLead}
      </SettingGroup>
      ) : null}
      {panel === 'rig' ? (
      <SettingGroup
        title={labels.rigGroup}
        description={labels.rigGroupDescription}
        id="life-motion-asset"
      >
        {!sourceMasterAssetId ? (
          <p className="life-motion-home__help">{labels.assetNeedsPortrait}</p>
        ) : (
          <div className="life-motion-rig">
            <FaceTabs
              className="life-motion-rig__tabs"
              ariaLabel={labels.rigGroup}
              value={rigPath}
              options={rigPaths}
              onChange={setRigPath}
            />
            {rigPath === 'upload' ? (
              <section className="life-motion-rig__path">
                <p className="life-motion-rig__hint">{labels.rigPathUploadHint}</p>
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
              <section className="life-motion-rig__path">
                <p className="life-motion-rig__hint">
                  {labels.rigPathSeeThroughHint}
                </p>
                {seeThroughTokenConfigured ? (
                  <p className="life-motion-rig__token-ready">
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
            {rigImportStage ||
            rigImportResult ||
            rigImportError ||
            rigPreflight ? (
              <section className="life-motion-rig__status" aria-live="polite">
                <strong>{labels.rigPreflightTitle}</strong>
                {rigImportStage && !rigImportResult && !rigImportError ? (
                  <p className="life-motion-home__help" role="status">
                    {rigImportStage.stage} · {rigImportStage.status}
                  </p>
                ) : null}
                {rigImportResult ? (
                  <p className="life-motion-home__help" role="status">
                    {rigImportResult}
                  </p>
                ) : null}
                {rigPreflight && rigPreflight.report.issues.length > 0 ? (
                  <ul className="life-motion-rig__issues">
                    {rigPreflight.report.issues.slice(0, 6).map((item) => (
                      <li
                        key={`${item.code}:${item.clipId || item.boneId || ''}`}
                      >
                        {item.severity}: {item.message}
                      </li>
                    ))}
                  </ul>
                ) : null}
                {rigImportError ? (
                  <p className="life-motion-home__help" role="alert">
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
              <section className="life-motion-rig__status">
                <strong>{labels.rigReadyTitle}</strong>
                <p className="life-motion-rig__hint">{labels.rigReadyHint}</p>
                <SettingsButton
                  type="button"
                  size="sm"
                  variant="secondary"
                  onClick={() => onReviewModeChange(true)}
                >
                  {labels.motionReviewEnter}
                </SettingsButton>
              </section>
            ) : null}
          </div>
        )}
        <p className="life-character-home__credit">
          {labels.anime25dRuntimeCredit}{' '}
          <a
            href="https://github.com/852wa/Anime2.5DRig"
            target="_blank"
            rel="noreferrer"
          >
            Anime2.5DRig
          </a>
        </p>
      </SettingGroup>
      ) : null}
      {panel === 'motion' ? (
        <>
      <SettingGroup
        title={labels.expressionGroup}
        description={
          motionEnabled
            ? labels.expressionGroupDescription
            : labels.motionNeedsRig
        }
        id="life-motion-expression"
      >
        <div className="life-motion-home__chips">
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
          motionEnabled ? labels.poseGroupDescription : labels.motionNeedsRig
        }
        id="life-motion-pose"
      >
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
        id="life-motion-hair-body"
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
          'armY',
          'armPos',
          'bust',
          'bustY',
        ])}
      </SettingGroup>
      <SettingGroup
        title={labels.anime25dInspect}
        description={labels.inspectGroupDescription}
        id="life-motion-inspect"
      >
        <InfoActionCard
          copyable={false}
          title={labels.anime25dInspect}
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
                      open: snapshot.mouthOpenLayers,
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
                ]
              : undefined
          }
          empty={!snapshot}
          emptyText={labels.anime25dInspectEmpty}
        />
      </SettingGroup>
        </>
      ) : null}
      {reviewBar}
    </>
  )
}

function fillInspect(
  template: string,
  vars: Record<string, string | number>,
): string {
  return template.replace(/\{(\w+)\}/g, (_, key: string) =>
    String(vars[key] ?? ''),
  )
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
      className={['life-motion-page__tabs', className].filter(Boolean).join(' ')}
      role="tablist"
      aria-label={ariaLabel}
    >
      {options.map((item) => (
        <button
          key={item.value}
          type="button"
          role="tab"
          aria-selected={value === item.value}
          className={`life-motion-page__tab${value === item.value ? ' is-active' : ''}`}
          onClick={() => onChange(item.value)}
        >
          {item.label}
        </button>
      ))}
    </div>
  )
}

function presetLabel(
  labels: TranslationKeys['companion'],
  id: string,
): string {
  if (id === 'neutral') return labels.anime25dPresetIdle
  if (id === 'smile') return labels.anime25dPresetSmile
  if (id === 'usume') return labels.anime25dPresetUsume
  if (id === 'surprise') return labels.anime25dPresetShock
  if (id === 'jito') return labels.anime25dPresetDeadpan
  if (id === 'winkL') return labels.anime25dPresetWinkLeft
  if (id === 'winkR') return labels.anime25dPresetWinkRight
  return id
}
