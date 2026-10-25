import { useEffect } from 'react'
import { embedHost } from './embedHost'
import { watchPalette } from './pagePaletteDom'

/**
 * 颜色从框背后那块博客页上取（和 AI 伴读同一套，pagePalette.ts）：底色、字色、线条、
 * 主站的强调色以 `--p-*` 写到自己的根上，评论区、友链申请看起来是那一页的一部分，
 * 而不是嵌进去的另一张页。
 */
export function useHostPalette(): void {
  useEffect(() => {
    const host = embedHost()
    const frame = host?.frame
    if (!host || !frame) return
    return watchPalette(host.doc, [{
      element: document.documentElement,
      prefix: 'p-',
      // 框不在窗口里时取不到它背后那块（会取到页头之类），滚进来时取一次；
      // 背后是正文的底色，不随滚动变，不必每帧都去博客页上取样。
      visibleWith: () => frame,
      textFrom: () => frame.parentElement,
      at: () => {
        const rect = frame.getBoundingClientRect()
        const y = Math.max(rect.top + 12, 0)
        if (rect.bottom <= 0 || y >= host.win.innerHeight) return null
        return { x: rect.left + rect.width / 2, y }
      },
    }])
  }, [])
}
