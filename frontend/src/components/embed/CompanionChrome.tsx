import type { FocusEvent, ReactNode } from 'react'
import type { CompanionPage } from './companionBridge'
import type { Box } from './companionLayout'
import type { Kind } from './CompanionPanels'
import {
  LuMessageSquare as MessageSquare,
  LuMic as Mic,
  LuMoon as Moon,
  LuSparkles as Sparkles,
  LuSun as Sun,
  LuType as TypeIcon,
} from '@lib/icons'
import { useEffect, useRef, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { CAPSULE_WIDTH, companionLayout, readingProgress } from './companionLayout'
import { PanelShell, Stepper } from './CompanionPanels'
import { pageIsDark, pageSchemeSwitch, switchSiteScheme } from './companionScheme'
import { ChromeVisibility } from './companionVisibility'

export type Panel = Kind | 'reading'
type Copy = ReturnType<typeof useI18n>['t']['embed']

/** 胶囊外侧这么宽、内侧四分之一的地方算「伸手去拿胶囊」。 */
const HOT_ZONE = 80

function box(element: Element): Box {
  const rect = element.getBoundingClientRect()
  return { left: rect.left, right: rect.right, top: rect.top, bottom: rect.bottom }
}

/** 正文的位置、导航岛的位置和读到哪：窗口变了、正文变了、滚动了都重算。 */
function usePageGeometry(page: CompanionPage) {
  const measure = () => {
    const nav = page.doc.querySelector('#myriad-nav .myriad-nav__island')
    const article = box(page.root)
    const view = { width: page.win.innerWidth, height: page.win.innerHeight }
    return {
      layout: companionLayout(view, article, nav ? box(nav) : null),
      progress: readingProgress(article, view.height),
    }
  }
  const [state, setState] = useState(measure)
  useEffect(() => {
    let frame = 0
    const update = () => {
      cancelAnimationFrame(frame)
      frame = requestAnimationFrame(() => setState(measure))
    }
    const observer = new ResizeObserver(update)
    observer.observe(page.root)
    page.win.addEventListener('resize', update)
    page.win.addEventListener('scroll', update, { passive: true })
    return () => {
      cancelAnimationFrame(frame)
      observer.disconnect()
      page.win.removeEventListener('resize', update)
      page.win.removeEventListener('scroll', update)
    }
  }, [page])
  return state
}

/** 显隐的规则在 companionVisibility.ts；这里只把页面上的事件接过去。 */
function useChromeVisibility(
  win: Window,
  hold: boolean,
  reaching: (x: number) => boolean,
): { visible: boolean; ownScroll: () => void } {
  const [visible, setVisible] = useState(true)
  const machineRef = useRef<ChromeVisibility | null>(null)
  const reachingRef = useRef(reaching)
  reachingRef.current = reaching

  useEffect(() => {
    const machine = new ChromeVisibility(win.scrollY, setVisible)
    machineRef.current = machine
    const onScroll = () => machine.scroll(win.scrollY)
    // 鼠标移到胶囊那一带；手指在那一带点一下（胶囊收起时点不到它，点到的是正文）。
    const onPointer = (event: PointerEvent) => {
      if (event.type === 'pointermove' && event.pointerType !== 'mouse') return
      if (reachingRef.current(event.clientX)) machine.reach()
    }
    // 收起的胶囊 Tab 进不去：按 Tab 就先让它出来。
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Tab') machine.reach()
    }
    win.addEventListener('scroll', onScroll, { passive: true })
    win.addEventListener('pointermove', onPointer, { passive: true })
    win.addEventListener('pointerdown', onPointer, { passive: true })
    win.addEventListener('keydown', onKey)
    return () => {
      machine.dispose()
      machineRef.current = null
      win.removeEventListener('scroll', onScroll)
      win.removeEventListener('pointermove', onPointer)
      win.removeEventListener('pointerdown', onPointer)
      win.removeEventListener('keydown', onKey)
    }
  }, [win])

  useEffect(() => {
    machineRef.current?.hold(hold)
  }, [hold])

  return { visible, ownScroll: () => machineRef.current?.ownScroll() }
}

function ProgressRing({ progress }: { progress: number }) {
  const radius = 14
  const length = 2 * Math.PI * radius
  return (
    <svg className="ring" viewBox="0 0 36 36" aria-hidden="true">
      <circle className="ring__track" cx="18" cy="18" r={radius} />
      <circle
        className="ring__fill"
        cx="18"
        cy="18"
        r={radius}
        strokeDasharray={length}
        strokeDashoffset={length * (1 - progress)}
      />
      <text x="18" y="18" dominantBaseline="central" textAnchor="middle">
        {Math.round(progress * 100)}
      </text>
    </svg>
  )
}

export interface CompanionChromeProps {
  page: CompanionPage
  copy: Copy
  /** 左边胶囊上的 AI 工具，按顺序。 */
  tools: Kind[]
  annotationCount: number
  playing: boolean
  panel: Panel | null
  onPanel: (panel: Panel | null) => void
  /** 面板的内容（不含外壳）。`ownScroll` 在自己滚动页面（跳到注释）之前调。 */
  renderPanel: (panel: Panel, ownScroll: () => void) => ReactNode
  /** 面板标题栏里、收起按钮前面的东西（站长的生成按钮）。 */
  panelExtra: (panel: Panel) => ReactNode
  size: { label: string; onStep: (direction: 1 | -1) => void }
  leading: { label: string; onStep: (direction: 1 | -1) => void }
}

export function CompanionChrome({
  page,
  copy,
  tools,
  annotationCount,
  playing,
  panel,
  onPanel,
  renderPanel,
  panelExtra,
  size,
  leading,
}: CompanionChromeProps) {
  const { format } = useI18n()
  const { layout, progress } = usePageGeometry(page)
  const [hovered, setHovered] = useState(false)
  const [focused, setFocused] = useState(false)
  // 并成右边一个时，手指点到窗口右边那一条就算伸手。
  const reaching = (x: number) =>
    layout.mode === 'sides'
      ? (x >= layout.left - HOT_ZONE && x <= layout.left + CAPSULE_WIDTH + HOT_ZONE / 4) ||
        (x >= layout.right - HOT_ZONE / 4 && x <= layout.right + CAPSULE_WIDTH + HOT_ZONE)
      : x >= page.win.innerWidth - layout.right - CAPSULE_WIDTH - HOT_ZONE / 4
  const { visible, ownScroll } = useChromeVisibility(page.win, hovered || focused || panel != null, reaching)
  const comments = page.doc.querySelector<HTMLElement>('iframe.myriad-comments')
  // 主题能被替着切深浅色才有这颗按钮：只跟系统、没有深色的主题切了也看不出来。
  const schemeSwitch = pageSchemeSwitch(page.doc)
  const dark = schemeSwitch ? pageIsDark(page.doc, schemeSwitch) : false
  const schemeLabel = dark ? copy.schemeToLight : copy.schemeToDark
  const schemeButton = schemeSwitch ? (
    <button
      type="button"
      className="btn"
      aria-label={schemeLabel}
      title={schemeLabel}
      onClick={() => switchSiteScheme(page.win, !dark)}
    >
      {dark ? <Sun aria-hidden="true" /> : <Moon aria-hidden="true" />}
    </button>
  ) : null
  const quiet = page.win.matchMedia('(prefers-reduced-motion: reduce)').matches
  const behavior: ScrollBehavior = quiet ? 'auto' : 'smooth'

  const toggle = (next: Panel) => onPanel(panel === next ? null : next)
  const toTop = () => {
    ownScroll()
    page.win.scrollTo({ top: 0, behavior })
  }
  const toComments = () => {
    if (!comments) return
    ownScroll()
    const top = comments.getBoundingClientRect().top + page.win.scrollY - 80
    page.win.scrollTo({ top: Math.max(0, top), behavior })
    onPanel(null)
  }
  const progressLabel = `${format(copy.readingProgress, { percent: Math.round(progress * 100) })} · ${copy.backToTop}`
  const hold = {
    onPointerEnter: () => setHovered(true),
    onPointerLeave: () => setHovered(false),
    onFocus: () => setFocused(true),
    onBlur: (event: FocusEvent) => {
      if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setFocused(false)
    },
  }

  const toolButton = (kind: Kind) => (
    <button
      key={kind}
      type="button"
      className={`btn btn--${kind}${panel === kind ? ' is-on' : ''}${kind === 'podcast' && playing ? ' is-playing' : ''}`}
      aria-label={kind === 'annotations' ? copy.tabAnnotations : copy.tabPodcast}
      title={kind === 'annotations' ? copy.tabAnnotations : copy.tabPodcast}
      aria-expanded={panel === kind}
      onClick={() => toggle(kind)}
    >
      {kind === 'annotations' ? <Sparkles aria-hidden="true" /> : <Mic aria-hidden="true" />}
      {kind === 'annotations' && annotationCount > 0 ? <span className="badge">{annotationCount}</span> : null}
    </button>
  )

  const shell = (open: Panel) => (
    <PanelShell
      title={open === 'annotations' ? copy.tabAnnotations : open === 'podcast' ? copy.tabPodcast : copy.readingTools}
      extra={panelExtra(open)}
      closeLabel={copy.close}
      onClose={() => onPanel(null)}
    >
      {renderPanel(open, ownScroll)}
    </PanelShell>
  )

  if (layout.mode === 'sides') {
    return (
      <>
        <div
          className={`capsule capsule--left${visible ? '' : ' is-hidden'}`}
          style={{ left: layout.left }}
          inert={!visible}
          aria-hidden={!visible}
          {...hold}
        >
          <button type="button" className="btn btn--ring" aria-label={progressLabel} title={progressLabel} onClick={toTop}>
            <ProgressRing progress={progress} />
          </button>
          {tools.length > 0 ? <span className="divider" aria-hidden="true" /> : null}
          {tools.map(toolButton)}
          {panel === 'annotations' || panel === 'podcast' ? <div className="pop">{shell(panel)}</div> : null}
        </div>
        <div
          className={`capsule capsule--right${visible ? '' : ' is-hidden'}`}
          style={{ left: layout.right }}
          inert={!visible}
          aria-hidden={!visible}
          {...hold}
        >
          {schemeButton}
          {comments ? (
            <button type="button" className="btn" aria-label={copy.commentsTitle} title={copy.commentsTitle} onClick={toComments}>
              <MessageSquare aria-hidden="true" />
            </button>
          ) : null}
          {schemeButton || comments ? <span className="divider" aria-hidden="true" /> : null}
          <Stepper label={copy.fontSize} kind="size" value={size.label} copy={copy} onStep={size.onStep} />
          <Stepper label={copy.lineHeight} kind="leading" value={leading.label} copy={copy} onStep={leading.onStep} />
        </div>
      </>
    )
  }

  return (
      <>
        <div
          className={`capsule capsule--right capsule--rail${visible ? '' : ' is-hidden'}`}
          style={{ right: layout.right }}
          inert={!visible}
          aria-hidden={!visible}
          {...hold}
        >
          <button type="button" className="btn btn--ring" aria-label={progressLabel} title={progressLabel} onClick={toTop}>
            <ProgressRing progress={progress} />
          </button>
          {tools.map(toolButton)}
          {comments ? (
            <button type="button" className="btn" aria-label={copy.commentsTitle} title={copy.commentsTitle} onClick={toComments}>
              <MessageSquare aria-hidden="true" />
            </button>
          ) : null}
          {schemeButton}
          <button
            type="button"
            className={`btn${panel === 'reading' ? ' is-on' : ''}`}
            aria-label={copy.readingTools}
            title={copy.readingTools}
            aria-expanded={panel === 'reading'}
            onClick={() => toggle('reading')}
          >
            <TypeIcon aria-hidden="true" />
          </button>
        </div>
        {panel ? (
          <>
            <div className="scrim" aria-hidden="true" onClick={() => onPanel(null)} />
            <div className="sheet">{shell(panel)}</div>
          </>
        ) : null}
      </>
  )
}
