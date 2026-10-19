import type {
  AnnotationItem,
  CompanionStatus,
  PodcastResponse,
} from '../../services/phantasiaiApi'
import type { IslandLink } from './companionBridge'
import {
  LuPause as Pause,
  LuPlay as Play,
  LuSkipBack as SkipBack,
  LuSkipForward as SkipForward,
  LuSparkles as Sparkles,
  LuSquare as Square,
  LuX as X,
} from '@lib/icons'
import { useEffect, useRef, useState } from 'react'
import { useParams } from 'react-router-dom'
import { useAuth } from '../../contexts/AuthContext'
import { useI18n } from '../../contexts/I18nContext'
import * as phantasiaiApi from '../../services/phantasiaiApi'
import { userFacingError } from '../../utils/userFacingError'
import { connectIsland, findIslandHost } from './companionBridge'
import { embedHost } from './embedHost'
import { useCompanionPodcast } from './useCompanionPodcast'
import './embed.css'
import './companion.css'

type Kind = 'annotations' | 'podcast'

const TYPE_KEYS = {
  term: 'typeTerm',
  reference: 'typeReference',
  implicit: 'typeImplicit',
  context: 'typeContext',
  abbreviation: 'typeAbbreviation',
} as const

type KnownType = keyof typeof TYPE_KEYS

function knownType(type: string): KnownType {
  return Object.hasOwn(TYPE_KEYS, type) ? (type as KnownType) : 'term'
}

/** Scrolls inside the panel only: `scrollIntoView` would also move the article behind it. */
function keepInView(container: HTMLElement | null, selector: string): void {
  const target = container?.querySelector<HTMLElement>(selector)
  if (!container || !target) return
  const top = target.offsetTop - container.offsetTop
  const bottom = top + target.offsetHeight
  if (top < container.scrollTop) { container.scrollTop = top - 8
}
  else if (bottom > container.scrollTop + container.clientHeight) {
    container.scrollTop = bottom - container.clientHeight + 8
  }
}

export function EmbedCompanion() {
  const { itemId: rawId } = useParams()
  const itemId = Number(rawId)
  const valid = Number.isSafeInteger(itemId) && itemId > 0
  const { t, format, locale } = useI18n()
  const copy = t.embed
  const { isAuthenticated, isAdmin } = useAuth()
  const [status, setStatus] = useState<CompanionStatus | null>(null)
  const [annotations, setAnnotations] = useState<AnnotationItem[]>([])
  const [inText, setInText] = useState<ReadonlySet<number>>(new Set())
  const [show, setShow] = useState<PodcastResponse | null>(null)
  const [tab, setTab] = useState<Kind>('annotations')
  const [loading, setLoading] = useState(true)
  const [busy, setBusy] = useState<Kind | null>(null)
  const [error, setError] = useState('')
  const [picked, setPicked] = useState<number | null>(null)
  const [linked, setLinked] = useState(false)
  const linkRef = useRef<IslandLink | null>(null)
  const notesRef = useRef<HTMLOListElement>(null)
  const linesRef = useRef<HTMLOListElement>(null)
  const copyRef = useRef(copy)
  copyRef.current = copy

  const podcast = useCompanionPodcast({
    itemId,
    sourceId: status?.source_id ?? 0,
    signedIn: isAuthenticated,
    copy,
  })
  const { prepare } = podcast

  useEffect(() => {
    const host = findIslandHost(embedHost()?.frame ?? null)
    if (!host) return
    const link = connectIsland(host, {
      typeLabel: (type) => copyRef.current[TYPE_KEYS[knownType(type)]],
      onPick: (index) => {
        setTab('annotations')
        setPicked(index)
      },
    })
    linkRef.current = link
    setLinked(true)
    const release = () => link.dispose()
    window.addEventListener('pagehide', release)
    return () => {
      window.removeEventListener('pagehide', release)
      release()
      linkRef.current = null
    }
  }, [])

  useEffect(() => {
    linkRef.current?.setLabel(copy.islandLabel)
  }, [copy.islandLabel, linked])

  useEffect(() => {
    if (!valid) {
      setLoading(false)
      return
    }
    const controller = new AbortController()
    const { signal } = controller
    void (async () => {
      try {
        const next = await phantasiaiApi.getCompanion(itemId, signal)
        if (signal.aborted) return
        setStatus(next)
        const [notes, script] = await Promise.all([
          next.annotations > 0 ? phantasiaiApi.getAnnotations(itemId, signal) : null,
          next.podcast ? phantasiaiApi.getPodcastScript(itemId, signal) : null,
        ])
        if (signal.aborted) return
        if (notes) setAnnotations(notes.annotations ?? [])
        if (script) setShow(script)
        setTab(next.annotations > 0 || !next.podcast ? 'annotations' : 'podcast')
      } catch (err) {
        if (!signal.aborted) setError(userFacingError(err, copyRef.current.loadCompanionFailed))
      } finally {
        if (!signal.aborted) setLoading(false)
      }
    })()
    return () => controller.abort()
  }, [itemId, valid])

  useEffect(() => {
    const link = linkRef.current
    if (!link) return
    setInText(new Set(link.paint(annotations)))
  }, [annotations, linked])

  useEffect(() => {
    if (!status || !show) return
    void prepare(show.dialogues ?? [], show.language || locale)
  }, [locale, prepare, show, status])

  useEffect(() => {
    const playing = podcast.state === 'playing'
    linkRef.current?.setPlaying(
      playing
        ? format(copy.playingLabel, {
            current: podcast.index + 1,
            total: podcast.dialogues.length,
          })
        : null,
    )
  }, [copy.playingLabel, format, linked, podcast.dialogues.length, podcast.index, podcast.state])

  useEffect(() => {
    if (picked != null) keepInView(notesRef.current, `[data-index="${picked}"]`)
  }, [picked, tab])

  useEffect(() => {
    if (podcast.state !== 'stopped') keepInView(linesRef.current, `[data-line="${podcast.index}"]`)
  }, [podcast.index, podcast.state, tab])

  const generate = async (kind: Kind, again: boolean) => {
    if (busy) return
    setBusy(kind)
    setError('')
    try {
      if (kind === 'annotations') {
        const response = again
          ? await phantasiaiApi.regenerateAnnotations(itemId)
          : await phantasiaiApi.getAnnotations(itemId)
        if (!response.success) throw new Error(response.error || copy.generateFailed)
        setAnnotations(response.annotations ?? [])
      } else {
        const response = again
          ? await phantasiaiApi.regeneratePodcastScript(itemId)
          : await phantasiaiApi.getPodcastScript(itemId)
        if (!response.success || !response.dialogues?.length) {
          throw new Error(response.error || copy.generateFailed)
        }
        setShow(response)
      }
    } catch (err) {
      setError(userFacingError(err, copy.generateFailed))
    } finally {
      setBusy(null)
      phantasiaiApi.getCompanion(itemId).then(setStatus, () => {})
    }
  }

  const hasNotes = annotations.length > 0
  const hasShow = podcast.dialogues.length > 0
  const tabs = (['annotations', 'podcast'] as const).filter(
    (kind) => isAdmin || (kind === 'annotations' ? hasNotes : hasShow),
  )
  const current = tabs.includes(tab) ? tab : tabs[0]

  const adminBar = (kind: Kind, has: boolean) =>
    isAdmin ? (
      <div className="companion-admin">
        <button type="button" disabled={busy !== null} onClick={() => { void generate(kind, has) }}>
          <Sparkles aria-hidden="true" />
          {busy === kind ? copy.generating : has ? copy.regenerate : copy.generate}
        </button>
      </div>
    ) : null

  const playing = podcast.state === 'playing'
  const minutes = Math.max(1, Math.round((show?.estimated_duration ?? 0) / 60))

  return (
    <main className="embed-root companion">
      <header className="companion-head">
        <div className="companion-tabs" role="tablist" aria-label={copy.islandLabel}>
          {tabs.map((kind) => (
            <button
              key={kind}
              type="button"
              role="tab"
              id={`companion-tab-${kind}`}
              aria-selected={current === kind}
              aria-controls={`companion-panel-${kind}`}
              className="companion-tab"
              onClick={() => setTab(kind)}
            >
              {kind === 'annotations' ? copy.tabAnnotations : copy.tabPodcast}
              {kind === 'annotations' && hasNotes ? (
                <span className="companion-tab__count">{annotations.length}</span>
              ) : null}
            </button>
          ))}
        </div>
        <button
          type="button"
          className="companion-close"
          aria-label={copy.close}
          title={copy.close}
          onClick={() => linkRef.current?.close()}
        >
          <X aria-hidden="true" />
        </button>
      </header>

      {error ? <p className="embed-error" role="alert">{error}</p> : null}
      {loading ? <p className="companion-hint">{copy.loading}</p> : null}
      {!loading && tabs.length === 0 ? <p className="companion-hint">{copy.companionEmpty}</p> : null}

      {!loading && current === 'annotations' ? (
        <section
          className="companion-body"
          role="tabpanel"
          id="companion-panel-annotations"
          aria-labelledby="companion-tab-annotations"
        >
          {adminBar('annotations', hasNotes)}
          {hasNotes && linked ? (
            <p className="companion-hint">{format(copy.annotationsInText, { count: inText.size })}</p>
          ) : null}
          <ol className="companion-notes" ref={notesRef}>
            {annotations.map((annotation, index) => {
              const type = knownType(annotation.type)
              return (
                <li key={annotation.id ?? index} data-index={index}>
                  <button
                    type="button"
                    className={`companion-note${picked === index ? ' is-picked' : ''}`}
                    data-type={type}
                    onClick={() => {
                      setPicked(index)
                      linkRef.current?.reveal(index)
                    }}
                  >
                    <span className="companion-note__type">{copy[TYPE_KEYS[type]]}</span>
                    <strong className="companion-note__term">{annotation.term}</strong>
                    <span className="companion-note__text">{annotation.explanation}</span>
                    {linked && !inText.has(index) ? (
                      <small className="companion-note__miss">{copy.notInText}</small>
                    ) : null}
                  </button>
                </li>
              )
            })}
          </ol>
        </section>
      ) : null}

      {!loading && current === 'podcast' ? (
        <section
          className="companion-body"
          role="tabpanel"
          id="companion-panel-podcast"
          aria-labelledby="companion-tab-podcast"
        >
          {adminBar('podcast', hasShow)}
          {hasShow ? (
            <>
              <div className="companion-show">
                <strong>{show?.title}</strong>
                <span>{format(copy.duration, { minutes })}</span>
              </div>
              <div className="companion-player">
                <button type="button" aria-label={copy.prev} title={copy.prev} disabled={podcast.audioLoading || podcast.index <= 0} onClick={() => { void podcast.prev() }}>
                  <SkipBack aria-hidden="true" />
                </button>
                <button
                  type="button"
                  className="companion-player__main"
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
                <span className="companion-player__at">
                  {podcast.index + 1} / {podcast.dialogues.length}
                </span>
              </div>
              {podcast.cloudAvailable ? (
                <div className="companion-voice">
                  <div className="companion-segment" role="group" aria-label={copy.voiceEngine}>
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
                      <button type="button" disabled={podcast.audioLoading} onClick={() => { void podcast.regenerateAudio() }}>
                        {copy.regenerateAudio}
                      </button>
                    </>
                  ) : null}
                </div>
              ) : null}
              {podcast.audioLoading ? (
                <p className="companion-hint" role="status">
                  {format(copy.audioLoading, {
                    loaded: podcast.audioProgress.loaded,
                    total: podcast.audioProgress.total,
                  })}
                </p>
              ) : null}
              {podcast.notice ? <p className="companion-hint" role="status">{podcast.notice}</p> : null}
              <ol className="companion-lines" ref={linesRef}>
                {podcast.dialogues.map((line, index) => (
                  <li key={index} data-line={index}>
                    <button
                      type="button"
                      className={`companion-line${podcast.state !== 'stopped' && podcast.index === index ? ' is-current' : ''}`}
                      data-speaker={line.speaker === 'host_a' ? 'host' : 'guest'}
                      disabled={podcast.audioLoading}
                      onClick={() => { void podcast.seek(index) }}
                    >
                      <span className="companion-line__who">
                        {line.speaker === 'host_a' ? copy.speakerHost : copy.speakerGuest}
                      </span>
                      <span>{line.text}</span>
                    </button>
                  </li>
                ))}
              </ol>
            </>
          ) : null}
        </section>
      ) : null}
    </main>
  )
}
