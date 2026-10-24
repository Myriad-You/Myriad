/**
 * 工具窗口的强调色跟覆盖层同一个：覆盖层在宿主节点上写了 `--p-accent` / `--p-on-accent`
 * （主站的强调色，按博客页深浅校准过，见 embed/pagePalette.ts），窗口照抄到自己的根上。
 * 不在覆盖层里（直接打开）时不写，用 StudioShell.css 里的后备值。
 */

import { findStudioHost } from './studioIsland'

// 根上的 `--cfg-accent` 也写：ConfigForm.css 在 :root 上由它算出浅底、描边、深色字，
// 只写在 .studio-embed 上的话那几个还是按主站默认色算的。
const PAIRS = [
  ['--p-accent', '--studio-accent'],
  ['--p-accent', '--cfg-accent'],
  ['--p-on-accent', '--studio-on-accent'],
] as const

export function followStudioAccent(win: Window = window): () => void {
  const host = findStudioHost(win)
  if (!host) return () => {}
  const root = win.document.documentElement
  const copy = () => {
    for (const [from, to] of PAIRS) {
      const value = host.style.getPropertyValue(from).trim()
      if (value) root.style.setProperty(to, value)
      else root.style.removeProperty(to)
    }
  }
  copy()
  const observer = new MutationObserver(copy)
  observer.observe(host, { attributes: true, attributeFilter: ['style'] })
  return () => observer.disconnect()
}
