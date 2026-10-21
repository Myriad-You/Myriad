import type { TappCodeStructure, TappInstance } from '../types'
import type { AnimationConfigRef, WidgetRenderProps } from './sandbox'
import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import {
  TAPP_WIDGET_SKELETON,
  WidgetSkeletonCover,
} from '../../components/widgets/shared/WidgetSkeleton'
import { useI18n } from '../../contexts/I18nContext'
import { useAnimationLevel } from '../../hooks/useAnimationLevel'
import { emitAppEvent } from '../../utils/appEvents'
import {
  buildTappMediaState,
  mergeMusicPlayerEventDetail,
} from '../../utils/musicPlayerState'
import { useTappSubject } from '../../utils/tappSubject'

import {
  calculateWidgetDimensions,
  sendResizeMessage,
  useIframeResize,
} from '../utils/iframeResize'
import {
  buildLayerScript,
  getCodeStructureFingerprint,
  getTappRuntimeFingerprint,
} from './codeStructure'
import {
  cspOptionsFromPermissions,
  escapeSandboxHtmlText,
  escapeSandboxScriptSource,
  generateCSP,
  generateSecurityWrapper,
  generateSessionToken,
  generateThemeCSS,
  generateWidgetSDK,
  IFRAME_SANDBOX_ATTRS,
  sandboxScriptNonce,
  serializeSandboxScriptValue,
  WIDGET_STATIC_CSS,
} from './sandbox'
import {
  registerAgentInteractionHandlers,
  registerAIHandlers,
  registerAnalyticsHandlers,
  registerAnimationHandlers,
  registerAssetHandlers,
  registerBackgroundHandlers,
  registerContextHandlers,
  registerDataExchangeHandlers,
  registerEventHandlers,
  registerFileHandlers,
  registerLifecycleHandlers,
  registerMediaHandlers,
  registerPersonaHandlers,
  registerPlatformHandlers,
  registerReportHandlers,
  registerSchedulerHandlers,
  registerSpeechHandlers,
  registerStorageHandlers,
  registerUIHandlers,
  registerUserHandlers,
  registerWidgetInvalidateTargetHandler,
} from './sandbox/handlers'
import { registerPlaygroundPreviewHandlers } from './sandbox/handlers/playgroundPreviewHandlers'
import { TappBridge } from './TappBridge'
import { TappRuntimeGrant } from './TappRuntimeGrant'
import { useSandboxSubscriptions } from './useSandboxSubscriptions'
import { widgetPerfMark } from './WidgetLoadPerf'
import { bindAllTappKvChanges } from './WidgetRuntimeSignals'

export interface TappWidgetSandboxProps {
  tappInstance: TappInstance
  code: TappCodeStructure
  widgetId: string
  widgetProps: WidgetRenderProps
  /** Playground 预览。无 Runtime Grant；不得把声明权限当作已安装授予。 */
  paused?: boolean
  previewMode?: boolean
  previewStores?: {
    storage: Map<string, unknown>
    settings: Map<string, unknown>
    shared: Map<string, unknown>
    private: Map<string, unknown>
  }
  onError?: (error: Error) => void
  onReady?: () => void
  onInstanceSettingsChange?: (patch: Record<string, unknown>) => boolean
  onInvalidate?: (reason: string) => void
  /** 卡片形态：作者调 Tapp.widget.openPopup()。宿主只在用户刚点过这张卡时放行。 */
  onOpenPopup?: (params: unknown) => void
  /** widget = 首页格子里的卡片；popup = 同一个 widget 的弹窗层，跑在单独的沙箱里。 */
  surface?: WidgetSandboxSurface
  /** popup 形态：最近一次 openPopup 带来的参数。 */
  popupParams?: unknown
  onClosePopup?: () => void
  className?: string
  style?: React.CSSProperties
}

export type WidgetSandboxSurface = 'widget' | 'popup'

/** openPopup 参数上限：只够传一个视图选择，大数据走 storage。 */
const POPUP_PARAMS_MAX_BYTES = 4 * 1024
/** 点进卡片之后多久内算「刚点过」。 */
const POPUP_GESTURE_WINDOW_MS = 1500
const POPUP_OPEN_MIN_INTERVAL_MS = 800
/** 浏览器的瞬时激活时长取上限（Chrome、Firefox 为 5 秒；其他浏览器 uncertain，按更长算更严）。 */
const TRANSIENT_ACTIVATION_MS = 5000

/** 宿主页面自己最近一次被点或按键的时刻：父窗口的激活可能来自这里，不能记在卡片头上。 */
let hostActivatedAt = 0
let hostActivationTracked = false
function lastHostActivationAt(): number {
  if (!hostActivationTracked && typeof window !== 'undefined') {
    hostActivationTracked = true
    const mark = () => { hostActivatedAt = Date.now() }
    window.addEventListener('pointerdown', mark, { capture: true, passive: true })
    window.addEventListener('keydown', mark, { capture: true, passive: true })
  }
  return hostActivatedAt
}

function plainJson(value: unknown): unknown {
  if (value === undefined || value === null) return null
  const encoded = JSON.stringify(value)
  if (encoded === undefined) throw new Error('Popup params must be JSON')
  if (new TextEncoder().encode(encoded).byteLength > POPUP_PARAMS_MAX_BYTES) {
    throw new Error('Popup params exceed 4 KiB')
  }
  return JSON.parse(encoded)
}

function generateWidgetHTML(
  tappInstance: TappInstance,
  code: TappCodeStructure,
  widgetId: string,
  widgetProps: WidgetRenderProps,
  sessionToken: string,
  labels: { missing: string; renderFailed: string },
  surface: WidgetSandboxSurface = 'widget',
  popupParams: unknown = null,
): string {
  const { manifest } = tappInstance
  const isDark = widgetProps.theme === 'dark'
  const primaryColor = widgetProps.primaryColor || '#8b5cf6'

  const nonce = sandboxScriptNonce()
  const cspOptions = cspOptionsFromPermissions(
    tappInstance.grantedPermissions,
    tappInstance.grantedRemoteMedia,
  )
  const csp = generateCSP(nonce, cspOptions)
  const securityWrapper = escapeSandboxScriptSource(
    generateSecurityWrapper(sessionToken, cspOptions.remoteMediaHosts),
  )
  const sdkCode = escapeSandboxScriptSource(
    generateWidgetSDK(tappInstance, sessionToken),
  )
  const themeCSS = generateThemeCSS(isDark, primaryColor)

  const customCSS = code.styles || ''

  const isPopup = surface === 'popup'
  // 尺寸模板只属于卡片；弹窗层纯靠 render()。
  const hasHtmlTemplate = !isPopup && !!code.widgetHtml
  const widgetHtmlContent = hasHtmlTemplate ? code.widgetHtml || '' : ''
  const registry = isPopup ? 'popups' : 'widgets'

  const widgetCode = buildLayerScript(code, surface, widgetId).source

  const tailwindCSS = code.widgetCSS || ''

  const hasHtmlTemplateLiteral = hasHtmlTemplate ? 'true' : 'false'

  return `<!DOCTYPE html>
<html>
<head>
  <meta charset="UTF-8">
  <meta http-equiv="Content-Security-Policy" content="${csp}">
  <meta name="viewport" content="width=device-width, initial-scale=1.0, maximum-scale=1.0, user-scalable=no">
  <title>${escapeSandboxHtmlText(manifest.name)} ${isPopup ? 'Popup' : 'Widget'}</title>
  <style>
    ${WIDGET_STATIC_CSS}
    ${tailwindCSS}
    ${themeCSS}
    ${customCSS}
  </style>
</head>
<body class="${isDark ? 'dark' : 'light'}">
  <div id="widget-root">${widgetHtmlContent}</div>

  <script nonce="${nonce}">
    window._TAPP_MODE = ${serializeSandboxScriptValue(surface)};
    window._TAPP_WIDGET_ID = ${serializeSandboxScriptValue(widgetId)};
    window._TAPP_POPUP_PARAMS = ${serializeSandboxScriptValue(isPopup ? popupParams ?? null : null)};
    window._TAPP_WIDGET_PROPS = ${serializeSandboxScriptValue(widgetProps)};
    window._TAPP_LOCALE = ${serializeSandboxScriptValue(widgetProps.locale)};
    window._TAPP_I18N = ${serializeSandboxScriptValue(code.i18n || {})};
    window._TAPP_SESSION_TOKEN = ${serializeSandboxScriptValue(sessionToken)};
    window._TAPP_DIMENSIONS = { width: 0, height: 0, scale: 1, fontScale: 1, isCompact: false, isMini: false };
    window._TAPP_HAS_HTML = ${hasHtmlTemplateLiteral};

    window.addEventListener('message', function(e) {
      const msg = e.data;
      if (msg?.type === 'event' && msg.action === 'container:resize') {
        window._TAPP_DIMENSIONS = msg.payload;
        const root = document.documentElement;
        root.style.setProperty('--tapp-scale', msg.payload.scale || 1);
        root.style.setProperty('--tapp-font-scale', msg.payload.fontScale || 1);
        window.dispatchEvent(new CustomEvent('tapp:resize', { detail: msg.payload }));
      }
    });
  </script>

  <!-- 安全包装（冻结危险 API；边界仍以 CSP/sandbox 为准） -->
  <script nonce="${nonce}">${securityWrapper}</script>

  <script nonce="${nonce}">${sdkCode}</script>

  <script nonce="${nonce}">
    (function() {
      'use strict';
      try {
        ${escapeSandboxScriptSource(widgetCode)}
      } catch (error) {
        console.error('[Widget] Code error:', error);
      }
    })();
  </script>

  <!-- Always try render(): pure-JS fills container; hybrid paints data into template.
       Ready after first paint so the host skeleton covers bootstrap. -->
  <script nonce="${nonce}">
    (function() {
      'use strict';
      let readySent = false;
      const postWidgetReady = function() {
        if (readySent) return;
        readySent = true;
        window.parent.postMessage({
          type: 'event',
          id: 'widget-ready-' + Date.now(),
          action: 'tapp.ready',
          payload: null,
          timestamp: Date.now(),
          _sessionToken: window._TAPP_SESSION_TOKEN
        }, document.referrer ? new URL(document.referrer).origin : '*');
      };
      const runRender = function() {
        try {
          const widgetId = ${serializeSandboxScriptValue(widgetId)};
          const widgetDef = Tapp.${registry} && Tapp.${registry}[widgetId];
          const container = document.getElementById('widget-root');
          if (!container) return;

          if (!widgetDef || typeof widgetDef.render !== 'function') {
            if (!window._TAPP_HAS_HTML) {
              console.warn('[Widget] Not found:', widgetId);
              container.innerHTML = '<div class="tapp-empty">' + ${serializeSandboxScriptValue(labels.missing)} + '</div>';
            }
            postWidgetReady();
            return;
          }

          const props = window._TAPP_WIDGET_PROPS || {};
          props.scale = window._TAPP_DIMENSIONS.scale;
          props.fontScale = window._TAPP_DIMENSIONS.fontScale;
          if (${isPopup ? 'true' : 'false'}) props.params = window._TAPP_POPUP_PARAMS;

          const painted = widgetDef.render(container, props);
          if (painted && typeof painted.then === 'function') {
            painted.then(postWidgetReady, function(error) {
              console.error('[Widget] Render error:', error);
              container.innerHTML =
                '<div class="tapp-empty tapp-text-error">' + ${serializeSandboxScriptValue(labels.renderFailed)} + '</div>';
              postWidgetReady();
            });
            return;
          }
          postWidgetReady();
        } catch (error) {
          console.error('[Widget] Render error:', error);
          const root = document.getElementById('widget-root');
          if (root) {
            root.innerHTML =
              '<div class="tapp-empty tapp-text-error">' + ${serializeSandboxScriptValue(labels.renderFailed)} + '</div>';
          }
          postWidgetReady();
        }
      };
      if (typeof Promise !== 'undefined' && Promise.resolve) {
        Promise.resolve().then(runRender);
      } else {
        setTimeout(runRender, 0);
      }
    })();
  </script>
</body>
</html>`
}

export const TappWidgetSandbox = memo(
  ({
    tappInstance,
    code,
    widgetId,
    widgetProps,
    paused = false,
    previewMode = false,
    previewStores,
    onReady,
    onInstanceSettingsChange,
    onInvalidate,
    onOpenPopup,
    surface = 'widget',
    popupParams,
    onClosePopup,
    className,
    style,
  }: TappWidgetSandboxProps) => {
    const { t } = useI18n()
    const { containerRef, dimensions } = useIframeResize<HTMLDivElement>()
    const iframeRef = useRef<HTMLIFrameElement>(null)
    const bridgeRef = useRef<TappBridge | null>(null)
    const previewStorageRef = useRef(new Map<string, unknown>())
    const previewSettingsRef = useRef(new Map<string, unknown>())
    const [isReady, setIsReady] = useState(false)
    const [readyStalled, setReadyStalled] = useState(false)
    const subject = useTappSubject()
  const subjectEpoch = subject.epoch
    const animationConfig = useAnimationLevel()
    const animationConfigRef = useRef<AnimationConfigRef>(animationConfig)

    useEffect(() => {
      if (isReady) {
        setReadyStalled(false)
        return
      }
      setReadyStalled(false)
      const id = window.setTimeout(() => {
        setReadyStalled(true)
      }, TAPP_WIDGET_SKELETON.readyTimeoutMs)
      return () => window.clearTimeout(id)
    }, [isReady, tappInstance.id, widgetId, subjectEpoch])

    const tappInstanceRef = useRef(tappInstance)
    const codeRef = useRef(code)
    const instanceSettingsChangeRef = useRef(onInstanceSettingsChange)
    const invalidateRef = useRef(onInvalidate)
    const openPopupRef = useRef(onOpenPopup)
    const closePopupRef = useRef(onClosePopup)
    const popupParamsRef = useRef(popupParams)
    tappInstanceRef.current = tappInstance
    codeRef.current = code
    instanceSettingsChangeRef.current = onInstanceSettingsChange
    invalidateRef.current = onInvalidate
    openPopupRef.current = onOpenPopup
    closePopupRef.current = onClosePopup
    popupParamsRef.current = popupParams
    animationConfigRef.current = animationConfig

    // 宿主外观/语言变化不重建 iframe。
    const configString = JSON.stringify(widgetProps.config || {})
    const latestThemeRef = useRef(widgetProps.theme)
    const latestColorRef = useRef(widgetProps.primaryColor)
    const latestLocaleRef = useRef(widgetProps.locale)
    latestThemeRef.current = widgetProps.theme
    latestColorRef.current = widgetProps.primaryColor
    latestLocaleRef.current = widgetProps.locale
    const stableWidgetProps = useMemo(
      () => ({
        size: widgetProps.size,
        config: widgetProps.config,
        isEditMode: widgetProps.isEditMode,
        isPreview: widgetProps.isPreview,
        locale: latestLocaleRef.current,
        theme: latestThemeRef.current,
        primaryColor: latestColorRef.current,
      }),
      [
        widgetProps.size,
        widgetProps.isEditMode,
        widgetProps.isPreview,
        configString,
      ],
    )

    const handleReady = useCallback(() => {
      setIsReady(true)
      widgetPerfMark(
        tappInstance.id,
        widgetId,
        'iframe-ready',
        widgetProps.size,
      )
      onReady?.()
    }, [onReady, tappInstance.id, widgetId, widgetProps.size])

    useSandboxSubscriptions(bridgeRef, isReady, paused)

    useEffect(
      () =>
        bindAllTappKvChanges(
          () => bridgeRef.current,
          tappInstance.id,
          (reason) => invalidateRef.current?.(reason),
        ),
      [tappInstance.id],
    )

    const buildMediaState = useCallback((detail: Record<string, unknown>) => {
      return buildTappMediaState(detail)
    }, [])

    useEffect(() => {
      if (!isReady) return

      const bridge = bridgeRef.current
      const tapp = tappInstanceRef.current

      if (!bridge || !tapp?.grantedPermissions?.includes('media:read')) return

      const handleMusicStateChange = (e: Event) => {
        const detail = (e as CustomEvent).detail
        if (!detail || !bridgeRef.current) return
        const currentTapp = tappInstanceRef.current
        if (!currentTapp?.grantedPermissions?.includes('media:read')) return
        const globalState =
          (window as { __musicPlayerState?: Record<string, unknown> })
            .__musicPlayerState || {}
        const merged = mergeMusicPlayerEventDetail(globalState, detail)
        bridgeRef.current.emit('mediaStateChange', buildMediaState(merged))
      }

      window.addEventListener(
        'music-player-state-change',
        handleMusicStateChange,
      )

      const pushCurrentState = () => {
        const state = (window as any).__musicPlayerState
        if (state && bridgeRef.current) {
          bridgeRef.current.emit('mediaStateChange', buildMediaState(state))
        }
      }

      const currentGlobalState = (window as any).__musicPlayerState
      if (currentGlobalState) {
        bridge.emit('mediaStateChange', buildMediaState(currentGlobalState))
      } else {
        emitAppEvent('request-music-state-sync')
      }

      const retryTimer = setTimeout(pushCurrentState, 150)

      return () => {
        clearTimeout(retryTimer)
        window.removeEventListener(
          'music-player-state-change',
          handleMusicStateChange,
        )
      }
    }, [isReady])

    useEffect(() => {
      if (!isReady) return

      const handleProgress = (e: Event) => {
        const bridge = bridgeRef.current
        if (!bridge) return

        const tapp = tappInstanceRef.current
        if (!tapp?.grantedPermissions?.includes('media:read')) return

        const { currentTime, audioDuration, songId } = (e as CustomEvent)
          .detail
        if (songId != null) {
          const globalState =
            (window as { __musicPlayerState?: Record<string, unknown> })
              .__musicPlayerState || {}
          const currentId = (
            globalState.currentSong as
              | { id?: string | number }
              | null
              | undefined
          )?.id
          if (currentId != null && String(currentId) !== String(songId)) {
            return
          }
        }
        const progress = {
          current: currentTime,
          duration: audioDuration,
          percentage:
            audioDuration > 0 ? (currentTime / audioDuration) * 100 : 0,
        }

        bridge.emit('mediaProgress', progress)
      }

      window.addEventListener('music-player-progress', handleProgress)
      return () => {
        window.removeEventListener('music-player-progress', handleProgress)
      }
    }, [isReady])

    const codeFingerprint = useMemo(
      () => getCodeStructureFingerprint(code, surface, widgetId),
      [code, surface, widgetId],
    )

    // 跨源 iframe 里的点击不会冒泡到宿主。能看到的是：iframe 刚拿到焦点（父窗口 blur），
    // 以及子框架里的点击会把激活传给父窗口（navigator.userActivation）。焦点确实在这张卡上，
    // 且刚点进来、或指针在卡上时激活只可能来自卡里，才算用户点了它。
    const gestureRef = useRef({ focusedAt: 0, pointerInside: false, lastOpenAt: 0 })
    useEffect(() => {
      if (surface !== 'widget') return
      lastHostActivationAt()
      // 当场记下：点击里的 openPopup 可能比任何延后检查先到。焦点是不是落在这张卡上，
      // 放行时再看 activeElement。
      const onBlur = () => {
        gestureRef.current.focusedAt = Date.now()
      }
      window.addEventListener('blur', onBlur)
      return () => window.removeEventListener('blur', onBlur)
    }, [surface])
    const runtimeFingerprint = getTappRuntimeFingerprint(tappInstance)

    useEffect(() => {
      if (!previewMode && !subject.ready) return
      const container = containerRef.current
      if (!container) return

      const currentTappInstance = tappInstanceRef.current
      const currentCode = codeRef.current

      const propsForHtml = {
        ...stableWidgetProps,
        theme: latestThemeRef.current,
        primaryColor: latestColorRef.current,
      }

      const sessionToken = generateSessionToken()

      const iframe = document.createElement('iframe')
      iframe.className = 'tapp-widget-iframe'
      const onPointerEnter = () => { gestureRef.current.pointerInside = true }
      const onPointerLeave = () => { gestureRef.current.pointerInside = false }
      iframe.addEventListener('pointerenter', onPointerEnter)
      iframe.addEventListener('pointerleave', onPointerLeave)
      const pointerEvents =
        stableWidgetProps.isEditMode || stableWidgetProps.isPreview
          ? 'none'
          : 'auto'
      iframe.style.cssText = `position:absolute;top:0;left:0;width:100%;height:100%;border:none;background-color:transparent;display:block;border-radius:inherit;visibility:visible;opacity:1;pointer-events:${pointerEvents};`
      iframe.setAttribute('sandbox', IFRAME_SANDBOX_ATTRS)
      iframe.setAttribute('referrerpolicy', 'no-referrer')
      iframe.title = `${currentTappInstance.manifest.name} ${surface === 'popup' ? 'Popup' : 'Widget'}`
      iframe.allowFullscreen = true
      iframeRef.current = iframe

      const bridge = new TappBridge()
      // 预览无 Runtime Grant。已安装 widget 按 tappId 共享宿主 grant。
      if (previewMode) {
        bridge.initialize(
          iframe,
          { ...currentTappInstance, previewMode: true },
          sessionToken,
          undefined,
        )
      } else {
        const shared = TappRuntimeGrant.acquireSharedWidget(
          currentTappInstance.id,
        )
        const runtimeGrant = shared.grant
        bridge.initialize(
          iframe,
          currentTappInstance,
          sessionToken,
          runtimeGrant,
          {
            releaseSharedGrant: shared.release,
            reacquireSharedGrant: () =>
              TappRuntimeGrant.acquireSharedWidget(currentTappInstance.id),
          },
        )
        // 与 iframe 解析并行预热 Runtime Grant；仍仅宿主持有，不进 srcdoc。
        void runtimeGrant.getToken().catch(() => {
        })
      }
      bridgeRef.current = bridge
      widgetPerfMark(
        currentTappInstance.id,
        widgetId,
        'sandbox-mount',
        propsForHtml.size,
      )

      registerLifecycleHandlers(bridge, currentTappInstance, handleReady)
      registerUIHandlers(bridge, currentTappInstance, () => {
        try {
          return (
            localStorage.getItem('locale') ||
            document.documentElement.lang ||
            navigator.language ||
            'en-US'
          )
        } catch {
          return 'en-US'
        }
      })
      bridge.registerHandler(
        'widget.instanceSettings.update',
        async (message) => {
          const [patch] = (message.payload as { args?: unknown[] }).args || []
          if (
            !patch ||
            typeof patch !== 'object' ||
            Array.isArray(patch) ||
            Object.getPrototypeOf(patch) !== Object.prototype
          ) {
            return {
              success: false,
              error: 'Settings patch must be a plain object',
            }
          }
          if (JSON.stringify(patch).length > 64 * 1024) {
            return { success: false, error: 'Settings patch exceeds 64 KiB' }
          }
          const accepted = instanceSettingsChangeRef.current?.(
            patch as Record<string, unknown>,
          )
          if (accepted === false) {
            return {
              success: false,
              error: 'Settings patch failed schema validation',
            }
          }
          return { success: true, data: null }
        },
      )
      bridge.registerHandler('widget.invalidate', async (message) => {
        const [rawReason] = (message.payload as { args?: unknown[] }).args || []
        const reason =
          typeof rawReason === 'string' ? rawReason.slice(0, 256) : 'requested'
        invalidateRef.current?.(reason)
        return { success: true, data: null }
      })
      registerWidgetInvalidateTargetHandler(bridge, currentTappInstance, {
        preview: previewMode,
      })
      if (surface === 'widget') {
        bridge.registerHandler('widget.popup.open', async (message) => {
          const declared = currentTappInstance.manifest.widgets?.some(
            (widget) => widget.id === widgetId && widget.popup?.entry,
          )
          if (!declared) {
            return { success: false, error: 'This widget declares no popup layer' }
          }
          const open = openPopupRef.current
          if (!open) {
            return { success: false, error: 'Popup is unavailable here' }
          }
          const gesture = gestureRef.current
          const now = Date.now()
          const focused = document.activeElement === iframeRef.current
          // 刚点进卡片：这次聚焦晚于上一次打开，一次点击只换一次弹窗。
          const enteredNow =
            gesture.focusedAt > gesture.lastOpenAt &&
            now - gesture.focusedAt < POPUP_GESTURE_WINDOW_MS
          // 焦点本来就在卡里、又点了一下：父窗口有瞬时激活，而宿主这边没被点过、
          // 上次打开时那一下激活也已过期，激活就只能来自卡里。不支持的浏览器只认上一种。
          const clickedAgain =
            gesture.pointerInside &&
            navigator.userActivation?.isActive === true &&
            now - lastHostActivationAt() > TRANSIENT_ACTIVATION_MS &&
            now - gesture.lastOpenAt > TRANSIENT_ACTIVATION_MS
          if (!focused || !(enteredNow || clickedAgain)) {
            return { success: false, error: 'Popup must follow a user action on this widget' }
          }
          if (now - gesture.lastOpenAt < POPUP_OPEN_MIN_INTERVAL_MS) {
            return { success: false, error: 'Popup opened too recently' }
          }
          const [rawParams] = (message.payload as { args?: unknown[] }).args || []
          let params: unknown
          try {
            params = plainJson(rawParams)
          } catch (error) {
            return { success: false, error: (error as Error).message }
          }
          gesture.lastOpenAt = now
          open(params)
          return { success: true, data: null }
        })
      } else {
        bridge.registerHandler('popup.close', async () => {
          closePopupRef.current?.()
          return { success: true, data: null }
        })
      }
      registerAnimationHandlers(bridge, animationConfigRef)

      let closeAITaskStreams: () => void = () => {}
      let closeDataExchange: () => void = () => {}
      let closeEventStream: () => void = () => {}
      let closeAgentInteractions: () => void = () => {}
      let closeScheduler: () => void = () => {}
      let closeMedia: () => void = () => {}

      if (previewMode) {
        registerUserHandlers(bridge, currentTappInstance)
        registerFileHandlers(bridge)
        const defaults = currentTappInstance.manifest.settings || []
        for (const setting of defaults) {
          if (
            !previewSettingsRef.current.has(setting.key) &&
            setting.defaultValue !== undefined
          ) {
            previewSettingsRef.current.set(setting.key, setting.defaultValue)
          }
        }
        registerPlaygroundPreviewHandlers(
          bridge,
          currentTappInstance,
          previewStores?.storage ?? previewStorageRef.current,
          previewStores?.settings ?? previewSettingsRef.current,
          currentCode.assets || {},
          previewStores?.shared,
          previewStores?.private,
        )
      } else {
        // 始终挂 Widget 热路径；按授予权限惰性挂重型能力。
        const granted = new Set(
          (currentTappInstance.grantedPermissions || []) as string[],
        )
        const hasExact = (perm: string) => granted.has(perm)
        const hasAi =
          hasExact('ai:generate') ||
          hasExact('ai:analyze') ||
          hasExact('ai:chat') ||
          hasExact('ai:image') ||
          hasExact('ai:search')
        const hasMedia =
          hasExact('media:read') ||
          hasExact('media:control') ||
          hasExact('media:audio')
        const hasSpeech = hasExact('speech:tts') || hasExact('speech:asr')
        const hasEvents =
          hasExact('event:publish') || hasExact('event:subscribe')
        const hasAgent = hasExact('component:agent')
        const hasScheduler = hasExact('scheduler:register')
        const hasPlatform = hasExact('platform:read')
        const hasAnalytics = hasExact('analytics:read')
        const hasReport = hasExact('report:read')

        registerStorageHandlers(bridge, currentTappInstance.id)
        registerUserHandlers(bridge, currentTappInstance)
        registerFileHandlers(bridge)
        registerAssetHandlers(bridge, currentTappInstance)
        registerContextHandlers(bridge, currentTappInstance)
        registerPersonaHandlers(bridge)
        // 共享 core 在 Widget 模式同样执行，必须能声明常驻需求。
        registerBackgroundHandlers(bridge, currentTappInstance)

        // 仅已授予时挂载；后端仍强制 Runtime Grant + 权限。
        closeAITaskStreams = hasAi ? registerAIHandlers(bridge) : () => {}
        if (hasPlatform) {
          registerPlatformHandlers(bridge, currentTappInstance, {
            readOnly: true,
          })
        }
        if (hasAnalytics) {
          registerAnalyticsHandlers(bridge)
        }
        if (hasReport) {
          registerReportHandlers(bridge, currentTappInstance, {
            readOnly: true,
          })
        }
        closeDataExchange = registerDataExchangeHandlers(
          bridge,
          currentTappInstance,
        )
        closeEventStream = hasEvents
          ? registerEventHandlers(bridge, currentTappInstance)
          : () => {}
        closeAgentInteractions = hasAgent
          ? registerAgentInteractionHandlers(bridge, currentTappInstance)
          : () => {}
        closeMedia = hasMedia
          ? registerMediaHandlers(bridge, currentTappInstance)
          : () => {}
        if (hasSpeech) {
          registerSpeechHandlers(bridge, currentTappInstance)
        }
        closeScheduler = hasScheduler
          ? registerSchedulerHandlers(bridge, currentTappInstance)
          : () => {}
      }

      // tapp.ready 须先 allowSandboxEvent。
      bridge.allowSandboxEvent('tapp.ready')
      const unsubscribeReady = bridge.on('tapp.ready', () => {
        handleReady()
      })

      const html = generateWidgetHTML(
        currentTappInstance,
        currentCode,
        widgetId,
        propsForHtml,
        sessionToken,
        {
          missing: t.tapp.widgetNotFound,
          renderFailed: t.tapp.appCodeLoadFailed,
        },
        surface,
        popupParamsRef.current,
      )

      // srcdoc 先赋值，再 appendChild。
      iframe.srcdoc = html
      container.appendChild(iframe)
      bridge.attachSource()
      const onIframeLoad = () => bridge.attachSource()
      iframe.addEventListener('load', onIframeLoad)

      return () => {
        unsubscribeReady()
        closeScheduler()
        closeMedia()
        closeDataExchange()
        closeAITaskStreams()
        closeEventStream()
        closeAgentInteractions()
        iframe.removeEventListener('load', onIframeLoad)
        iframe.removeEventListener('pointerenter', onPointerEnter)
        iframe.removeEventListener('pointerleave', onPointerLeave)
        iframeRef.current = null
        if (container.contains(iframe)) {
          container.removeChild(iframe)
        }
        bridge.destroy()
        bridgeRef.current = null
        setIsReady(false)
      }
    }, [
      tappInstance.id,
      runtimeFingerprint,
      widgetId,
      codeFingerprint,
      handleReady,
      stableWidgetProps,
      previewStores,
      subjectEpoch,
      subject.ready,
      previewMode,
      surface,
      t.tapp.widgetNotFound,
      t.tapp.appCodeLoadFailed,
    ])

    // 弹窗重开带新参数：推给已在跑的沙箱，不重建 iframe。
    const paramsString = surface === 'popup' ? JSON.stringify(popupParams ?? null) : ''
    useEffect(() => {
      if (surface !== 'popup' || !isReady || !bridgeRef.current) return
      bridgeRef.current.emit('popup:params', JSON.parse(paramsString))
    }, [surface, isReady, paramsString])

    useEffect(() => {
      if (!isReady || !bridgeRef.current) return
      bridgeRef.current.emit('locale:change', widgetProps.locale)
    }, [widgetProps.locale, isReady])

    useEffect(() => {
      if (!isReady || !bridgeRef.current) return
      bridgeRef.current.emit('animationLevel:change', animationConfig.level)
    }, [isReady, animationConfig.level])

    useEffect(() => {
      if (!isReady || !iframeRef.current) return

      const widgetDims = calculateWidgetDimensions(
        stableWidgetProps.size,
        dimensions.width,
        dimensions.height,
      )
      sendResizeMessage(iframeRef.current, widgetDims)
    }, [isReady, dimensions, stableWidgetProps.size])

    const themeAccent =
      tappInstance.manifest.themeColor?.trim() ||
      widgetProps.primaryColor ||
      'var(--color-primary, #6366f1)'

    return (
      <div
        ref={containerRef}
        className={`tapp-widget-sandbox ${className || ''}`}
        // 弹窗收回时焦点还到这里，不还进 iframe（见 WidgetPopupHost 的 focusReturnTarget）。
        tabIndex={surface === 'widget' ? -1 : undefined}
        data-focus-return={surface === 'widget' ? '' : undefined}
        style={{
          position: 'relative',
          width: '100%',
          height: '100%',
          borderRadius: 'inherit',
          ...style,
        }}
      >
        <WidgetSkeletonCover
          active={!isReady}
          preset={TAPP_WIDGET_SKELETON.preset}
          deferMs={TAPP_WIDGET_SKELETON.deferMs}
          exitMs={TAPP_WIDGET_SKELETON.exitMs}
          accent={themeAccent}
          stallMessage={readyStalled ? t.common.loadingSlow : undefined}
        />
      </div>
    )
  },
)

export default TappWidgetSandbox
