import type { ReactNode } from 'react'
import { useLayoutEffect } from 'react'
import { useAuth } from '../../contexts/AuthContext'
import { DocumentReady } from '../DocumentReady'

/** 嵌入页跟着系统明暗走，并给根节点打上 `embed-frame`。 */
export function useEmbedFrame() {
  useLayoutEffect(() => {
    const root = document.documentElement
    const previous = root.className
    const media = window.matchMedia('(prefers-color-scheme: dark)')
    const apply = () => {
      root.classList.add('embed-frame')
      root.classList.toggle('dark', media.matches)
      root.classList.toggle('light', !media.matches)
    }
    apply()
    media.addEventListener('change', apply)
    return () => {
      media.removeEventListener('change', apply)
      root.className = previous
    }
  }, [])
}

/** 嵌入页里不跳登录页或首页：不是站长就什么都不显示。 */
export function EmbedAdminOnly({ children }: { children: ReactNode }) {
  const { isAdmin, hasChecked } = useAuth()
  if (!hasChecked) return null
  if (!isAdmin) return <DocumentReady>{null}</DocumentReady>
  return children
}
