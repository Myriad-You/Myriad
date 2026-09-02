import type { PublishEventRequest, TappEvent, TappInstance } from '../types'
import type { TappBridge } from './TappBridge'
import { getDefaultLocale } from '../../i18n'
import { subscribeToTheme } from '../../utils/themeSubscriber'
import { userFacingError } from '../../utils/userFacingError'
import * as TappApiService from '../services/TappApiService'
import { onSpaNavigation } from './spaNavigation'

const RECONNECT_DELAY_MS = 500

/**
 * Connect one Page/Widget/headless runtime to the online event broker.
 * Runtime Grants remain host-only; the sandbox receives validated envelopes.
 */
export function registerEventHandlers(
  bridge: TappBridge,
  tappInstance: TappInstance,
): () => void {
  let stopped = false
  let streamController: AbortController | null = null
  const cleanupSystemProducers: Array<() => void> = []
  const subscriptions = new Set(tappInstance.manifest.events?.subscribe ?? [])
  const hasServerSubscriptions = [...subscriptions].some(
    (topic) => !topic.startsWith('system.'),
  )

  // system.* topics are produced by the trusted host, never by sandbox code.
  // They are browser-local facts, so delivering them directly also avoids
  // pretending that theme/network/visibility state is global server state.
  let hostRuntimeId = 'host'
  void bridge
    .getRuntimeId()
    .then((runtimeId) => {
      hostRuntimeId = runtimeId
    })
    .catch(() => undefined)
  const emitSystem = (topic: string, payload: unknown) => {
    if (!subscriptions.has(topic) || stopped) return
    bridge.emit('tappEvent', {
      version: 2,
      eventId: `sys_${crypto.randomUUID().replaceAll('-', '')}`,
      topic,
      scope: 'instance',
      source: { tappId: 'system.host', runtimeId: hostRuntimeId },
      payload,
      occurredAt: new Date().toISOString(),
    } satisfies TappEvent)
  }

  if (subscriptions.has('system.theme.changed')) {
    cleanupSystemProducers.push(
      subscribeToTheme((isDark) =>
        emitSystem('system.theme.changed', {
          theme: isDark ? 'dark' : 'light',
        }),
      ),
    )
  }
  if (subscriptions.has('system.network.changed')) {
    const onNetwork = () =>
      emitSystem('system.network.changed', { online: navigator.onLine })
    window.addEventListener('online', onNetwork)
    window.addEventListener('offline', onNetwork)
    onNetwork()
    cleanupSystemProducers.push(() => {
      window.removeEventListener('online', onNetwork)
      window.removeEventListener('offline', onNetwork)
    })
  }
  if (subscriptions.has('system.locale.changed')) {
    let lastLocale: string | null = null
    const onLocale = (event?: Event) => {
      // storage 是全局事件；只响应其他标签页真正改写 locale 的情况。
      if (event instanceof StorageEvent && event.key !== 'locale') return
      const locale = getDefaultLocale()
      if (locale === lastLocale) return
      lastLocale = locale
      emitSystem('system.locale.changed', { locale })
    }
    window.addEventListener('languagechange', onLocale)
    window.addEventListener('storage', onLocale)
    onLocale()
    cleanupSystemProducers.push(() => {
      window.removeEventListener('languagechange', onLocale)
      window.removeEventListener('storage', onLocale)
    })
  }
  if (subscriptions.has('system.visibility.changed')) {
    const onVisibility = () =>
      emitSystem('system.visibility.changed', {
        visibility: document.visibilityState,
      })
    document.addEventListener('visibilitychange', onVisibility)
    onVisibility()
    cleanupSystemProducers.push(() =>
      document.removeEventListener('visibilitychange', onVisibility),
    )
  }
  if (subscriptions.has('system.navigation.changed')) {
    let lastKey = ''
    const onNavigation = () => {
      const key = `${location.pathname}${location.search}${location.hash}`
      // Always emit on first call; skip exact duplicates from multi-sources
      if (key === lastKey && lastKey !== '') return
      lastKey = key
      emitSystem('system.navigation.changed', {
        pathname: location.pathname,
        search: location.search,
        hash: location.hash,
      })
    }
    // pushState/replaceState (React Router) + popstate/hash via shared helper
    const unsubSpa = onSpaNavigation(onNavigation)
    document.addEventListener('astro:page-load', onNavigation)
    onNavigation()
    cleanupSystemProducers.push(() => {
      unsubSpa()
      document.removeEventListener('astro:page-load', onNavigation)
    })
  }

  bridge.registerHandler('event.publish', async (message) => {
    const [request] = (message.payload as { args: unknown[] }).args || []
    if (!request) return { success: false, error: 'Event request required' }
    try {
      const result = await TappApiService.publishEvent(
        request as PublishEventRequest,
        await bridge.getRuntimeGrant(),
      )
      return { success: true, data: result }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  if (hasServerSubscriptions) {
    void (async () => {
      for (;;) {
        if (stopped) break
        streamController = new AbortController()
        try {
          await TappApiService.streamEvents(
            await bridge.getRuntimeGrant(),
            (event, data) => {
              if (event === 'event' && data && typeof data === 'object') {
                bridge.emit('tappEvent', data as TappEvent)
              }
            },
            streamController.signal,
          )
        } catch (error) {
          if (!stopped && !streamController.signal.aborted) {
            console.warn('[TappEventBroker] Runtime stream disconnected', error)
          }
        } finally {
          streamController = null
        }
        if (!stopped) {
          await new Promise((resolve) =>
            setTimeout(resolve, RECONNECT_DELAY_MS),
          )
        }
      }
    })()
  }

  return () => {
    stopped = true
    cleanupSystemProducers.forEach((cleanup) => cleanup())
    streamController?.abort()
    streamController = null
  }
}
