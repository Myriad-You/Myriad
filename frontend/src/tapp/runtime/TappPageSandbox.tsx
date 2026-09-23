import type { TappCodeStructure, TappInstance } from '../types'
import type { AnimationConfigRef, SafeInsets } from './sandbox'
import type { TappBridge } from './TappBridge'
import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react'

import { useI18n } from '../../contexts/I18nContext'
import { useAnimationLevel } from '../../hooks/useAnimationLevel'
import { emitAppEvent } from '../../utils/appEvents'
import {
  buildTappMediaState,
  mergeMusicPlayerEventDetail,
} from '../../utils/musicPlayerState'
import { useTappSubject } from '../../utils/tappSubject'
import { getIsDarkMode } from '../../utils/themeSubscriber'
import { sendResizeMessage, useIframeResize } from '../utils/iframeResize'
import {
  buildLayerScript,
  getCodeStructureFingerprint,
  getTappRuntimeFingerprint,
} from './codeStructure'
import {
  applySandboxCapabilityProfile,
  cspOptionsFromPermissions,
  escapeSandboxHtmlText,
  escapeSandboxScriptSource,
  generateCSP,
  generateFullSDK,
  generateNonce,
  generateSecurityWrapper,
  generateSessionToken,
  generateThemeCSS,
  IFRAME_SANDBOX_ATTRS,
  PAGE_STATIC_CSS,
  serializeSandboxScriptValue,
} from './sandbox'
import {
  registerAdvancedHandlers,
  registerAgentInteractionHandlers,
  registerAIHandlers,
  registerAnalyticsHandlers,
  registerAnimationHandlers,
  registerAssetHandlers,
  registerBackgroundHandlers,
  registerContextHandlers,
  registerDataExchangeHandlers,
  registerDynamicContentHandlers,
  registerEventHandlers,
  registerFederationHandlers,
  registerFileHandlers,
  registerGameHandlers,
  registerLifecycleHandlers,
  registerMediaHandlers,
  registerModel3dHandlers,
  registerPersonaHandlers,
  registerPhantasiListHandlers,
  registerPlatformHandlers,
  registerReportHandlers,
  registerSchedulerHandlers,
  registerSpeechHandlers,
  registerStorageHandlers,
  registerTappListHandlers,
  registerUIHandlers,
  registerUserHandlers,
  registerWidgetHandlers,
  registerWidgetInvalidateTargetHandler,
} from './sandbox/handlers'
import { registerPlaygroundPreviewHandlers } from './sandbox/handlers/playgroundPreviewHandlers'
import {
  loadHostRuntimeModule,
  manifestRequestsRuntimeModule,
} from './sandbox/hostRuntimeModules'
import { onSpaNavigation } from './spaNavigation'
import { createTappBridge } from './TappBridge'
import { TappRuntimeGrant } from './TappRuntimeGrant'
import { useSandboxSubscriptions } from './useSandboxSubscriptions'
import { bindAllTappKvChanges } from './WidgetRuntimeSignals'

export { isWebKit } from '../../utils/platformDetect'

export interface TappPageSandboxProps {
  tappInstance: TappInstance
  code: TappCodeStructure
  onReady?: () => void
  onError?: (error: Error) => void
  onDestroy?: () => void
  className?: string
  style?: React.CSSProperties
  safeInsets?: SafeInsets
  /** Headless core：无 UI，只跑 core。 */
  headless?: boolean
  /** 会话内 Playground 预览。用生产 iframe/CSP，无 Runtime Grant。UI handlers 仍会挂上。 */
  previewMode?: boolean
  previewStores?: {
    storage: Map<string, unknown>
    settings: Map<string, unknown>
    shared: Map<string, unknown>
    private: Map<string, unknown>
  }
  /** 宿主隐藏此表面（如最小化）。与 document 可见性合成 lifecycle:pause/resume。隐藏保留 iframe，不销毁。 */
  paused?: boolean
}

/** 后台逻辑写在 core，且仅在 _TAPP_MODE === 'core' 时执行。 */
function generateHeadlessCoreHTML(
  tappInstance: TappInstance,
  code: TappCodeStructure,
  sessionToken: string,
  locale: string,
): string {
  const { manifest } = tappInstance
  const nonce = generateNonce()
  const cspOptions = cspOptionsFromPermissions(tappInstance.grantedPermissions)
  const csp = generateCSP(nonce, cspOptions)
  const securityWrapper = escapeSandboxScriptSource(
    // 包装层图片 URL 判断必须与 CSP 同一份选项。
    generateSecurityWrapper(sessionToken, cspOptions.allowRemoteMedia),
  )
  const sdkCode = escapeSandboxScriptSource(
    generateFullSDK(tappInstance, sessionToken, 'headless'),
  )
  const coreCode = escapeSandboxScriptSource(buildLayerScript(code, 'background').source)

  const i18nScript =
    code.i18n && Object.keys(code.i18n).length > 0
      ? `window._TAPP_I18N = ${serializeSandboxScriptValue(code.i18n)};`
      : 'window._TAPP_I18N = {};'

  return `<!DOCTYPE html>
<html class="tapp-mode-core">
<head>
  <meta charset="UTF-8">
  <meta http-equiv="Content-Security-Policy" content="${csp}">
  <title>${escapeSandboxHtmlText(manifest.name)} (core)</title>
</head>
<body>
  <script nonce="${nonce}">
    window._TAPP_MODE = 'core';
    window._TAPP_HAS_HTML = false;
    window._TAPP_HEADLESS = true;
    window._TAPP_LOCALE = ${serializeSandboxScriptValue(locale)};
    window._TAPP_SESSION_TOKEN = ${serializeSandboxScriptValue(sessionToken)};
    ${i18nScript}
  </script>
  <script nonce="${nonce}">${securityWrapper}</script>
  <script nonce="${nonce}">${sdkCode}</script>
  <script nonce="${nonce}">
    (function() {
      'use strict';
      try {
        ${coreCode}
      } catch (error) {
        console.error('[Core] Code error:', error);
        if (window.Tapp && Tapp.lifecycle && Tapp.lifecycle._notifyError) {
          Tapp.lifecycle._notifyError(error);
        }
      }
    })();
  </script>
</body>
</html>`
}

function generatePageHTML(
  tappInstance: TappInstance,
  code: TappCodeStructure,
  sessionToken: string,
  locale: string,
  safeInsets?: SafeInsets,
  launchParams?: Record<string, string>,
  runtimeScripts?: string,
  renderFailed = 'Error',
): string {
  const { manifest } = tappInstance
  const isDark = getIsDarkMode()
  const primaryColor =
    getComputedStyle(document.documentElement)
      .getPropertyValue('--color-primary')
      .trim() || '#94a3b8'

  const nonce = generateNonce()
  const cspOptions = cspOptionsFromPermissions(tappInstance.grantedPermissions)
  const csp = generateCSP(nonce, cspOptions)
  const securityWrapper = escapeSandboxScriptSource(
    // 包装层图片 URL 判断必须与 CSP 同一份选项。
    generateSecurityWrapper(sessionToken, cspOptions.allowRemoteMedia),
  )
  const sdkCode = escapeSandboxScriptSource(
    generateFullSDK(tappInstance, sessionToken),
  )
  const themeCSS = generateThemeCSS(isDark, primaryColor)

  const customCSS = code.styles || ''

  const hasHtmlTemplate = !!code.pageHtml
  const pageHtmlContent = code.pageHtml || ''

  const hasLayeredStructure =
    pageHtmlContent.includes('id="tapp-background"') ||
    pageHtmlContent.includes("id='tapp-background'") ||
    pageHtmlContent.includes('id="tapp-content"') ||
    pageHtmlContent.includes("id='tapp-content'")

  const pagePlan = buildLayerScript(code, 'page')
  const pageCode = pagePlan.source

  const loadedModulesScript = `window._TAPP_LOADED_MODULES = ${serializeSandboxScriptValue(pagePlan.includedModules)};`

  const i18nScript =
    code.i18n && Object.keys(code.i18n).length > 0
      ? `window._TAPP_I18N = ${serializeSandboxScriptValue(code.i18n)};`
      : 'window._TAPP_I18N = {};'

  const tailwindCSS = code.pageCSS || ''

  const needsJsRender = !hasHtmlTemplate

  const initialPadding = `${safeInsets?.top ?? 0}px ${safeInsets?.right ?? 0}px ${safeInsets?.bottom ?? 0}px ${safeInsets?.left ?? 0}px`

  const bodyContent = hasLayeredStructure
    ? `<div id="tapp-root">${pageHtmlContent}</div>`
    : `<div id="tapp-root">
    <div id="tapp-background"></div>
    <div id="tapp-content">${pageHtmlContent}</div>
  </div>`

  return `<!DOCTYPE html>
<html class="tapp-mode-page">
<head>
  <meta charset="UTF-8">
  <meta http-equiv="Content-Security-Policy" content="${csp}">
  <meta name="viewport" content="width=device-width, initial-scale=1.0, maximum-scale=1.0, user-scalable=no">
  <title>${escapeSandboxHtmlText(manifest.name)}</title>
  <style>
    ${PAGE_STATIC_CSS}
    ${tailwindCSS}
    ${themeCSS}
    ${customCSS}
    /* 初始安全区域 padding - 确保全屏模式下内容不被遮挡 */
    #tapp-content { padding: ${initialPadding}; box-sizing: border-box; }
  </style>
</head>
<body class="${isDark ? 'dark' : 'light'}">
  ${bodyContent}

  <script nonce="${nonce}">
    window._TAPP_MODE = 'page';
    window._TAPP_LAUNCH_PARAMS = ${serializeSandboxScriptValue(launchParams || {})};
    window._TAPP_HAS_HTML = ${hasHtmlTemplate};
    ${loadedModulesScript}
    window._TAPP_LOCALE = ${serializeSandboxScriptValue(locale)};
    window._TAPP_SESSION_TOKEN = ${serializeSandboxScriptValue(sessionToken)};
    ${i18nScript}
    window._TAPP_INITIAL_SAFE_INSETS = {
      top: ${safeInsets?.top ?? 0},
      right: ${safeInsets?.right ?? 0},
      bottom: ${safeInsets?.bottom ?? 0},
      left: ${safeInsets?.left ?? 0}
    };
    window._TAPP_DIMENSIONS = { width: 0, height: 0, scale: 1, fontScale: 1 };
    window.addEventListener('message', function(e) {
      const msg = e.data;
      if (msg?.type === 'event' && msg.action === 'container:resize') {
        window._TAPP_DIMENSIONS = msg.payload;
        const root = document.documentElement;
        root.style.setProperty('--tapp-scale', msg.payload.scale || 1);
        root.style.setProperty('--tapp-font-scale', msg.payload.fontScale || 1);
        const content = document.getElementById('tapp-content');
        if (content) {
          content.style.padding =
            (msg.payload.safeInsetTop || 0) + 'px ' +
            (msg.payload.safeInsetRight || 0) + 'px ' +
            (msg.payload.safeInsetBottom || 0) + 'px ' +
            (msg.payload.safeInsetLeft || 0) + 'px';
        }
        window.dispatchEvent(new CustomEvent('tapp:resize', { detail: msg.payload }));
      }
    });
  </script>

  <script nonce="${nonce}">${securityWrapper}</script>
  <script nonce="${nonce}">${sdkCode}</script>
  ${runtimeScripts ? `<script nonce="${nonce}">${runtimeScripts}</script>` : ''}

  <script nonce="${nonce}">
    (function() {
      'use strict';
      console.log('[Tapp] Page modules: ' + (window._TAPP_LOADED_MODULES || []).join(', '));
      try {
        ${escapeSandboxScriptSource(pageCode)}
      } catch (error) {
        console.error('[Page] Code error:', error);
        Tapp.lifecycle._notifyError(error);
      }
    })();
  </script>

  ${
    needsJsRender
      ? `
  <script nonce="${nonce}">
    (function() {
      'use strict';
      setTimeout(function() {
        try {
          const pageKeys = Object.keys(Tapp.pages || {});
          if (pageKeys.length > 0) {
            const pageId = pageKeys[0];
            const pageDef = Tapp.pages[pageId];
            if (pageDef && typeof pageDef.render === 'function') {
              const container = document.getElementById('tapp-content');
              container.innerHTML = '';
              pageDef.render(container, {});
            }
          }
        } catch (error) {
          console.error('[Page] Render error:', error);
          Tapp.lifecycle._notifyError(error);
          document.getElementById('tapp-content').innerHTML =
            '<div class="tapp-empty tapp-text-error">' + ${serializeSandboxScriptValue(renderFailed)} + '</div>';
        }
      }, 50);
    })();
  </script>
  `
      : ''
  }
</body>
</html>`
}

export const TappPageSandbox: React.FC<TappPageSandboxProps> = ({
  tappInstance,
  code,
  onReady,
  onError,
  onDestroy,
  className,
  style,
  safeInsets,
  headless = false,
  previewMode = false,
  paused = false,
  previewStores,
}) => {
  const iframeRef = useRef<HTMLIFrameElement>(null)
  const bridgeRef = useRef<TappBridge | null>(null)
  const [isReady, setIsReady] = useState(false)
  /** Bumps when host identity settles after login/logout so iframe remounts. */
  const subject = useTappSubject()
  const subjectEpoch = subject.epoch
  const previewStorageRef = useRef(new Map<string, unknown>())
  const previewSettingsRef = useRef(new Map<string, unknown>())

  const { containerRef, dimensions } = useIframeResize<HTMLDivElement>()
  const { locale, t } = useI18n()
  // 后台 headless 挂在 App 根，不经过 I18nNamespace；chrome 没有 tapp 文案。
  const cannotLoadApp = headless ? undefined : t.tapp.cannotLoadApp
  const animationConfig = useAnimationLevel()

  // 对象引用放 ref，避免 deps 重建 iframe
  const tappInstanceRef = useRef(tappInstance)
  const codeRef = useRef(code)
  const safeInsetsRef = useRef(safeInsets)
  const pausedRef = useRef(paused)
  tappInstanceRef.current = tappInstance
  codeRef.current = code
  safeInsetsRef.current = safeInsets
  pausedRef.current = paused

  // Visibility + host minimize/paused composed into one lifecycle stream;
  // also theme / primary-color subscriptions (shared with widget sandbox).
  useSandboxSubscriptions(bridgeRef, isReady, paused)

  useEffect(
    () =>
      bindAllTappKvChanges(() => bridgeRef.current, tappInstance.id),
    [tappInstance.id],
  )

  // code/runtime 指纹：iframe remount 的输入之一。
  const codeFingerprint = useMemo(
    () => getCodeStructureFingerprint(code, headless ? 'background' : 'page'),
    [code, headless],
  )
  const runtimeFingerprint = getTappRuntimeFingerprint(tappInstance)

  const localeRef = useRef(locale)
  useEffect(() => {
    localeRef.current = locale
  }, [locale])

  const animationConfigRef = useRef<AnimationConfigRef>(animationConfig)
  useEffect(() => {
    animationConfigRef.current = animationConfig
  }, [animationConfig])

  useEffect(() => {
    if (!iframeRef.current || dimensions.width === 0) return
    const dims = {
      ...dimensions,
      safeInsetTop: safeInsets?.top ?? 0,
      safeInsetRight: safeInsets?.right ?? 0,
      safeInsetBottom: safeInsets?.bottom ?? 0,
      safeInsetLeft: safeInsets?.left ?? 0,
    }
    sendResizeMessage(iframeRef.current, dims)
  }, [dimensions, safeInsets])

  useEffect(() => {
    if (!bridgeRef.current || !isReady) return
    bridgeRef.current.emit('locale:change', locale)
  }, [locale, isReady])

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
      // 部分派发缺字段时用全局态兜底；切歌时禁止串曲歌词/进度
      const globalState =
        (window as { __musicPlayerState?: Record<string, unknown> })
          .__musicPlayerState || {}
      const merged = mergeMusicPlayerEventDetail(globalState, detail)
      bridgeRef.current.emit('mediaStateChange', buildMediaState(merged))
    }

    // 先注册监听，再触发同步（确保不会错过同步事件）
    window.addEventListener('music-player-state-change', handleMusicStateChange)

    // Tapp 就绪时立即推送当前音乐状态（解决初始化竞态）
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
      // 全局状态尚未初始化，触发同步请求（监听器已就位，会收到结果）
      emitAppEvent('request-music-state-sync')
    }

    // SDK 监听器就绪后再推一次
    const retryTimer = setTimeout(pushCurrentState, 150)

    return () => {
      clearTimeout(retryTimer)
      window.removeEventListener(
        'music-player-state-change',
        handleMusicStateChange,
      )
    }
  }, [isReady])

  // 媒体进度使用轻量事件单独推送，避免每个 tick 重发完整状态。
  useEffect(() => {
    if (!isReady) return

    const handleProgress = (e: Event) => {
      const bridge = bridgeRef.current
      if (!bridge) return

      const tapp = tappInstanceRef.current
      if (!tapp?.grantedPermissions?.includes('media:read')) return

      const { currentTime, audioDuration, songId } = (e as CustomEvent).detail
      // 丢弃与当前曲目不一致的进度（快速切歌时旧 timeupdate 可能晚到）
      if (songId != null) {
        const globalState =
          (window as { __musicPlayerState?: Record<string, unknown> })
            .__musicPlayerState || {}
        const currentId = (
          globalState.currentSong as { id?: string | number } | null | undefined
        )?.id
        if (currentId != null && String(currentId) !== String(songId)) {
          return
        }
      }
      const progress = {
        current: currentTime,
        duration: audioDuration,
        percentage: audioDuration > 0 ? (currentTime / audioDuration) * 100 : 0,
      }

      bridge.emit('mediaProgress', progress)
    }

    window.addEventListener('music-player-progress', handleProgress)
    return () => {
      window.removeEventListener('music-player-progress', handleProgress)
    }
  }, [isReady])

  useEffect(() => {
    if (!isReady) return
    bridgeRef.current?.emit('animationLevel:change', animationConfig.level)
  }, [isReady, animationConfig.level])

  const handleReady = useCallback(() => {
    setIsReady(true)
    onReady?.()
  }, [onReady])

  const handleError = useCallback(
    (error: Error) => {
      onError?.(error)
    },
    [onError],
  )

  // Safari：imperative iframe。已挂载的 sandboxed iframe 不会因 srcdoc 变更重绘。
  useEffect(() => {
    if (!previewMode && !subject.ready) return
    const container = containerRef.current
    if (!container) return

    // 从 ref 获取当前对象，避免闭包陈旧问题
    const currentTappInstance = tappInstanceRef.current
    const currentCode = codeRef.current

    // 生成 session token（独立于 Bridge，确保 HTML 生成和 Bridge 使用同一 token）
    const sessionToken = generateSessionToken()

    const iframe = document.createElement('iframe')
    iframe.className = 'tapp-page-iframe'
    iframe.setAttribute('sandbox', IFRAME_SANDBOX_ATTRS)
    iframe.setAttribute('referrerpolicy', 'no-referrer')
    iframe.title = currentTappInstance.manifest.name
    iframe.allowFullscreen = true
    iframeRef.current = iframe

    // DOM 插入前挂消息监听
    const bridge = createTappBridge()
    bridgeRef.current = bridge

    const runtimeGrant = previewMode
      ? undefined
      : new TappRuntimeGrant(
          currentTappInstance.id,
          `${headless ? 'headless' : 'page'}_${sessionToken.slice(0, 32)}`,
          headless ? 'headless' : 'page',
        )

    const instanceForBridge = previewMode
      ? { ...currentTappInstance, previewMode: true }
      : currentTappInstance
    bridge.initialize(iframe, instanceForBridge, sessionToken, runtimeGrant)
    // Apply current minimize/paused state (effect may have run before bridge existed).
    bridge.setSurfaceActive(!pausedRef.current)

    registerLifecycleHandlers(
      bridge,
      currentTappInstance,
      handleReady,
      handleError,
    )
    registerUIHandlers(
      bridge,
      currentTappInstance,
      () => localeRef.current,
      { headless },
    )
    registerUserHandlers(bridge, currentTappInstance)
    if (!headless) registerFileHandlers(bridge)
    registerAnimationHandlers(bridge, animationConfigRef)

    const cleanups: (() => void)[] = []
    if (previewMode) {
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
      registerWidgetInvalidateTargetHandler(bridge, currentTappInstance, {
        preview: true,
      })
    } else {
      // Always mount the hot path; gate heavy optional capabilities by
      // grantedPermissions (same pattern as TappWidgetSandbox).
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
      const hasPlatform = hasExact('platform:read') || hasExact('platform:write')
      const hasAnalytics = hasExact('analytics:read')
      const hasReport = hasExact('report:read')
      const hasPhantasi =
        hasExact('phantasi:read') ||
        hasExact('phantasi:write') ||
        hasExact('phantasi:commentWrite') ||
        hasExact('phantasi:manage')
      const hasFederation =
        hasExact('federation:read') ||
        hasExact('federation:post') ||
        hasExact('federation:interact') ||
        hasExact('federation:channel') ||
        hasExact('federation:room') ||
        hasExact('federation:ring') ||
        hasExact('federation:message') ||
        hasExact('federation:trust') ||
        hasExact('federation:files')
      const hasTappList =
        hasExact('tappList:read') || hasExact('tappList:manage')
      // dynamicContent.set/update/remove require ui:notification; get is public
      // but only useful after set — gate the whole surface with the write perm.
      const hasDynamicContent = hasExact('ui:notification')
      const hasAdvanced =
        hasExact('component:theme') ||
        hasExact('component:agent') ||
        hasExact('shortcut:register')

      registerStorageHandlers(bridge, currentTappInstance.id)
      registerAssetHandlers(bridge, currentTappInstance)
      if (!headless) registerWidgetHandlers(bridge, currentTappInstance)
      registerWidgetInvalidateTargetHandler(bridge, currentTappInstance)
      if (hasPlatform) {
        registerPlatformHandlers(bridge, currentTappInstance)
      }
      if (hasAnalytics) {
        registerAnalyticsHandlers(bridge)
      }
      if (!headless && hasTappList) {
        registerTappListHandlers(bridge, currentTappInstance)
      }
      if (hasPhantasi) {
        registerPhantasiListHandlers(bridge, currentTappInstance)
      }
      const closeAITaskStreams = hasAi ? registerAIHandlers(bridge) : () => {}
      if (!headless) {
        registerModel3dHandlers(bridge)
      }
      if (hasReport) {
        registerReportHandlers(bridge, currentTappInstance)
      }
      const closeMedia = hasMedia
        ? registerMediaHandlers(bridge, currentTappInstance)
        : () => {}
      if (hasSpeech) {
        registerSpeechHandlers(bridge, currentTappInstance)
      }
      registerBackgroundHandlers(bridge, currentTappInstance)
      const closeScheduler = hasScheduler
        ? registerSchedulerHandlers(bridge, currentTappInstance)
        : () => {}
      if (!headless && hasDynamicContent) {
        registerDynamicContentHandlers(bridge, currentTappInstance)
      }
      const closeAdvanced = hasAdvanced
        ? registerAdvancedHandlers(bridge, currentTappInstance)
        : () => {}
      const closeFederationSockets = hasFederation
        ? registerFederationHandlers(bridge, currentTappInstance)
        : () => {}
      if (hasExact('game:session') && hasFederation) {
        registerGameHandlers(bridge, currentTappInstance)
      }
      registerContextHandlers(bridge, currentTappInstance)
      registerPersonaHandlers(bridge)
      // data-exchange stays public (consent host + broker still enforce)
      const closeDataExchange = registerDataExchangeHandlers(
        bridge,
        currentTappInstance,
      )
      const closeEventStream = hasEvents
        ? registerEventHandlers(bridge, currentTappInstance)
        : () => {}
      const closeAgentInteractions = hasAgent
        ? registerAgentInteractionHandlers(bridge, currentTappInstance)
        : () => {}
      cleanups.push(
        closeAdvanced,
        closeFederationSockets,
        closeScheduler,
        closeMedia,
        closeDataExchange,
        closeAITaskStreams,
        closeEventStream,
        closeAgentInteractions,
      )
      applySandboxCapabilityProfile(bridge, headless ? 'headless' : 'page')
    }

    const launchParams: Record<string, string> = {}
    try {
      const sp = new URLSearchParams(window.location.search)
      sp.forEach((v, k) => {
        launchParams[k] = v
      })
    } catch {
      /* ignore */
    }

    let cancelled = false
    const detachIframe = () => {
      container.style.pointerEvents = ''
      if (container.contains(iframe)) {
        container.removeChild(iframe)
      }
    }
    cleanups.push(detachIframe)
    const mount = async () => {
      let runtimeScripts = ''
      if (
        !headless &&
        manifestRequestsRuntimeModule(
          currentTappInstance.manifest.runtimeModules,
          'three',
        )
      ) {
        try {
          runtimeScripts = await loadHostRuntimeModule('three')
        } catch (error) {
          console.error('[Tapp] host Three runtime failed', error)
        }
      }
      if (cancelled) return

      const html = headless
        ? generateHeadlessCoreHTML(
            currentTappInstance,
            currentCode,
            sessionToken,
            localeRef.current,
          )
        : generatePageHTML(
            currentTappInstance,
            currentCode,
            sessionToken,
            localeRef.current,
            safeInsetsRef.current,
            launchParams,
            runtimeScripts
              ? escapeSandboxScriptSource(runtimeScripts)
              : undefined,
            cannotLoadApp ?? 'Error',
          )
      if (cancelled) return

      iframe.style.cssText =
        'position:absolute;inset:0;width:100%;height:100%;border:none;display:block;overflow:hidden;border-bottom-left-radius:0.75rem;border-bottom-right-radius:0.75rem;pointer-events:auto;touch-action:manipulation;-webkit-tap-highlight-color:transparent;'
      container.style.pointerEvents = 'auto'
      if (!container.contains(iframe)) {
        container.appendChild(iframe)
      }
      // appendChild 后再赋 srcdoc。
      iframe.srcdoc = html
      if (cancelled) {
        detachIframe()
        return
      }
      bridge.attachSource()
      const onIframeLoad = () => {
        if (!cancelled) bridge.attachSource()
      }
      iframe.addEventListener('load', onIframeLoad)
      cleanups.push(() => iframe.removeEventListener('load', onIframeLoad))
    }
    void mount()

    return () => {
      cancelled = true
      cleanups.forEach((fn) => fn())
      setIsReady(false)
      iframeRef.current = null
      bridge.destroy()
      onDestroy?.()
    }
    // remount 看下方 deps，不只 id/指纹；safeInsets 走 ref + postMessage
  }, [
    tappInstance.id,
    runtimeFingerprint,
    codeFingerprint,
    handleReady,
    headless,
    previewMode,
    previewStores,
    subjectEpoch,
    subject.ready,
    cannotLoadApp,
  ])

  // When already open (Aro etc.), React Router query changes must refresh
  // launchParams — they are baked into srcdoc only at iframe create time.
  useEffect(() => {
    if (!isReady || headless) return

    const syncLaunchParams = () => {
      const iframe = iframeRef.current
      const bridge = bridgeRef.current
      if (!iframe?.contentWindow) return

      const launchParams: Record<string, string> = {}
      try {
        const sp = new URLSearchParams(window.location.search)
        sp.forEach((v, k) => {
          launchParams[k] = v
        })
      } catch {
        return
      }

      try {
        ;(
          iframe.contentWindow as Window & {
            _TAPP_LAUNCH_PARAMS?: Record<string, string>
          }
        )._TAPP_LAUNCH_PARAMS = launchParams
      } catch {
        /* ignore */
      }

      // Event name matches host→sandbox convention (camelCase event action)
      bridge?.emit('launchParamsChange', launchParams)
    }

    syncLaunchParams()
    return onSpaNavigation(syncLaunchParams)
  }, [isReady, headless])

  return (
    <div
      ref={containerRef}
      className={`tapp-page-sandbox ${className || ''}`}
      style={{
        position: 'absolute',
        top: 0,
        right: 0,
        bottom: 0,
        left: 0,
        ...style,
      }}
      data-no-ripple
    >
    </div>
  )
}

export default TappPageSandbox
