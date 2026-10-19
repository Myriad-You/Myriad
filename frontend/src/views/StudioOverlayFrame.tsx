import type { Root } from 'react-dom/client'
import type { StudioOverlayCopy, StudioPageContext } from '../components/studio/overlay/StudioOverlay'
import type { StudioOverlayLayout, StudioTool, StudioToolSpec } from '../components/studio/overlay/studioTools'
import type { BlogBuildMark } from '../components/studio/studioIsland'
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
  openTool,
  parseToolSpec,
} from '../components/studio/overlay/studioTools'
import {
  blogReloadDecision,
  findStudioHost,
  frameLeaveHandler,
  setStudioOverlayApi,
  withoutStudioParam,
} from '../components/studio/studioIsland'
import { useI18n } from '../contexts/I18nContext'
import { getBlogSettings } from '../services/blogApi'

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
  /** 每个窗口实际加载的地址，只有 `open` 会改它。 */
  const frames = useRef(new Map<string, StudioToolSpec>())
  const frameEls = useRef(new Map<StudioTool, HTMLIFrameElement>())
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

  /**
   * 编辑器要换到另一篇时，先让它自己检查（没保存的先问、空的新笔记清掉）；
   * 通过了再换。换回布局里已经开着的同一篇不算换。
   */
  const whenEditorMayLeave = (spec: StudioToolSpec | undefined, apply: () => void) => {
    const loaded = frames.current.get('editor')
    const showing = layoutNow.current.open.find((item) => item.tool === 'editor')
    const swapping = spec?.tool === 'editor' && loaded != null && showing?.arg !== spec.arg
    const leave = swapping ? frameLeaveHandler(frameEls.current.get('editor')) : null
    if (leave) leave(apply)
    else apply()
  }

  const updateLayout = (next: StudioOverlayLayout) => {
    whenEditorMayLeave(next.open.find((item) => item.tool === 'editor'), () => applyLayout(next))
  }

  const applyLayout = (next: StudioOverlayLayout) => {
    // 工具栏、细条上的「编辑这篇」直接改布局；参数变了就让窗口换地址。
    for (const spec of next.open) {
      const loaded = frames.current.get(spec.tool)
      const fromBar = !layout.open.some((item) => item.tool === spec.tool && item.arg === spec.arg)
      if (!loaded || fromBar) frames.current.set(spec.tool, spec)
    }
    for (const tool of [...frames.current.keys()]) {
      if (!next.open.some((item) => item.tool === tool)) frames.current.delete(tool)
    }
    setLayout(next)
    writeLayout(next)
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
          // 已经开着同一份内容就只提到最上面，不重新加载。
          const showing = layoutNow.current.open.find((item) => item.tool === spec.tool)
          if (showing?.arg !== spec.arg || !frames.current.has(spec.tool)) {
            frames.current.set(spec.tool, spec)
          }
          setLayout((current) => {
            const next = openTool(current, spec)
            writeLayout(next)
            return next
          })
          setOpen(true)
        })
      },
      close: (tool) => {
        setLayout((current) => {
          const next = closeTool(current, tool)
          writeLayout(next)
          return next
        })
      },
      remember: (spec: StudioToolSpec) => {
        setLayout((current) => {
          if (!current.open.some((item) => item.tool === spec.tool)) return current
          const next = {
            ...current,
            open: current.open.map((item) => (item.tool === spec.tool ? spec : item)),
          }
          writeLayout(next)
          return next
        })
      },
      hide: () => setOpen(false),
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
    return () => {
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
        frameSpec={(spec) => {
          const loaded = frames.current.get(spec.tool)
          if (loaded) return loaded
          frames.current.set(spec.tool, spec)
          return spec
        }}
        onOpenChange={setOpen}
        onLayout={updateLayout}
        frameRefs={frameEls}
        onFrameLoad={(frame) => {
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
