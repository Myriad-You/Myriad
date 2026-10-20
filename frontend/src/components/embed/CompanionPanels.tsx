import type { ReactNode, RefObject } from 'react'
import type { AnnotationItem, PodcastResponse } from '../../services/phantasiaiApi'
import type { ReadingPrefs } from './companionLayout'
import type { useCompanionPodcast } from './useCompanionPodcast'
import {
  LuALargeSmall as ALargeSmall,
  LuArrowRight as ArrowRight,
  LuMinus as Minus,
  LuPause as Pause,
  LuPlay as Play,
  LuPlus as Plus,
  LuSkipBack as SkipBack,
  LuSkipForward as SkipForward,
  LuSparkles as Sparkles,
  LuSquare as Square,
  LuUnfoldVertical as UnfoldVertical,
  LuX as X,
} from '@lib/icons'
import { useI18n } from '../../contexts/I18nContext'

export type Kind = 'annotations' | 'podcast'
type Copy = ReturnType<typeof useI18n>['t']['embed']
type Podcast = ReturnType<typeof useCompanionPodcast>

export const TYPE_KEYS = {
  term: 'typeTerm',
  reference: 'typeReference',
  implicit: 'typeImplicit',
  context: 'typeContext',
  abbreviation: 'typeAbbreviation',
} as const

export type KnownType = keyof typeof TYPE_KEYS

export function knownType(type: string): KnownType {
  return Object.hasOwn(TYPE_KEYS, type) ? (type as KnownType) : 'term'
}

/** 面板的外壳：标题栏（标题、补充、收起），下面是可以滚的内容。 */
export function PanelShell({
  title,
  extra,
  closeLabel,
  onClose,
  children,
}: {
  title: string
  extra?: ReactNode
  closeLabel: string
  onClose: () => void
  children: ReactNode
}) {
  return (
    <section className="panel" role="dialog" aria-label={title}>
      <header className="panel__head">
        <strong>{title}</strong>
        {extra}
        <button type="button" className="panel__close" aria-label={closeLabel} title={closeLabel} onClick={onClose}>
          <X aria-hidden="true" />
        </button>
      </header>
      <div className="panel__body">{children}</div>
    </section>
  )
}

/**
 * 站长补生成或重新生成。已经有了：标题栏里一颗和收起一样安静的图标按钮；
 * 还没有：空面板里一颗看得见的按钮。
 */
export function AdminGenerate({
  copy,
  busy,
  has,
  onGenerate,
}: {
  copy: Copy
  busy: boolean
  has: boolean
  onGenerate: () => void
}) {
  if (has) {
    const label = busy ? copy.generating : copy.regenerate
    return (
      <button
        type="button"
        className="panel__icon"
        aria-label={label}
        title={label}
        aria-busy={busy}
        disabled={busy}
        onClick={onGenerate}
      >
        <Sparkles aria-hidden="true" />
      </button>
    )
  }
  return (
    <button type="button" className="panel__generate" disabled={busy} onClick={onGenerate}>
      <Sparkles aria-hidden="true" />
      {busy ? copy.generating : copy.generate}
    </button>
  )
}

/**
 * 注释列表：点一条展开整段解释，在面板里就能读完；右边的箭头才跳去正文。
 * 正文里没找到的词排在最后，单独一组。
 */
export function AnnotationsPanel({
  copy,
  annotations,
  inText,
  linked,
  picked,
  listRef,
  onToggle,
  onReveal,
}: {
  copy: Copy
  annotations: AnnotationItem[]
  inText: ReadonlySet<number>
  linked: boolean
  picked: number | null
  listRef: RefObject<HTMLOListElement | null>
  onToggle: (index: number) => void
  onReveal: (index: number) => void
}) {
  const { format } = useI18n()
  const indexed = annotations.map((annotation, index) => ({ annotation, index }))
  const found = linked ? indexed.filter(({ index }) => inText.has(index)) : indexed
  const missing = linked ? indexed.filter(({ index }) => !inText.has(index)) : []
  const row = ({ annotation, index }: (typeof indexed)[number], locatable: boolean) => {
    const type = knownType(annotation.type)
    const open = picked === index
    return (
      <li key={annotation.id ?? index} data-index={index}>
        <div className={`note${open ? ' is-picked' : ''}`} data-type={type}>
          <button type="button" className="note__main" aria-expanded={open} onClick={() => onToggle(index)}>
            <span className="note__top">
              <span className="note__type">{copy[TYPE_KEYS[type]]}</span>
              <strong className="note__term">{annotation.term}</strong>
            </span>
            <span className="note__text">{annotation.explanation}</span>
          </button>
          {locatable ? (
            <button
              type="button"
              className="note__go"
              aria-label={`${copy.showInText}: ${annotation.term}`}
              title={copy.showInText}
              onClick={() => onReveal(index)}
            >
              <ArrowRight aria-hidden="true" />
            </button>
          ) : null}
        </div>
      </li>
    )
  }
  if (annotations.length === 0) return <p className="panel__hint">{copy.annotationsEmpty}</p>
  return (
    <>
      {linked ? (
        <p className="panel__hint">
          {format(copy.annotationsInText, { total: annotations.length, count: inText.size })}
        </p>
      ) : null}
      <ol className="notes" ref={listRef}>
        {found.map((entry) => row(entry, linked))}
        {missing.length > 0 ? (
          <li className="notes__group" aria-hidden="true">{copy.notInText}</li>
        ) : null}
        {missing.map((entry) => row(entry, false))}
      </ol>
    </>
  )
}

export function PodcastPanel({
  copy,
  show,
  podcast,
  linesRef,
}: {
  copy: Copy
  show: PodcastResponse | null
  podcast: Podcast
  linesRef: RefObject<HTMLOListElement | null>
}) {
  const { format } = useI18n()
  const playing = podcast.state === 'playing'
  const minutes = Math.max(1, Math.round((show?.estimated_duration ?? 0) / 60))
  const total = podcast.dialogues.length
  if (total === 0) return <p className="panel__hint">{copy.podcastEmpty}</p>
  return (
    <>
      {/* 节目名、播放和声音钉在上面，只有台词在滚。 */}
      <div className="deck">
        <div className="show">
          <strong>{show?.title}</strong>
          <span>{format(copy.duration, { minutes })}</span>
        </div>
        <div className="player">
          <button type="button" aria-label={copy.prev} title={copy.prev} disabled={podcast.audioLoading || podcast.index <= 0} onClick={() => { void podcast.prev() }}>
            <SkipBack aria-hidden="true" />
          </button>
          <button
            type="button"
            className="player__main"
            aria-label={playing ? copy.pause : copy.play}
            title={playing ? copy.pause : copy.play}
            disabled={podcast.audioLoading}
            onClick={() => { void (playing ? podcast.pause() : podcast.play()) }}
          >
            {playing ? <Pause aria-hidden="true" /> : <Play aria-hidden="true" />}
          </button>
          <button type="button" aria-label={copy.next} title={copy.next} disabled={podcast.audioLoading || podcast.index >= podcast.dialogues.length - 1} onClick={() => { void podcast.next() }}>
            <SkipForward aria-hidden="true" />
          </button>
          <button type="button" className="player__stop" aria-label={copy.stop} title={copy.stop} disabled={podcast.state === 'stopped'} onClick={podcast.stop}>
            <Square aria-hidden="true" />
          </button>
          <span className="player__at">
            {podcast.index + 1} / {total}
          </span>
        </div>
        <div className="player__track" aria-hidden="true">
          <span style={{ width: `${((podcast.index + (podcast.state === 'stopped' ? 0 : 1)) / total) * 100}%` }} />
        </div>
        {podcast.cloudAvailable ? (
          <div className="voice">
            <div className="segment" role="group" aria-label={copy.voiceEngine}>
              {(['system', 'cloud'] as const).map((engine) => (
                <button
                  key={engine}
                  type="button"
                  aria-pressed={podcast.engine === engine}
                  disabled={podcast.audioLoading}
                  onClick={() => { void podcast.switchEngine(engine) }}
                >
                  {engine === 'system' ? copy.engineSystem : copy.engineCloud}
                </button>
              ))}
            </div>
            {podcast.engine === 'cloud' ? (
              <>
                {(['host', 'guest'] as const).map((role) => (
                  <label key={role}>
                    <span>{role === 'host' ? copy.hostVoice : copy.guestVoice}</span>
                    <select
                      value={(role === 'host' ? podcast.hostVoiceId : podcast.guestVoiceId) ?? 0}
                      disabled={podcast.audioLoading}
                      onChange={(event) => {
                        const id = Number(event.target.value)
                        void podcast.changeVoice(role, id || undefined)
                      }}
                    >
                      <option value={0}>{copy.voiceDefault}</option>
                      {podcast.voices.map((voice) => (
                        <option key={voice.id} value={voice.id}>{voice.name}</option>
                      ))}
                    </select>
                  </label>
                ))}
                <button type="button" className="voice__redo" disabled={podcast.audioLoading} onClick={() => { void podcast.regenerateAudio() }}>
                  {copy.regenerateAudio}
                </button>
              </>
            ) : null}
          </div>
        ) : null}
        {podcast.audioLoading ? (
          <p className="panel__hint" role="status">
            {format(copy.audioLoading, {
              loaded: podcast.audioProgress.loaded,
              total: podcast.audioProgress.total,
            })}
          </p>
        ) : null}
        {podcast.notice ? <p className="panel__hint" role="status">{podcast.notice}</p> : null}
      </div>
      <ol className="lines" ref={linesRef}>
        {podcast.dialogues.map((line, index) => (
          <li key={index} data-line={index}>
            <button
              type="button"
              className={`line${podcast.state !== 'stopped' && podcast.index === index ? ' is-current' : ''}`}
              data-speaker={line.speaker === 'host_a' ? 'host' : 'guest'}
              disabled={podcast.audioLoading}
              onClick={() => { void podcast.seek(index) }}
            >
              <span className="line__who">
                {line.speaker === 'host_a' ? copy.speakerHost : copy.speakerGuest}
              </span>
              <span>{line.text}</span>
            </button>
          </li>
        ))}
      </ol>
    </>
  )
}

/** 一组 图标 + 值 −，竖着（胶囊里）或横着（手机的面板里）；图标说明调的是什么。 */
export function Stepper({
  label,
  kind,
  value,
  copy,
  onStep,
}: {
  label: string
  kind: 'size' | 'leading'
  value: string
  copy: Copy
  onStep: (direction: 1 | -1) => void
}) {
  const Icon = kind === 'size' ? ALargeSmall : UnfoldVertical
  return (
    <div className="stepper" role="group" aria-label={label} title={label}>
      <Icon className="stepper__icon" aria-hidden="true" />
      <button type="button" className="stepper__up" aria-label={`${label} ${copy.increase}`} onClick={() => onStep(1)}>
        <Plus aria-hidden="true" />
      </button>
      <span className="stepper__value">{value}</span>
      <button type="button" className="stepper__down" aria-label={`${label} ${copy.decrease}`} onClick={() => onStep(-1)}>
        <Minus aria-hidden="true" />
      </button>
    </div>
  )
}

export function readingValues(prefs: ReadingPrefs, computed: { size: number; leading: number }) {
  return {
    size: `${prefs.size ?? 100}%`,
    leading: (prefs.leading ?? computed.leading).toFixed(2).replace(/0$/, ''),
  }
}
