/**
 * 「它现在看得到什么」—— Context Card 的文字部分。
 *
 * 助手每次请求都会带上当前路由和页面正文，但界面上从来没说过这件事。这里把它
 * 摊开：先算出该报哪个上下文，组件再配上文案。纯函数，不认识 React 也不认识 i18n。
 */

/** 报得出名字的路由。认不出来的一律 `other`，不瞎猜。 */
export type AgentContextRoute =
  'home' | 'library' | 'brew' | 'reports' | 'config' | 'tapp' | 'other'

const ROUTE_PREFIXES: ReadonlyArray<readonly [string, AgentContextRoute]> = [
  ['/library', 'library'],
  ['/brew', 'brew'],
  ['/reports', 'reports'],
  ['/config', 'config'],
  ['/tapp', 'tapp'],
]

export function agentContextRoute(pathname: string): AgentContextRoute {
  if (pathname === '' || pathname === '/') return 'home'
  for (const [prefix, route] of ROUTE_PREFIXES) {
    // 只认整段，`/librarything` 不算 `/library`
    if (pathname === prefix || pathname.startsWith(`${prefix}/`)) return route
  }
  return 'other'
}

export interface AgentVisibleContext {
  /**
   * `selection`：用户划出了一段字，指着的东西比在哪儿具体。
   * `content`：页面有正文，助手看得到内容本身，可以总结/翻译。
   * `route`：只知道在哪一页。
   */
  kind: 'selection' | 'content' | 'route'
  /** 有正文时是标题，否则是路由标识。 */
  route: AgentContextRoute
  title?: string
  /** 已归一的选中文本。 */
  selection?: string
}

export function resolveAgentContext(input: {
  pathname: string
  pageTitle?: string | null
  hasPageContent: boolean
  selection?: string
  /** 关掉「读当前页」之后，页面正文这一路就当不存在。 */
  contextConsent?: boolean
}): AgentVisibleContext {
  const route = agentContextRoute(input.pathname)
  const selection = input.selection?.trim()
  // 选区压过页面正文：用户指着的东西比他在哪一页具体得多
  if (selection) return { kind: 'selection', route, selection }

  const consent = input.contextConsent ?? true
  const title = input.pageTitle?.trim()
  // 有正文但没标题时仍然算 content —— 能总结的是正文，不是标题
  if (consent && input.hasPageContent) {
    return title
      ? { kind: 'content', route, title }
      : { kind: 'content', route }
  }
  return { kind: 'route', route }
}
