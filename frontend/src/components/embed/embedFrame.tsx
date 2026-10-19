import type { ReactNode, RefObject } from 'react'
import { useLayoutEffect } from 'react'
import { useAuth } from '../../contexts/AuthContext'
import { DocumentReady } from '../DocumentReady'

/**
 * 嵌在博客页里时明暗跟着外面那页：底色够暗就是深色，读不到（跨源、透明底）再看它声明的
 * color-scheme，都没有才跟系统。和 utils/themeBootScript.ts 第一帧之前的判断是同一套。
 */
export function embedScheme(): 'dark' | 'light' | null {
  try {
    if (window.parent === window) return null
    const doc = window.parent.document
    const view = doc.defaultView
    if (!view) return null
    for (const node of [doc.body, doc.documentElement]) {
      if (!node) continue
      const match = /rgba?\(([\d.]+)[,\s]+([\d.]+)[,\s]+([\d.]+)(?:[,\s/]+([\d.]+))?/.exec(
        view.getComputedStyle(node).backgroundColor,
      )
      if (!match || match[4] === '0') continue
      const luma = 0.2126 * Number(match[1]) + 0.7152 * Number(match[2]) + 0.0722 * Number(match[3])
      return luma < 128 ? 'dark' : 'light'
    }
    const declared = view.getComputedStyle(doc.documentElement).colorScheme
    return declared === 'dark' || declared === 'light' ? declared : null
  } catch {
    return null
  }
}

/** 给根节点打上 `embed-frame`，明暗按 `embedScheme`。 */
export function useEmbedFrame() {
  useLayoutEffect(() => {
    const root = document.documentElement
    const previous = root.className
    const media = window.matchMedia('(prefers-color-scheme: dark)')
    const apply = () => {
      const dark = (embedScheme() ?? (media.matches ? 'dark' : 'light')) === 'dark'
      root.classList.add('embed-frame')
      root.classList.toggle('dark', dark)
      root.classList.toggle('light', !dark)
    }
    apply()
    media.addEventListener('change', apply)
    return () => {
      media.removeEventListener('change', apply)
      root.className = previous
    }
  }, [])
}

/**
 * 博客页不跑脚本，没法按内容调 iframe 的高度；嵌入页和它同源，自己来调。
 * 只调带 `frameClass` 的那种 iframe，`enabled` 之前保持页面给的高度，免得加载中先缩再撑开。
 */
export function useFitFrameHeight(ref: RefObject<HTMLElement | null>, frameClass: string, enabled: boolean) {
  useLayoutEffect(() => {
    const node = ref.current
    let frame: Element | null = null
    try {
      frame = window.frameElement
    } catch {
      return
    }
    // frameElement 属于外面那页，不能拿这一页的 HTMLElement 做 instanceof。
    if (!enabled || !node || !frame?.classList.contains(frameClass)) return
    const host = frame as HTMLElement
    const fit = () => {
      host.style.height = `${Math.ceil(node.getBoundingClientRect().height)}px`
    }
    fit()
    const observer = new ResizeObserver(fit)
    observer.observe(node)
    return () => observer.disconnect()
  }, [enabled, frameClass, ref])
}

/** 嵌入页里不跳登录页或首页：不是站长就什么都不显示。 */
export function EmbedAdminOnly({ children }: { children: ReactNode }) {
  const { isAdmin, hasChecked } = useAuth()
  if (!hasChecked) return null
  if (!isAdmin) return <DocumentReady>{null}</DocumentReady>
  return children
}
