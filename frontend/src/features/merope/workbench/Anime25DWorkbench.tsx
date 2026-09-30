import type { ReactNode, RefObject } from 'react'
import type { PoseCorrection } from '../anime25drig/poseCorrections'
import type { Anime25DPlayback } from '../anime25drig/types'
import type { RigCharacterHandle } from '../character/RigCharacter'
import type { AuthoredExpressionKind } from '../rig/authoredExpression'
import type { RigPath } from './RigImportPanel'
import type { RigImportSource } from './useRigImport'
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
  wardrobeLead?: ReactNode
  outfitLead?: ReactNode
  outfitRig?: boolean
  personaLead?: ReactNode
  overviewLead?: ReactNode
  motionEnabled?: boolean
  correctionPlayback?: Anime25DPlayback | null
  correctionAssetId?: string | null
  onSavePoseCorrections?: (corrections: PoseCorrection[]) => Promise<void>
}

type FacePanel = 'overview' | 'persona' | 'wardrobe' | 'motion'

export default function Anime25DWorkbench({
  characterRef,
  aiExpressions = [],
  wardrobeLead = null,
  outfitLead = null,
  outfitRig = true,
  personaLead = null,
  overviewLead = null,
  motionEnabled = false,
  correctionPlayback,
  correctionAssetId,
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
    { value: 'wardrobe', label: labels.wardrobeTitle },
    { value: 'motion', label: labels.anime25dDebug },
  ]
  const rig = useRigImport(source)
  const motion = useWorkbenchDriver(
    characterRef,
    panel === 'motion' && motionEnabled,
    `${source.sourceMasterAssetId}\n${source.sourceGenerationFingerprint ?? ''}`,
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
          onSavePoseCorrections={onSavePoseCorrections}
        />
      ) : null}
      </div>
    </>
  )
}
