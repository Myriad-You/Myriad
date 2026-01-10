/**
 * Tapp Widgets Hook
 * 管理 Tapp 注册的小组件并提供给 WidgetGrid 使用
 */

import type { WidgetComponentProps, WidgetSize, WidgetType } from '../components/WidgetGrid'
import type { RegisteredWidget } from '../tapp/types'
import { createElement, useCallback, useEffect, useMemo, useState } from 'react'
import { TappWidgetComponent } from '../components/widgets/TappWidget'
import { getTappRuntime } from '../tapp/runtime'

// Tapp WidgetSize 到 WidgetGrid WidgetSize 的映射
const TAPP_SIZE_MAP: Record<string, WidgetSize> = {
  '1x1': '1x1',
  '2x1': '2x1',
  '1x2': '1x2',
  '2x2': '2x2',
  '2x4': '2x4',
  '4x1': '4x1',
  '4x2': '4x2',
  '4x4': '4x4',
}

// 将 Tapp WidgetSize 转换为 WidgetGrid 兼容的尺寸
function mapTappSize(size: string | undefined): WidgetSize {
  if (!size)
    return '2x2'
  return TAPP_SIZE_MAP[size] || '2x2'
}

// 将 Tapp WidgetSize 数组转换为 WidgetGrid 兼容的尺寸数组
function mapTappSizes(sizes: string[] | undefined): WidgetSize[] {
  if (!sizes || !Array.isArray(sizes)) {
    return ['2x2'] // 默认尺寸
  }
  const mapped = sizes
    .map(s => TAPP_SIZE_MAP[s])
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
 * Tapp Widget 到 WidgetType 的适配器
 */
function createTappWidgetType(widget: RegisteredWidget): TappWidgetType {
  const config = widget.config || {}

  // 创建一个包装组件 - 使用 createElement 而不是 JSX
  const WrappedComponent = (props: WidgetComponentProps) =>
    createElement(TappWidgetComponent, {
      ...props,
      tappWidgetId: widget.id,
    })
  WrappedComponent.displayName = `TappWidget_${widget.id}`

  return {
    id: widget.id,
    name: config.name || 'Unknown Widget',
    defaultSize: mapTappSize(config.defaultSize),
    component: WrappedComponent,
    supportedSizes: mapTappSizes(config.sizes),
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
 * 重要：会等待 TappRuntime 同步完成后再加载小组件
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

  // 加载已注册的 Widgets（同步方法，需要确保 runtime 已同步）
  const loadWidgetsSync = useCallback(() => {
    try {
      const runtime = getTappRuntime()
      const registeredWidgets = runtime.getRegisteredWidgets()

      const widgetTypes = registeredWidgets.map(createTappWidgetType)
      setTappWidgets(widgetTypes)
      setError(null)
    }
    catch (err) {
      console.error('[useTappWidgets] Failed to load widgets:', err)
      setError(err instanceof Error ? err.message : 'Failed to load widgets')
    }
    finally {
      setIsLoading(false)
    }
  }, [])

  // 异步加载：等待 runtime 同步完成后再加载
  const loadWidgetsAsync = useCallback(async () => {
    try {
      setIsLoading(true)
      const runtime = getTappRuntime()

      // 等待 runtime 同步完成
      await runtime.waitForSync()

      const registeredWidgets = runtime.getRegisteredWidgets()
      const widgetTypes = registeredWidgets.map(createTappWidgetType)
      setTappWidgets(widgetTypes)
      setError(null)
    }
    catch (err) {
      console.error('[useTappWidgets] Failed to load widgets:', err)
      setError(err instanceof Error ? err.message : 'Failed to load widgets')
    }
    finally {
      setIsLoading(false)
    }
  }, [])

  // 初始加载 - 等待同步完成
  useEffect(() => {
    loadWidgetsAsync()
  }, [loadWidgetsAsync])

  // 监听 Widget 注册/注销事件 和 同步完成事件
  useEffect(() => {
    const runtime = getTappRuntime()

    // 当有新的 widget 注册时更新
    const unsubRegistered = runtime.on('widget:registered', () => {
      loadWidgetsSync()
    })

    // 当有 widget 注销时更新
    const unsubUnregistered = runtime.on('widget:unregistered', () => {
      loadWidgetsSync()
    })

    // 当后端同步完成时更新（确保获取最新数据）
    const unsubSync = runtime.on('sync:complete', () => {
      loadWidgetsSync()
    })

    return () => {
      unsubRegistered()
      unsubUnregistered()
      unsubSync()
    }
  }, [loadWidgetsSync])

  return {
    tappWidgets,
    isLoading,
    error,
    refreshWidgets: loadWidgetsAsync,
  }
}

/**
 * 合并系统 Widgets 和 Tapp Widgets
 */
export function useCombinedWidgets(systemWidgets: WidgetType[]): WidgetType[] {
  const { tappWidgets } = useTappWidgets()

  return useMemo(() => {
    // 过滤掉重复的（基于 ID）
    const systemIds = new Set(systemWidgets.map(w => w.id))
    const uniqueTappWidgets = tappWidgets.filter(w => !systemIds.has(w.id))

    return [...systemWidgets, ...uniqueTappWidgets]
  }, [systemWidgets, tappWidgets])
}

export default useTappWidgets
