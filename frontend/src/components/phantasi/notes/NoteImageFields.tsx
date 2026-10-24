import { useEffect, useState } from 'react'
import { useI18n } from '../../../contexts/I18nContext'
import { NoteButton } from './NoteControls'

export interface NoteImageValues { alt: string; src: string; href: string }

/** Keep incomplete URLs local until the user applies all image settings together. */
export function NoteImageFields({ values, onApply, onFocusChange }: {
  values: NoteImageValues
  onApply: (values: NoteImageValues) => void
  onFocusChange: (focused: boolean) => void
}) {
  const { t } = useI18n()
  const [draft, setDraft] = useState(values)
  const [error, setError] = useState(false)
  useEffect(() => { setDraft(values); setError(false) }, [values.alt, values.src, values.href])
  const apply = () => {
    const src = draft.src.trim()
    const href = draft.href.trim()
    const valid = (url: string, link: boolean) => {
      try {
        const parsed = new URL(url, 'https://note.invalid/')
        return ['http:', 'https:', ...(link ? ['mailto:', 'tel:'] : [])].includes(parsed.protocol)
      } catch { return false }
    }
    if (!src || !valid(src, false) || (href && !valid(href, true))) { setError(true); return }
    const destination = (url: string) => url.replace(/[\s()]/g, ch => encodeURIComponent(ch).replace('(', '%28').replace(')', '%29'))
    onApply({ alt: draft.alt, src: destination(src), href: destination(href) })
    setError(false)
  }
  return <div className="phantasi-note__image-fields">
    {([
      ['src', t.phantasi.noteImageSource],
      ['href', t.phantasi.noteImageDestination],
      ['alt', t.phantasi.noteImageAlt],
    ] as const).map(([key, label]) => (
      <label key={key} className="note-field">
        <span className="note-field__label">{label}</span>
        <input
          className="note-input"
          value={draft[key]}
          spellCheck={key === 'alt'}
          inputMode={key === 'alt' ? 'text' : 'url'}
          onFocus={() => onFocusChange(true)}
          onBlur={() => onFocusChange(false)}
          onChange={event => setDraft(current => ({ ...current, [key]: event.target.value }))}
          onKeyDown={event => {
            if (event.key === 'Enter') { event.preventDefault(); event.stopPropagation(); apply() }
            if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); setDraft(values); setError(false); event.currentTarget.blur() }
          }}
        />
      </label>
    ))}
    <p className="note-field__hint">{t.phantasi.noteImageDestinationHint}</p>
    {error ? <p className="phantasi-note__image-error" role="alert">{t.phantasi.noteImageUrlInvalid}</p> : null}
    <NoteButton variant="tint" onClick={apply}>{t.phantasi.noteImageApply}</NoteButton>
  </div>
}
