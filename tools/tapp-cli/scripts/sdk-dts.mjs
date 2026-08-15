/**
 * Generate sandbox-facing tapp-sdk.d.ts from the action permission catalog.
 * High-traffic namespaces use curated signatures; remaining namespaces are
 * projected generically from PERMISSION_MAP so the file stays complete.
 */

function indent(level) {
  return '  '.repeat(level)
}

function buildTree(actions) {
  const tree = {}
  for (const action of Object.keys(actions).sort()) {
    const parts = action.split('.')
    if (parts.length < 2) continue
    let cursor = tree
    for (let i = 0; i < parts.length - 1; i += 1) {
      const part = parts[i]
      cursor[part] ||= { methods: {}, children: {} }
      if (i === parts.length - 2) {
        cursor[part].methods[parts[i + 1]] = actions[action]
      } else {
        cursor = cursor[part].children
      }
    }
  }
  return tree
}

function renderNamespace(name, node, level = 1) {
  const lines = []
  const pad = indent(level)
  lines.push(`${pad}${name}: {`)
  for (const method of Object.keys(node.methods).sort()) {
    const permission = node.methods[method]
    const note =
      permission && permission !== 'public' ? ` // permission: ${permission}` : ''
    lines.push(
      `${indent(level + 1)}${method}(...args: unknown[]): Promise<unknown>${note}`,
    )
  }
  for (const child of Object.keys(node.children).sort()) {
    lines.push(...renderNamespace(child, node.children[child], level + 1))
  }
  lines.push(`${pad}}`)
  return lines
}

function localMembers() {
  return `  lifecycle: {
    onReady(callback: () => void | Promise<void>): void
    onDestroy(callback: () => void): void
    onPause(callback: () => void): void
    onResume(callback: () => void): void
    getInfo(): { id: string; version: string; name: string; permissions: string[]; sandboxed: true }
  }

  i18n: {
    t(key: string, params?: Record<string, unknown>): string
    getLocale(): string
    getAll(): Record<string, unknown>
  }

  storage: {
    get(key: string): Promise<unknown>
    set(key: string, value: unknown): Promise<unknown>
    remove(key: string): Promise<unknown>
    keys(): Promise<string[]>
    getAll(): Promise<Record<string, unknown>>
    clear(): Promise<unknown>
    usage(): Promise<unknown>
    onChanged(callback: (event: { key?: string; operation?: string }) => void): () => void
  }

  settings: {
    get(key: string): Promise<unknown>
    set(key: string, value: unknown): Promise<unknown>
    getAll(): Promise<Record<string, unknown>>
  }

  ui: {
    setTitle(title: string): Promise<unknown>
    getTheme(): Promise<unknown>
    onThemeChange(callback: (theme: unknown) => void): () => void
    getPrimaryColor(): Promise<unknown>
    onPrimaryColorChange(callback: (color: unknown) => void): () => void
    getLocale(): Promise<unknown>
    onLocaleChange(callback: (locale: unknown) => void): () => void
    showNotification(options: unknown): Promise<unknown>
    confirm(message: string): Promise<boolean>
    /**
     * Open a host browser tab for a Manifest \`openUrls\` allowlisted id only.
     * Pass \`{ id, path?, query? }\` — never a free-form absolute URL.
     * Requires permission \`ui:openUrl\`.
     */
    openUrl(request: {
      id: string
      path?: string
      query?: Record<string, string>
    } | string): Promise<unknown>
    /** List install-time openUrls declarations for this Tapp. */
    listOpenUrls(): Promise<
      Array<{ id: string; url: string; match: 'exact' | 'prefix' | 'origin' }>
    >
    requestFullscreen(): Promise<unknown>
    exitFullscreen(): Promise<unknown>
    fullscreen: {
      request(): Promise<unknown>
      exit(): Promise<unknown>
      toggle(): Promise<unknown>
      isFullscreen(): Promise<boolean>
    }
  }

  api: {
    (name: string, params?: unknown): Promise<unknown>
    list(): Promise<unknown>
  }

  assets: {
    list(): Promise<unknown>
    get(path: string): Promise<unknown>
    getUrl(path: string): Promise<{ url: string; mimeType?: string; size?: number; path?: string }>
    getArrayBuffer(path: string): Promise<{ path: string; mimeType?: string; size?: number; buffer: ArrayBuffer }>
    getUrlMap(): Promise<Record<string, string>>
    resolve(path: string): Promise<{ url: string; mimeType?: string; size?: number; path?: string }>
    rewriteUrl(url: string): string
    revoke(url: string): void
    revokeAll(): void
  }

  dataExchange: {
    request(request: unknown): Promise<unknown>
    provide(exportId: string, handler: (request: unknown) => unknown | Promise<unknown>): Promise<() => void>
  }

  event: {
    publish(request: unknown): Promise<unknown>
    on(topic: string, callback: (event: unknown) => void): () => void
  }

  agent: {
    onInteraction(
      type: string,
      callback: (interaction: {
        type: string
        interactionId: string
        accept(): Promise<unknown>
        submitResult(result: unknown): Promise<unknown>
        reject(reason?: unknown): Promise<unknown>
        requestIntent(request: unknown): Promise<unknown>
        [key: string]: unknown
      }) => void,
    ): () => void
  }

  media: {
    play(): Promise<unknown>
    pause(): Promise<unknown>
    next(): Promise<unknown>
    prev(): Promise<unknown>
    seek(position: unknown): Promise<unknown>
    setVolume(volume: unknown): Promise<unknown>
    setMode(mode: unknown): Promise<unknown>
    mute(): Promise<unknown>
    unmute(): Promise<unknown>
    getStatus(): Promise<unknown>
    getPlaylist(): Promise<unknown>
    getSpectrum(): Promise<unknown>
    getLyrics(options?: unknown): Promise<unknown>
    getBeatGrid(): Promise<unknown>
    playTrack(id: unknown, index?: unknown): Promise<unknown>
    jumpToIndex(index: unknown): Promise<unknown>
    loadNeteasePlaylist(playlistId: unknown): Promise<unknown>
    getSkipVip(): Promise<unknown>
    setSkipVip(value: unknown): Promise<unknown>
    onStateChange(callback: (state: unknown) => void): () => void
    onProgress(callback: (progress: unknown) => void): () => void
  }

  ai: {
    tasks: {
      create(request: unknown): Promise<unknown>
      get(taskId: string): Promise<unknown>
      cancel(taskId: string): Promise<unknown>
      usage(): Promise<unknown>
      subscribe(
        taskId: string,
        callback: (event: { event: unknown; data: unknown }) => void,
      ): Promise<() => void>
    }
  }

  scheduler: {
    register(options: unknown): Promise<unknown>
    unregister(taskId: string): Promise<unknown>
    list(): Promise<unknown>
    get(taskId: string): Promise<unknown>
    enable(taskId: string): Promise<unknown>
    disable(taskId: string): Promise<unknown>
    trigger(taskId: string): Promise<unknown>
    onTask(
      taskId: string,
      callback: (payload: unknown, event: unknown) => unknown | Promise<unknown>,
    ): () => void
  }

  widgets: Record<
    string,
    {
      render(
        container: HTMLElement,
        props: {
          size?: string
          theme?: string
          settings?: Record<string, unknown>
          [key: string]: unknown
        },
      ): void | Promise<void>
    }
  >

  pages: Record<
    string,
    {
      render(container: HTMLElement): void | Promise<void>
    }
  >

  dom: {
    setAttribute(element: Element, name: string, value: string): void
    [key: string]: unknown
  }
`
}

/**
 * @param {{ actions: Record<string, string>, headlessDeniedActions?: string[] }} catalog
 */
export function generateTappSdkDts(catalog) {
  const actions = catalog.actions || {}
  const tree = buildTree(actions)
  const denied = [...(catalog.headlessDeniedActions || [])].sort()
  const curated = new Set([
    'lifecycle',
    'storage',
    'settings',
    'ui',
    'api',
    'assets',
    'dataExchange',
    'event',
    'agent',
    'media',
    'ai',
    'scheduler',
  ])

  const genericLines = []
  for (const name of Object.keys(tree).sort()) {
    if (curated.has(name)) continue
    genericLines.push(...renderNamespace(name, tree[name], 1))
  }

  const deniedComment =
    denied.length > 0
      ? `\n * Headless-denied Bridge actions: ${denied.join(', ')}\n *`
      : ''

  return `/**
 * Myriad Tapp sandbox SDK (window.Tapp).
 * Generated from the runtime permission catalog. Call sites still need
 * matching Manifest permissions and a handler in the current sandbox profile.
 *${deniedComment}
 * Do not edit by hand — run: npm run sync-contract
 */

export interface TappSdk {
${localMembers()}
${genericLines.join('\n')}
}

declare const Tapp: TappSdk

interface Window {
  Tapp: TappSdk
}

export {}
`
}
