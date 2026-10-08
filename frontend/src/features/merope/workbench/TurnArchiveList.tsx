import type { TurnArchive } from '../turnKeyformsApi'
import { useEffect, useState } from 'react'
import { SettingsButton } from '../../../components/settings'
import { useI18n } from '../../../contexts/I18nContext'
import { deleteTurnArchive, downloadTurnArchive, listTurnArchives } from '../turnKeyformsApi'

interface Props {
  /** null: the worn bust's archives; else that outfit's. */
  outfitId: string | null
  /** Another rig action is running: nothing can be refitted meanwhile. */
  busy: boolean
  onRefit: (archiveId: string) => void
}

/**
 * The slot's kept turn-keys jobs: download one whole, fit one again with the
 * Space's current fit (no drawing, no decomposing), or delete it.
 */
export function TurnArchiveList({ outfitId, busy, onRefit }: Props) {
  const { t, format } = useI18n()
  const labels = t.merope
  const [archives, setArchives] = useState<TurnArchive[] | null>(null)
  const [keep, setKeep] = useState(2)
  const [working, setWorking] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [revision, setRevision] = useState(0)

  // Reloaded when a rig action ends (a new job may have been kept) or after a deletion.
  useEffect(() => {
    if (busy) return
    let live = true
    listTurnArchives().then(
      (found) => {
        if (!live) return
        setArchives(found.archives.filter((archive) => (archive.outfitId ?? null) === outfitId))
        setKeep(found.keepPerMaster)
      },
      () => {
        if (live) setError(labels.turnArchiveError)
      },
    )
    return () => {
      live = false
    }
  }, [busy, outfitId, revision, labels.turnArchiveError])

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
            {' · '}
            {archive.current ? labels.turnArchiveCurrent : labels.turnArchiveOld}
            {archive.parent ? ` · ${labels.turnArchiveRefitted}` : ''}
            {archive.redrawn.length > 0 ? ` · ${format(labels.turnArchiveRedrawn, { count: String(archive.redrawn.length) })}` : ''}
            {` · ${(archive.bytes / 1024 / 1024).toFixed(0)} MB`}
          </span>
          <span className="merope-turn-archives__actions">
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
              disabled={busy || working !== null || !archive.current}
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
      {error ? (
        <p className="merope-motion-rig__hint" role="alert">
          {error}
        </p>
      ) : null}
    </section>
  )
}
