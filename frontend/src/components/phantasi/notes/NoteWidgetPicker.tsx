import type { KeyboardEvent } from 'react'
import type { WidgetType } from '../../widgetGridTypes'
import { LuChevronLeft as ChevronLeft, LuSearch as Search } from '@lib/icons'
import { useEffect, useMemo, useRef, useState } from 'react'
import { useI18n } from '../../../contexts/I18nContext'
import { widgetDisplayLabel, widgetSearchExtras } from '../../widgetLibraryModel'
import { widgetTypeMatchesLibrarySearch } from '../../widgetLibrarySearch'
import { normalizeNoteWidgetSize, NOTE_WIDGET_SIZES } from './noteLayout'

interface NoteWidgetPageProps {
  widgets: WidgetType[]
  onPick: (type: string, size: string) => void
  onBack: () => void
}

/** 小组件优先放它在笔记里能用的第一个尺寸，都不认就退回默认尺寸。 */
function noteWidgetSize(widget: WidgetType): string {
  return normalizeNoteWidgetSize(
    widget.supportedSizes?.find((item) =>
      (NOTE_WIDGET_SIZES as readonly string[]).includes(item),
    ) ?? widget.defaultSize,
  )
}

/**
 * 「+」菜单的二级页：挑一个小组件。卡片沿用一级页的类名，方向键导航由外层菜单统一处理。
 * 搜索框里回车放第一个结果，空着退格回一级页。
 */
export function NoteWidgetPage({ widgets, onPick, onBack }: NoteWidgetPageProps) {
  const { t } = useI18n()
  const [query, setQuery] = useState('')
  const searchRef = useRef<HTMLInputElement>(null)
  const widgetsI18n = t.widgets as Record<string, unknown>
  const filtered = useMemo(
    () =>
      widgets.filter((widget) =>
        widgetTypeMatchesLibrarySearch(query, {
          id: widget.id,
          name: widget.name,
          label: widgetDisplayLabel(widget, widgetsI18n),
          extras: widgetSearchExtras(widget),
        }),
      ),
    [query, widgets, widgetsI18n],
  )

  useEffect(() => {
    searchRef.current?.focus()
  }, [])

  const onSearchKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'Enter') {
      event.preventDefault()
      const first = filtered[0]
      if (first) onPick(first.id, noteWidgetSize(first))
    } else if (event.key === 'Backspace' && query === '') {
      event.preventDefault()
      onBack()
    }
  }

  return (
    <>
      <div className="phantasi-note__menu-page-head">
        <button
          type="button"
          className="phantasi-note__menu-back"
          aria-label={t.phantasi.noteInsertBack}
          title={t.phantasi.noteInsertBack}
          onMouseDown={(event) => event.preventDefault()}
          onClick={onBack}
        >
          <ChevronLeft aria-hidden />
        </button>
        <span className="phantasi-note__menu-page-title">{t.phantasi.noteToolWidget}</span>
        <span className="phantasi-note__menu-page-count">{filtered.length}</span>
      </div>
      <label className="phantasi-note__menu-search">
        <Search aria-hidden />
        <input
          ref={searchRef}
          type="search"
          value={query}
          placeholder={t.phantasi.noteWidgetSearch}
          aria-label={t.phantasi.noteWidgetSearch}
          autoComplete="off"
          onChange={(event) => setQuery(event.target.value)}
          onKeyDown={onSearchKeyDown}
        />
      </label>
      {filtered.length === 0 ? (
        <p className="phantasi-note__menu-empty">{t.phantasi.noteWidgetEmpty}</p>
      ) : (
        <div className="phantasi-note__menu-group">
          {filtered.map((widget) => {
            const label = widgetDisplayLabel(widget, widgetsI18n)
            const size = noteWidgetSize(widget)
            const desc =
              widget.description?.trim() ||
              (widget.isTappWidget
                ? t.phantasi.noteWidgetSourceTapp
                : t.phantasi.noteWidgetSourceBuiltin)
            return (
              <button
                key={widget.id}
                type="button"
                role="menuitem"
                className="phantasi-note__menu-item"
                onMouseDown={(event) => event.preventDefault()}
                onClick={() => onPick(widget.id, size)}
              >
                <span className="phantasi-note__menu-icon phantasi-note__menu-icon--glyph" aria-hidden="true">
                  {Array.from(label.trim())[0]?.toUpperCase() ?? '?'}
                </span>
                <span className="phantasi-note__menu-text">
                  <span className="phantasi-note__menu-title">
                    <span className="phantasi-note__menu-label">{label}</span>
                    <kbd className="phantasi-note__menu-key">{size}</kbd>
                  </span>
                  <span className="phantasi-note__menu-desc">{desc}</span>
                </span>
              </button>
            )
          })}
        </div>
      )}
    </>
  )
}
