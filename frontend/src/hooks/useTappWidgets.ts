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
import { formatUserFacingError } from '../utils/formatUserFacingError'

// Tapp runtime/沙箱按需加载：布局没有 Tapp 小组件时不进 Home 首屏。
// lazy() 吃 default 导出（withI18nNamespace）。具名 TappWidgetComponent 没有语言包。
const TappWidget = lazy(() => import('../components/widgets/TappWidget'))

/** 第三方 Tapp 的默认加载表面（chunk + runtime）。 */
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

// runtime 模块级缓存，多个调用方只加载一次。
let runtimeModulePromise: Promise<TappRuntimeModule> | null = null

function loadTappRuntimeModule(): Promise<TappRuntimeModule> {
  runtimeModulePromise ||= import('../tapp/runtime')
  return runtimeModulePromise
}

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

function mapTappSize(size: string | undefined): WidgetSize {
  if (!size) return '2x2'
  return TAPP_SIZE_MAP[size] || '2x2'
}

function mapTappSizes(sizes: string[] | undefined): WidgetSize[] {
  if (!sizes || !Array.isArray(sizes)) {
    return ['2x2']
  }
  const mapped = sizes
    .map((s) => TAPP_SIZE_MAP[s])
    .filter((s): s is WidgetSize => s !== undefined)
  return mapped.length > 0 ? mapped : ['2x2']
}

export interface TappWidgetType extends WidgetType {
  isTappWidget: boolean
  tappId: string
  category?: string
}

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

  }
  return undefined
}

function createTappWidgetType(
  widget: RegisteredWidget,
  runtime?: {
    getTapp?: (id: string) => { manifest?: { themeColor?: string } } | undefined
  },
  previousComponent?: WidgetType['component'],
): TappWidgetType {
  const config = widget.config || {}
  const WrappedComponent = (props: WidgetComponentProps) =>
    createElement(
      Suspense,
      { fallback: createElement(TappDefaultSkeleton, { accent: resolveTappAccent(widget, runtime) }) },
      createElement(TappWidget, {
        ...props,
        tappWidgetId: widget.id,
      }),
    )
  WrappedComponent.displayName = `TappWidget_${widget.id}`

  return {
    id: widget.id,
    name: config.name || 'Unknown Widget',
    defaultSize: mapTappSize(config.defaultSize),
    component: previousComponent || WrappedComponent,
    supportedSizes: mapTappSizes(config.sizes),
    settings: config.settings,

    isTappWidget: true,
    tappId: widget.tappId,
    category: config.category,
  }
}

/** 等 TappRuntime 同步完成后再读；runtime 模块动态加载，不进 Home 首屏。 */
export function useTappWidgets(enabled = true): {
  tappWidgets: TappWidgetType[]
  isLoading: boolean
  error: string | null
  refreshWidgets: () => void
} {
  const componentsRef = useRef(new Map<string, WidgetType['component']>())
  const mapWidgets = useCallback((widgets: RegisteredWidget[], runtime: Parameters<typeof createTappWidgetType>[1]) => {
    const types = widgets.map(widget => createTappWidgetType(widget, runtime, componentsRef.current.get(widget.id)))
    componentsRef.current = new Map(types.map(type => [type.id, type.component]))
    return types
  }, [])
  const [tappWidgets, setTappWidgets] = useState<TappWidgetType[]>([])
  const [isLoading, setIsLoading] = useState(enabled)
  const [error, setError] = useState<string | null>(null)

  // 用函数读 mounted：TS 会把字面量比较收窄成 true/false，后续比较报 TS2367。
  const mountedRef = useRef<boolean>(true)
  const isMounted = (): boolean => mountedRef.current
  useEffect(() => {
    mountedRef.current = true
    return () => {
      mountedRef.current = false
    }
  }, [])

  // 失败指数退避重试；waitForSync 会重抛缓存错误，重试必须 syncFromBackend(true)。
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
          // 首次等构造时的初始同步（含 10s 超时）。
          await runtime.waitForSync()
        } else {
          // force 绕过 30s 缓存检查。
          await runtime.syncFromBackend(true)
        }

        const registeredWidgets = runtime.getRegisteredWidgets()
        const widgetTypes = mapWidgets(registeredWidgets, runtime)

        if (widgetTypes.length > 0) {
          // 有注册小组件时预热 chunk，避免渲染时才拉。
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
              await formatUserFacingError(
                err,
                currentCopy().errors.widgetsLoadFailed,
              ),
            )
          }
          return
        }

        await new Promise((resolve) =>
          setTimeout(resolve, RETRY_DELAYS[attempt]),
        )
      } finally {
        if (isMounted()) setIsLoading(false)
      }
    }
  }, [mapWidgets])

  useEffect(() => {
    if (!enabled) {
      setIsLoading(false)
      return
    }
    loadWidgetsAsync()
  }, [enabled, loadWidgetsAsync])

  useEffect(() => {
    if (!enabled) return
    let disposed = false
    const unsubs: Array<() => void> = []

    loadTappRuntimeModule()
      .then(({ getTappRuntime }) => {
        if (disposed) return
        const runtime = getTappRuntime()

        const reloadSync = () => {
          try {
            const registeredWidgets = runtime.getRegisteredWidgets()
            setTappWidgets(
              mapWidgets(registeredWidgets, runtime),
            )
            setError(null)
          } catch (err) {
            console.error('[useTappWidgets] Failed to load widgets:', err)
            void formatUserFacingError(
              err,
              currentCopy().errors.widgetsLoadFailed,
            ).then(setError)
          } finally {
            setIsLoading(false)
          }
        }

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
  }, [enabled, mapWidgets])

  return {
    tappWidgets,
    isLoading,
    error,
    refreshWidgets: loadWidgetsAsync,
  }
}

export default useTappWidgets
