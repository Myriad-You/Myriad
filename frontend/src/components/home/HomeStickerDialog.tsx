import type { DragEvent, FormEvent, KeyboardEvent } from 'react'
import type { StickerCrop } from '../../utils/homeStickerCrop'
import type { GuideCoords } from '../settings/settingTitleGuideLogic'
import type { WidgetSize } from '../widgetGridTypes'
import { LuPlus, LuSparkles, LuX } from '@lib/icons'
import { useEffect, useId, useLayoutEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { useI18n } from '../../contexts/I18nContext'
import {
  defaultStickerCrop,
  stickerCropForSlot,
  stickerSlotAspect,
} from '../../utils/homeStickerCrop'
import { HOME_STICKER_MAX_REFERENCES } from '../../utils/homeStickers'
import { stickerAspectKey, stickerPixelSize } from '../../utils/homeStickerSize'
import { isImeComposing } from '../../utils/ime'
import { SegmentedControl } from '../settings/items/ChoiceControls'
import { computeGuidePosition } from '../settings/settingTitleGuideLogic'
import { ButtonSpinner } from '../Spinner'
import { HomeStickerCrop } from './HomeStickerCrop'
import '../ConfigForm.css'
import '../settings/SettingTitleGuideEntry.css'
import './HomeStickerDialog.css'

export type HomeStickerMode = 'generate' | 'upload'

export interface StickerAnchorRect {
  top: number
  left: number
  width: number
  height: number
  right: number
  bottom: number
}

export interface HomeStickerDialogProps {
  size: WidgetSize
  busy: boolean
  error: string
  anchor: StickerAnchorRect
  onCancel: () => void
  onGenerate: (prompt: string, referenceImages: string[]) => void | Promise<void>
  onUpload: (image: string, crop: StickerCrop) => void | Promise<void>
}

interface ReferenceThumb {
  url: string
  name: string
}

function fitPromptField(el: HTMLTextAreaElement | null): void {
  if (!el) return
  el.style.height = 'auto'
  el.style.height = `${Math.min(el.scrollHeight, 132)}px`
}

function readImageFiles(
  files: FileList | File[] | null,
  limit: number,
  onEach: (url: string, name: string) => void,
): void {
  if (!files || limit <= 0) return
  const picked = Array.from(files)
    .filter((file) => file.type.startsWith('image/'))
    .slice(0, limit)
  for (const file of picked) {
    const reader = new FileReader()
    reader.onload = () => {
      const url = typeof reader.result === 'string' ? reader.result : ''
      if (url) onEach(url, file.name)
    }
    reader.readAsDataURL(file)
  }
}

export function HomeStickerDialog({
  size,
  busy,
  error,
  anchor,
  onCancel,
  onGenerate,
  onUpload,
}: HomeStickerDialogProps) {
  const { t } = useI18n()
  const refFileId = useId()
  const uploadFileId = useId()
  const panelRef = useRef<HTMLDivElement>(null)
  const fieldRef = useRef<HTMLTextAreaElement>(null)
  const refFileRef = useRef<HTMLInputElement>(null)
  const uploadFileRef = useRef<HTMLInputElement>(null)
  const [mode, setMode] = useState<HomeStickerMode>('generate')
  const [prompt, setPrompt] = useState('')
  const [references, setReferences] = useState<ReferenceThumb[]>([])
  const [upload, setUpload] = useState<ReferenceThumb | null>(null)
  const [crop, setCrop] = useState<StickerCrop>(defaultStickerCrop)
  const [ready, setReady] = useState(false)
  const [dropping, setDropping] = useState(false)
  const [coords, setCoords] = useState<GuideCoords>({
    top: 0,
    left: 0,
    placement: 'top',
  })

  useLayoutEffect(() => {
    const panel = panelRef.current
    if (!panel) return

    const place = () => {
      const next = computeGuidePosition(
        anchor,
        panel.offsetWidth,
        panel.offsetHeight,
        window.innerWidth,
        window.innerHeight,
      )
      setCoords(next)
    }

    place()
    const raf = requestAnimationFrame(() => setReady(true))
    const ro = new ResizeObserver(place)
    ro.observe(panel)
    window.addEventListener('resize', place)
    return () => {
      cancelAnimationFrame(raf)
      ro.disconnect()
      window.removeEventListener('resize', place)
    }
  }, [anchor])

  useEffect(() => {
    const onKey = (event: globalThis.KeyboardEvent) => {
      if (event.key !== 'Escape' || busy) return
      event.preventDefault()
      onCancel()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [busy, onCancel])

  const trimmed = prompt.trim()
  const canAddRef = mode === 'generate' && references.length < HOME_STICKER_MAX_REFERENCES && !busy
  const canSubmit =
    !busy && (mode === 'generate' ? Boolean(trimmed) : Boolean(upload))

  const appendRefs = (files: FileList | File[] | null) => {
    if (!canAddRef) return
    const remaining = HOME_STICKER_MAX_REFERENCES - references.length
    readImageFiles(files, remaining, (url, name) => {
      setReferences((prev) =>
        prev.length >= HOME_STICKER_MAX_REFERENCES
          ? prev
          : [...prev, { url, name }],
      )
    })
    if (refFileRef.current) refFileRef.current.value = ''
  }

  const setUploadFromFiles = (files: FileList | File[] | null) => {
    if (busy || mode !== 'upload') return
    readImageFiles(files, 1, (url, name) => {
      setCrop(defaultStickerCrop())
      setUpload({ url, name })
    })
    if (uploadFileRef.current) uploadFileRef.current.value = ''
  }

  const handleSubmit = (event?: FormEvent) => {
    event?.preventDefault()
    if (!canSubmit) return
    if (mode === 'upload') {
      if (upload) void onUpload(upload.url, crop)
      return
    }
    void onGenerate(
      trimmed,
      references.map((item) => item.url),
    )
  }

  const handlePromptKey = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key !== 'Enter' || event.shiftKey || isImeComposing(event)) return
    event.preventDefault()
    handleSubmit()
  }

  const handleDragOver = (event: DragEvent) => {
    const allow = mode === 'upload' ? !busy : canAddRef
    if (!allow) return
    event.preventDefault()
    setDropping(true)
  }

  const handleDragLeave = (event: DragEvent) => {
    if (event.currentTarget.contains(event.relatedTarget as Node | null)) return
    setDropping(false)
  }

  const handleDrop = (event: DragEvent) => {
    event.preventDefault()
    setDropping(false)
    if (mode === 'upload') setUploadFromFiles(event.dataTransfer.files)
    else appendRefs(event.dataTransfer.files)
  }

  if (typeof document === 'undefined') return null

  return createPortal(
    <div
      ref={panelRef}
      className={[
        'setting-title-guide-float',
        'home-sticker-guide',
        `setting-title-guide-float--${coords.placement}`,
        ready ? 'is-ready' : '',
        busy ? 'is-busy' : '',
      ]
        .filter(Boolean)
        .join(' ')}
      style={{ top: coords.top, left: coords.left }}
      role="dialog"
      aria-modal="false"
      aria-busy={busy}
      aria-label={t.home.createSticker}
      data-library-dock-chrome=""
      onDragOver={handleDragOver}
      onDragLeave={handleDragLeave}
      onDrop={handleDrop}
    >
      <form className="home-sticker-guide__body" onSubmit={handleSubmit}>
        <header className="home-sticker-guide__head">
          <h2 className="home-sticker-guide__title">{t.home.createSticker}</h2>
          <span className="home-sticker-guide__chip">
            {stickerAspectKey(size)} · {size.replace('x', '×')}
          </span>
        </header>

        <SegmentedControl
          size="sm"
          columns={2}
          ariaLabel={t.home.createSticker}
          value={mode}
          onChange={setMode}
          disabled={busy}
          options={[
            { value: 'generate', label: t.home.stickerModeGenerate },
            { value: 'upload', label: t.home.stickerModeUpload },
          ]}
        />

        {mode === 'generate' ? (
          <>
            <textarea
              ref={fieldRef}
              className="field-input"
              data-drop={dropping || undefined}
              value={prompt}
              maxLength={2000}
              rows={2}
              disabled={busy}
              placeholder={t.home.stickerPromptPlaceholder}
              autoFocus
              onChange={(event) => {
                setPrompt(event.target.value)
                fitPromptField(event.target)
              }}
              onKeyDown={handlePromptKey}
            />
            {stickerCropForSlot(
              stickerPixelSize(size).width,
              stickerPixelSize(size).height,
              size,
            ) ? (
              <p className="home-sticker-guide__desc">
                {t.home.stickerGenerateCropHint.replace(
                  '{aspect}',
                  stickerAspectKey(size),
                )}
              </p>
            ) : null}
          </>
        ) : upload ? (
          <div className="home-sticker-guide__preview">
            <HomeStickerCrop
              src={upload.url}
              aspect={stickerSlotAspect(size)}
              crop={crop}
              disabled={busy}
              hint={t.home.stickerCropHint}
              onChange={setCrop}
            />
            <button
              type="button"
              className="home-sticker-guide__ref-remove"
              aria-label={t.common.delete}
              disabled={busy}
              onClick={() => {
                setUpload(null)
                setCrop(defaultStickerCrop())
              }}
            >
              <LuX size={9} />
            </button>
          </div>
        ) : (
          <label className="home-sticker-guide__drop" htmlFor={uploadFileId} data-drop={dropping}>
            <LuPlus size={16} />
            {t.home.stickerUploadImage}
          </label>
        )}

        <div className={`home-sticker-guide__bar${mode === 'upload' ? ' is-end' : ''}`}>
          <div className="home-sticker-guide__refs">
            {mode === 'generate'
              ? references.map((item, index) => (
                  <span key={`${item.name}-${index}`} className="home-sticker-guide__ref">
                    <img src={item.url} alt="" />
                    <button
                      type="button"
                      className="home-sticker-guide__ref-remove"
                      aria-label={t.common.delete}
                      disabled={busy}
                      onClick={() =>
                        setReferences((prev) => prev.filter((_, i) => i !== index))
                      }
                    >
                      <LuX size={9} />
                    </button>
                  </span>
                ))
              : null}
            {mode === 'generate' && canAddRef ? (
              <label className="home-sticker-guide__ref-add" htmlFor={refFileId}>
                <LuPlus size={14} />
                <span className="sr-only">{t.home.stickerAddReference}</span>
              </label>
            ) : null}
            <input
              id={refFileId}
              ref={refFileRef}
              type="file"
              accept="image/png,image/jpeg,image/webp"
              multiple
              hidden
              disabled={!canAddRef}
              onChange={(event) => appendRefs(event.target.files)}
            />
            <input
              id={uploadFileId}
              ref={uploadFileRef}
              type="file"
              accept="image/png,image/jpeg,image/webp"
              hidden
              disabled={busy || mode !== 'upload'}
              onChange={(event) => setUploadFromFiles(event.target.files)}
            />
          </div>
          <div className="home-sticker-guide__actions">
            <button
              type="button"
              className="home-sticker-guide__cancel"
              disabled={busy}
              onClick={onCancel}
            >
              {t.common.cancel}
            </button>
            <button
              type="submit"
              className="home-sticker-guide__go"
              disabled={!canSubmit}
            >
              {busy ? (
                <ButtonSpinner />
              ) : mode === 'generate' ? (
                <LuSparkles size={13} />
              ) : (
                <LuPlus size={13} />
              )}
              {busy
                ? mode === 'generate'
                  ? t.home.stickerGenerating
                  : t.home.stickerUploading
                : t.home.createSticker}
            </button>
          </div>
        </div>

        {error ? (
          <p className="home-sticker-guide__error" role="alert">
            {error}
          </p>
        ) : null}
      </form>
    </div>,
    document.body,
  )
}
