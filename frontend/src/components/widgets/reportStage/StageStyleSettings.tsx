import type { KeyboardEvent as ReactKeyboardEvent } from 'react'
import type {
  ReportStyle,
  ReportStyleLength,
  ReportStylePreview,
  ReportStylesState,
  ReportStyleVoice,
} from '../../../utils/reportStyles'
import { LuChevronDown, LuChevronUp } from '@lib/icons'
import { useCallback, useEffect, useId, useRef, useState } from 'react'
import { useI18n } from '../../../contexts/I18nContext'
import { ApiError } from '../../../services/api'
import {
  fetchReportStyles,
  newCustomStyleId,
  previewReportStyle,
  saveReportStyles,
} from '../../../utils/reportStyles'
import { showToast } from '../../../utils/toastManager'
import { userFacingError } from '../../../utils/userFacingError'
import { cleanStageText } from './stageScript'

const TOAST_KEY = 'report-stage-style'
const VOICES: ReportStyleVoice[] = ['narrator', 'owner']
const LENGTHS: ReportStyleLength[] = ['short', 'standard', 'long']
const NEW_STYLE_CHAPTERS = ['portrait', 'taste', 'highlight']
/** 第一次点删除后，多久内再点一次才算数。 */
const DELETE_CONFIRM_MS = 4000

/** 后端按字符（码点）数限长；maxLength 按 UTF-16 算，emoji 会被多算，所以自己截。 */
function clip(text: string, max: number): string {
  const chars = Array.from(text)
  return chars.length > max ? chars.slice(0, max).join('') : text
}

/** 单选的一排按钮：方向键在选项间移动并选中，只有选中的那个进 Tab 顺序。 */
function Segmented<T extends string>({
  options,
  value,
  labelledBy,
  label,
  onChange,
}: {
  options: readonly T[]
  value: T
  labelledBy: string
  label: (option: T) => string
  onChange: (option: T) => void
}) {
  const groupRef = useRef<HTMLDivElement>(null)
  const onKeyDown = (event: ReactKeyboardEvent) => {
    const step =
      event.key === 'ArrowRight' || event.key === 'ArrowDown'
        ? 1
        : event.key === 'ArrowLeft' || event.key === 'ArrowUp'
          ? -1
          : 0
    if (!step) return
    event.preventDefault()
    // 方向键归这一组，不让舞台拿去换幕。
    event.stopPropagation()
    const index = (options.indexOf(value) + step + options.length) % options.length
    onChange(options[index])
    groupRef.current?.querySelectorAll<HTMLButtonElement>('[role="radio"]')[index]?.focus()
  }
  return (
    <div ref={groupRef} className="report-stage__segmented" role="radiogroup" aria-labelledby={labelledBy} onKeyDown={onKeyDown}>
      {options.map((option) => (
        <button
          key={option}
          type="button"
          role="radio"
          aria-checked={value === option}
          tabIndex={value === option ? 0 : -1}
          onClick={() => onChange(option)}
        >
          {label(option)}
        </button>
      ))}
    </div>
  )
}

/** 编辑中的风格。「不想出现的说法」按一行一条的原文存，保存时才拆。 */
interface Draft extends Omit<ReportStyle, 'avoid'> {
  avoidText: string
  isNew: boolean
}

function toDraft(style: ReportStyle, isNew: boolean): Draft {
  const { avoid, ...rest } = style
  return { ...rest, sample: style.sample ?? '', avoidText: avoid.join('\n'), isNew }
}

function fromDraft(draft: Draft): ReportStyle {
  const sample = draft.sample?.trim()
  return {
    id: draft.id,
    name: draft.name.trim(),
    voice: draft.voice,
    tone: draft.tone.trim(),
    length: draft.length,
    chapters: draft.chapters,
    avoid: draft.avoidText
      .split('\n')
      .map((line) => line.trim())
      .filter(Boolean),
    ...(sample ? { sample } : {}),
  }
}

/** 站长区里的报告风格：选一套、复制或新建一套、按当前平台试写一幕。 */
export function StageStyleSettings({
  platform,
  onStyleChanged,
}: {
  platform: string
  /** 当前风格变了：报告上的「风格已变」要跟着刷新。 */
  onStyleChanged: () => void
}) {
  const { t, format } = useI18n()
  const copy = t.reportStage
  const builtinCopy = copy.builtinStyles as Record<string, { name: string; desc: string } | undefined>
  const chapterNames = copy.chapters as Record<string, string | undefined>
  const chapterMeanings = copy.chapterMeanings as Record<string, string | undefined>
  const [state, setState] = useState<ReportStylesState | null>(null)
  const [loadError, setLoadError] = useState<string | null>(null)
  const [saving, setSaving] = useState(false)
  const [draft, setDraft] = useState<Draft | null>(null)
  const [trial, setTrial] = useState<ReportStylePreview | null>(null)
  const [trying, setTrying] = useState(false)
  const [pendingDelete, setPendingDelete] = useState<string | null>(null)
  // 每换一份草稿加一：迟到的试写结果不落到别的草稿下面。
  const trialSeq = useRef(0)
  const id = useId()

  const loadStyles = useCallback(() => {
    let cancelled = false
    setLoadError(null)
    fetchReportStyles()
      .then((loaded) => {
        if (!cancelled) setState(loaded)
      })
      .catch((err) => {
        if (!cancelled) setLoadError(userFacingError(err, copy.styleLoadFailed))
      })
    return () => {
      cancelled = true
    }
  }, [copy.styleLoadFailed])
  useEffect(loadStyles, [loadStyles])

  useEffect(() => {
    if (!pendingDelete) return undefined
    const timer = window.setTimeout(setPendingDelete, DELETE_CONFIRM_MS, null)
    return () => window.clearTimeout(timer)
  }, [pendingDelete])

  if (!state) {
    return loadError ? (
      <div className="report-stage__styles-error" role="alert">
        <span>{loadError}</span>
        <button type="button" className="report-stage__chip" onClick={() => loadStyles()}>
          {t.common.retry}
        </button>
      </div>
    ) : null
  }

  const isBuiltin = (style: ReportStyle) => state.builtin.some((item) => item.id === style.id)
  const nameOf = (style: ReportStyle) => (isBuiltin(style) ? builtinCopy[style.id]?.name : undefined) ?? style.name
  const describe = (style: ReportStyle) =>
    (isBuiltin(style) ? builtinCopy[style.id]?.desc : undefined) ||
    style.tone ||
    style.chapters.map((key) => chapterNames[key] ?? key).join(' · ')
  const atLimit = state.custom.length >= state.limits.custom_styles
  const takenIds = [...state.builtin, ...state.custom].map((style) => style.id)

  const persist = async (current: string, custom: ReportStyle[], message: string) => {
    setSaving(true)
    try {
      const next = await saveReportStyles(state.revision, current, custom)
      const changed = next.current_hash !== state.current_hash
      setState(next)
      showToast({ message, type: 'success', replaceKey: TOAST_KEY })
      if (changed) onStyleChanged()
      return true
    } catch (err) {
      showToast({ message: userFacingError(err, copy.styleSaveFailed), type: 'error', replaceKey: TOAST_KEY })
      // 别处（另一个标签页、另一台设备）先改过：换成最新的，草稿留着可以再存一次。
      if (err instanceof ApiError && err.code === 'report_styles_conflict') {
        fetchReportStyles().then(setState, () => {})
      }
      return false
    } finally {
      setSaving(false)
    }
  }

  const startDraft = (next: Draft | null) => {
    trialSeq.current += 1
    setDraft(next)
    setTrial(null)
    setTrying(false)
  }

  const duplicate = (style: ReportStyle) =>
    startDraft(
      toDraft(
        {
          ...style,
          id: newCustomStyleId(takenIds),
          name: clip(format(copy.copyOf, { name: nameOf(style) }), state.limits.name_chars),
        },
        true,
      ),
    )

  const remove = (style: ReportStyle) => {
    if (pendingDelete !== style.id) {
      setPendingDelete(style.id)
      return
    }
    setPendingDelete(null)
    void persist(
      state.current === style.id ? 'default' : state.current,
      state.custom.filter((item) => item.id !== style.id),
      format(copy.styleDeleted, { name: nameOf(style) }),
    )
  }

  const saveDraft = async () => {
    if (!draft) return
    const style = fromDraft(draft)
    const custom = draft.isNew
      ? [...state.custom, style]
      : state.custom.map((item) => (item.id === style.id ? style : item))
    if (await persist(state.current, custom, copy.styleSaved)) startDraft(null)
  }

  const tryDraft = async () => {
    if (!draft || trying) return
    const seq = trialSeq.current
    setTrying(true)
    setTrial(null)
    try {
      const result = await previewReportStyle(platform, fromDraft(draft))
      if (seq === trialSeq.current) setTrial(result)
    } catch (err) {
      if (seq === trialSeq.current) {
        showToast({ message: userFacingError(err, copy.generateFailed), type: 'error', replaceKey: TOAST_KEY })
      }
    } finally {
      if (seq === trialSeq.current) setTrying(false)
    }
  }

  if (draft) {
    const update = (patch: Partial<Draft>) => setDraft({ ...draft, ...patch })
    const selected = draft.chapters
    const ordered = [
      ...selected,
      ...state.chapter_keys.map((item) => item.key).filter((key) => !selected.includes(key)),
    ]
    const move = (key: string, delta: -1 | 1) => {
      const from = selected.indexOf(key)
      const to = from + delta
      if (from < 0 || to < 0 || to >= selected.length) return
      const next = selected.slice()
      ;[next[from], next[to]] = [next[to], next[from]]
      update({ chapters: next })
    }
    const toggle = (key: string) =>
      update({
        chapters: selected.includes(key) ? selected.filter((item) => item !== key) : [...selected, key],
      })
    const canSave = draft.name.trim().length > 0 && selected.length > 0 && !saving
    const trialChapter = trial?.chapter

    return (
      <section className="report-stage__style-editor" aria-labelledby={`${id}-editor`}>
        <h3 id={`${id}-editor`}>{draft.isNew ? copy.styleNew : copy.styleEdit}</h3>
        <label className="report-stage__field">
          <span>{copy.styleName}</span>
          <input
            value={draft.name}
            onChange={(event) => update({ name: clip(event.target.value, state.limits.name_chars) })}
          />
        </label>
        <div className="report-stage__field">
          <span id={`${id}-voice`}>{copy.styleVoice}</span>
          <Segmented
            options={VOICES}
            value={draft.voice}
            labelledBy={`${id}-voice`}
            label={(voice) => (voice === 'owner' ? copy.voiceOwner : copy.voiceNarrator)}
            onChange={(voice) => update({ voice })}
          />
        </div>
        <div className="report-stage__field">
          <span id={`${id}-length`}>{copy.styleLength}</span>
          <Segmented
            options={LENGTHS}
            value={draft.length}
            labelledBy={`${id}-length`}
            label={(length) =>
              length === 'short' ? copy.lengthShort : length === 'long' ? copy.lengthLong : copy.lengthStandard}
            onChange={(length) => update({ length })}
          />
        </div>
        <div className="report-stage__field">
          <span>{copy.styleChapters}</span>
          <small>{copy.styleChaptersHint}</small>
          <ol className="report-stage__chapters">
            {ordered.map((key) => {
              const index = selected.indexOf(key)
              const on = index >= 0
              return (
                <li key={key} data-on={on || undefined}>
                  <label>
                    <input
                      type="checkbox"
                      checked={on}
                      disabled={on && selected.length === 1}
                      onChange={() => toggle(key)}
                    />
                    <span>
                      <strong>{chapterNames[key] ?? key}</strong>
                      <small>{chapterMeanings[key]}</small>
                    </span>
                  </label>
                  {on ? (
                    <span className="report-stage__chapter-move">
                      <button type="button" aria-label={copy.moveUp} disabled={index === 0} onClick={() => move(key, -1)}>
                        <LuChevronUp aria-hidden="true" />
                      </button>
                      <button
                        type="button"
                        aria-label={copy.moveDown}
                        disabled={index === selected.length - 1}
                        onClick={() => move(key, 1)}
                      >
                        <LuChevronDown aria-hidden="true" />
                      </button>
                    </span>
                  ) : null}
                </li>
              )
            })}
          </ol>
        </div>
        <label className="report-stage__field">
          <span>
            {copy.styleTone}
            <small>{format(copy.charCount, { n: Array.from(draft.tone).length, max: state.limits.tone_chars })}</small>
          </span>
          <textarea
            rows={2}
            value={draft.tone}
            placeholder={copy.styleTonePlaceholder}
            onChange={(event) => update({ tone: clip(event.target.value, state.limits.tone_chars) })}
          />
        </label>
        <label className="report-stage__field">
          <span>
            {copy.styleAvoid}
            <small>{copy.styleAvoidHint}</small>
          </span>
          <textarea
            rows={3}
            value={draft.avoidText}
            onChange={(event) =>
              update({
                avoidText: event.target.value
                  .split('\n')
                  .slice(0, state.limits.avoid_items)
                  .map((line) => clip(line, state.limits.avoid_chars))
                  .join('\n'),
              })}
          />
        </label>
        <label className="report-stage__field">
          <span>
            {copy.styleSample}
            <small>{copy.styleSampleHint}</small>
          </span>
          <textarea
            rows={3}
            value={draft.sample ?? ''}
            onChange={(event) => update({ sample: clip(event.target.value, state.limits.sample_chars) })}
          />
        </label>

        <div className="report-stage__trial">
          <button
            type="button"
            className="report-stage__chip"
            disabled={trying || selected.length === 0 || !draft.name.trim()}
            onClick={tryDraft}
          >
            {trying ? copy.styleTrying : copy.styleTry}
          </button>
          <small>{copy.styleTryHint}</small>
          {trial ? (
            <figure aria-live="polite">
              <figcaption>
                {copy.styleTryResult}
                {trialChapter ? ` · ${trialChapter.title || chapterNames[trialChapter.key] || trialChapter.key}` : ''}
              </figcaption>
              {trial.headline ? <p className="report-stage__trial-headline">{cleanStageText(trial.headline)}</p> : null}
              {(trialChapter?.beats ?? []).map((beat, index) => (
                <p key={`${index}:${beat.text}`}>{cleanStageText(beat.text)}</p>
              ))}
            </figure>
          ) : null}
        </div>

        <div className="report-stage__editor-actions">
          <button type="button" className="report-stage__chip" onClick={() => startDraft(null)}>
            {copy.styleCancel}
          </button>
          <button type="button" className="report-stage__primary" disabled={!canSave} onClick={() => void saveDraft()}>
            {copy.styleSave}
          </button>
        </div>
      </section>
    )
  }

  return (
    <section className="report-stage__styles" aria-labelledby={`${id}-styles`} aria-busy={saving}>
      <h3 id={`${id}-styles`}>{copy.style}</h3>
      <p className="report-stage__styles-hint">{copy.styleHint}</p>
      <ul>
        {[...state.builtin, ...state.custom].map((style) => {
          const current = style.id === state.current
          const builtin = isBuiltin(style)
          return (
            <li key={style.id} data-current={current || undefined}>
              <span className="report-stage__style-text">
                <strong>{nameOf(style)}</strong>
                <small>{describe(style)}</small>
              </span>
              <span className="report-stage__style-actions">
                {builtin ? (
                  <button type="button" disabled={saving || atLimit} onClick={() => duplicate(style)}>
                    {copy.styleCopy}
                  </button>
                ) : (
                  <>
                    <button type="button" disabled={saving} onClick={() => startDraft(toDraft(style, false))}>
                      {copy.styleEdit}
                    </button>
                    <button
                      type="button"
                      disabled={saving}
                      data-confirm={pendingDelete === style.id || undefined}
                      onClick={() => remove(style)}
                    >
                      {pendingDelete === style.id ? copy.styleDeleteConfirm : copy.styleDelete}
                    </button>
                  </>
                )}
                {current ? (
                  <span className="report-stage__style-badge">{copy.styleCurrent}</span>
                ) : (
                  <button
                    type="button"
                    className="report-stage__style-use"
                    disabled={saving}
                    onClick={() =>
                      void persist(style.id, state.custom, format(copy.styleSwitched, { name: nameOf(style) }))}
                  >
                    {copy.styleUse}
                  </button>
                )}
              </span>
            </li>
          )
        })}
      </ul>
      <div className="report-stage__styles-foot">
        <button
          type="button"
          className="report-stage__chip"
          disabled={saving || atLimit}
          onClick={() =>
            startDraft(
              toDraft(
                {
                  id: newCustomStyleId(takenIds),
                  name: '',
                  voice: 'narrator',
                  tone: '',
                  length: 'standard',
                  chapters: NEW_STYLE_CHAPTERS,
                  avoid: [],
                },
                true,
              ),
            )}
        >
          {copy.styleNew}
        </button>
        {atLimit ? <small>{format(copy.styleLimit, { n: state.limits.custom_styles })}</small> : null}
      </div>
    </section>
  )
}
