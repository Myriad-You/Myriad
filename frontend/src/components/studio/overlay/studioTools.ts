/**
 * 创作台覆盖层里的工具。每个工具是一个独立窗口，内容是嵌入页
 * `/embed/studio/tool/{tool}`；覆盖层只管开关、摆放和钉住。
 */

export const STUDIO_TOOLS = [
  'overview',
  'editor',
  'notes',
  'comments',
  'categories',
  'reviews',
  'transfer',
  'media',
  'settings',
] as const

export type StudioTool = (typeof STUDIO_TOOLS)[number]

const TOOL_SET = new Set<string>(STUDIO_TOOLS)

export function isStudioTool(value: string): value is StudioTool {
  return TOOL_SET.has(value)
}

/** 覆盖层要打开的工具和参数：`editor-12`、`editor-new`、`media-kept`。 */
export interface StudioToolSpec {
  tool: StudioTool
  arg: string | null
}

export function parseToolSpec(raw: string | null | undefined): StudioToolSpec | null {
  const value = raw?.trim() ?? ''
  if (!/^[a-z0-9-]{1,40}$/.test(value)) return null
  const dash = value.indexOf('-')
  const tool = dash < 0 ? value : value.slice(0, dash)
  const arg = dash < 0 ? null : value.slice(dash + 1) || null
  if (!isStudioTool(tool)) return null
  return { tool, arg }
}

export function formatToolSpec(spec: StudioToolSpec): string {
  return spec.arg ? `${spec.tool}-${spec.arg}` : spec.tool
}

/** 工具窗口里加载的嵌入页。上下文（这一页对应的笔记）跟着 query 走。 */
export function toolFrameSrc(
  spec: StudioToolSpec,
  context: { itemId: number | null; docId: number | null },
): string {
  const query = new URLSearchParams()
  if (spec.arg) query.set('arg', spec.arg)
  if (context.itemId != null) query.set('item', String(context.itemId))
  if (context.docId != null) query.set('doc', String(context.docId))
  const suffix = query.size > 0 ? `?${query}` : ''
  return `/embed/studio/tool/${spec.tool}${suffix}`
}

/** 一个窗口在屏幕上的样子。坐标是视口像素。 */
export interface StudioWindowLayout {
  x: number
  y: number
  w: number
  h: number
  pinned: boolean
  /** 钉住后、覆盖层收起时的不透明度，0.2–1。 */
  opacity: number
}

/**
 * 一个开着的窗口。`arg` 是它现在显示的内容；`loaded` 是它的 iframe 实际加载的参数，
 * 只在明确打开别的内容时才跟上 `arg`。窗口自己换了内容（新笔记拿到 id）只改 `arg`，
 * 不重新加载。两者都只活在这一条里：窗口一关就一起没了。
 */
export interface StudioOpenWindow extends StudioToolSpec {
  loaded: string | null
}

export interface StudioOverlayLayout {
  /** 开着的窗口，按叠放次序，最后一个在最上面。 */
  open: StudioOpenWindow[]
  windows: Partial<Record<StudioTool, StudioWindowLayout>>
  /** 工具栏只显示图标还是带文字。 */
  toolbar: 'icons' | 'list'
}

export const DEFAULT_LAYOUT: StudioOverlayLayout = {
  open: [{ tool: 'overview', arg: null, loaded: null }],
  windows: {},
  toolbar: 'list',
}

const DEFAULT_SIZE: Record<StudioTool, { w: number; h: number }> = {
  overview: { w: 420, h: 520 },
  editor: { w: 920, h: 760 },
  notes: { w: 760, h: 640 },
  comments: { w: 560, h: 640 },
  categories: { w: 620, h: 560 },
  reviews: { w: 640, h: 600 },
  transfer: { w: 620, h: 560 },
  media: { w: 860, h: 680 },
  settings: { w: 760, h: 720 },
}

export const WINDOW_MIN = { w: 300, h: 220 }
/** 左侧工具栏和顶部细条让出来的地方。 */
export const STAGE_INSET = { left: 232, top: 56, right: 16, bottom: 16 }

function clamp(value: number, low: number, high: number): number {
  return Math.min(Math.max(value, low), Math.max(low, high))
}

/** 窗口不跑出视口，也不小于最小尺寸。视口太小就尽量铺满。 */
export function fitWindow(
  layout: StudioWindowLayout,
  viewport: { w: number; h: number },
): StudioWindowLayout {
  const w = clamp(layout.w, WINDOW_MIN.w, viewport.w - 16)
  const h = clamp(layout.h, WINDOW_MIN.h, viewport.h - STAGE_INSET.top - 8)
  return {
    ...layout,
    w,
    h,
    x: clamp(layout.x, 8, viewport.w - w - 8),
    y: clamp(layout.y, STAGE_INSET.top, viewport.h - h - 8),
    opacity: clamp(layout.opacity, 0.2, 1),
  }
}

/** 第一次打开的窗口：从工具栏右边开始，按已开窗口数往右下错开。 */
export function defaultWindow(
  tool: StudioTool,
  openCount: number,
  viewport: { w: number; h: number },
): StudioWindowLayout {
  const size = DEFAULT_SIZE[tool]
  const step = 28 * (openCount % 6)
  return fitWindow(
    {
      x: STAGE_INSET.left + step,
      y: STAGE_INSET.top + step,
      w: size.w,
      h: size.h,
      pinned: false,
      opacity: 0.85,
    },
    viewport,
  )
}

function num(value: unknown, fallback: number): number {
  return typeof value === 'number' && Number.isFinite(value) ? value : fallback
}

/** 从浏览器存储读回来的布局。坏数据一律退回默认。 */
export function layoutFromValue(value: unknown): StudioOverlayLayout {
  if (!value || typeof value !== 'object') return DEFAULT_LAYOUT
  const data = value as Record<string, unknown>
  const open: StudioOpenWindow[] = []
  for (const raw of Array.isArray(data.open) ? data.open : []) {
    const spec = typeof raw === 'string' ? parseToolSpec(raw) : null
    if (spec && !open.some((item) => item.tool === spec.tool)) open.push({ ...spec, loaded: spec.arg })
  }
  const windows: StudioOverlayLayout['windows'] = {}
  const rawWindows =
    data.windows && typeof data.windows === 'object' ? (data.windows as Record<string, unknown>) : {}
  for (const [tool, raw] of Object.entries(rawWindows)) {
    if (!isStudioTool(tool) || !raw || typeof raw !== 'object') continue
    const row = raw as Record<string, unknown>
    const size = DEFAULT_SIZE[tool]
    windows[tool] = {
      x: num(row.x, STAGE_INSET.left),
      y: num(row.y, STAGE_INSET.top),
      w: num(row.w, size.w),
      h: num(row.h, size.h),
      pinned: row.pinned === true,
      opacity: num(row.opacity, 0.85),
    }
  }
  return {
    open: Array.isArray(data.open) ? open : DEFAULT_LAYOUT.open,
    windows,
    toolbar: data.toolbar === 'icons' ? 'icons' : 'list',
  }
}

/** 存的是窗口现在显示的内容；下次进来按它加载。 */
export function layoutToValue(layout: StudioOverlayLayout): unknown {
  return {
    open: layout.open.map((item) => formatToolSpec({ tool: item.tool, arg: item.arg })),
    windows: layout.windows,
    toolbar: layout.toolbar,
  }
}

/**
 * 打开一个工具并提到最上面。已经开着同一份内容就原样留着（不重新加载）；
 * 内容不同或没开，就按新参数加载。
 */
export function openTool(layout: StudioOverlayLayout, spec: StudioToolSpec): StudioOverlayLayout {
  const showing = layout.open.find((item) => item.tool === spec.tool)
  const entry: StudioOpenWindow =
    showing && showing.arg === spec.arg ? showing : { tool: spec.tool, arg: spec.arg, loaded: spec.arg }
  const rest = layout.open.filter((item) => item.tool !== spec.tool)
  return { ...layout, open: [...rest, entry] }
}

/** 窗口自己换了内容：记下它现在显示什么，但不重新加载。没开着就不管。 */
export function rememberTool(layout: StudioOverlayLayout, spec: StudioToolSpec): StudioOverlayLayout {
  if (!layout.open.some((item) => item.tool === spec.tool && item.arg !== spec.arg)) return layout
  return {
    ...layout,
    open: layout.open.map((item) => (item.tool === spec.tool ? { ...item, arg: spec.arg } : item)),
  }
}

/** 窗口的 iframe 该加载的地址。 */
export function windowFrameSrc(
  item: StudioOpenWindow,
  context: { itemId: number | null; docId: number | null },
): string {
  return toolFrameSrc({ tool: item.tool, arg: item.loaded }, context)
}

export function closeTool(layout: StudioOverlayLayout, tool: StudioTool): StudioOverlayLayout {
  return { ...layout, open: layout.open.filter((item) => item.tool !== tool) }
}

/**
 * 下一个该开始加载的窗口。窗口一个接一个地加载：几份前端同时下载整套模块时，
 * 开发模式下观察到会互相卡住。只加载看得见的窗口，最上面的先来。
 */
export function nextFrameToLoad(
  visibleTopFirst: readonly StudioTool[],
  loaded: ReadonlySet<StudioTool>,
  loading: StudioTool | null,
): StudioTool | null {
  if (loading && visibleTopFirst.includes(loading)) return loading
  return visibleTopFirst.find((tool) => !loaded.has(tool)) ?? null
}

/** 开关覆盖层的快捷键：Alt+`。在输入框里也照样生效，因为它不会打出字。 */
export function isOverlayHotkey(event: { altKey: boolean; ctrlKey: boolean; metaKey: boolean; code: string }): boolean {
  return event.altKey && !event.ctrlKey && !event.metaKey && event.code === 'Backquote'
}

/** 旧的 `/studio/...` 换成覆盖层要打开的工具。 */
export function legacySectionSpec(rest: string): StudioToolSpec {
  const path = rest.replace(/\/+$/, '')
  const note = /^\/notes\/(new|[1-9]\d*)$/.exec(path)
  if (note) return { tool: 'editor', arg: note[1] }
  const media = /^\/media(?:\/(uploads|generated|kept|channel|imported))?$/.exec(path)
  if (media) return { tool: 'media', arg: media[1] ?? null }
  switch (path) {
    case '/notes':
      return { tool: 'notes', arg: null }
    case '/comments':
      return { tool: 'comments', arg: null }
    case '/reviews':
      return { tool: 'reviews', arg: null }
    case '/categories':
      return { tool: 'categories', arg: null }
    case '/import':
      return { tool: 'transfer', arg: null }
    case '/blog':
      return { tool: 'settings', arg: null }
    default:
      return { tool: 'overview', arg: null }
  }
}
