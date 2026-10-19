/**
 * 创作台覆盖层住在博客页上。博客页自己不跑脚本（CSP 是 `script-src 'none'`），
 * 由一个看不见的同源控制器 iframe 把覆盖层画进宿主节点，并在宿主节点上挂一个接口；
 * 各个工具窗口（也是同源 iframe）通过它开关别的工具。父页面的节点不做 `instanceof` 判断。
 */

import type { StudioTool, StudioToolSpec } from './overlay/studioTools'

export const STUDIO_HOST_ID = 'myriad-studio-host'

/** 控制器挂在宿主节点上的接口。 */
export interface StudioOverlayApi {
  open: (spec: StudioToolSpec) => void
  close: (tool: StudioTool) => void
  /** 窗口自己换了内容（新笔记拿到 id）时记进布局，但不重新加载窗口。 */
  remember: (spec: StudioToolSpec) => void
  /** 收起覆盖层，钉住的窗口留着。 */
  hide: () => void
}

interface HostWithApi extends HTMLElement {
  __myriadStudio?: StudioOverlayApi
}

/** 博客页上的宿主节点。不在博客页里（被直接打开、跨源）时为 null。 */
export function findStudioHost(win: Window = window): HostWithApi | null {
  try {
    const parent = win.parent
    if (!parent || parent === win) return null
    return (parent.document.getElementById(STUDIO_HOST_ID) as HostWithApi | null) ?? null
  } catch {
    return null
  }
}

export function setStudioOverlayApi(host: HTMLElement, api: StudioOverlayApi | null): void {
  const target = host as HostWithApi
  if (api) target.__myriadStudio = api
  else delete target.__myriadStudio
}

export function studioOverlayApi(): StudioOverlayApi | null {
  return findStudioHost()?.__myriadStudio ?? null
}

const FRAME_CLOSE = '__myriadStudioClose'

interface FrameWindow extends Window {
  [FRAME_CLOSE]?: () => void
}

/**
 * 工具窗口自己的关闭（编辑器要先问未保存的改动）。挂在窗口自己的 window 上，
 * 覆盖层点标题栏的关闭时先交给它；它决定关不关，关就调 `close`。
 */
export function setFrameCloseHandler(close: (() => void) | null, win: Window = window): void {
  const target = win as FrameWindow
  if (close) target[FRAME_CLOSE] = close
  else delete target[FRAME_CLOSE]
}

export function frameCloseHandler(frame: HTMLIFrameElement | null | undefined): (() => void) | null {
  try {
    return (frame?.contentWindow as FrameWindow | null)?.[FRAME_CLOSE] ?? null
  } catch {
    return null
  }
}

const FRAME_LEAVE = '__myriadStudioLeave'

interface LeaveWindow extends Window {
  [FRAME_LEAVE]?: (next: () => void) => void
}

/**
 * 工具窗口换内容之前的检查（编辑器换篇前先问没保存的改动）。通过了才调 `next`，
 * 窗口本身不关。挂法和关闭一样。
 */
export function setFrameLeaveHandler(
  leave: ((next: () => void) => void) | null,
  win: Window = window,
): void {
  const target = win as LeaveWindow
  if (leave) target[FRAME_LEAVE] = leave
  else delete target[FRAME_LEAVE]
}

export function frameLeaveHandler(
  frame: HTMLIFrameElement | null | undefined,
): ((next: () => void) => void) | null {
  try {
    return (frame?.contentWindow as LeaveWindow | null)?.[FRAME_LEAVE] ?? null
  } catch {
    return null
  }
}

/** 文章页在外层整页打开；不在覆盖层里时开新标签。 */
export function openBlogPage(link: string): void {
  if (findStudioHost()) {
    window.top?.location.assign(link)
    return
  }
  window.open(link, '_blank', 'noopener,noreferrer')
}

/** 去掉外层地址里的 `?studio=`：收起后刷新或前进后退时，覆盖层不会自己再展开。 */
export function withoutStudioParam(href: string): string {
  const url = new URL(href)
  url.searchParams.delete('studio')
  return url.toString()
}

/** 博客构建的快照：上次生成时间和状态。 */
export interface BlogBuildMark {
  status: string
  lastBuiltAt: string | null
}

/** 收起覆盖层时要不要刷新外层页面、要不要再等一会儿。 */
export function blogReloadDecision(
  opened: BlogBuildMark | null,
  now: BlogBuildMark,
): 'reload' | 'wait' | 'stay' {
  if (now.status === 'queued' || now.status === 'running') return 'wait'
  if (!opened) return 'stay'
  return now.lastBuiltAt && now.lastBuiltAt !== opened.lastBuiltAt ? 'reload' : 'stay'
}
