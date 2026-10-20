import type { Root } from 'react-dom/client'
import type { StudioOverlayCopy, StudioPageContext } from '../components/studio/overlay/StudioOverlay'
import type { StudioOverlayLayout, StudioTool, StudioToolSpec } from '../components/studio/overlay/studioTools'
import type { BlogBuildMark, StudioWindowPort } from '../components/studio/studioIsland'
import { useEffect, useMemo, useRef, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { DocumentReady } from '../components/DocumentReady'
import { listenOverlayKeys, StudioOverlay } from '../components/studio/overlay/StudioOverlay'
import overlayCss from '../components/studio/overlay/studioOverlay.css?inline'
import {
  closeTool,
  DEFAULT_LAYOUT,
  layoutFromValue,
  layoutToValue,
  markWindowReady,
  openTool,
  parseToolSpec,
  rememberTool,
} from '../components/studio/overlay/studioTools'
import {
  blogReloadDecision,
  findStudioHost,
  setStudioOverlayApi,
  withoutStudioParam,
} from '../components/studio/studioIsland'
import { useI18n } from '../contexts/I18nContext'
import { getBlogSettings } from '../services/blogApi'
import { SITE_CHROME_COOKIE } from '../utils/siteChromeCookie'

const LAYOUT_KEY = 'myriad.studio.overlay.v1'
const RELOAD_POLL_MS = 1500
const RELOAD_POLL_LIMIT = 40

function readLayout(): StudioOverlayLayout {
  try {
    const raw = window.localStorage.getItem(LAYOUT_KEY)
    return raw ? layoutFromValue(JSON.parse(raw)) : DEFAULT_LAYOUT
  } catch {
    return DEFAULT_LAYOUT
  }
}

function writeLayout(layout: StudioOverlayLayout): void {
  try {
    window.localStorage.setItem(LAYOUT_KEY, JSON.stringify(layoutToValue(layout)))
  } catch {
    // 存不下就只在这一页记着。
  }
}

async function buildMark(): Promise<BlogBuildMark | null> {
  try {
    const { build } = await getBlogSettings()
    return { status: build.status, lastBuiltAt: build.last_built_at ?? null }
  } catch {
    return null
  }
}

/** 主站写给博客页的 cookie 里的强调色（`{设置}.{深浅色}.{主色}.{强调色}`，见 siteChromeCookie.ts）。 */
function siteAccent(doc: Document): string | null {
  const raw = doc.cookie.split('; ').find((part) => part.startsWith(`${SITE_CHROME_COOKIE}=`))
  const accent = raw?.slice(SITE_CHROME_COOKIE.length + 1).split('.')[3]
  return accent && /^[0-9a-f]{6}$/.test(accent) ? `#${accent}` : null
}

function numberAttr(host: HTMLElement, name: string): number | null {
  const value = Number(host.dataset[name])
  return Number.isSafeInteger(value) && value > 0 ? value : null
}

/**
 * `/embed/studio/overlay`：博客页上看不见的控制器。把覆盖层画进宿主节点的
 * Shadow DOM（React 根建在博客页的文档里，事件才收得到），并挂上给工具窗口用的接口。
 */
export default function StudioOverlayFrame() {
  const { t } = useI18n()
  const host = useMemo(() => findStudioHost(), [])
  const rootRef = useRef<Root | null>(null)
  const [open, setOpen] = useState(() => Boolean(host?.dataset.open))
  const [layout, setLayout] = useState<StudioOverlayLayout>(() => {
    const saved = readLayout()
    const spec = parseToolSpec(host?.dataset.open)
    return spec ? openTool(saved, spec) : saved
  })
  const frameEls = useRef(new Map<StudioTool, HTMLIFrameElement>())
  /** 窗口登记的离开前检查，连同登记它的那一页：窗口换了地址，旧页的检查就不算数。 */
  const guards = useRef(new Map<StudioTool, { doc: Document; check: () => Promise<boolean> }>())
  const layoutNow = useRef(layout)
  layoutNow.current = layout
  const openedMark = useRef<BlogBuildMark | null>(null)
  const settleTimer = useRef(0)
  const openRef = useRef(open)
  openRef.current = open

  const context = useMemo<StudioPageContext>(
    () => ({
      itemId: host ? numberAttr(host, 'item') : null,
      docId: host ? numberAttr(host, 'doc') : null,
    }),
    [host],
  )

  const copy = useMemo<StudioOverlayCopy>(
    () => ({
      studio: t.studio.title,
      editPost: t.studio.overlayEditPost,
      hotkey: t.studio.overlayHotkey,
      pin: t.studio.overlayPin,
      unpin: t.studio.overlayUnpin,
      opacity: t.studio.overlayOpacity,
      close: t.common.close,
      toolbarIcons: t.studio.overlayToolbarIcons,
      toolbarList: t.studio.overlayToolbarList,
      groupPage: t.studio.overlayGroupPage,
      groupBlog: t.studio.blog,
      groupSite: t.studio.overlayGroupSite,
      tools: {
        overview: t.studio.overlayThisPage,
        editor: t.studio.overlayEditor,
        notes: t.phantasi.workbenchNotes,
        comments: t.phantasi.workbenchComments,
        categories: t.studio.navCategories,
        reviews: t.phantasi.workbenchReviews,
        transfer: t.phantasi.workbenchNavTransfer,
        media: t.phantasi.workbenchMedia,
        settings: t.studio.blogSettings,
      },
    }),
    [t],
  )

  const markReady = (tool: StudioTool) => changeLayout((current) => markWindowReady(current, tool))

  /** 关窗口或换内容之前问窗口自己；它没登记检查（或登记的是换地址前的旧页）就直接放行。 */
  const mayLeave = (tool: StudioTool): Promise<boolean> => {
    const guard = guards.current.get(tool)
    let doc: Document | null = null
    try {
      doc = frameEls.current.get(tool)?.contentDocument ?? null
    } catch {
      doc = null
    }
    return guard && guard.doc === doc ? guard.check() : Promise.resolve(true)
  }

  /** 认出调用者是哪个窗口：工具窗口的 window 就是它那个 iframe 的 contentWindow。 */
  const portFor = (win: Window): StudioWindowPort => {
    const tool = [...frameEls.current].find(([, frame]) => frame.contentWindow === win)?.[0]
    if (!tool) return { ready: () => {}, guard: () => {} }
    return {
      ready: () => markReady(tool),
      guard: (check) => {
        if (check) guards.current.set(tool, { doc: win.document, check })
        else if (guards.current.get(tool)?.doc === win.document) guards.current.delete(tool)
      },
    }
  }

  /**
   * 编辑器要换到另一篇时，先让它自己检查（没保存的先问、空的新笔记清掉）；
   * 通过了再换。换回布局里已经开着的同一篇不算换。
   */
  const whenEditorMayLeave = (spec: StudioToolSpec | undefined, apply: () => void) => {
    const showing = layoutNow.current.open.find((item) => item.tool === 'editor')
    const swapping = spec?.tool === 'editor' && showing != null && showing.arg !== spec.arg
    if (!swapping) {
      apply()
      return
    }
    void mayLeave('editor').then((ok) => {
      if (ok) apply()
    })
  }

  /** 布局（连同每个窗口加载的地址、画好没有）只从这里改；存下来的那部分变了才写。 */
  const changeLayout = (update: (current: StudioOverlayLayout) => StudioOverlayLayout) => {
    setLayout((current) => {
      const next = update(current)
      const persisted = (value: StudioOverlayLayout) => JSON.stringify(layoutToValue(value))
      if (next !== current && persisted(next) !== persisted(current)) writeLayout(next)
      return next
    })
  }

  // 工具栏、细条、拖动：先按眼下的布局看编辑器换不换篇，放行了再按那时的最新布局改。
  const updateLayout = (update: (current: StudioOverlayLayout) => StudioOverlayLayout) => {
    const editor = update(layoutNow.current).open.find((item) => item.tool === 'editor')
    whenEditorMayLeave(editor, () => changeLayout(update))
  }

  // 展开时记下博客构建状态；收起时博客若已重新生成就刷新外层页面，还在生成就等它。
  useEffect(() => {
    const top = host?.ownerDocument.defaultView
    if (!host || !top) return
    window.clearTimeout(settleTimer.current)
    if (open) {
      void buildMark().then((mark) => {
        if (openRef.current) openedMark.current = mark
      })
      return
    }
    const href = top.location.href
    const clean = withoutStudioParam(href)
    if (clean !== href) top.history.replaceState(top.history.state, '', clean)
    const opened = openedMark.current
    if (!opened) return
    const settle = (tries: number) => {
      void buildMark().then((now) => {
        if (openRef.current || !now) return
        const decision = blogReloadDecision(opened, now)
        if (decision === 'reload') { top.location.reload()
}
        else if (decision === 'wait' && tries < RELOAD_POLL_LIMIT) {
          settleTimer.current = window.setTimeout(settle, RELOAD_POLL_MS, tries + 1)
        }
      })
    }
    settle(0)
  }, [host, open])

  // 给工具窗口用的接口。
  useEffect(() => {
    if (!host) return
    setStudioOverlayApi(host, {
      open: (spec: StudioToolSpec) => {
        whenEditorMayLeave(spec, () => {
          changeLayout((current) => openTool(current, spec))
          setOpen(true)
        })
      },
      close: (tool) => changeLayout((current) => closeTool(current, tool)),
      remember: (spec: StudioToolSpec) => changeLayout((current) => rememberTool(current, spec)),
      hide: () => setOpen(false),
      port: portFor,
    })
    return () => setStudioOverlayApi(host, null)
  }, [host])

  // 博客页上的快捷键和 Esc。
  useEffect(() => {
    const top = host?.ownerDocument.defaultView
    if (!top) return
    return listenOverlayKeys(top, {
      toggle: () => setOpen((value) => !value),
      escape: () => setOpen(false),
    })
  }, [host])

  // 把覆盖层画进宿主节点的 Shadow DOM。
  useEffect(() => {
    if (!host) return
    const shadow = host.shadowRoot ?? host.attachShadow({ mode: 'open' })
    shadow.replaceChildren()
    const style = host.ownerDocument.createElement('style')
    style.textContent = overlayCss
    const container = host.ownerDocument.createElement('div')
    shadow.append(style, container)
    rootRef.current = createRoot(container)
    // 明暗跟着博客页：这一页的 dark 类和博客页读的是同一个主题偏好。强调色用主站算出来的那个
    // （博客页的 myriad_chrome cookie），和窗口里主站页面的按钮一样；读不到就用样式表里的金色。
    const html = document.documentElement
    const accent = siteAccent(host.ownerDocument)
    if (accent) host.style.setProperty('--so-accent', accent)
    const syncScheme = () => {
      host.dataset.scheme = html.classList.contains('dark') ? 'dark' : 'light'
    }
    syncScheme()
    const observer = new MutationObserver(syncScheme)
    observer.observe(html, { attributes: true, attributeFilter: ['class'] })
    return () => {
      observer.disconnect()
      const root = rootRef.current
      rootRef.current = null
      queueMicrotask(() => root?.unmount())
      shadow.replaceChildren()
    }
  }, [host])

  useEffect(() => {
    const top = host?.ownerDocument.defaultView
    if (!rootRef.current || !top) return
    rootRef.current.render(
      <StudioOverlay
        win={top}
        copy={copy}
        context={context}
        open={open}
        layout={layout}
        onOpenChange={setOpen}
        onLayout={updateLayout}
        frameRefs={frameEls}
        mayLeave={mayLeave}
        onFrameLoad={(tool, frame) => {
          // 载进来的不是这个应用（服务器出错页之类）就不会有人报就绪，直接露出来。
          let app = false
          try {
            app = frame.contentDocument?.getElementById('app-root') != null
          } catch {
            app = false
          }
          if (!app) markReady(tool)
          const inner = frame.contentWindow
          if (!inner) return
          // 工具窗口里只听开关快捷键；Esc 留给窗口自己的对话框。
          listenOverlayKeys(inner, { toggle: () => setOpen((value) => !value), escape: () => {} })
        }}
      />,
    )
  })

  return <DocumentReady>{null}</DocumentReady>
}
