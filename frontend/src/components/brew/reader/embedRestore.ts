/** innerHTML 更新前摘出已加载节点，避免闪烁和重复 fetch。 */

export interface SavedIframe {
  key: string
  element: HTMLElement
}

export function saveEmbedElements(container: HTMLElement): SavedIframe[] {
  const saved: SavedIframe[] = []

  container
    .querySelectorAll('.brew-bilibili-embed[data-video-id]')
    .forEach((el) => {
      const videoId = el.getAttribute('data-video-id')
      if (videoId && el.querySelector('iframe')) {
        saved.push({ key: `bilibili:${videoId}`, element: el as HTMLElement })
      }
    })

  container.querySelectorAll('.rss-content-iframe-wrapper').forEach((el) => {
    const iframe = el.querySelector('iframe')
    if (iframe) {
      const src = iframe.getAttribute('src') || iframe.src || ''
      if (src) {
        saved.push({ key: `rss:${src}`, element: el as HTMLElement })
      }
    }
  })

  container
    .querySelectorAll('iframe[data-iframe-wrapped="true"]')
    .forEach((iframe) => {
      if (
        iframe.closest('.brew-bilibili-embed') ||
        iframe.closest('.rss-content-iframe-wrapper')
      ) {
        return
      }
      const wrapper = iframe.parentElement
      if (wrapper) {
        const src =
          iframe.getAttribute('src') || (iframe as HTMLIFrameElement).src || ''
        if (src) {
          saved.push({ key: `wrapped:${src}`, element: wrapper })
        }
      }
    })

  // overlay 变化时保留 data-loaded，避免重新 fetch。
  container
    .querySelectorAll(
      '.brew-netease-music[data-loaded="true"], .brew-netease-music[data-loaded="loading"]',
    )
    .forEach((el) => {
      const songId = el.getAttribute('data-song-id')
      if (songId) {
        saved.push({ key: `netease:${songId}`, element: el as HTMLElement })
      }
    })
  container
    .querySelectorAll(
      '.brew-steam-game[data-loaded="true"], .brew-steam-game[data-loaded="loading"]',
    )
    .forEach((el) => {
      const appId = el.getAttribute('data-app-id')
      if (appId) {
        saved.push({ key: `steam:${appId}`, element: el as HTMLElement })
      }
    })
  container
    .querySelectorAll(
      '.brew-github-repo[data-loaded="true"], .brew-github-repo[data-loaded="loading"]',
    )
    .forEach((el) => {
      const repo = el.getAttribute('data-repo')
      if (repo) {
        saved.push({ key: `github:${repo}`, element: el as HTMLElement })
      }
    })

  // 摘出后再赋 innerHTML，避免销毁已加载节点。
  saved.forEach((s) => s.element.remove())

  return saved
}

export function restoreEmbedElements(
  container: HTMLElement,
  saved: SavedIframe[],
): void {
  if (saved.length === 0) return
  const savedMap = new Map(saved.map((s) => [s.key, s.element]))
  const restored = new Set<string>()

  const restoreBySelector = (
    selector: string,
    keyFn: (el: Element) => string | null,
  ) => {
    container.querySelectorAll(selector).forEach((newEl) => {
      const key = keyFn(newEl)
      if (key && !restored.has(key)) {
        const savedEl = savedMap.get(key)
        if (savedEl && newEl.parentNode) {
          newEl.parentNode.replaceChild(savedEl, newEl)
          restored.add(key)
        }
      }
    })
  }

  restoreBySelector('.brew-bilibili-embed[data-video-id]', (el) =>
    el.getAttribute('data-video-id')
      ? `bilibili:${el.getAttribute('data-video-id')}`
      : null,
  )

  restoreBySelector('.rss-content-iframe-wrapper', (el) => {
    const src = el.querySelector('iframe')?.getAttribute('src') || ''
    return src ? `rss:${src}` : null
  })

  restoreBySelector('.brew-netease-music[data-song-id]', (el) =>
    el.getAttribute('data-song-id')
      ? `netease:${el.getAttribute('data-song-id')}`
      : null,
  )
  restoreBySelector('.brew-steam-game[data-app-id]', (el) =>
    el.getAttribute('data-app-id')
      ? `steam:${el.getAttribute('data-app-id')}`
      : null,
  )
  restoreBySelector('.brew-github-repo[data-repo]', (el) =>
    el.getAttribute('data-repo')
      ? `github:${el.getAttribute('data-repo')}`
      : null,
  )

  container.querySelectorAll('iframe').forEach((newIframe) => {
    if (
      newIframe.closest('.brew-bilibili-embed') ||
      newIframe.closest('.rss-content-iframe-wrapper')
    ) {
      return
    }
    const src = newIframe.getAttribute('src') || newIframe.src || ''
    const key = `wrapped:${src}`
    if (src && !restored.has(key)) {
      const savedEl = savedMap.get(key)
      if (savedEl && newIframe.parentNode) {
        newIframe.parentNode.replaceChild(savedEl, newIframe)
        restored.add(key)
      }
    }
  })
}
