import type { LocalTrack } from '../../services/localMusicApi'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useConfigI18n } from '../../contexts/I18nContext'
import { updateConfig } from '../../services/configApi'
import {
  deleteLocalTrack,
  fetchLocalLyrics,
  formatBytes,
  formatDuration,
  listLocalTracks,
  updateLocalTrack,
  uploadLocalTrack,
} from '../../services/localMusicApi'
import { uploadMedia } from '../../services/mediaApi'
import { emitAppEvent } from '../../utils/appEvents'
import { clearPlaylistCache } from '../../utils/musicPlayer'
import { userFacingError } from '../../utils/userFacingError'
import { SettingsButton } from '../settings'

const EXT_FILTERS = ['all', 'mp3', 'flac', 'ogg'] as const
const ACCEPT = '.mp3,.flac,.ogg,audio/mpeg,audio/flac,audio/ogg'
const COVER_ACCEPT = 'image/jpeg,image/png,image/webp'

type CoverDraft =
  | { mode: 'keep' }
  | { mode: 'clear' }
  | { mode: 'replace'; file: File; previewUrl: string }

async function readAudioDurationMs(file: File): Promise<number> {
  return new Promise((resolve) => {
    const url = URL.createObjectURL(file)
    const audio = new Audio()
    const done = (ms: number) => {
      URL.revokeObjectURL(url)
      resolve(ms)
    }
    audio.preload = 'metadata'
    audio.onloadedmetadata = () => {
      const d = audio.duration
      done(Number.isFinite(d) && d > 0 ? Math.round(d * 1000) : 0)
    }
    audio.onerror = () => done(0)
    audio.src = url
  })
}

export function LocalMusicManager() {
  const { t } = useConfigI18n()
  const [tracks, setTracks] = useState<LocalTrack[]>([])
  const [query, setQuery] = useState('')
  const [ext, setExt] = useState<(typeof EXT_FILTERS)[number]>('all')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const [editingTrack, setEditingTrack] = useState<LocalTrack | null>(null)
  const [coverDraft, setCoverDraft] = useState<CoverDraft>({ mode: 'keep' })
  const [lyricsDraft, setLyricsDraft] = useState('')
  const [confirmDeleteId, setConfirmDeleteId] = useState<number | null>(null)
  const fileInputRef = useRef<HTMLInputElement>(null)
  const coverInputRef = useRef<HTMLInputElement>(null)
  const coverPreviewRef = useRef<string | null>(null)

  const releaseCoverPreview = useCallback(() => {
    if (coverPreviewRef.current) {
      URL.revokeObjectURL(coverPreviewRef.current)
      coverPreviewRef.current = null
    }
  }, [])

  const closeEditor = useCallback(() => {
    releaseCoverPreview()
    setEditingTrack(null)
    setCoverDraft({ mode: 'keep' })
    setLyricsDraft('')
  }, [releaseCoverPreview])

  const openEditor = useCallback((track: LocalTrack) => {
    releaseCoverPreview()
    setEditingTrack({ ...track })
    setCoverDraft({ mode: 'keep' })
    setLyricsDraft('')
    void fetchLocalLyrics(track.id)
      .then((lrc) => setLyricsDraft(lrc))
      .catch(() => setLyricsDraft(''))
  }, [releaseCoverPreview])

  const coverPreviewUrl = useMemo(() => {
    if (coverDraft.mode === 'replace') return coverDraft.previewUrl
    if (coverDraft.mode === 'clear') return null
    return editingTrack?.coverUrl ?? null
  }, [coverDraft, editingTrack])

  const hasCover = Boolean(coverPreviewUrl)

  const refresh = useCallback(async () => {
    setBusy(true)
    setError('')
    try {
      setTracks(await listLocalTracks())
    } catch (err) {
      setError(userFacingError(err, t.errors.mediaLoadFailed))
    } finally {
      setBusy(false)
    }
  }, [t.errors.mediaLoadFailed])

  useEffect(() => {
    void refresh()
  }, [refresh])

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase()
    return tracks.filter(track => {
      if (ext !== 'all' && track.ext.toLowerCase() !== ext) return false
      if (!q) return true
      return (
        track.title.toLowerCase().includes(q)
        || track.artist.toLowerCase().includes(q)
        || track.filename.toLowerCase().includes(q)
        || track.album.toLowerCase().includes(q)
      )
    })
  }, [tracks, query, ext])

  async function onUploadFiles(files: FileList | File[]) {
    const list = Array.from(files)
    if (!list.length) return
    setBusy(true)
    setError('')
    try {
      for (const file of list) {
        const durationMs = await readAudioDurationMs(file)
        const form = new FormData()
        form.append('audio', file, file.name)
        // Leave title empty so backend fills from embedded tags.
        if (durationMs > 0) form.append('duration_ms', String(durationMs))
        await uploadLocalTrack(form)
      }
      await refresh()
    } catch (err) {
      setError(userFacingError(err, t.errors.mediaUploadFailed))
    } finally {
      setBusy(false)
    }
  }

  async function saveTrack() {
    if (!editingTrack) return
    setBusy(true)
    setError('')
    try {
      let coverMediaId: number | null = editingTrack.coverMediaId
      if (coverDraft.mode === 'clear') {
        coverMediaId = null
      } else if (coverDraft.mode === 'replace') {
        const asset = await uploadMedia(coverDraft.file)
        coverMediaId = asset.id
      }
      await updateLocalTrack(editingTrack.id, {
        title: editingTrack.title,
        artist: editingTrack.artist,
        album: editingTrack.album,
        durationMs: editingTrack.durationMs,
        audioMediaId: editingTrack.audioMediaId,
        coverMediaId,
        lyrics: lyricsDraft,
        sortOrder: editingTrack.sortOrder,
        enabled: editingTrack.enabled,
      })
      closeEditor()
      await refresh()
    } catch (err) {
      setError(userFacingError(err, t.errors.operationFailed))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="mt-3 rounded-2xl border border-black/10 dark:border-white/10 p-4 space-y-3">
      <div className="flex items-center justify-between gap-2 flex-wrap">
        <strong className="text-base font-semibold">{t.config.localMusicManagerTitle}</strong>
        <span className="text-xs opacity-60">{t.config.localMusicPlaylistHint}</span>
      </div>

      {error && <p role="alert" className="text-sm text-red-600">{error}</p>}

      <div className="flex flex-wrap items-center gap-2">
        <input
          ref={fileInputRef}
          type="file"
          multiple
          accept={ACCEPT}
          className="hidden"
          onChange={(e) => {
            if (e.target.files?.length) void onUploadFiles(e.target.files)
            e.target.value = ''
          }}
        />
        <SettingsButton
          variant="secondary"
          size="sm"
          loading={busy}
          onClick={() => fileInputRef.current?.click()}
        >
          {t.config.localMusicUpload}
        </SettingsButton>
        <SettingsButton
          variant="primary"
          size="sm"
          loading={busy}
          onClick={() => {
            setBusy(true)
            setError('')
            void (async () => {
              try {
                await updateConfig({
                  music_enabled: 'true',
                  music_source: 'local',
                  music_playlist_id: 'local',
                })
                clearPlaylistCache()
                emitAppEvent('music-player-load-playlist', {
                  playlistId: 'local',
                  source: 'local',
                  autoPlay: false,
                  timestamp: Date.now(),
                })
                await refresh()
              } catch (err) {
                setError(userFacingError(err, t.errors.operationFailed))
              } finally {
                setBusy(false)
              }
            })()
          }}
        >
          {t.config.localMusicGeneratePlaylist}
        </SettingsButton>
        <input
          value={query}
          onChange={e => setQuery(e.target.value)}
          placeholder={t.config.localMusicSearchPlaceholder}
          className="flex-1 min-w-40 px-3 py-2 rounded-lg bg-black/5 dark:bg-white/10 text-sm"
        />
        <div className="flex flex-wrap gap-1">
          {EXT_FILTERS.map(value => (
            <SettingsButton
              key={value}
              variant={ext === value ? 'primary' : 'ghost'}
              size="sm"
              onClick={() => setExt(value)}
            >
              {value === 'all' ? t.config.localMusicFilterAll : value}
            </SettingsButton>
          ))}
        </div>
      </div>

      <div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
        {filtered.map(track => (
          <div
            key={track.id}
            className="rounded-xl border border-black/10 dark:border-white/10 p-3 bg-black/[0.03] dark:bg-white/[0.04]"
          >
            {editingTrack?.id === track.id ? (
              <div className="space-y-2">
                <input
                  className="w-full px-2 py-1 rounded bg-black/5 dark:bg-white/10 text-sm"
                  value={editingTrack.title}
                  onChange={e => setEditingTrack({ ...editingTrack, title: e.target.value })}
                />
                <input
                  className="w-full px-2 py-1 rounded bg-black/5 dark:bg-white/10 text-sm"
                  value={editingTrack.artist}
                  placeholder={t.config.localMusicArtist}
                  onChange={e => setEditingTrack({ ...editingTrack, artist: e.target.value })}
                />

                {/* Cover */}
                <div className="flex items-center gap-2">
                  <span className="text-xs opacity-70 w-10 shrink-0">{t.config.localMusicCover}</span>
                  <div className="relative group">
                    {hasCover && coverPreviewUrl ? (
                      <img
                        src={coverPreviewUrl}
                        alt=""
                        className="w-14 h-14 rounded-lg object-cover border border-black/10 dark:border-white/10"
                      />
                    ) : (
                      <div className="w-14 h-14 rounded-lg border border-dashed border-black/20 dark:border-white/20 flex items-center justify-center text-[10px] opacity-50">
                        {t.config.localMusicCoverEmpty}
                      </div>
                    )}
                    {hasCover && (
                      <button
                        type="button"
                        aria-label={t.config.localMusicCoverClear}
                        title={t.config.localMusicCoverClear}
                        className="absolute -top-1.5 -right-1.5 w-5 h-5 rounded-full bg-black/70 text-white text-xs leading-none opacity-0 group-hover:opacity-100 transition-opacity"
                        onClick={() => {
                          releaseCoverPreview()
                          setCoverDraft({ mode: 'clear' })
                        }}
                      >
                        ×
                      </button>
                    )}
                  </div>
                  <SettingsButton
                    size="sm"
                    variant="secondary"
                    onClick={() => coverInputRef.current?.click()}
                  >
                    {hasCover ? t.config.localMusicCoverReplace : t.config.localMusicCoverUpload}
                  </SettingsButton>
                  <input
                    ref={coverInputRef}
                    type="file"
                    accept={COVER_ACCEPT}
                    className="hidden"
                    onChange={(e) => {
                      const file = e.target.files?.[0]
                      e.target.value = ''
                      if (!file) return
                      releaseCoverPreview()
                      const previewUrl = URL.createObjectURL(file)
                      coverPreviewRef.current = previewUrl
                      setCoverDraft({ mode: 'replace', file, previewUrl })
                    }}
                  />
                </div>

                {/* Lyrics */}
                <div className="space-y-1">
                  <div className="text-xs opacity-70">{t.config.localMusicLyrics}</div>
                  <textarea
                    className="w-full min-h-20 px-2 py-1.5 rounded bg-black/5 dark:bg-white/10 text-xs font-mono leading-4 resize-y"
                    value={lyricsDraft}
                    placeholder={t.config.localMusicLyricsEmpty}
                    onChange={e => setLyricsDraft(e.target.value)}
                  />
                </div>

                <div className="flex gap-2">
                  <SettingsButton size="sm" variant="primary" onClick={() => void saveTrack()}>
                    {t.common.save}
                  </SettingsButton>
                  <SettingsButton size="sm" variant="ghost" onClick={closeEditor}>
                    {t.common.cancel}
                  </SettingsButton>
                </div>
              </div>
            ) : (
              <>
                {/* Row 1: ext tag + title (left) · duration (right) */}
                <div className="flex items-start justify-between gap-3">
                  <div className="flex items-start gap-2 min-w-0">
                    <span className="text-[10px] uppercase px-1.5 py-0.5 rounded bg-black/10 dark:bg-white/15 shrink-0 mt-0.5">
                      {track.ext || '?'}
                    </span>
                    <div className="min-w-0">
                      <div className="font-semibold text-sm truncate leading-5">
                        {track.title}
                      </div>
                      <div className="text-xs opacity-70 truncate leading-4">
                        {track.artist || '—'}
                      </div>
                    </div>
                  </div>
                  <div className="text-xs opacity-80 shrink-0 tabular-nums leading-5">
                    {formatDuration(track.durationMs)}
                  </div>
                </div>
                {/* Row 2: size + actions */}
                <div className="mt-2 flex items-center justify-between gap-2">
                  <div className="text-[11px] opacity-50 tabular-nums">
                    {formatBytes(track.sizeBytes)}
                  </div>
                  {confirmDeleteId === track.id ? (
                    <div className="flex items-center gap-1 shrink-0">
                      <span className="text-xs opacity-80">{t.config.localMusicDeleteConfirm}</span>
                      <SettingsButton
                        size="sm"
                        variant="danger"
                        onClick={() => {
                          void deleteLocalTrack(track.id).then(() => {
                            setConfirmDeleteId(null)
                            return refresh()
                          })
                        }}
                      >
                        {t.common.delete}
                      </SettingsButton>
                      <SettingsButton
                        size="sm"
                        variant="ghost"
                        onClick={() => setConfirmDeleteId(null)}
                      >
                        {t.common.cancel}
                      </SettingsButton>
                    </div>
                  ) : (
                    <div className="flex gap-1 shrink-0">
                      <SettingsButton size="sm" variant="ghost" onClick={() => openEditor(track)}>
                        {t.common.edit}
                      </SettingsButton>
                      <SettingsButton
                        size="sm"
                        variant="danger"
                        onClick={() => setConfirmDeleteId(track.id)}
                      >
                        {t.common.delete}
                      </SettingsButton>
                    </div>
                  )}
                </div>
              </>
            )}
          </div>
        ))}
        {!filtered.length && (
          <p className="opacity-60 text-sm">{t.config.localMusicEmpty}</p>
        )}
      </div>

      {busy && <p role="status" className="text-sm opacity-70">{t.common.loading}</p>}
    </div>
  )
}
