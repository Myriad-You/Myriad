import type { ReactNode, PointerEvent as ReactPointerEvent } from 'react'
import type {
  StudioOverlayLayout,
  StudioTool,
  StudioToolSpec,
  StudioWindowLayout,
} from './studioTools'
import {
  LuColumns2,
  LuDownload,
  LuEye,
  LuFileText,
  LuImage,
  LuInbox,
  LuMessageCircle,
  LuNotebookPen,
  LuPin,
  LuPinOff,
  LuSettings,
  LuTag,
  LuX,
} from '@lib/icons'
import { useCallback, useEffect, useRef, useState } from 'react'
import {
  closeTool,
  defaultWindow,
  fitWindow,
  isOverlayHotkey,
  nextFrameToLoad,
  openTool,
  toolFrameSrc,
  WINDOW_MIN,
} from './studioTools'

export interface StudioOverlayCopy {
  studio: string
  editPost: string
  hotkey: string
  pin: string
  unpin: string
  opacity: string
  close: string
  toolbarIcons: string
  toolbarList: string
  groupPage: string
  groupBlog: string
  groupSite: string
  tools: Record<StudioTool, string>
}

export interface StudioPageContext {
  itemId: number | null
  docId: number | null
}

const ICONS: Record<StudioTool, ReactNode> = {
  overview: <LuEye />,
  editor: <LuNotebookPen />,
  notes: <LuFileText />,
  comments: <LuMessageCircle />,
  categories: <LuTag />,
  reviews: <LuInbox />,
  transfer: <LuDownload />,
  media: <LuImage />,
  settings: <LuSettings />,
}

/** 工具栏分组：先是这一页，再是博客，最后是全站共用的媒体。 */
const GROUPS: Array<{ key: 'groupPage' | 'groupBlog' | 'groupSite'; tools: StudioTool[] }> = [
  { key: 'groupPage', tools: ['overview', 'editor'] },
  { key: 'groupBlog', tools: ['notes', 'comments', 'categories', 'reviews', 'transfer', 'settings'] },
  { key: 'groupSite', tools: ['media'] },
]

type Drag =
  | { kind: 'move'; tool: StudioTool; startX: number; startY: number; from: StudioWindowLayout }
  | { kind: 'resize'; tool: StudioTool; startX: number; startY: number; from: StudioWindowLayout }

export function StudioOverlay({
  win,
  copy,
  context,
  open,
  layout,
  frameSpec,
  onOpenChange,
  onLayout,
  onFrameLoad,
  frameRefs,
  ready,
  mayLeave,
}: {
  /** 博客页的 window：视口尺寸和指针事件都以它为准。 */
  win: Window
  copy: StudioOverlayCopy
  context: StudioPageContext
  open: boolean
  layout: StudioOverlayLayout
  /** 窗口里实际加载的工具地址。只在明确打开时才变，免得窗口自己记下新 id 时被重新加载。 */
  frameSpec: (spec: StudioToolSpec) => StudioToolSpec
  onOpenChange: (open: boolean) => void
  onLayout: (next: StudioOverlayLayout) => void
  onFrameLoad: (tool: StudioTool, frame: HTMLIFrameElement) => void
  /** 每个窗口的 iframe。控制器要认出是哪个窗口在跟它说话，所以由它传进来。 */
  frameRefs: { current: Map<StudioTool, HTMLIFrameElement> }
  /** 窗口里的应用已经画出来了（窗口自己报的）；之前窗口只显示占位，不露出整页的启动画面。 */
  ready: ReadonlySet<StudioTool>
  /** 关窗口之前问窗口自己（编辑器要先问没保存的改动），给 false 就不关。 */
  mayLeave: (tool: StudioTool) => Promise<boolean>
}) {
  const [viewport, setViewport] = useState(() => ({ w: win.innerWidth, h: win.innerHeight }))
  const [drag, setDrag] = useState<Drag | null>(null)
  const [loaded, setLoaded] = useState<ReadonlySet<StudioTool>>(() => new Set())
  const layoutRef = useRef(layout)
  layoutRef.current = layout

  useEffect(() => {
    const onResize = () => setViewport({ w: win.innerWidth, h: win.innerHeight })
    win.addEventListener('resize', onResize)
    return () => win.removeEventListener('resize', onResize)
  }, [win])

  const windowOf = useCallback(
    (tool: StudioTool, index: number): StudioWindowLayout =>
      fitWindow(layoutRef.current.windows[tool] ?? defaultWindow(tool, index, viewport), viewport),
    [viewport],
  )

  const patchWindow = useCallback(
    (tool: StudioTool, patch: Partial<StudioWindowLayout>) => {
      const current = layoutRef.current
      const index = current.open.findIndex((item) => item.tool === tool)
      const base = current.windows[tool] ?? defaultWindow(tool, Math.max(index, 0), viewport)
      onLayout({ ...current, windows: { ...current.windows, [tool]: { ...base, ...patch } } })
    },
    [onLayout, viewport],
  )

  const raise = useCallback(
    (spec: StudioToolSpec) => {
      const current = layoutRef.current
      if (current.open.at(-1)?.tool === spec.tool) return
      onLayout(openTool(current, spec))
    },
    [onLayout],
  )

  useEffect(() => {
    if (!drag) return
    const onMove = (event: PointerEvent) => {
      const dx = event.clientX - drag.startX
      const dy = event.clientY - drag.startY
      if (drag.kind === 'move') {
        patchWindow(drag.tool, { x: drag.from.x + dx, y: drag.from.y + dy })
      } else {
        patchWindow(drag.tool, {
          w: Math.max(WINDOW_MIN.w, drag.from.w + dx),
          h: Math.max(WINDOW_MIN.h, drag.from.h + dy),
        })
      }
    }
    const onUp = () => setDrag(null)
    win.addEventListener('pointermove', onMove)
    win.addEventListener('pointerup', onUp)
    win.addEventListener('pointercancel', onUp)
    return () => {
      win.removeEventListener('pointermove', onMove)
      win.removeEventListener('pointerup', onUp)
      win.removeEventListener('pointercancel', onUp)
    }
  }, [drag, patchWindow, win])

  const startDrag = (
    event: ReactPointerEvent,
    kind: Drag['kind'],
    tool: StudioTool,
    from: StudioWindowLayout,
  ) => {
    if (event.button !== 0) return
    if ((event.target as Element).closest('button, input')) return
    event.preventDefault()
    setDrag({ kind, tool, startX: event.clientX, startY: event.clientY, from })
  }

  /** 关一个窗口。先问窗口自己（编辑器要先问没保存的改动），它点头了才关。 */
  const closeWindow = (tool: StudioTool) => {
    void mayLeave(tool).then((ok) => {
      if (ok) onLayout(closeTool(layoutRef.current, tool))
    })
  }

  const toggleTool = (tool: StudioTool) => {
    const current = layoutRef.current
    const isOpen = current.open.some((item) => item.tool === tool)
    const isTop = current.open.at(-1)?.tool === tool
    if (isOpen && isTop) {
      closeWindow(tool)
    } else {
      const arg =
        tool === 'editor' ? (context.docId != null ? String(context.docId) : 'new') : null
      const existing = current.open.find((item) => item.tool === tool)
      onLayout(openTool(current, existing ?? { tool, arg }))
    }
  }

  const topTool = layout.open.at(-1)?.tool
  const editingHere =
    context.docId != null &&
    layout.open.some((item) => item.tool === 'editor' && item.arg === String(context.docId))
  const visibleTopFirst = layout.open
    .filter((spec) => open || layout.windows[spec.tool]?.pinned)
    .map((spec) => spec.tool)
    .reverse()
  const openTools = new Set(layout.open.map((spec) => spec.tool))
  const loadedOpen = new Set([...loaded].filter((tool) => openTools.has(tool)))
  if (loadedOpen.size !== loaded.size) setLoaded(loadedOpen)
  const loadingNow = nextFrameToLoad(visibleTopFirst, loadedOpen, null)

  return (
    <div className="stage" data-dragging={drag ? 'true' : undefined}>
      <div className="bar glass">
        <button
          type="button"
          className="bar__btn"
          aria-pressed={open}
          onClick={() => onOpenChange(!open)}
        >
          <span className="bar__dot" aria-hidden />
          <span>{copy.studio}</span>
        </button>
        {context.docId != null ? (
          <button
            type="button"
            className="bar__btn"
            aria-pressed={open && editingHere}
            onClick={() => {
              // 已经在最上面编辑这篇：再点就是关掉，和工具栏一样，不是原地再开一次。
              if (open && editingHere && topTool === 'editor') {
                closeWindow('editor')
                return
              }
              onLayout(openTool(layoutRef.current, { tool: 'editor', arg: String(context.docId) }))
              onOpenChange(true)
            }}
          >
            {copy.editPost}
          </button>
        ) : null}
        <span className="bar__key" title={copy.hotkey}>
          Alt+`
        </span>
      </div>

      {open ? <div className="dim" onClick={() => onOpenChange(false)} /> : null}

      {open ? (
        <nav className="toolbar glass" data-mode={layout.toolbar} aria-label={copy.studio}>
          {GROUPS.map((group) => (
            <div key={group.key} role="group" aria-label={copy[group.key]}>
              <div className="toolbar__group">{copy[group.key]}</div>
              {group.tools.map((tool) => {
                const isOpen = layout.open.some((item) => item.tool === tool)
                return (
                  <button
                    key={tool}
                    type="button"
                    className="tool"
                    aria-pressed={isOpen}
                    title={copy.tools[tool]}
                    onClick={() => toggleTool(tool)}
                  >
                    {ICONS[tool]}
                    <span className="tool__label">{copy.tools[tool]}</span>
                    {layout.windows[tool]?.pinned && isOpen ? <LuPin className="tool__pin" /> : null}
                  </button>
                )
              })}
            </div>
          ))}
          <div className="toolbar__foot">
            <button
              type="button"
              className="tool"
              title={layout.toolbar === 'list' ? copy.toolbarIcons : copy.toolbarList}
              onClick={() =>
                onLayout({ ...layoutRef.current, toolbar: layout.toolbar === 'list' ? 'icons' : 'list' })
              }
            >
              <LuColumns2 />
              <span className="tool__label">
                {layout.toolbar === 'list' ? copy.toolbarIcons : copy.toolbarList}
              </span>
            </button>
          </div>
        </nav>
      ) : null}

      {layout.open.map((spec, index) => {
        const box = windowOf(spec.tool, index)
        const pinnedView = !open && box.pinned
        const visible = open || box.pinned
        return (
          <section
            key={spec.tool}
            className="win"
            data-away={visible ? undefined : 'true'}
            inert={!visible}
            aria-label={copy.tools[spec.tool]}
            data-focus={open && topTool === spec.tool ? 'true' : undefined}
            data-pinned-view={pinnedView ? 'true' : undefined}
            data-dragging={drag?.tool === spec.tool ? 'true' : undefined}
            style={{
              left: box.x,
              top: box.y,
              width: box.w,
              height: box.h,
              zIndex: 10 + index,
              opacity: pinnedView ? box.opacity : 1,
            }}
            onPointerDownCapture={() => {
              if (open) raise(spec)
            }}
          >
            {pinnedView ? null : (
              <header
                className="win__title"
                onPointerDown={(event) => startDrag(event, 'move', spec.tool, box)}
              >
                {ICONS[spec.tool]}
                <span className="win__name">{copy.tools[spec.tool]}</span>
                {box.pinned ? (
                  <input
                    className="win__opacity"
                    type="range"
                    min={20}
                    max={100}
                    step={5}
                    value={Math.round(box.opacity * 100)}
                    aria-label={copy.opacity}
                    title={copy.opacity}
                    onChange={(event) =>
                      patchWindow(spec.tool, { opacity: Number(event.target.value) / 100 })
                    }
                  />
                ) : null}
                <button
                  type="button"
                  className="win__btn"
                  aria-pressed={box.pinned}
                  aria-label={box.pinned ? copy.unpin : copy.pin}
                  title={box.pinned ? copy.unpin : copy.pin}
                  onClick={() => patchWindow(spec.tool, { pinned: !box.pinned })}
                >
                  {box.pinned ? <LuPinOff /> : <LuPin />}
                </button>
                <button
                  type="button"
                  className="win__btn"
                  aria-label={copy.close}
                  title={copy.close}
                  onClick={() => closeWindow(spec.tool)}
                >
                  <LuX />
                </button>
              </header>
            )}
            <div className="win__body" data-ready={ready.has(spec.tool)}>
              {ready.has(spec.tool) ? null : <FramePlaceholder />}
              {loadedOpen.has(spec.tool) || loadingNow === spec.tool ? (
                <iframe
                  ref={(node) => {
                    if (node) frameRefs.current.set(spec.tool, node)
                    else frameRefs.current.delete(spec.tool)
                  }}
                  src={toolFrameSrc(frameSpec(spec), context)}
                  title={copy.tools[spec.tool]}
                  onLoad={(event) => {
                    const tool = spec.tool
                    setLoaded((current) => (current.has(tool) ? current : new Set([...current, tool])))
                    onFrameLoad(tool, event.currentTarget)
                  }}
                />
              ) : null}
            </div>
            {pinnedView ? null : (
              <div
                className="win__resize"
                aria-hidden
                onPointerDown={(event) => startDrag(event, 'resize', spec.tool, box)}
              />
            )}
          </section>
        )
      })}
    </div>
  )
}

/** 窗口还没画出来时的占位：几条灰条，形状像工具条加列表。 */
function FramePlaceholder() {
  return (
    <div className="win__placeholder" aria-hidden>
      <span className="win__placeholder-bar" />
      <span />
      <span />
      <span />
    </div>
  )
}

/** 快捷键在博客页和每个工具窗口里都要听：键盘事件不会从 iframe 冒到外面。 */
export function listenOverlayKeys(
  target: Window,
  handlers: { toggle: () => void; escape: () => void },
): () => void {
  const onKey = (event: KeyboardEvent) => {
    if (isOverlayHotkey(event)) {
      event.preventDefault()
      handlers.toggle()
    } else if (event.key === 'Escape' && !event.defaultPrevented) {
      handlers.escape()
    }
  }
  target.addEventListener('keydown', onKey)
  return () => target.removeEventListener('keydown', onKey)
}
