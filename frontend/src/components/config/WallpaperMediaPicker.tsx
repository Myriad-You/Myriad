import type { MediaAsset, MediaCursor } from '../../services/mediaApi'
import { useEffect, useState } from 'react'
import { useConfigI18n } from '../../contexts/I18nContext'
import { listMedia } from '../../services/mediaApi'
import { userFacingError } from '../../utils/userFacingError'
import { AuthenticatedMedia } from '../phantasi/skin/AuthenticatedMedia'

export function WallpaperMediaPicker({ onSelect }: { onSelect: (url: string) => void }) {
  const { t } = useConfigI18n()
  const [open, setOpen] = useState(false)
  const [items, setItems] = useState<MediaAsset[]>([])
  const [cursor, setCursor] = useState<MediaCursor>()
  const [next, setNext] = useState<MediaCursor | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  useEffect(() => {
    if (!open) return
    const controller = new AbortController()
    setBusy(true)
    setError('')
    void listMedia({ cursor }, controller.signal).then(page => {
      if (controller.signal.aborted) return
      setItems(old => cursor ? [...old, ...page.items] : page.items)
      setNext(page.next_cursor)
    }).catch(error => {
      if (!controller.signal.aborted) setError(userFacingError(error, t.errors.mediaUploadFailed))
    }).finally(() => {
      if (!controller.signal.aborted) setBusy(false)
    })
    return () => controller.abort()
  }, [open, cursor, t.errors.mediaUploadFailed])
  const images = items.filter(item => item.mime.startsWith('image/') && (!item.state || item.state === 'ready'))
  return (
    <div className="my-3">
      <button
        type="button"
        aria-expanded={open}
        className="px-3 py-2 rounded-lg bg-black/5 dark:bg-white/10"
        onClick={() => { setCursor(undefined); setOpen(value => !value) }}
      >
        {t.config.wallpaperMediaChoose}
      </button>
      {open && (
        <div className="mt-3">
          <p className="text-sm opacity-70">{t.config.wallpaperMediaPublishHint}</p>
          {error && <p role="alert">{error}</p>}
          <div className="grid grid-cols-3 gap-2 mt-2">
            {images.map(item => (
              <button
                key={item.id}
                type="button"
                title={item.name}
                aria-label={item.name}
                className="rounded-lg overflow-hidden border border-black/10 dark:border-white/10"
                onClick={() => { onSelect(item.content_path || item.url); setOpen(false) }}
              >
                <AuthenticatedMedia src={item.content_path || item.url} className="w-full h-24 object-cover" />
                <span className="block truncate text-xs p-1">{item.name}</span>
              </button>
            ))}
          </div>
          {busy && <p role="status">{t.common.loading}</p>}
          {next && !busy && <button type="button" className="mt-2 px-3 py-2" onClick={() => setCursor(next)}>{t.config.wallpaperMediaMore}</button>}
        </div>
      )}
    </div>
  )
}
