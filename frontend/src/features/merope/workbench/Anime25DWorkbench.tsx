import type { ReactNode, RefObject } from 'react'
import type { PoseCorrection } from '../anime25drig/poseCorrections'
import type { Anime25DPlayback } from '../anime25drig/types'
import type { RigCharacterHandle } from '../character/RigCharacter'
import type { AuthoredExpressionKind } from '../rig/authoredExpression'
import type { RigPath } from './RigImportPanel'
import type { RigImportSource, RigMode } from './useRigImport'
import { useState, useSyncExternalStore } from 'react'
import { SettingGroup } from '../../../components/settings'
import {
  getTourSnapshot,
  subscribeTour,
} from '../../../components/tour/tourEngine'
import { personaTourPanel } from '../../../components/tour/tourLogic'
import { useI18n } from '../../../contexts/I18nContext'
import { FaceTabs } from './FaceTabs'
import { MotionPanel } from './MotionPanel'
import { RigImportPanel } from './RigImportPanel'
import { useRigImport } from './useRigImport'
import { useWorkbenchDriver } from './useWorkbenchDriver'

interface Props extends RigImportSource {
  characterRef: RefObject<RigCharacterHandle | null>
  /** Expressions the image model has redrawn for the current portrait. */
  aiExpressions?: readonly AuthoredExpressionKind[]
  /** The mode the worn rig was made in; null when this portrait has none yet. */
  activeRigMode?: RigMode | null
  wardrobeLead?: ReactNode
  outfitLead?: ReactNode
  outfitRig?: boolean
  personaLead?: ReactNode
  /** Sits beside the persona page's title. */
  personaTitleExtra?: ReactNode
  /** Her mind, read only; the tab shows only when given. */
  mindLead?: ReactNode
  overviewLead?: ReactNode
  motionEnabled?: boolean
  /** Sits after the tabs; picks which figure the stage plays. */
  stageSwitch?: ReactNode
  /** Changes when the stage plays another figure; the controls are re-sent to it. */
  stageKey?: string
  correctionPlayback?: Anime25DPlayback | null
  correctionAssetId?: string | null
  correctionTarget?: string
  onSavePoseCorrections?: (corrections: PoseCorrection[]) => Promise<void>
}

type FacePanel = 'overview' | 'persona' | 'mind' | 'wardrobe' | 'motion'

export default function Anime25DWorkbench({
  characterRef,
  aiExpressions = [],
  activeRigMode = null,
  wardrobeLead = null,
  outfitLead = null,
  outfitRig = true,
  personaLead = null,
  personaTitleExtra = null,
  mindLead = null,
  overviewLead = null,
  motionEnabled = false,
  stageSwitch = null,
  stageKey = '',
  correctionPlayback,
  correctionAssetId,
  correctionTarget,
  onSavePoseCorrections,
  ...source
}: Props) {
  const { t } = useI18n()
  const labels = t.merope
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
    ...(mindLead ? [{ value: 'mind' as const, label: labels.mind.title }] : []),
    { value: 'wardrobe', label: labels.wardrobeTitle },
    { value: 'motion', label: labels.anime25dDebug },
  ]
  const rig = useRigImport(source)
  const motion = useWorkbenchDriver(
    characterRef,
    panel === 'motion' && motionEnabled,
    `${source.sourceMasterAssetId}\n${source.sourceGenerationFingerprint ?? ''}\n${stageKey}`,
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
          trailing={stageSwitch}
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
          titleExtra={personaTitleExtra}
          id="merope-motion-persona"
        >
          {personaLead}
        </SettingGroup>
      ) : null}
      </div>
      {panel === 'mind' ? mindLead : null}
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
          {!source.sourceMasterAssetId ? (
            <p className="merope-motion-home__help">
              {labels.assetNeedsPortrait}
            </p>
          ) : (
            <RigImportPanel
              rig={rig}
              rigPath={rigPath}
              onRigPathChange={setRigPath}
              seeThroughTokenConfigured={source.seeThroughTokenConfigured}
              aiExpressions={aiExpressions}
              canGenerateAiExpressions={Boolean(source.onGenerateAiExpressions)}
              activeMode={activeRigMode}
              motionEnabled={motionEnabled}
            />
          )}
        </SettingGroup>
      ) : null}
      </div>
      <div data-tour="config-persona-motion">
      {panel === 'motion' ? (
        <MotionPanel
          characterRef={characterRef}
          motion={motion}
          motionEnabled={motionEnabled}
          correctionPlayback={correctionPlayback}
          correctionAssetId={correctionAssetId}
          correctionTarget={correctionTarget}
          onSavePoseCorrections={onSavePoseCorrections}
        />
      ) : null}
      </div>
    </>
  )
}
