/**
 * 创作台住在博客页的创作台岛里（`details.myriad-studio`）。博客页自己不跑脚本
 * （CSP 是 `script-src 'none'`），这个 iframe 同源，由它关岛、去别的博客页、
 * 在博客重新生成后刷新外层页面。父页面的节点不做 `instanceof` 判断。
 */

export interface StudioIslandHost {
  island: HTMLDetailsElement
  /** 岛下面盖住整页的遮罩。点它就收起。 */
  sheet: HTMLElement | null
  top: Window
}

/** 不在创作台岛里（被直接打开、跨源）时为 null。 */
export function findStudioIsland(): StudioIslandHost | null {
  try {
    const frame = window.frameElement
    const island = frame?.closest('details.myriad-studio')
    if (!frame || !island || !window.top) return null
    return {
      island: island as HTMLDetailsElement,
      sheet: frame.parentElement,
      top: window.top,
    }
  } catch {
    return null
  }
}

export function closeStudioIsland(host: StudioIslandHost | null = findStudioIsland()): void {
  if (host) host.island.open = false
}

/** 去掉外层地址里的 `?studio=`：收岛后刷新或前进后退时，岛不会自己再展开。 */
export function withoutStudioParam(href: string): string {
  const url = new URL(href)
  url.searchParams.delete('studio')
  return url.toString()
}

/** 文章页在外层整页打开；不在岛里时开新标签。 */
export function openBlogPage(link: string): void {
  const host = findStudioIsland()
  if (host) {
    host.top.location.assign(link)
    return
  }
  window.open(link, '_blank', 'noopener,noreferrer')
}

/** 博客构建的快照：上次生成时间和状态。 */
export interface BlogBuildMark {
  status: string
  lastBuiltAt: string | null
}

/** 收岛时要不要刷新外层页面、要不要再等一会儿。 */
export function blogReloadDecision(
  opened: BlogBuildMark | null,
  now: BlogBuildMark,
): 'reload' | 'wait' | 'stay' {
  if (now.status === 'queued' || now.status === 'running') return 'wait'
  if (!opened) return 'stay'
  return now.lastBuiltAt && now.lastBuiltAt !== opened.lastBuiltAt ? 'reload' : 'stay'
}
