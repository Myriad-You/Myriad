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
  return (
    <div className="panel__admin">
      <button type="button" disabled={busy} onClick={onGenerate}>
        <Sparkles aria-hidden="true" />
        {busy ? copy.generating : has ? copy.regenerate : copy.generate}
      </button>
    </div>
  )
}

export function AnnotationsPanel({
  copy,
  annotations,
  inText,
  linked,
  picked,
  listRef,
  admin,
  onPick,
}: {
  copy: Copy
  annotations: AnnotationItem[]
  inText: ReadonlySet<number>
  linked: boolean
  picked: number | null
  listRef: RefObject<HTMLOListElement | null>
  admin: ReactNode
  onPick: (index: number) => void
}) {
  const { format } = useI18n()
  return (
    <>
      {admin}
      {annotations.length === 0 ? <p className="panel__hint">{copy.annotationsEmpty}</p> : null}
      {annotations.length > 0 && linked ? (
        <p className="panel__hint">
          {format(copy.annotationsInText, { total: annotations.length, count: inText.size })}
        </p>
      ) : null}
      <ol className="notes" ref={listRef}>
        {annotations.map((annotation, index) => {
          const type = knownType(annotation.type)
          const missing = linked && !inText.has(index)
          return (
            <li key={annotation.id ?? index} data-index={index}>
              <button
                type="button"
                className={`note${picked === index ? ' is-picked' : ''}`}
                data-type={type}
                onClick={() => onPick(index)}
              >
                <span className="note__top">
                  <span className="note__type">{copy[TYPE_KEYS[type]]}</span>
                  <strong className="note__term">{annotation.term}</strong>
                  {missing ? null : <ArrowRight className="note__go" aria-hidden="true" />}
                </span>
                <span className="note__text">{annotation.explanation}</span>
                {missing ? <small className="note__miss">{copy.notInText}</small> : null}
              </button>
            </li>
          )
        })}
      </ol>
    </>
  )
}

export function PodcastPanel({
  copy,
  show,
  podcast,
  linesRef,
  admin,
}: {
  copy: Copy
  show: PodcastResponse | null
  podcast: Podcast
  linesRef: RefObject<HTMLOListElement | null>
  admin: ReactNode
}) {
  const { format } = useI18n()
  const playing = podcast.state === 'playing'
  const minutes = Math.max(1, Math.round((show?.estimated_duration ?? 0) / 60))
  if (podcast.dialogues.length === 0) {
    return (
      <>
        {admin}
        <p className="panel__hint">{copy.podcastEmpty}</p>
      </>
    )
  }
  return (
    <>
      {admin}
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
        <button type="button" aria-label={copy.stop} title={copy.stop} disabled={podcast.state === 'stopped'} onClick={podcast.stop}>
          <Square aria-hidden="true" />
        </button>
        <span className="player__at">
          {podcast.index + 1} / {podcast.dialogues.length}
        </span>
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
