/**
 * Tapp Page 沙箱组件
 *
 * 用于渲染 Tapp 的页面模式（全屏应用）
 */

import type { TappCodeStructure, TappInstance } from '../types'
import type { AnimationConfigRef, SafeInsets } from './sandbox'
import type { TappBridge } from './TappBridge'

import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { useAnimationLevel } from '../../hooks/useAnimationLevel'
import {
  buildTappMediaState,
  mergeMusicPlayerEventDetail,
} from '../../utils/musicPlayerState'
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
  registerBrewListHandlers,
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
  registerPlatformHandlers,
  registerReportHandlers,
  registerSchedulerHandlers,
  registerSpeechHandlers,
  registerStorageHandlers,
  registerTappListHandlers,
  registerUIHandlers,
  registerUserHandlers,
  registerWidgetHandlers,
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
import { onTappSharedChange, onTappStorageChange } from './WidgetRuntimeSignals'

// 核心模块

// 处理器

// WebKit/Safari 引擎检测：统一走 platformDetect（与 OS/硬件档位共用 UA 基础）
// 供运行页工具栏 portal、禁用多窗口等宿主决策使用。
export { isWebKit } from '../../utils/platformDetect'

export interface TappPageSandboxProps {
  /** Tapp 实例 */
  tappInstance: TappInstance
  /** Tapp 代码 */
  code: TappCodeStructure
  /** 准备就绪回调 */
  onReady?: () => void
  /** 错误回调 */
  onError?: (error: Error) => void
  /** 销毁回调 */
  onDestroy?: () => void
  /** 自定义类名 */
  className?: string
  /** 自定义样式 */
  style?: React.CSSProperties
  /** 安全区域内边距 */
  safeInsets?: SafeInsets
  /**
   * Headless "core" 模式：不渲染任何 UI，只运行 core（大脑）代码。
   * 用于 background.require 声明的后台运行，取代过去在后台隐形挂一整页的做法。
   */
  headless?: boolean
  /**
   * Session-local Playground preview. It uses the production iframe/CSP shell,
   * but never issues a backend Runtime Grant or registers host-mutating APIs.
   */
  previewMode?: boolean
  /**
   * When true, host has hidden this surface (e.g. multi-window minimize).
   * Composed with document visibility into a single lifecycle:pause/resume
   * stream (see useSandboxSubscriptions). Minimize intentionally keeps the
   * sandbox/iframe alive so restore is instant; only work is frozen via
   * lifecycle events (no teardown).
   */
  paused?: boolean
}

/**
 * 生成 headless "core" 沙箱 HTML
 *
 * 无 UI、无 pageHtml、无 page 模块，只运行 core（大脑）代码，
 * body 为空。用于 background.require 声明的后台运行——取代过去在后台
 * 隐形挂一整页（含完整 DOM）的做法，把后台实例从「整页」降到「无头 JS」。
 *
 * 契约：需要后台运行的逻辑（拉数据/轮询/调度）应写在 core 里，并在
 * `window._TAPP_MODE === 'core'` 时执行；UI（page/widget）作为纯视图订阅。
 *
 * 复用与 page 相同的 CSP nonce / 安全包装器 / SDK，确保 core 能正常使用
 * storage / federation / scheduler 等能力。
 */
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
    // 包装层的图片 URL 判断必须与 CSP 用同一份选项，否则提示与实际拦截会脱节
    generateSecurityWrapper(sessionToken, cspOptions.allowRemoteMedia),
  )
  const sdkCode = escapeSandboxScriptSource(
    generateFullSDK(tappInstance, sessionToken, 'headless'),
  )
  // 'background' 模式即返回纯 code.core（无 page/widget UI 代码）
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

/**
 * 生成 Page 沙箱 HTML
 *
 * 支持三种渲染方式：
 * 1. 纯 JS 模式：Tapp.pages[id].render(container, props)
 * 2. 纯 HTML 模式：pageHtml 直接渲染（适合静态页面）
 * 3. 混合模式：pageHtml 定义结构 + JS 处理交互（性能最优）
 *
 * 🔒 安全特性：
 * - 使用 CSP nonce 替代 unsafe-inline，只有带正确 nonce 的脚本才能执行
 * - 安全包装器禁用危险 API（eval, Function 等）
 *
 * @param tappInstance - Tapp 实例
 * @param code - Tapp 代码结构
 * @param sessionToken - 会话 token（用于消息验证）
 * @param safeInsets - 安全区域内边距
 */
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

  // 生成唯一 nonce（每个沙箱实例独立）
  const nonce = generateNonce()
  const cspOptions = cspOptionsFromPermissions(tappInstance.grantedPermissions)
  const csp = generateCSP(nonce, cspOptions)
  const securityWrapper = escapeSandboxScriptSource(
    // 包装层的图片 URL 判断必须与 CSP 用同一份选项，否则提示与实际拦截会脱节
    generateSecurityWrapper(sessionToken, cspOptions.allowRemoteMedia),
  )
  const sdkCode = escapeSandboxScriptSource(
    generateFullSDK(tappInstance, sessionToken),
  )
  const themeCSS = generateThemeCSS(isDark, primaryColor)

  // 自定义 CSS
  const customCSS = code.styles || ''

  // HTML 模板（如果有）
  const hasHtmlTemplate = !!code.pageHtml
  const pageHtmlContent = code.pageHtml || ''

  // 检测 pageHtml 是否已经包含分层结构
  // 如果包含 #tapp-background 或 #tapp-content，说明 Tapp 自己定义了分层
  const hasLayeredStructure =
    pageHtmlContent.includes('id="tapp-background"') ||
    pageHtmlContent.includes("id='tapp-background'") ||
    pageHtmlContent.includes('id="tapp-content"') ||
    pageHtmlContent.includes("id='tapp-content'")

  // JS 代码 - 混合模式下也会加载。core 先于 page 层执行。
  const pagePlan = buildLayerScript(code, 'page')
  const pageCode = pagePlan.source

  // 已装入的模块清单（用于调试和验证）
  const loadedModulesScript = `window._TAPP_LOADED_MODULES = ${serializeSandboxScriptValue(pagePlan.includedModules)};`

  // i18n 注入脚本
  const i18nScript =
    code.i18n && Object.keys(code.i18n).length > 0
      ? `window._TAPP_I18N = ${serializeSandboxScriptValue(code.i18n)};`
      : 'window._TAPP_I18N = {};'

  // 使用安装时预编译的 CSS
  const tailwindCSS = code.pageCSS || ''

  // 是否需要调用 Tapp.pages.render()
  // 仅在没有 HTML 模板时才需要（纯 JS 模式）
  const needsJsRender = !hasHtmlTemplate

  // 初始安全区域 padding（确保首次渲染就有正确的间距）
  const initialPadding = `${safeInsets?.top ?? 0}px ${safeInsets?.right ?? 0}px ${safeInsets?.bottom ?? 0}px ${safeInsets?.left ?? 0}px`

  // 根据是否有分层结构决定 body 内容
  // - 有分层：直接使用 pageHtmlContent（已包含 #tapp-background 和 #tapp-content）
  // - 无分层：用默认结构包装
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
      var msg = e.data;
      if (msg?.type === 'event' && msg.action === 'container:resize') {
        window._TAPP_DIMENSIONS = msg.payload;
        var root = document.documentElement;
        root.style.setProperty('--tapp-scale', msg.payload.scale || 1);
        root.style.setProperty('--tapp-font-scale', msg.payload.fontScale || 1);
        var content = document.getElementById('tapp-content');
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

  <!-- JS 代码始终加载（用于事件绑定等） -->
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
  <!-- 纯 JS 模式：调用 render 函数 -->
  <script nonce="${nonce}">
    (function() {
      'use strict';
      setTimeout(function() {
        try {
          var pageKeys = Object.keys(Tapp.pages || {});
          if (pageKeys.length > 0) {
            var pageId = pageKeys[0];
            var pageDef = Tapp.pages[pageId];
            if (pageDef && typeof pageDef.render === 'function') {
              var container = document.getElementById('tapp-content');
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
      : '<!-- 混合/HTML 模式：HTML 已渲染，JS 用于交互 -->'
  }
</body>
</html>`
}

/**
 * Tapp Page 沙箱组件
 */
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
}) => {
  const iframeRef = useRef<HTMLIFrameElement>(null)
  const bridgeRef = useRef<TappBridge | null>(null)
  const [isReady, setIsReady] = useState(false)
  /** Bumps when host identity settles after login/logout so iframe remounts. */
  const [subjectEpoch, setSubjectEpoch] = useState(0)
  const previewStorageRef = useRef(new Map<string, unknown>())
  const previewSettingsRef = useRef(new Map<string, unknown>())

  useEffect(() => {
    if (previewMode) return
    const onSubjectReady = () => setSubjectEpoch((n) => n + 1)
    window.addEventListener('tapp-subject-ready', onSubjectReady)
    return () => window.removeEventListener('tapp-subject-ready', onSubjectReady)
  }, [previewMode])

  const { containerRef, dimensions } = useIframeResize<HTMLDivElement>()
  const { locale, t } = useI18n()
  const animationConfig = useAnimationLevel()

  // 使用 ref 存储对象引用，避免依赖变化触发 iframe 重建
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

  // Host keyboard shortcuts skip minimized / paused surfaces.
  // Init path also applies pausedRef (this effect may run before bridge exists).
  useEffect(() => {
    bridgeRef.current?.setSurfaceActive(!paused)
  }, [paused, isReady])

  // 同一 Tapp 的其他 Page、headless core 或 Widget 修改 storage 时通知本沙箱。
  useEffect(
    () =>
      onTappStorageChange((change) => {
        const bridge = bridgeRef.current
        if (
          !bridge ||
          change.tappId !== tappInstance.id ||
          change.source === bridge
        ) {
          return
        }
        bridge.emit('storageChanged', {
          key: change.key,
          operation: change.operation,
        })
      }),
    [tappInstance.id],
  )

  useEffect(
    () =>
      onTappSharedChange((change) => {
        const bridge = bridgeRef.current
        if (
          !bridge ||
          change.tappId !== tappInstance.id ||
          change.source === bridge
        ) {
          return
        }
        bridge.emit('sharedChanged', {
          key: change.key,
          operation: change.operation,
        })
      }),
    [tappInstance.id],
  )

  // 生成稳定的代码指纹，只有代码实际变化时才重建 iframe
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

  // 尺寸更新
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

  // 语言变化
  useEffect(() => {
    if (!bridgeRef.current || !isReady) return
    bridgeRef.current.emit('locale:change', locale)
  }, [locale, isReady])

  // 构建媒体状态对象（供 mediaStateChange 事件使用）— 与 Widget 共用纯函数
  const buildMediaState = useCallback((detail: Record<string, unknown>) => {
    return buildTappMediaState(detail)
  }, [])

  // 媒体状态变化 - 转发给 Tapp 沙箱
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
      // 直接从全局状态构建并推送
      bridge.emit('mediaStateChange', buildMediaState(currentGlobalState))
    } else {
      // 全局状态尚未初始化，触发同步请求（监听器已就位，会收到结果）
      window.dispatchEvent(new CustomEvent('request-music-state-sync'))
    }

    // 延迟重推：确保 iframe SDK 消息监听器就绪后再推一次
    // 解决初始推送早于 SDK 初始化的竞态
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

  // 动画级别变化
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

  // 初始化
  // 依赖优化：只使用稳定的 ID 和指纹，不使用对象引用
  // Safari 兼容：使用 imperative iframe 创建，确保 srcdoc 在 DOM 插入前设置
  // Safari/WebKit 不会重新渲染已挂载的 sandboxed iframe 的 srcdoc 变更
  useEffect(() => {
    const container = containerRef.current
    if (!container) return

    // 从 ref 获取当前对象，避免闭包陈旧问题
    const currentTappInstance = tappInstanceRef.current
    const currentCode = codeRef.current

    // 生成 session token（独立于 Bridge，确保 HTML 生成和 Bridge 使用同一 token）
    const sessionToken = generateSessionToken()

    // 创建 iframe
    const iframe = document.createElement('iframe')
    iframe.className = 'tapp-page-iframe'
    iframe.setAttribute('sandbox', IFRAME_SANDBOX_ATTRS)
    iframe.setAttribute('referrerpolicy', 'no-referrer')
    iframe.title = currentTappInstance.manifest.name
    iframe.allowFullscreen = true
    iframeRef.current = iframe

    // 初始化 Bridge（在 DOM 插入前设置消息监听）
    const bridge = createTappBridge()
    bridgeRef.current = bridge

    const runtimeGrant = previewMode
      ? undefined
      : new TappRuntimeGrant(
          currentTappInstance.id,
          `${headless ? 'headless' : 'page'}_${sessionToken.slice(0, 32)}`,
          headless ? 'headless' : 'page',
        )

    bridge.initialize(iframe, currentTappInstance, sessionToken, runtimeGrant)
    // Apply current minimize/paused state (effect may have run before bridge existed).
    bridge.setSurfaceActive(!pausedRef.current)

    // 注册所有处理器
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
        previewStorageRef.current,
        previewSettingsRef.current,
      )
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
        hasExact('ai:image')
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
      const hasBrew =
        hasExact('brew:read') ||
        hasExact('brew:write') ||
        hasExact('brew:commentWrite') ||
        hasExact('brew:manage')
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
      if (hasPlatform) {
        registerPlatformHandlers(bridge, currentTappInstance)
      }
      if (hasAnalytics) {
        registerAnalyticsHandlers(bridge)
      }
      if (!headless && hasTappList) {
        registerTappListHandlers(bridge, currentTappInstance)
      }
      if (hasBrew) {
        registerBrewListHandlers(bridge, currentTappInstance)
      }
      const closeAITaskStreams = hasAi ? registerAIHandlers(bridge) : () => {}
      if (!headless) {
        registerModel3dHandlers(bridge)
      }
      if (hasReport) {
        registerReportHandlers(bridge, currentTappInstance)
      }
      if (hasMedia) {
        registerMediaHandlers(bridge, currentTappInstance)
      }
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
        closeDataExchange,
        closeAITaskStreams,
        closeEventStream,
        closeAgentInteractions,
      )
      applySandboxCapabilityProfile(bridge, headless ? 'headless' : 'page')
    }

    // 收集 URL 启动参数传递给沙箱
    const launchParams: Record<string, string> = {}
    try {
      const sp = new URLSearchParams(window.location.search)
      sp.forEach((v, k) => {
        launchParams[k] = v
      })
    } catch (_) {
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
            t.tapp.cannotLoadApp,
          )
      if (cancelled) return

      iframe.style.cssText =
        'position:absolute;inset:0;width:100%;height:100%;border:none;display:block;overflow:hidden;border-bottom-left-radius:0.75rem;border-bottom-right-radius:0.75rem;pointer-events:auto;touch-action:manipulation;-webkit-tap-highlight-color:transparent;'
      container.style.pointerEvents = 'auto'
      if (!container.contains(iframe)) {
        container.appendChild(iframe)
      }
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
    // 稳定依赖：只有这些真正改变时才重建 iframe
    // - tappInstance.id: Tapp 实例 ID
    // - codeFingerprint: 代码指纹（内容变化才会变）
    // 注意：safeInsets 通过 ref 获取，不作为依赖（通过 postMessage 动态更新）
  }, [
    tappInstance.id,
    runtimeFingerprint,
    codeFingerprint,
    handleReady,
    headless,
    previewMode,
    subjectEpoch,
    t.tapp.cannotLoadApp,
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
        // srcdoc sandbox is same-document accessible for this assignment
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
      {/* iframe 内联挂载到本容器（全浏览器一致） */}
    </div>
  )
}

export default TappPageSandbox
