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
    // `/librarything` is not `/library`
    if (pathname === prefix || pathname.startsWith(`${prefix}/`)) return route
  }
  return 'other'
}

export interface AgentVisibleContext {
  kind: 'selection' | 'content' | 'route'
  route: AgentContextRoute
  title?: string
  selection?: string
}

export function resolveAgentContext(input: {
  pathname: string
  pageTitle?: string | null
  hasPageContent: boolean
  selection?: string
  /** Off: treat page content as absent. */
  contextConsent?: boolean
}): AgentVisibleContext {
  const route = agentContextRoute(input.pathname)
  const selection = input.selection?.trim()
  if (selection) return { kind: 'selection', route, selection }

  const consent = input.contextConsent ?? true
  const title = input.pageTitle?.trim()
  // untitled page with body is still content
  if (consent && input.hasPageContent) {
    return title
      ? { kind: 'content', route, title }
      : { kind: 'content', route }
  }
  return { kind: 'route', route }
}
