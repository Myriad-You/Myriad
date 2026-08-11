/**
 * Tapp 注册主题的消费端 Hook
 *
 * 背景：Tapp 可通过 SDK 的 component.registerTheme() 注册主题组件，后端按
 * 用户存储为 `_component:theme:{id}`。此前注册链路（SDK → bridge → 后端）已通，
 * 但宿主端无人消费。本 Hook 将其接通为小组件主题系统的「第三方预设源」。
 *
 * 安全边界（务必保持）：
 * - **只消费白名单枚举**：surface ∈ {glass,solid,flat,outline}，
 *   glow ∈ {identity,primary,none}。任何其它值一律丢弃。
 * - **绝不消费 `styles`**（自由 CSS 字符串）——注入风险。
 * - **暂不消费 `colors`**——会与壁纸取色系统争夺 `--color-*`，留作未来扩展。
 * 因此一个 Tapp 主题在宿主侧最终只等价于「surface + glow 的一个具名组合」，
 * 应用时走 useWidgetTheme 已有的、同样白名单校验的 setter。
 *
 * 可见性：列表接口需登录态且按 user_id 过滤，故仅管理员在编辑模式可见/可选，
 * 访客看到的是已固化进 widget_theme 的最终 surface/glow，无需再解析 Tapp。
 */

import type { WidgetGlowMode, WidgetSurface } from './useWidgetTheme'

import { useEffect, useRef, useState } from 'react'
// TappApiService 动态加载：该 hook 被 TitleFontSelector（Home 编辑模式）引用，
// 静态 import 会把整个 tapp 服务层拖进首屏关键路径
import {
  listSessionThemes,
  subscribeSessionThemes,
} from '../tapp/runtime/sessionThemeRegistry'
import { GLOW_OPTIONS, SURFACE_OPTIONS } from './useWidgetTheme'

// 类型

/** 消毒后的 Tapp 主题预设：只保留宿主可安全应用的字段 */
export interface SafeTappTheme {
  id: string
  name: string
  surface?: WidgetSurface
  glow?: WidgetGlowMode
}

// 白名单从选项配置派生，新增表面/光晕模式时自动跟上
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

/**
 * 把一个后端返回的原始 theme 组件消毒成 SafeTappTheme。
 * 至少要有 id + name，且 surface/glow 至少命中一个白名单值，否则视为无效丢弃。
 */
function sanitizeTappTheme(raw: unknown): SafeTappTheme | null {
  if (!raw || typeof raw !== 'object') return null
  const comp = raw as { config?: unknown; id?: unknown }
  const config = (comp.config ?? {}) as Record<string, unknown>

  const id = typeof config.id === 'string' ? config.id : undefined
  const name = typeof config.name === 'string' ? config.name : undefined
  if (!id || !name) return null

  const surface = asSurface(config.surface)
  const glow = asGlow(config.glow)
  // 既不带合法 surface 也不带合法 glow 的主题对宿主无意义
  if (!surface && !glow) return null

  return { id, name, surface, glow }
}

// Hook

/**
 * 拉取并消毒当前用户已注册的 Tapp 主题预设。
 * @param enabled 仅在需要时（如编辑模式打开面板）拉取，避免无谓请求
 */
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
    if (!enabled) return
    const updateSessionThemes = () => {
      setThemes((persisted) => {
        const session = listSessionThemes()
          .map(sanitizeTappTheme)
          .filter((theme): theme is SafeTappTheme => theme !== null)
        const sessionIds = new Set(session.map((theme) => theme.id))
        return [
          ...persisted.filter((theme) => !sessionIds.has(theme.id)),
          ...session,
        ]
      })
    }
    updateSessionThemes()
    return subscribeSessionThemes(updateSessionThemes)
  }, [enabled])

  useEffect(() => {
    if (!enabled || fetchedRef.current) return
    fetchedRef.current = true

    import('../tapp/services/TappApiService')
      .then(({ listAllComponentsByType }) => listAllComponentsByType('theme'))
      .then((res) => {
        if (!mountedRef.current) return
        const list = Array.isArray(res?.components) ? res.components : []
        const safe = list
          .map(sanitizeTappTheme)
          .filter((t): t is SafeTappTheme => t !== null)
        const session = listSessionThemes()
          .map(sanitizeTappTheme)
          .filter((theme): theme is SafeTappTheme => theme !== null)
        const sessionIds = new Set(session.map((theme) => theme.id))
        setThemes([
          ...safe.filter((theme) => !sessionIds.has(theme.id)),
          ...session,
        ])
      })
      .catch((err) => {
        // 未登录 / 无权限 / 无主题都走这里，静默降级为空列表
        console.debug('[useTappThemes] 加载 Tapp 主题失败:', err)
      })
  }, [enabled])

  return { themes }
}
