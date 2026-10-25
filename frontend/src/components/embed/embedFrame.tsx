import type { ReactNode, RefCallback } from 'react'
import { useCallback, useEffect } from 'react'
import { useAuth } from '../../contexts/AuthContext'
import { DocumentReady } from '../DocumentReady'
import { EMBED_FIT_ATTR, EMBED_READY_ATTR, embedHost } from './embedHost'

/**
 * 外面的 iframe 带着 `data-embed-fit` 时，按挂上这个 ref 的节点的高度调它：博客页不跑脚本，
 * 框的高度只能由里面来定。`enabled` 之前保持页面给的高度，免得加载中先缩再撑开。
 * 节点换了（比如提交后换成另一屏）会跟着换。
 */
export function useFitFrameHeight(enabled: boolean): RefCallback<HTMLElement> {
  return useCallback((node: HTMLElement | null) => {
    const frame = embedHost()?.frame
    if (!enabled || !node || !frame?.hasAttribute(EMBED_FIT_ATTR)) return
    const fit = () => {
      frame.style.height = `${Math.ceil(node.getBoundingClientRect().height)}px`
    }
    fit()
    const observer = new ResizeObserver(fit)
    observer.observe(node)
    return () => observer.disconnect()
  }, [enabled])
}

/**
 * 和要显示的内容放在同一个 Suspense 里：内容提交、画出第一帧之后才告诉外面的框可以显示了。
 * 不在框里时什么也不做。
 */
export function EmbedFrameReady() {
  useEffect(() => {
    embedHost()?.frame?.setAttribute(EMBED_READY_ATTR, '')
  }, [])
  return null
}

/** 框里的元素离博客页窗口上下边至少留这么多，才算看得清，不用滚。 */
const REVEAL_MARGIN = 96

/**
 * 程序把焦点挪到框里某处（第一处填错的栏、提交成功的标题）时用：浏览器自己的聚焦滚动是
 * 一下跳过去，这里先不让它滚，元素不在博客页窗口里再平滑滚过去，停在窗口上三分之一。
 * 偏好减少动态时直接到位。不在框里就照常聚焦。
 */
export function focusInFrame(element: HTMLElement | null | undefined): void {
  if (!element) return
  const host = embedHost()
  const frame = host?.frame
  if (!host || !frame) {
    element.focus()
    return
  }
  element.focus({ preventScroll: true })
  const offset = frame.getBoundingClientRect().top
  const rect = element.getBoundingClientRect()
  const view = host.win.innerHeight
  if (offset + rect.top >= REVEAL_MARGIN && offset + rect.bottom <= view - REVEAL_MARGIN) return
  const quiet = host.win.matchMedia('(prefers-reduced-motion: reduce)').matches
  host.win.scrollBy({ top: offset + rect.top - view / 3, behavior: quiet ? 'auto' : 'smooth' })
}

/** 嵌入页里不跳登录页或首页：不是站长就什么都不显示。 */
export function EmbedAdminOnly({ children }: { children: ReactNode }) {
  const { isAdmin, hasChecked } = useAuth()
  if (!hasChecked) return null
  if (!isAdmin) return <DocumentReady>{null}</DocumentReady>
  return children
}
