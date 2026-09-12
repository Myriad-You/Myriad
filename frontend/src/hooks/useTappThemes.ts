import type { WidgetGlowMode, WidgetSurface } from './useWidgetTheme'

import { useEffect, useRef, useState } from 'react'
import { GLOW_OPTIONS, SURFACE_OPTIONS } from './useWidgetTheme'

/** 消毒后只保留宿主可安全应用的字段。 */
export interface SafeTappTheme {
  id: string
  name: string
  surface?: WidgetSurface
  glow?: WidgetGlowMode
}

// 白名单从选项配置派生，新增表面/光晕模式时自动跟上。
const VALID_SURFACES = new Set<WidgetSurface>(SURFACE_OPTIONS.map((o) => o.id))
const VALID_GLOWS = new Set<WidgetGlowMode>(GLOW_OPTIONS.map((o) => o.id))

function asSurface(v: unknown): WidgetSurface | undefined {
  return typeof v === 'string' && VALID_SURFACES.has(v as WidgetSurface)
    ? (v as WidgetSurface)
    : undefined
}

function asGlow(v: unknown): WidgetGlowMode | undefined {
  return typeof v === 'string' && VALID_GLOWS.has(v as WidgetGlowMode)
    ? (v as WidgetGlowMode)
    : undefined
}

/** 至少要有 id + name，且 surface/glow 至少命中一个白名单值。 */
function sanitizeTappTheme(raw: unknown): SafeTappTheme | null {
  if (!raw || typeof raw !== 'object') return null
  const comp = raw as { config?: unknown; id?: unknown }
  const config = (comp.config ?? {}) as Record<string, unknown>

  const id = typeof config.id === 'string' ? config.id : undefined
  const name = typeof config.name === 'string' ? config.name : undefined
  if (!id || !name) return null

  const surface = asSurface(config.surface)
  const glow = asGlow(config.glow)

  // 既不带合法 surface 也不带合法 glow 的主题对宿主无意义。
  if (!surface && !glow) return null

  return { id, name, surface, glow }
}

/** enabled 仅在需要时拉取，避免无谓请求。 */
export function useTappThemes(enabled: boolean) {
  const [themes, setThemes] = useState<SafeTappTheme[]>([])
  const fetchedRef = useRef(false)
  const mountedRef = useRef(true)

  useEffect(() => {
    mountedRef.current = true
    return () => {
      mountedRef.current = false
    }
  }, [])

  useEffect(() => {
    if (!enabled || fetchedRef.current) return
    fetchedRef.current = true

    // 动态加载：静态 import 会把 tapp 服务层拖进首屏关键路径。
    import('../tapp/services/TappApiService')
      .then(({ listAllComponentsByType }) => listAllComponentsByType('theme'))
      .then((res) => {
        if (!mountedRef.current) return
        const list = Array.isArray(res?.components) ? res.components : []
        const safe = list
          .map(sanitizeTappTheme)
          .filter((t): t is SafeTappTheme => t !== null)
        setThemes(safe)
      })
      .catch((err) => {
        // 未登录 / 无权限 / 无主题都静默降级为空列表。
        console.debug('[useTappThemes] 加载 Tapp 主题失败:', err)
      })
  }, [enabled])

  return { themes }
}
