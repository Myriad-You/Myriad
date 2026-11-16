import type { TappCodeStructure, TappInstance } from '@myriad/tapp-runtime'
import React, { lazy, Suspense, useCallback, useEffect, useRef, useState } from 'react'
import {
  clearBackgroundResidents,
  publishBackgroundResidents,
  registerBackgroundResidentStopHandler,
} from '../runtime/backgroundResidentStore'
import { getTappRuntime } from '../runtime/TappRuntime'
import { tappKernel } from '../tappKernel'

const TappPageSandbox = lazy(() => import('../runtime/TappPageSandbox').then(module => ({ default: module.TappPageSandbox })))

export const TappBackgroundRunner: React.FC = () => {
  const disabledRef = useRef(new Set<string>())
  const mountedRef = useRef(false)
  const backgroundTappsRef = useRef<TappInstance[]>([])
  const [backgroundTapps, setBackgroundTapps] = useState<TappInstance[]>([])
  const [tappCodes, setTappCodes] = useState<Map<string, TappCodeStructure>>(
    new Map(),
  )
  const loadingRef = useRef(false)
  const reloadPendingRef = useRef(false)
  const runtime = getTappRuntime()

  const commitBackgroundTapps = useCallback(
    (next: TappInstance[]) => {
      backgroundTappsRef.current = next
      setBackgroundTapps(next)
      publishBackgroundResidents(next, (tappId) =>
        runtime.getBackgroundRequirements(tappId),
      )
    },
    [runtime],
  )

  const loadBackgroundTapps = useCallback(async (): Promise<void> => {
    // 合并并发加载，但不丢加载期间的 start/stop/background 变化。
    if (loadingRef.current) {
      reloadPendingRef.current = true
      return
    }
    loadingRef.current = true

    try {
      await runtime.waitForSync()

      if (!mountedRef.current) return
      const tappsToRun = runtime
        .getBackgroundTapps()
        .filter((tapp) => !disabledRef.current.has(tapp.id))

      const codes = new Map<string, TappCodeStructure>()
      for (const tapp of tappsToRun) {
        if (codes.size >= 4) break
        if (!mountedRef.current || reloadPendingRef.current) return
        try {
          // 后台只加载 core，不生成 Page HTML/CSS。
          const resources = await tappKernel.resources.loadCoreResources(tapp)

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
      }

      if (!mountedRef.current || reloadPendingRef.current) return
      commitBackgroundTapps(tappsToRun.filter((tapp) => codes.has(tapp.id)))
      setTappCodes(codes)
    } catch (error) {
      console.error(
        '[TappBackgroundRunner] Failed to load background Tapps:',
        error,
      )
    } finally {
      loadingRef.current = false
      if (mountedRef.current && reloadPendingRef.current) {
        reloadPendingRef.current = false
        void loadBackgroundTapps()
      }
    }
  }, [runtime, commitBackgroundTapps])

  useEffect(() => {
    mountedRef.current = true
    loadBackgroundTapps()
    return () => {
      mountedRef.current = false
      backgroundTappsRef.current = []
      clearBackgroundResidents()
    }
  }, [loadBackgroundTapps])

  useEffect(() => {
    const handleTappEvent = () => {
      // Remove revoked / uninstalled instances immediately, before awaiting code.
      const allowed = runtime
        .getBackgroundTapps()
        .filter((tapp) => !disabledRef.current.has(tapp.id))
      commitBackgroundTapps(
        backgroundTappsRef.current.flatMap((tapp) => {
          const next = allowed.find((candidate) => candidate.id === tapp.id)
          return next ? [next] : []
        }),
      )
      loadBackgroundTapps()
    }

    const handleSubjectChanged = () => {
      disabledRef.current.clear()
      commitBackgroundTapps([])
      setTappCodes(new Map())
      loadBackgroundTapps()
    }
    const unsubResidentStop = registerBackgroundResidentStopHandler(
      (tappId) => {
        disabledRef.current.add(tappId)
        commitBackgroundTapps(
          backgroundTappsRef.current.filter((tapp) => tapp.id !== tappId),
        )
        void loadBackgroundTapps()
      },
    )
    window.addEventListener('tapp-subject-ready', handleSubjectChanged)
    const unsubStarted = runtime.on('tapp:started', handleTappEvent)
    const unsubStopped = runtime.on('tapp:stopped', handleTappEvent)
    const unsubInstalled = runtime.on('tapp:installed', handleTappEvent)
    const unsubUninstalled = runtime.on('tapp:uninstalled', handleTappEvent)
    const unsubUpdated = runtime.on('tapp:updated', handleTappEvent)
    const unsubSync = runtime.on('sync:complete', handleTappEvent)
    const unsubBackground = runtime.on('background:changed', handleTappEvent)

    return () => {
      window.removeEventListener('tapp-subject-ready', handleSubjectChanged)
      unsubStarted()
      unsubStopped()
      unsubInstalled()
      unsubUninstalled()
      unsubUpdated()
      unsubSync()
      unsubBackground()
      unsubResidentStop()
    }
  }, [runtime, loadBackgroundTapps, commitBackgroundTapps])

  return (
    <div
      className="fixed top-0 left-0 w-0 h-0 overflow-hidden invisible pointer-events-none"
      aria-hidden="true"
    >
      <Suspense fallback={null}>
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
      </Suspense>
    </div>
  )
}

export default TappBackgroundRunner
