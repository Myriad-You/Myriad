import type { ReactNode, RefCallback } from 'react'
import { useCallback } from 'react'
import { useAuth } from '../../contexts/AuthContext'
import { DocumentReady } from '../DocumentReady'
import { EMBED_FIT_ATTR, embedHost } from './embedHost'

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

/** 嵌入页里不跳登录页或首页：不是站长就什么都不显示。 */
export function EmbedAdminOnly({ children }: { children: ReactNode }) {
  const { isAdmin, hasChecked } = useAuth()
  if (!hasChecked) return null
  if (!isAdmin) return <DocumentReady>{null}</DocumentReady>
  return children
}
