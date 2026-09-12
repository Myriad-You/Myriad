import type { TappCodeStructure, TappInstance } from '../types'
import React, { useCallback, useEffect, useRef, useState } from 'react'
import { getTappRuntime } from '../runtime'
import { loadCoreResources } from '../runtime/sandbox/resourceLoader'
import { TappPageSandbox } from '../runtime/TappPageSandbox'

export const TappBackgroundRunner: React.FC = () => {
  const [backgroundTapps, setBackgroundTapps] = useState<TappInstance[]>([])
  const [tappCodes, setTappCodes] = useState<Map<string, TappCodeStructure>>(
    new Map(),
  )
  const loadingRef = useRef(false)
  const reloadPendingRef = useRef(false)
  const runtime = getTappRuntime()

  const loadBackgroundTapps = useCallback(async (): Promise<void> => {
    // 合并并发加载，但不丢加载期间的 start/stop/background 变化。
    if (loadingRef.current) {
      reloadPendingRef.current = true
      return
    }
    loadingRef.current = true

    try {
      await runtime.waitForSync()

      const tappsToRun = runtime.getBackgroundTapps()

      const codes = new Map<string, TappCodeStructure>()
      await Promise.all(
        tappsToRun.map(async (tapp) => {
          try {
            // 后台只加载 core，不生成 Page HTML/CSS。
            const resources = await loadCoreResources(tapp)

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

  useEffect(() => {
    loadBackgroundTapps()
  }, [loadBackgroundTapps])

  useEffect(() => {
    const handleTappEvent = () => {
      loadBackgroundTapps()
    }

    const unsubStarted = runtime.on('tapp:started', handleTappEvent)
    const unsubStopped = runtime.on('tapp:stopped', handleTappEvent)
    const unsubInstalled = runtime.on('tapp:installed', handleTappEvent)
    const unsubUninstalled = runtime.on('tapp:uninstalled', handleTappEvent)
    const unsubUpdated = runtime.on('tapp:updated', handleTappEvent)
    const unsubSync = runtime.on('sync:complete', handleTappEvent)
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

  // 不渲染可见 UI。headless：只跑 core，不挂整页 DOM。
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
