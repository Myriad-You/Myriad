import type { TranslationKeys } from '../../../i18n'
import type { TurnArchive } from '../turnKeyformsApi'
import { useEffect, useRef, useState } from 'react'
import { SettingsButton } from '../../../components/settings'
import { useI18n } from '../../../contexts/I18nContext'
import { ApiError } from '../../../services/api'
import { deleteTurnArchive, downloadTurnArchive, listTurnArchives, uploadTurnArchive } from '../turnKeyformsApi'

interface Props {
  /** null: the worn bust's archives; else that outfit's. */
  outfitId: string | null
  /** The portrait whose archives are shown; others' are not this one's to refit or keep. */
  masterAssetId: string
  /** Another rig action is running: nothing can be refitted meanwhile. */
  busy: boolean
  /** The Space can be reached: kept jobs can be fitted again (loading one needs nothing of it). */
  canRefit: boolean
  onRefit: (archiveId: string) => void
  /** Hands a kept job to the importer as it was (a job whose page went away before it was imported). */
  onLoad: (archiveId: string) => void
}

/**
 * The slot's kept turn-keys jobs: import one as it was, download one whole,
 * fit one again with the Space's current fit (no drawing, no decomposing), or
 * delete it. A downloaded one of this portrait can be uploaded back.
 */
export function TurnArchiveList({ outfitId, masterAssetId, busy, canRefit, onRefit, onLoad }: Props) {
  const { t, format } = useI18n()
  const labels = t.merope
  const [archives, setArchives] = useState<TurnArchive[] | null>(null)
  const [keep, setKeep] = useState(2)
  const [working, setWorking] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [revision, setRevision] = useState(0)
  /** Percent of an upload sent; null while none runs. */
  const [uploading, setUploading] = useState<number | null>(null)
  const picker = useRef<HTMLInputElement>(null)

  // Reloaded when a rig action ends (a new job may have been kept) or after a deletion.
  useEffect(() => {
    if (busy) return
    let live = true
    listTurnArchives().then(
      (found) => {
        if (!live) return
        setArchives(found.archives.filter((archive) =>
          (archive.outfitId ?? null) === outfitId && archive.sourceMasterAssetId === masterAssetId))
        setKeep(found.keepPerMaster)
      },
      () => {
        if (live) setError(labels.turnArchiveError)
      },
    )
    return () => {
      live = false
    }
  }, [busy, outfitId, masterAssetId, revision, labels.turnArchiveError])

  const act = async (id: string, action: () => Promise<void>) => {
    setWorking(id)
    setError(null)
    try {
      await action()
    } catch {
      setError(labels.turnArchiveError)
    } finally {
      setWorking(null)
    }
  }

  const upload = async (zip: File) => {
    setUploading(0)
    setError(null)
    try {
      await uploadTurnArchive(zip, outfitId, (sent, total) => setUploading(Math.floor((sent / total) * 100)))
      setRevision((value) => value + 1)
    } catch (reason) {
      const code = reason instanceof ApiError ? reason.code : undefined
      setError(
        code === 'turn_archive_other_portrait'
          ? labels.turnArchiveUploadOtherPortrait
          : code === 'turn_archive_upload_kind'
            ? (outfitId ? labels.turnArchiveUploadNotFullBody : labels.turnArchiveUploadNotBust)
            : code === 'turn_archive_upload_invalid'
              ? labels.turnArchiveUploadInvalid
              : labels.turnArchiveError,
      )
    } finally {
      setUploading(null)
    }
  }

  return (
    <section className="merope-motion-rig__status merope-turn-archives">
      <strong>{labels.turnArchivesTitle}</strong>
      <p className="merope-motion-rig__hint">{format(labels.turnArchivesHint, { keep: String(keep) })}</p>
      {archives && archives.length === 0 ? (
        <p className="merope-motion-rig__hint">{labels.turnArchivesEmpty}</p>
      ) : null}
      {archives?.map((archive) => (
        <div key={archive.id} className="merope-turn-archives__row">
          <span className="merope-turn-archives__label">
            {new Date(archive.createdAt).toLocaleString()}
            {archive.parent ? ` · ${labels.turnArchiveRefitted}` : ''}
            {archive.redrawn.length > 0 ? ` · ${format(labels.turnArchiveRedrawn, { count: String(archive.redrawn.length) })}` : ''}
            {archive.bodyMotions ? ` · ${bodyMotionsLabel(labels, archive.bodyMotions)}` : ''}
            {` · ${(archive.bytes / 1024 / 1024).toFixed(0)} MB`}
          </span>
          <span className="merope-turn-archives__actions">
            <SettingsButton
              type="button"
              size="sm"
              disabled={busy || working !== null || !archive.current}
              onClick={() => onLoad(archive.id)}
            >
              {labels.turnArchiveLoad}
            </SettingsButton>
            <SettingsButton
              type="button"
              size="sm"
              disabled={working !== null}
              loading={working === archive.id}
              onClick={() => void act(archive.id, () => downloadTurnArchive(archive.id))}
            >
              {labels.turnArchiveDownload}
            </SettingsButton>
            <SettingsButton
              type="button"
              size="sm"
              disabled={busy || working !== null || !archive.current || !canRefit}
              onClick={() => onRefit(archive.id)}
            >
              {labels.turnArchiveRefit}
            </SettingsButton>
            <SettingsButton
              type="button"
              size="sm"
              variant="danger"
              confirm={labels.turnArchiveDeleteConfirm}
              disabled={busy || working !== null}
              onClick={() =>
                void act(archive.id, async () => {
                  await deleteTurnArchive(archive.id)
                  setRevision((value) => value + 1)
                })}
            >
              {labels.turnArchiveDelete}
            </SettingsButton>
          </span>
        </div>
      ))}
      <span className="merope-turn-archives__actions">
        <input
          ref={picker}
          type="file"
          accept=".zip,application/zip"
          hidden
          onChange={(event) => {
            const zip = event.target.files?.[0]
            event.target.value = ''
            if (zip) void upload(zip)
          }}
        />
        <SettingsButton
          type="button"
          size="sm"
          disabled={busy || working !== null || uploading !== null}
          loading={uploading !== null}
          title={labels.turnArchiveUploadHint}
          onClick={() => picker.current?.click()}
        >
          {uploading === null ? labels.turnArchiveUpload : format(labels.turnArchiveUploading, { percent: String(uploading) })}
        </SettingsButton>
      </span>
      {error ? (
        <p className="merope-motion-rig__hint" role="alert">
          {error}
        </p>
      ) : null}
    </section>
  )
}

/** What a figure's archive keyed beside its head: the body's turn and weight shift, or the head alone. */
function bodyMotionsLabel(labels: TranslationKeys['merope'], motions: ReadonlyArray<'turn' | 'weight'>): string {
  if (motions.length === 0) return labels.turnArchiveHeadOnly
  return motions.map((motion) => (motion === 'turn' ? labels.rigEnhanceBodyTurn : labels.rigEnhanceBodyWeight)).join(' / ')
}
