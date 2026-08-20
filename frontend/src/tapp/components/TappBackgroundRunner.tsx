/**
 * Tapp Background Runner
 * 按需在后台运行已启动的 Tapp
 *
 * 架构说明：
 * - 只运行有后台需求声明的 Tapp
 * - 默认情况下，Tapp 离开页面后会被冻结
 * - Tapp 需要通过 Tapp.background.require() 声明后台需求
 * - Widget 渲染由 TappWidget 组件单独处理（widget 模式）
 *
 * 后台需求类型：
 * - media: 媒体控制（如音乐播放器扩展）
 * - sync: 后台数据同步
 * - notification: 定时通知
 * - scheduler: 定时任务
 * - event-listener: 事件监听（跨 Tapp 通信）
 * - realtime: 实时数据更新
 */

import type { TappCodeStructure, TappInstance } from '../types'
import React, { useCallback, useEffect, useRef, useState } from 'react'
import { getTappRuntime } from '../runtime'
import { loadCoreResources } from '../runtime/sandbox/resourceLoader'
import { TappPageSandbox } from '../runtime/TappPageSandbox'

/**
 * 后台 Tapp 运行器
 * 只为有后台需求的 running 状态 Tapp 创建隐藏的 headless 沙箱
 */
export const TappBackgroundRunner: React.FC = () => {
  const [backgroundTapps, setBackgroundTapps] = useState<TappInstance[]>([])
  const [tappCodes, setTappCodes] = useState<Map<string, TappCodeStructure>>(
    new Map(),
  )
  const loadingRef = useRef(false)
  const reloadPendingRef = useRef(false)
  const runtime = getTappRuntime()

  // 加载需要后台运行的 Tapp（有后台需求声明的）
  const loadBackgroundTapps = useCallback(async (): Promise<void> => {
    // 合并并发加载，但不能丢掉加载期间发生的 start/stop/background 变化。
    if (loadingRef.current) {
      reloadPendingRef.current = true
      return
    }
    loadingRef.current = true

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
            // 后台实例只需要 core，不生成或缓存 Page HTML/CSS。
            const resources = await loadCoreResources(tapp)

            // 转换为 TappCodeStructure 格式
            const code: TappCodeStructure = {
              modules: resources.modules,
              moduleResolutions: resources.moduleResolutions,
              coreEntry: resources.coreEntry,
              i18n: resources.i18n,
            }

            codes.set(tapp.id, code)
          } catch (error) {
            console.error(
              `[TappBackgroundRunner] Failed to load code for Tapp ${tapp.id}:`,
              error,
            )
          }
        }),
      )

      setBackgroundTapps(tappsToRun)
      setTappCodes(codes)
    } catch (error) {
      console.error(
        '[TappBackgroundRunner] Failed to load background Tapps:',
        error,
      )
    } finally {
      loadingRef.current = false
      if (reloadPendingRef.current) {
        reloadPendingRef.current = false
        void loadBackgroundTapps()
      }
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
    const unsubUpdated = runtime.on('tapp:updated', handleTappEvent)
    const unsubSync = runtime.on('sync:complete', handleTappEvent)
    // 监听后台需求变化
    const unsubBackground = runtime.on('background:changed', handleTappEvent)

    return () => {
      unsubStarted()
      unsubStopped()
      unsubInstalled()
      unsubUninstalled()
      unsubUpdated()
      unsubSync()
      unsubBackground()
    }
  }, [runtime, loadBackgroundTapps])

  // 不渲染任何可见 UI，只在 DOM 中创建隐藏的 iframe。
  // headless=true：只运行 core（大脑）代码，不渲染整页 DOM——
  // 后台实例从「隐形整页」降到「无头 JS」，大幅减少内存占用。
  return (
    <div
      className="fixed top-0 left-0 w-0 h-0 overflow-hidden invisible pointer-events-none"
      aria-hidden="true"
    >
      {backgroundTapps.map((tapp) => {
        const code = tappCodes.get(tapp.id)
        if (!code) return null

        return (
          <TappPageSandbox
            key={`${tapp.id}:${tapp.manifest.version}`}
            tappInstance={tapp}
            code={code}
            headless
            onError={(error) => {
              console.error(
                `[TappBackgroundRunner] Tapp ${tapp.id} error:`,
                error,
              )
            }}
            className="w-px h-px"
          />
        )
      })}
    </div>
  )
}

export default TappBackgroundRunner
