/**
 * Tapp Widgets Hook
 * 管理 Tapp 注册的小组件并提供给 WidgetGrid 使用
 */

import type {
  WidgetComponentProps,
  WidgetSize,
  WidgetType,
} from '../components/widgetGridTypes'
import type { RegisteredWidget } from '../tapp/types'
import {
  createElement,
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useRef,
  useState,
} from 'react'
import {
  TAPP_WIDGET_SKELETON,
  WidgetSkeleton,
} from '../components/widgets/shared/WidgetSkeleton'
import { currentCopy } from '../i18n/localeCopy'
import { userFacingError } from '../utils/userFacingError'

// TappWidget 与其背后的整个 tapp runtime / 沙箱体系（生产 ~300KB）按需加载：
// 布局中没有 Tapp 小组件时，Home 首屏不需要执行这部分代码。
// 渲染点的 Suspense 由 WidgetGrid 提供；此处再包一层带主题色的通用骨架。
const TappWidgetComponent = lazy(() =>
  import('../components/widgets/TappWidget').then((m) => ({
    default: m.TappWidgetComponent,
  })),
)

/** Default loading surface for every third-party Tapp widget (chunk + runtime). */
function TappDefaultSkeleton({ accent }: { accent?: string }) {
  return createElement(
    'div',
    { className: 'h-full w-full overflow-hidden rounded-xl' },
    createElement(WidgetSkeleton, {
      preset: TAPP_WIDGET_SKELETON.preset,
      deferMs: TAPP_WIDGET_SKELETON.deferMs,
      accent: accent || 'var(--color-primary, #6366f1)',
    }),
  )
}

type TappRuntimeModule = typeof import('../tapp/runtime')

let runtimeModulePromise: Promise<TappRuntimeModule> | null = null

/** 共享的 runtime 模块动态加载（模块级缓存，多个调用方只加载一次） */
function loadTappRuntimeModule(): Promise<TappRuntimeModule> {
  runtimeModulePromise ||= import('../tapp/runtime')
  return runtimeModulePromise
}

// Tapp WidgetSize 到 WidgetGrid WidgetSize 的映射
const TAPP_SIZE_MAP: Record<string, WidgetSize> = {
  '1x1': '1x1',
  '2x1': '2x1',
  '1x2': '1x2',
  '2x2': '2x2',
  '2x3': '2x3',
  '3x2': '3x2',
  '3x3': '3x3',
  '2x4': '2x4',
  '4x1': '4x1',
  '4x2': '4x2',
  '4x4': '4x4',
}

// 将 Tapp WidgetSize 转换为 WidgetGrid 兼容的尺寸
function mapTappSize(size: string | undefined): WidgetSize {
  if (!size) return '2x2'
  return TAPP_SIZE_MAP[size] || '2x2'
}

// 将 Tapp WidgetSize 数组转换为 WidgetGrid 兼容的尺寸数组
function mapTappSizes(sizes: string[] | undefined): WidgetSize[] {
  if (!sizes || !Array.isArray(sizes)) {
    return ['2x2'] // 默认尺寸
  }
  const mapped = sizes
    .map((s) => TAPP_SIZE_MAP[s])
    .filter((s): s is WidgetSize => s !== undefined)
  return mapped.length > 0 ? mapped : ['2x2']
}

/**
 * 扩展 WidgetType 以支持 Tapp 元数据
 */
export interface TappWidgetType extends WidgetType {
  isTappWidget: boolean
  tappId: string
  category?: string
}

/**
 * Resolve brand accent for skeleton tint (manifest themeColor when known).
 */
function resolveTappAccent(
  widget: RegisteredWidget,
  runtime?: { getTapp?: (id: string) => { manifest?: { themeColor?: string } } | undefined },
): string | undefined {
  try {
    const fromManifest = runtime
      ?.getTapp?.(widget.tappId)
      ?.manifest?.themeColor?.trim()
    if (fromManifest) return fromManifest
  } catch {
    // runtime may not be ready during early map
  }
  return undefined
}

/**
 * Tapp Widget 到 WidgetType 的适配器
 * 默认 Suspense 兜底 = 通用 WidgetSkeleton（可按 themeColor 染色）
 */
function createTappWidgetType(
  widget: RegisteredWidget,
  runtime?: {
    getTapp?: (id: string) => { manifest?: { themeColor?: string } } | undefined
  },
): TappWidgetType {
  const config = widget.config || {}
  const accent = resolveTappAccent(widget, runtime)

  // 包装：懒加载 chunk + 统一骨架（所有第三方 Tapp 默认接上）
  const WrappedComponent = (props: WidgetComponentProps) =>
    createElement(
      Suspense,
      { fallback: createElement(TappDefaultSkeleton, { accent }) },
      createElement(TappWidgetComponent, {
        ...props,
        tappWidgetId: widget.id,
      }),
    )
  WrappedComponent.displayName = `TappWidget_${widget.id}`

  return {
    id: widget.id,
    name: config.name || 'Unknown Widget',
    defaultSize: mapTappSize(config.defaultSize),
    component: WrappedComponent,
    supportedSizes: mapTappSizes(config.sizes),
    settings: config.settings,
    // Tapp 特定字段
    isTappWidget: true,
    tappId: widget.tappId,
    category: config.category,
  }
}

/**
 * useTappWidgets Hook
 * 监听 Tapp Runtime 的 Widget 注册事件并返回可用的 Widget 类型
 *
 * 重要：会等待 TappRuntime 同步完成后再加载小组件。
 * runtime 模块本身为动态加载，不进入 Home 首屏关键路径。
 */
export function useTappWidgets(): {
  tappWidgets: TappWidgetType[]
  isLoading: boolean
  error: string | null
  refreshWidgets: () => void
} {
  const [tappWidgets, setTappWidgets] = useState<TappWidgetType[]>([])
  const [isLoading, setIsLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  // 挂载跟踪：卸载后停止后台重试，避免对已卸载组件 setState
  // 用函数读取 current：TS 控制流窄化会把字面量比较后的属性类型收窄
  // 成 true/false，导致后续比较报 TS2367（函数调用不会窄化）。
  const mountedRef = useRef<boolean>(true)
  const isMounted = (): boolean => mountedRef.current
  useEffect(() => {
    mountedRef.current = true
    return () => {
      mountedRef.current = false
    }
  }, [])

  // 异步加载：等待 runtime 模块加载 + 同步完成后再读取
  // 失败按指数退避重试（2s/5s/10s，共 4 次）：首次同步可能因后端 API
  // 瞬时不可用/超时而失败；若失败后不重试，Tapp widget 类型会永久缺失，
  // 已添加的小组件被 WidgetGrid 静默跳过（未知类型 return null），页面
  // 表现为 widget "消失"，只能靠切 Tab 重挂载恢复（issue #72）。
  // 注意：首次失败后 runtime 会把 synced=true 并缓存 syncError，
  // waitForSync() 只会重抛缓存错误而不重新请求后端，因此重试必须
  // 调用 syncFromBackend(true) 强制重新同步（去重器失败后已清空，
  // 能真正发出新请求；成功后 runtime 内部会清空 syncError）。
  const loadWidgetsAsync = useCallback(async () => {
    const MAX_ATTEMPTS = 4
    const RETRY_DELAYS = [2000, 5000, 10000]

    for (let attempt = 0; attempt < MAX_ATTEMPTS; attempt++) {
      if (!isMounted()) return
      try {
        setIsLoading(true)
        const { getTappRuntime } = await loadTappRuntimeModule()
        const runtime = getTappRuntime()

        if (attempt === 0) {
          // 首次：等待构造时启动的初始同步（含 10s 超时保护）
          await runtime.waitForSync()
        } else {
          // 重试：强制重新从后端拉取（force 绕过 30s 缓存检查）
          await runtime.syncFromBackend(true)
        }

        const registeredWidgets = runtime.getRegisteredWidgets()
        const widgetTypes = registeredWidgets.map((w) =>
          createTappWidgetType(w, runtime),
        )
        // 有注册的 Tapp 小组件时提前预热组件模块，避免渲染时才拉 chunk
        if (widgetTypes.length > 0) {
          void import('../components/widgets/TappWidget').catch(() => {})
        }
        if (!isMounted()) return
        setTappWidgets(widgetTypes)
        setError(null)
        return
      } catch (err) {
        console.error(
          `[useTappWidgets] Failed to load widgets (attempt ${attempt + 1}/${MAX_ATTEMPTS}):`,
          err,
        )
        if (attempt === MAX_ATTEMPTS - 1) {
          if (isMounted()) {
            setError(
              userFacingError(err, currentCopy().errors.widgetsLoadFailed),
            )
          }
          return
        }
        // 指数退避后重试；卸载后由循环开头检查终止
        await new Promise((resolve) =>
          setTimeout(resolve, RETRY_DELAYS[attempt]),
        )
      } finally {
        if (isMounted()) setIsLoading(false)
      }
    }
  }, [])

  // 初始加载 - 等待同步完成
  useEffect(() => {
    loadWidgetsAsync()
  }, [loadWidgetsAsync])

  // 监听 Widget 注册/注销事件 和 同步完成事件
  useEffect(() => {
    let disposed = false
    const unsubs: Array<() => void> = []

    loadTappRuntimeModule()
      .then(({ getTappRuntime }) => {
        if (disposed) return
        const runtime = getTappRuntime()

        // 同步方法：runtime 已就绪时直接读取注册表
        const reloadSync = () => {
          try {
            const registeredWidgets = runtime.getRegisteredWidgets()
            setTappWidgets(
              registeredWidgets.map((w) => createTappWidgetType(w, runtime)),
            )
            setError(null)
          } catch (err) {
            console.error('[useTappWidgets] Failed to load widgets:', err)
            setError(
              userFacingError(err, currentCopy().errors.widgetsLoadFailed),
            )
          } finally {
            setIsLoading(false)
          }
        }

        // 当有新的 widget 注册/注销、后端同步完成时更新
        unsubs.push(runtime.on('widget:registered', reloadSync))
        unsubs.push(runtime.on('widget:unregistered', reloadSync))
        unsubs.push(runtime.on('sync:complete', reloadSync))
      })
      .catch((err) => {
        console.error('[useTappWidgets] Failed to load tapp runtime:', err)
      })

    return () => {
      disposed = true
      unsubs.forEach((unsub) => unsub())
    }
  }, [])

  return {
    tappWidgets,
    isLoading,
    error,
    refreshWidgets: loadWidgetsAsync,
  }
}

export default useTappWidgets
