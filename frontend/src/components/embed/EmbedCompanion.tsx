import type {
  AnnotationItem,
  CompanionStatus,
  PodcastResponse,
} from '../../services/phantasiaiApi'
import type { ArticleLink, CompanionPage } from './companionBridge'
import type { Panel } from './CompanionChrome'
import type { ReadingPrefs } from './companionLayout'
import type { Kind } from './CompanionPanels'
import { useEffect, useMemo, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { useParams } from 'react-router-dom'
import { useAuth } from '../../contexts/AuthContext'
import { useI18n } from '../../contexts/I18nContext'
import * as phantasiaiApi from '../../services/phantasiaiApi'
import { userFacingError } from '../../utils/userFacingError'
import { DocumentReady } from '../DocumentReady'
import companionCss from './companion.css?inline'
import { companionHost, connectArticle, findCompanionPage } from './companionBridge'
import { CompanionChrome } from './CompanionChrome'
import {
  LEADING_STEPS,
  readingPrefsFromValue,
  SIZE_STEPS,
  stepFrom,
} from './companionLayout'
import {
  AdminGenerate,
  AnnotationsPanel,
  knownType,
  PodcastPanel,
  readingValues,
  Stepper,
  TYPE_KEYS,
} from './CompanionPanels'
import { embedHost } from './embedHost'
import { useCompanionPodcast } from './useCompanionPodcast'

const READING_KEY = 'myriad.blog.reading.v1'

/** Scrolls inside the panel only: `scrollIntoView` would also move the article behind it. */
function keepInView(container: HTMLElement | null, selector: string): void {
  const target = container?.querySelector<HTMLElement>(selector)
  if (!container || !target) return
  const scroller = container.closest('.panel__body') as HTMLElement | null
  if (!scroller) return
  const top = target.offsetTop - scroller.offsetTop
  const bottom = top + target.offsetHeight
  if (top < scroller.scrollTop) { scroller.scrollTop = top - 8
}
  else if (bottom > scroller.scrollTop + scroller.clientHeight) {
    scroller.scrollTop = bottom - scroller.clientHeight + 8
  }
}

function readPrefs(): ReadingPrefs {
  try {
    return readingPrefsFromValue(JSON.parse(window.localStorage.getItem(READING_KEY) ?? 'null'))
  } catch {
    return { size: null, leading: null }
  }
}

function writePrefs(prefs: ReadingPrefs): void {
  try {
    window.localStorage.setItem(READING_KEY, JSON.stringify(prefs))
  } catch {
    // 存不下就只在这一页记着。
  }
}

/** 正文眼下的行距倍数，主题写 `normal` 时按 1.2 算。 */
function computedLeading(root: HTMLElement): number {
  const style = root.ownerDocument.defaultView?.getComputedStyle(root)
  const size = Number.parseFloat(style?.fontSize ?? '')
  const line = Number.parseFloat(style?.lineHeight ?? '')
  return size > 0 && line > 0 ? line / size : 1.2
}

/** 外面那页（博客页）上的宿主节点和它的 Shadow DOM；明暗跟着这一页的 dark/light 类。 */
function useShadowMount(page: CompanionPage | null): HTMLElement | null {
  const [container, setContainer] = useState<HTMLElement | null>(null)
  useEffect(() => {
    if (!page) return
    const host = companionHost(page)
    const shadow = host.shadowRoot ?? host.attachShadow({ mode: 'open' })
    shadow.replaceChildren()
    const style = page.doc.createElement('style')
    style.textContent = companionCss
    const mount = page.doc.createElement('div')
    mount.className = 'companion'
    shadow.append(style, mount)
    const html = document.documentElement
    const syncScheme = () => {
      host.dataset.scheme = html.classList.contains('dark') ? 'dark' : 'light'
    }
    syncScheme()
    const observer = new MutationObserver(syncScheme)
    observer.observe(html, { attributes: true, attributeFilter: ['class'] })
    setContainer(mount)
    return () => {
      observer.disconnect()
      setContainer(null)
      shadow.replaceChildren()
    }
  }, [page])
  return container
}

/**
 * `/embed/companion/{id}`：文章末尾看不见的控制器。照改版前阅读器的两侧浮动，把左边的
 * AI 胶囊（进度、注释、播客）和右边的阅读胶囊（评论、字号、行距）画进博客页，
 * 并在正文里划出注释。窄的时候换成底部一条。
 */
export function EmbedCompanion() {
  const { itemId: rawId } = useParams()
  const itemId = Number(rawId)
  const valid = Number.isSafeInteger(itemId) && itemId > 0
  const { t, format, locale } = useI18n()
  const copy = t.embed
  const { isAuthenticated, isAdmin } = useAuth()
  const page = useMemo(() => findCompanionPage(embedHost()?.frame ?? null), [])
  const container = useShadowMount(page)
  const [status, setStatus] = useState<CompanionStatus | null>(null)
  const [annotations, setAnnotations] = useState<AnnotationItem[]>([])
  const [inText, setInText] = useState<ReadonlySet<number>>(new Set())
  const [show, setShow] = useState<PodcastResponse | null>(null)
  const [panel, setPanel] = useState<Panel | null>(null)
  const [loading, setLoading] = useState(true)
  const [busy, setBusy] = useState<Kind | null>(null)
  const [error, setError] = useState('')
  const [picked, setPicked] = useState<number | null>(null)
  const [linked, setLinked] = useState(false)
  const [prefs, setPrefs] = useState(readPrefs)
  const [leadingNow, setLeadingNow] = useState(1.75)
  const linkRef = useRef<ArticleLink | null>(null)
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
    if (!page) return
    const link = connectArticle(page, {
      typeLabel: (type) => copyRef.current[TYPE_KEYS[knownType(type)]],
      onPick: (index) => {
        setPanel('annotations')
        setPicked(index)
      },
      onEscape: () => setPanel(null),
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
  }, [page])

  // 字号和行距写在正文上；主题自己的值不动，只覆盖。
  useEffect(() => {
    if (!page) return
    page.root.style.fontSize = prefs.size ? `${prefs.size}%` : ''
    page.root.style.lineHeight = prefs.leading ? String(prefs.leading) : ''
    setLeadingNow(computedLeading(page.root))
  }, [page, prefs])

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
    if (picked != null && panel === 'annotations') keepInView(notesRef.current, `[data-index="${picked}"]`)
  }, [picked, panel])

  useEffect(() => {
    if (podcast.state !== 'stopped' && panel === 'podcast') {
      keepInView(linesRef.current, `[data-line="${podcast.index}"]`)
    }
  }, [podcast.index, podcast.state, panel])

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

  const changePrefs = (update: (current: ReadingPrefs) => ReadingPrefs) => {
    setPrefs((current) => {
      const next = update(current)
      writePrefs(next)
      return next
    })
  }
  const stepSize = (direction: 1 | -1) =>
    changePrefs((current) => {
      const next = stepFrom(SIZE_STEPS, current.size ?? 100, direction)
      return { ...current, size: next === 100 ? null : next }
    })
  const stepLeading = (direction: 1 | -1) =>
    changePrefs((current) => ({
      ...current,
      leading: stepFrom(LEADING_STEPS, current.leading ?? leadingNow, direction),
    }))
  const values = readingValues(prefs, { size: 100, leading: leadingNow })

  const hasNotes = annotations.length > 0
  const hasShow = podcast.dialogues.length > 0
  const tools = (['annotations', 'podcast'] as const).filter(
    (kind) => isAdmin || (kind === 'annotations' ? hasNotes : hasShow),
  )
  const panelExtra = (open: Panel) => {
    if (!isAdmin || open === 'reading') return null
    const has = open === 'annotations' ? hasNotes : hasShow
    return <AdminGenerate copy={copy} busy={busy !== null} has={has} onGenerate={() => { void generate(open, has) }} />
  }

  const renderPanel = (open: Panel, ownScroll: () => void) => (
    <>
      {error ? <p className="panel__error" role="alert">{error}</p> : null}
      {open === 'annotations' ? (
        <AnnotationsPanel
          copy={copy}
          annotations={annotations}
          inText={inText}
          linked={linked}
          picked={picked}
          listRef={notesRef}
          onToggle={(index) => setPicked((current) => (current === index ? null : index))}
          onReveal={(index) => {
            setPicked(index)
            ownScroll()
            if (linkRef.current?.reveal(index)) setPanel(null)
          }}
        />
      ) : open === 'podcast' ? (
        <PodcastPanel copy={copy} show={show} podcast={podcast} linesRef={linesRef} />
      ) : (
        <div className="reading">
          <Stepper label={copy.fontSize} kind="size" value={values.size} copy={copy} onStep={stepSize} />
          <Stepper label={copy.lineHeight} kind="leading" value={values.leading} copy={copy} onStep={stepLeading} />
        </div>
      )}
    </>
  )

  const chrome = container && page && !loading ? (
    <CompanionChrome
      page={page}
      copy={copy}
      tools={[...tools]}
      annotationCount={annotations.length}
      playing={podcast.state === 'playing'}
      panel={panel}
      onPanel={setPanel}
      renderPanel={renderPanel}
      panelExtra={panelExtra}
      size={{ label: values.size, onStep: stepSize }}
      leading={{ label: values.leading, onStep: stepLeading }}
    />
  ) : null

  // 播放时告诉读屏现在念到第几句；胶囊上只有按钮在跳。
  const playingNote = podcast.state === 'playing'
    ? format(copy.playingLabel, { current: podcast.index + 1, total: podcast.dialogues.length })
    : ''

  return (
    <DocumentReady>
      {chrome && container ? createPortal(
        <>
          {chrome}
          <p className="sr-only" role="status">{playingNote}</p>
        </>,
        container,
      ) : null}
    </DocumentReady>
  )
}
