/**
 * Tapp Background Runner
 * 按需在后台运行已启动的 Tapp
 *
 * 架构说明：
 * - 只运行有后台需求声明的 Tapp（如有 widget 在主页显示）
 * - 默认情况下，Tapp 离开页面后会被冻结
 * - Tapp 需要通过 Tapp.background.require() 声明后台需求
 * - Widget 渲染由 TappWidget 组件单独处理（widget 模式）
 *
 * 后台需求类型：
 * - widget: 有小组件在主页显示
 * - media: 媒体控制（如音乐播放器扩展）
 * - sync: 后台数据同步
 * - notification: 定时通知
 * - scheduler: 定时任务
 * - event-listener: 事件监听（跨 Tapp 通信）
 * - realtime: 实时数据更新
 */

import type { TappCodeStructure } from '../examples/tapps/types'
import type { TappInstance } from '../types'
import React, { useCallback, useEffect, useRef, useState } from 'react'
import { getTappRuntime } from '../runtime'
import { loadPageResources } from '../runtime/sandbox/resourceLoader'
import { TappPageSandbox } from '../runtime/TappPageSandbox'

/**
 * 后台 Tapp 运行器
 * 只为有后台需求的 running 状态 Tapp 创建隐藏的沙箱（page 模式）
 */
export const TappBackgroundRunner: React.FC = () => {
  const [backgroundTapps, setBackgroundTapps] = useState<TappInstance[]>([])
  const [tappCodes, setTappCodes] = useState<Map<string, TappCodeStructure>>(new Map())
  const [isLoading, setIsLoading] = useState(false)
  const loadingRef = useRef(false)
  const runtime = getTappRuntime()

  // 加载需要后台运行的 Tapp（有后台需求声明的）
  const loadBackgroundTapps = useCallback(async () => {
    // 防止并发加载
    if (loadingRef.current)
      return
    loadingRef.current = true
    setIsLoading(true)

    try {
      // 等待 runtime 同步完成
      await runtime.waitForSync()

      // 获取所有需要后台运行的 Tapp（running + 有后台需求）
      const tappsToRun = runtime.getBackgroundTapps()

      // 异步加载代码
      const codes = new Map<string, TappCodeStructure>()
      await Promise.all(
        tappsToRun.map(async (tapp) => {
          try {
            // 🎯 使用新的资源加载器获取 Page 专用资源
            const resources = await loadPageResources(tapp)

            // 转换为 TappCodeStructure 格式
            const code: TappCodeStructure = {
              core: resources.core,
              page: resources.page,
              pageHtml: resources.html,
              styles: resources.styles,
              pageCSS: resources.css,
            }

            codes.set(tapp.id, code)
          }
          catch (error) {
            console.error(`[TappBackgroundRunner] Failed to load code for Tapp ${tapp.id}:`, error)
          }
        }),
      )

      setBackgroundTapps(tappsToRun)
      setTappCodes(codes)
    }
    catch (error) {
      console.error('[TappBackgroundRunner] Failed to load background Tapps:', error)
    }
    finally {
      loadingRef.current = false
      setIsLoading(false)
    }
  }, [runtime])

  // 初始加载
  useEffect(() => {
    loadBackgroundTapps()
  }, [loadBackgroundTapps])

  // 监听 Tapp 启动/停止事件 和 后台需求变化
  useEffect(() => {
    // 包装为事件处理器
    const handleTappEvent = () => {
      loadBackgroundTapps()
    }

    const unsubStarted = runtime.on('tapp:started', handleTappEvent)
    const unsubStopped = runtime.on('tapp:stopped', handleTappEvent)
    const unsubInstalled = runtime.on('tapp:installed', handleTappEvent)
    const unsubUninstalled = runtime.on('tapp:uninstalled', handleTappEvent)
    // 监听后台需求变化
    const unsubBackground = runtime.on('background:changed', handleTappEvent)

    return () => {
      unsubStarted()
      unsubStopped()
      unsubInstalled()
      unsubUninstalled()
      unsubBackground()
    }
  }, [runtime, loadBackgroundTapps])

  // 不渲染任何可见 UI，只在 DOM 中创建隐藏的 iframe
  // 使用 page 模式运行，执行完整的生命周期回调（onReady）
  return (
    <div
      className="fixed top-0 left-0 w-0 h-0 overflow-hidden invisible pointer-events-none"
      aria-hidden="true"
    >
      {backgroundTapps.map((tapp) => {
        const code = tappCodes.get(tapp.id)
        if (!code)
          return null

        return (
          <TappPageSandbox
            key={tapp.id}
            tappInstance={tapp}
            code={code}
            onError={(error) => {
              console.error(`[TappBackgroundRunner] Tapp ${tapp.id} error:`, error)
            }}
            className="w-px h-px"
          />
        )
      })}
    </div>
  )
}

export default TappBackgroundRunner
