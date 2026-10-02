import type { ReactNode, RefObject } from 'react'
import type { PoseCorrection } from '../anime25drig/poseCorrections'
import type { Anime25DPlayback } from '../anime25drig/types'
import type { RigCharacterHandle } from '../character/RigCharacter'
import type { DriverSliderKey } from './motionControls'
import type { WorkbenchDriver } from './useWorkbenchDriver'
import {
  InfoActionCard,
  SettingGroup,
  SettingsButton,
  SliderItem,
  SwitchItem,
} from '../../../components/settings'
import { useI18n } from '../../../contexts/I18nContext'
import { ANIME25D_MOTION_ENVELOPE_PROBES } from '../anime25drig/motionEnvelope'
import {
  DRIVER_SLIDERS,
  envelopeProbeLabel,
  inspectFields,
  presetLabel,
  PRESETS,
} from './motionControls'
import { PoseCorrectionEditor } from './PoseCorrectionEditor'

interface Props {
  characterRef: RefObject<RigCharacterHandle | null>
  motion: WorkbenchDriver
  motionEnabled: boolean
  /** Shown above the controls. */
  lead?: ReactNode
  correctionPlayback?: Anime25DPlayback | null
  correctionAssetId?: string | null
  onSavePoseCorrections?: (corrections: PoseCorrection[]) => Promise<void>
}

/** The motion tab: expression presets, hand-set drivers, and the rig inspector. */
export function MotionPanel({
  characterRef,
  motion,
  motionEnabled,
  lead = null,
  correctionPlayback,
  correctionAssetId,
  onSavePoseCorrections,
}: Props) {
  const { t } = useI18n()
  const labels = t.merope
  const { driver, snapshot, applyDriver, patchDriver, applyPreset, resetPose } = motion

  const sliderCluster = (title: string, keys: DriverSliderKey[]) => (
    <div className="merope-motion-home__cluster">
      <h3 className="merope-motion-home__cluster-title">{title}</h3>
      {keys.flatMap((key) => {
        const slider = DRIVER_SLIDERS.find((item) => item.key === key)
        if (!slider) return []
        return [
          <SliderItem
            key={slider.key}
            itemKey={slider.key}
            label={labels[slider.label]}
            value={driver[slider.key]}
            min={slider.min}
            max={slider.max}
            step={0.01}
            formatValue={(value) => value.toFixed(2)}
            disabled={!motionEnabled}
            onChange={(value) => patchDriver({ [slider.key]: value })}
            layout="vertical"
          />,
        ]
      })}
    </div>
  )

  return (
    <>
      {lead}
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
          fields={snapshot ? inspectFields(labels, snapshot) : undefined}
          empty={!snapshot}
          emptyText={labels.anime25dInspectEmpty}
        />
      </SettingGroup>
      {motionEnabled && correctionPlayback && correctionAssetId && onSavePoseCorrections ? (
        <PoseCorrectionEditor
          key={correctionAssetId}
          playback={correctionPlayback}
          characterRef={characterRef}
          driver={driver}
          onDriver={applyDriver}
          onSave={onSavePoseCorrections}
        />
      ) : null}
    </>
  )
}
