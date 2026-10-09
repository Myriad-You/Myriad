import type { ReactNode } from 'react'
import type { SiteFace } from '../api'
import type { RigAssetCompileEvent } from '../assets/pipeline'
import type { RigPath } from './RigImportPanel'
import { useCallback, useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { LuDownload } from 'react-icons/lu'
import { generationFailureMessage } from '../../../components/agent/onboarding/generationError'
import { SettingGroup, SettingsButton } from '../../../components/settings'
import { useI18n } from '../../../contexts/I18nContext'
import { siteMediaUrl } from '../../../utils/siteMediaUrl'
import { userFacingError } from '../../../utils/userFacingError'
import {
  generateFullBodyPortrait,
  getFullBodyFace,
  uploadFullBodyPortrait,
} from '../api'
import { commitFullBodyPsdAsset, preflightFullBodyPsdAsset } from '../assets/pipeline'
import { decomposeWithTurnKeyforms } from '../turnKeyformsApi'
import { RigImportPanel, RigModeTags } from './RigImportPanel'
import { activeRigMode } from './rigMode'
import { useRigImport } from './useRigImport'

interface Props {
  /** The full-body set in the wardrobe. */
  outfitId: string
  seeThroughTokenConfigured: boolean
  onSaveSeeThroughToken: (token: string) => Promise<void>
  /** The set's picture was redrawn, replaced or its figure saved. */
  onChanged?: () => void
  onDownload?: (url: string) => void
  /** The set is the full body worn. */
  wearing: boolean
  onWear: () => Promise<void>
  /** Sits between the picture and the actions. */
  children?: ReactNode
  /** Where the picture's buttons go (the page's header row); beside the picture when absent. */
  actionsHost?: HTMLElement | null
  /** Sits between the actions and the rig. */
  trailing?: ReactNode
}

type Operation = 'generate' | 'upload' | 'wear'

/**
 * A full-body set's page: its picture drawn or uploaded, then split into
 * a rig the way the bust's is. The panel keeps playing the bust.
 */
export function FullBodyPanel({
  outfitId,
  seeThroughTokenConfigured,
  onSaveSeeThroughToken,
  onChanged,
  onDownload,
  wearing,
  onWear,
  children,
  actionsHost,
  trailing,
}: Props) {
  const { t } = useI18n()
  const labels = t.merope
  const copy = labels.fullBody
  const [face, setFace] = useState<SiteFace | null>(null)
  const [operation, setOperation] = useState<Operation | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [rigPath, setRigPath] = useState<RigPath>('upload')
  const uploadRef = useRef<HTMLInputElement>(null)

  const reload = useCallback(async () => {
    setFace(await getFullBodyFace(outfitId))
  }, [outfitId])

  useEffect(() => {
    setError(null)
    reload().catch(() => setFace(null))
  }, [reload])

  const portrait = face?.portraitUrl ?? null
  const fingerprint = face?.generationFingerprint ?? undefined
  const activeMode = activeRigMode(face?.manifest, portrait)
  const rig = useRigImport({
    sourceMasterAssetId: portrait ?? '',
    sourceGenerationFingerprint: fingerprint,
    seeThroughTokenConfigured,
    onSaveSeeThroughToken,
    onDecomposeRigPsd: (onStatus, signal, fromArchive) =>
      decomposeWithTurnKeyforms(
        { sourceMasterAssetId: portrait ?? '', sourceGenerationFingerprint: fingerprint },
        { outfitId, fromArchive, onStatus, signal },
      ),
    // Enlarged, decomposed in tiles and stitched on the server, as a job.
    onDecomposePlainPsd: (signal, onStatus) =>
      decomposeWithTurnKeyforms(
        { sourceMasterAssetId: portrait ?? '', sourceGenerationFingerprint: fingerprint },
        { outfitId, plain: true, onStatus, signal },
      ),
    onPreflightRigPsd: (
      file: File,
      onStage: (event: RigAssetCompileEvent) => void,
      signal?: AbortSignal,
    ) =>
      preflightFullBodyPsdAsset(
        outfitId,
        file,
        portrait ?? '',
        fingerprint,
        onStage,
        signal,
      ),
    onCommitRigPsd: async (preflight, onStage) => {
      const imported = await commitFullBodyPsdAsset(outfitId, preflight, onStage)
      await reload()
      onChanged?.()
      return { partCount: imported.partCount, score: imported.report.score }
    },
  })

  const run = async (
    next: Operation,
    task: () => Promise<void>,
    fallback: string,
  ) => {
    if (operation) return
    setOperation(next)
    setError(null)
    try {
      await task()
    } catch (reason) {
      setError(
        userFacingError(
          generationFailureMessage(
            reason,
            fallback,
            labels.motionSeeThroughTimeout,
          ),
          fallback,
        ),
      )
    } finally {
      setOperation(null)
    }
  }

  const generate = () =>
    run(
      'generate',
      async () => {
        await generateFullBodyPortrait(outfitId)
        await reload()
        onChanged?.()
      },
      copy.failed,
    )

  const wear = () => run('wear', onWear, labels.wardrobeApplyFailed)

  const upload = (file: File) =>
    run(
      'upload',
      async () => {
        await uploadFullBodyPortrait(outfitId, file)
        await reload()
        onChanged?.()
      },
      labels.portraitUploadFailed,
    )

  const actions = (
    <>
      {portrait && !wearing ? (
        <SettingsButton
          type="button"
          size="sm"
          variant="primary"
          disabled={operation !== null}
          loading={operation === 'wear'}
          onClick={() => void wear()}
        >
          {labels.wardrobeWear}
        </SettingsButton>
      ) : null}
      <SettingsButton
        type="button"
        size="sm"
        disabled={operation !== null}
        loading={operation === 'generate'}
        confirm={portrait ? copy.generateConfirm : undefined}
        onClick={() => void generate()}
      >
        {operation === 'generate'
          ? copy.generating
          : portrait
            ? copy.regenerate
            : copy.generate}
      </SettingsButton>
      <SettingsButton
        type="button"
        size="sm"
        disabled={operation !== null}
        loading={operation === 'upload'}
        confirm={portrait ? copy.generateConfirm : undefined}
        onClick={() => uploadRef.current?.click()}
      >
        {operation === 'upload' ? copy.uploading : copy.upload}
      </SettingsButton>
      {portrait && onDownload ? (
        <SettingsButton
          type="button"
          size="sm"
          variant="icon"
          icon={<LuDownload size={16} />}
          aria-label={copy.download}
          title={copy.download}
          disabled={operation !== null}
          onClick={() => onDownload(portrait)}
        />
      ) : null}
    </>
  )

  return (
    <div className="merope-motion-rig merope-full-body">
      {portrait ? (
        <div className="merope-wardrobe-page__portrait merope-wardrobe-page__portrait--standing">
          <img src={siteMediaUrl(portrait)} alt={copy.portrait} draggable={false} />
        </div>
      ) : (
        <p className="merope-motion-rig__hint">{copy.none}</p>
      )}
      {children}
      {actionsHost ? createPortal(actions, actionsHost) : (
        <div className="merope-motion-asset__actions">{actions}</div>
      )}
      <input
        ref={uploadRef}
        type="file"
        accept="image/png,image/jpeg,image/webp"
        hidden
        onChange={(event) => {
          const file = event.currentTarget.files?.[0]
          event.currentTarget.value = ''
          if (file) void upload(file)
        }}
      />
      {error ? (
        <p className="merope-motion-rig__hint" role="alert">
          {error}
        </p>
      ) : null}
      {trailing}
      {portrait ? (
        <SettingGroup
          title={labels.rigGroup}
          titleExtra={<RigModeTags rig={rig} activeMode={activeMode} figure="fullBody" />}
          description={labels.rigGroupDescription}
          id="merope-motion-full-body-rig"
        >
          <RigImportPanel
            rig={rig}
            rigPath={rigPath}
            onRigPathChange={setRigPath}
            seeThroughTokenConfigured={seeThroughTokenConfigured}
            aiExpressions={[]}
            canGenerateAiExpressions={false}
            archiveOutfitId={outfitId}
            figure="fullBody"
            activeMode={activeMode}
          />
        </SettingGroup>
      ) : null}
    </div>
  )
}
