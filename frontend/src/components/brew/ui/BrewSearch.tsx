import { LuSearch, LuX } from '@lib/icons'
import { useEffect, useId, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { useI18n } from '../../../contexts/I18nContext'
import { useManagementAccessory } from './BrewManagement'

export function BrewSearch({ value, onChange }: {
  value: string
  onChange?: (value: string) => void
}) {
  const slot = useManagementAccessory()
  const { t } = useI18n()
  const [open, setOpen] = useState(false)
  const root = useRef<HTMLDivElement>(null)
  const trigger = useRef<HTMLButtonElement>(null)
  const id = useId()
  useEffect(() => {
    if (!open) return
    const dismiss = (event: PointerEvent) => {
      if (event.target instanceof Node && !root.current?.contains(event.target)) setOpen(false)
    }
    document.addEventListener('pointerdown', dismiss)
    return () => document.removeEventListener('pointerdown', dismiss)
  }, [open])
  if (!slot || !onChange) return null
  return createPortal(
    <div className="brew-search" ref={root}>
      <button ref={trigger} type="button" className="brew-search__trigger" aria-label={t.brew.search} aria-expanded={open} aria-controls={id} onClick={() => setOpen(!open)}>
        <LuSearch aria-hidden />
      </button>
      {open && (
        <div
          className="brew-search__field"
          id={id}
          onKeyDown={(event) => {
          if (event.key === 'Escape') {
            event.stopPropagation()
            setOpen(false)
            trigger.current?.focus()
          }
        }}
        >
          <LuSearch aria-hidden />
          <input autoFocus aria-label={t.brew.searchSources} placeholder={t.brew.searchSources} value={value} onChange={(event) => onChange(event.target.value)} />
          <button type="button" aria-label={t.brew.closeSearch} onClick={() => { onChange(''); setOpen(false); trigger.current?.focus() }}><LuX aria-hidden /></button>
        </div>
      )}
    </div>, slot,
  )
}
