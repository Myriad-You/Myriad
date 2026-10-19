/**
 * 创作台覆盖层住在博客页上。博客页自己不跑脚本（CSP 是 `script-src 'none'`），
 * 由一个看不见的同源控制器 iframe 把覆盖层画进宿主节点，并在宿主节点上挂一个接口；
 * 各个工具窗口（也是同源 iframe）通过它开关别的工具。父页面的节点不做 `instanceof` 判断。
 */

import type { StudioTool, StudioToolSpec } from './overlay/studioTools'
import { isDocumentReady, subscribeDocumentReady } from '../../utils/pageLoader'
import { embedHost } from '../embed/embedHost'

export const STUDIO_HOST_ID = 'myriad-studio-host'

/** 控制器挂在宿主节点上的接口。 */
export interface StudioOverlayApi {
  open: (spec: StudioToolSpec) => void
  close: (tool: StudioTool) => void
  /** 窗口自己换了内容（新笔记拿到 id）时记进布局，但不重新加载窗口。 */
  remember: (spec: StudioToolSpec) => void
  /** 收起覆盖层，钉住的窗口留着。 */
  hide: () => void
  /** 工具窗口拿自己和覆盖层之间的那一头；认不出是哪个窗口时给一个什么也不做的。 */
  port: (win: Window) => StudioWindowPort
}

/** 一个工具窗口和覆盖层之间的全部约定。 */
export interface StudioWindowPort {
  /** 第一屏画好了，覆盖层撤掉占位。 */
  ready: () => void
  /**
   * 关窗口、换内容之前先问窗口（编辑器先问没保存的改动、清掉空的新笔记），
   * 给 false 就不关不换。传 null 撤回。只对登记它的那一页有效，窗口换了地址就失效。
   */
  guard: (check: (() => Promise<boolean>) | null) => void
}

interface HostWithApi extends HTMLElement {
  __myriadStudio?: StudioOverlayApi
}

/** 博客页上的宿主节点。不在博客页里（被直接打开、跨源）时为 null。 */
export function findStudioHost(win: Window = window): HostWithApi | null {
  return (embedHost(win)?.doc.getElementById(STUDIO_HOST_ID) as HostWithApi | null) ?? null
}

export function setStudioOverlayApi(host: HTMLElement, api: StudioOverlayApi | null): void {
  const target = host as HostWithApi
  if (api) target.__myriadStudio = api
  else delete target.__myriadStudio
}

export function studioOverlayApi(): StudioOverlayApi | null {
  return findStudioHost()?.__myriadStudio ?? null
}

/** 本窗口在覆盖层里的那一头。不在覆盖层里时为 null。 */
export function studioWindowPort(win: Window = window): StudioWindowPort | null {
  return findStudioHost(win)?.__myriadStudio?.port(win) ?? null
}

/**
 * 每个页面启动时调一次：在覆盖层的工具窗口里，第一屏画好（出错页也算）就告诉覆盖层，
 * 覆盖层这才撤掉占位。不在覆盖层里什么也不做。
 */
export function announceStudioWindow(): void {
  if (!findStudioHost()) return
  const done = () => studioWindowPort()?.ready()
  if (isDocumentReady()) {
    done()
    return
  }
  const stop = subscribeDocumentReady(() => {
    stop()
    done()
  })
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
