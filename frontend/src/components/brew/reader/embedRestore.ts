/**
 * iframe / 嵌入卡片保存与恢复
 * innerHTML 更新前摘出已加载节点，更新后按 key 换回，避免闪烁和重复 fetch
 */

export interface SavedIframe {
  key: string
  element: HTMLElement
}

export function saveEmbedElements(container: HTMLElement): SavedIframe[] {
  const saved: SavedIframe[] = []

  // 保存 bilibili 嵌入（通过 data-video-id 匹配）
  container
    .querySelectorAll('.brew-bilibili-embed[data-video-id]')
    .forEach((el) => {
      const videoId = el.getAttribute('data-video-id')
      if (videoId && el.querySelector('iframe')) {
        saved.push({ key: `bilibili:${videoId}`, element: el as HTMLElement })
      }
    })

  // 保存 RSS 内容中的 iframe 包装器（通过 iframe src 匹配）
  container.querySelectorAll('.rss-content-iframe-wrapper').forEach((el) => {
    const iframe = el.querySelector('iframe')
    if (iframe) {
      const src = iframe.getAttribute('src') || iframe.src || ''
      if (src) {
        saved.push({ key: `rss:${src}`, element: el as HTMLElement })
      }
    }
  })

  // 保存后处理阶段包装的 iframe（通过 iframe src 匹配）
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

  // 保存已加载数据的嵌入卡片（网易云音乐、Steam、GitHub）
  // 避免 overlay 变化时丢失 data-loaded 状态导致重新 fetch
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

  // 从 DOM 摘出保存的元素（防止 innerHTML 赋值时销毁它们）
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

  // 通用恢复：按 selector + key 生成器匹配
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

  // 恢复 bilibili 嵌入
  restoreBySelector('.brew-bilibili-embed[data-video-id]', (el) =>
    el.getAttribute('data-video-id')
      ? `bilibili:${el.getAttribute('data-video-id')}`
      : null,
  )

  // 恢复 RSS iframe 包装器
  restoreBySelector('.rss-content-iframe-wrapper', (el) => {
    const src = el.querySelector('iframe')?.getAttribute('src') || ''
    return src ? `rss:${src}` : null
  })

  // 恢复已加载的嵌入卡片（避免重新 fetch API 数据）
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

  // 恢复后处理包装的 iframe
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
