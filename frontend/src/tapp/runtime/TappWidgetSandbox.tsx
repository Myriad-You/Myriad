/**
 * Tapp Widget 沙箱组件
 *
 * 用于渲染 Tapp 的小组件模式（Dashboard 中的 Widget）
 *
 * 🎯 设计目标：
 * - 专为 Widget 渲染优化，结构简单
 * - 开发者友好：容器有正确尺寸，直接渲染即可
 * - 高性能：最小化 API，减少不必要的开销
 * - 编辑模式支持：正确处理拖拽交互
 * - 响应式主题：实时响应主题和主色调变化
 */

import type { TappCodeStructure, TappInstance } from '../types'
import type { WidgetRenderProps } from './sandbox'
import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import {
  getCodeForMode,
  getCodeStructureFingerprint,
  getTappRuntimeFingerprint,
} from './codeStructure'

import {
  calculateWidgetDimensions,
  sendResizeMessage,
  useIframeResize,
} from '../utils/iframeResize'
// 核心模块
import {
  escapeSandboxHtmlText,
  escapeSandboxScriptSource,
  generateCSP,
  generateNonce,
  generateSessionToken,
  generateThemeCSS,
  generateWidgetSDK,
  IFRAME_SANDBOX_ATTRS,
  serializeSandboxScriptValue,
  WIDGET_STATIC_CSS,
} from './sandbox'
// 处理器
import {
  registerAnimationHandlers,
  registerAIHandlers,
  registerAgentInteractionHandlers,
  registerBackgroundHandlers,
  registerContextHandlers,
  registerDataExchangeHandlers,
  registerEventHandlers,
  registerFileHandlers,
  registerLifecycleHandlers,
  registerMediaHandlers,
  registerPlatformHandlers,
  registerReportHandlers,
  registerSchedulerHandlers,
  registerSpeechHandlers,
  registerStorageHandlers,
  registerUIHandlers,
  registerUserHandlers,
} from './sandbox/handlers'
import { TappBridge } from './TappBridge'
import { TappRuntimeGrant } from './TappRuntimeGrant'
import { useSandboxSubscriptions } from './useSandboxSubscriptions'
import { onTappStorageChange } from './WidgetRuntimeSignals'

export interface TappWidgetSandboxProps {
  /** Tapp 实例 */
  tappInstance: TappInstance
  /** Tapp 代码 */
  code: TappCodeStructure
  /** Widget ID */
  widgetId: string
  /** Widget 渲染属性 */
  widgetProps: WidgetRenderProps
  /** 错误回调 */
  onError?: (error: Error) => void
  /** 就绪回调 */
  onReady?: () => void
  /** 当前 Dashboard 实例设置变更回调 */
  onInstanceSettingsChange?: (patch: Record<string, unknown>) => boolean
  /** 请求刷新当前 Widget 实例 */
  onInvalidate?: (reason: string) => void
  /** 额外的 className */
  className?: string
  /** 额外的 style */
  style?: React.CSSProperties
}

/**
 * 生成 Widget 沙箱 HTML
 *
 * 支持三种渲染方式：
 * 1. 纯 JS 模式：Tapp.widgets[id].render(container, props)
 * 2. 纯 HTML 模式：widgetHtml 直接渲染（适合静态展示）
 * 3. 混合模式：widgetHtml 定义结构 + JS 处理交互（性能最优）
 *
 * 🔒 安全特性：
 * - 使用 CSP nonce 替代 unsafe-inline，只有带正确 nonce 的脚本才能执行
 *
 * 🎯 CSS 策略：
 * - 优先使用安装时预编译的 CSS（零运行时开销）
 * - 如果预编译 CSS 不可用，降级到动态生成
 *
 * @param tappInstance - Tapp 实例
 * @param code - Tapp 代码结构
 * @param widgetId - Widget ID
 * @param widgetProps - Widget 渲染属性
 * @param sessionToken - 会话 token（用于消息验证）
 */
function generateWidgetHTML(
  tappInstance: TappInstance,
  code: TappCodeStructure,
  widgetId: string,
  widgetProps: WidgetRenderProps,
  sessionToken: string,
): string {
  const { manifest } = tappInstance
  const isDark = widgetProps.theme === 'dark'
  const primaryColor = widgetProps.primaryColor || '#8b5cf6'

  // 🔒 生成唯一 nonce（每个沙箱实例独立）
  const nonce = generateNonce()
  const csp = generateCSP(nonce)
  const sdkCode = escapeSandboxScriptSource(
    generateWidgetSDK(tappInstance, sessionToken),
  )
  const themeCSS = generateThemeCSS(isDark, primaryColor)

  // 自定义 CSS
  const customCSS = code.styles || ''

  // HTML 模板（如果有）
  const hasHtmlTemplate = !!code.widgetHtml
  const widgetHtmlContent = code.widgetHtml || ''

  // JS 代码 - 混合模式下也会加载
  const widgetCode = getCodeForMode(code, 'widget')

  // 🎯 使用安装时预编译的 CSS
  const tailwindCSS = code.widgetCSS || ''

  // 是否需要调用 Tapp.widgets.render()
  // 仅在没有 HTML 模板时才需要（纯 JS 模式）
  const needsJsRender = !hasHtmlTemplate

  return `<!DOCTYPE html>
<html>
<head>
  <meta charset="UTF-8">
  <meta http-equiv="Content-Security-Policy" content="${csp}">
  <meta name="viewport" content="width=device-width, initial-scale=1.0, maximum-scale=1.0, user-scalable=no">
  <title>${escapeSandboxHtmlText(manifest.name)} Widget</title>
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
    window._TAPP_MODE = 'widget';
    window._TAPP_WIDGET_ID = ${serializeSandboxScriptValue(widgetId)};
    window._TAPP_WIDGET_PROPS = ${serializeSandboxScriptValue(widgetProps)};
    window._TAPP_DIMENSIONS = { width: 0, height: 0, scale: 1, fontScale: 1, isCompact: false, isMini: false };
    window._TAPP_HAS_HTML = ${hasHtmlTemplate};

    window.addEventListener('message', function(e) {
      var msg = e.data;
      if (msg?.type === 'event' && msg.action === 'container:resize') {
        window._TAPP_DIMENSIONS = msg.payload;
        var root = document.documentElement;
        root.style.setProperty('--tapp-scale', msg.payload.scale || 1);
        root.style.setProperty('--tapp-font-scale', msg.payload.fontScale || 1);
        window.dispatchEvent(new CustomEvent('tapp:resize', { detail: msg.payload }));
      }
    });

    window.parent.postMessage({
      type: 'event',
      id: 'widget-ready-' + Date.now(),
      action: 'tapp.ready',
      payload: null,
      timestamp: Date.now()
    }, document.referrer ? new URL(document.referrer).origin : '*');
  </script>

  <!-- SDK 始终加载 -->
  <script nonce="${nonce}">${sdkCode}</script>

  <!-- JS 代码始终加载（用于事件绑定等） -->
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

  ${
    needsJsRender
      ? `
  <!-- 纯 JS 模式：调用 render 函数 -->
  <script nonce="${nonce}">
    (function() {
      'use strict';
      setTimeout(function() {
        try {
          var widgetId = ${serializeSandboxScriptValue(widgetId)};
          var widgetDef = Tapp.widgets[widgetId];
          var container = document.getElementById('widget-root');

          if (!widgetDef || typeof widgetDef.render !== 'function') {
            console.warn('[Widget] Not found:', widgetId);
            container.innerHTML = '<div class="tapp-empty">Widget not found: ' + widgetId + '</div>';
            return;
          }

          var props = window._TAPP_WIDGET_PROPS;
          props.scale = window._TAPP_DIMENSIONS.scale;
          props.fontScale = window._TAPP_DIMENSIONS.fontScale;

          widgetDef.render(container, props);

        } catch (error) {
          console.error('[Widget] Render error:', error);
          document.getElementById('widget-root').innerHTML =
            '<div class="tapp-empty tapp-text-error">Error: ' + error.message + '</div>';
        }
      }, 16);
    })();
  </script>
  `
      : '<!-- 混合/HTML 模式：HTML 已渲染，JS 用于交互 -->'
  }
</body>
</html>`
}

/**
 * Tapp Widget 沙箱组件
 */
export const TappWidgetSandbox = memo(
  ({
    tappInstance,
    code,
    widgetId,
    widgetProps,
    onReady,
    onInstanceSettingsChange,
    onInvalidate,
    className,
    style,
  }: TappWidgetSandboxProps) => {
    const { containerRef, dimensions } = useIframeResize<HTMLDivElement>()
    const iframeRef = useRef<HTMLIFrameElement>(null)
    const bridgeRef = useRef<TappBridge | null>(null)
    const [isReady, setIsReady] = useState(false)

    // 🎯 性能优化：使用 ref 存储对象引用，避免依赖变化触发 iframe 重建
    // 这些对象的内容变化通过 ID 来追踪，而不是对象引用
    const tappInstanceRef = useRef(tappInstance)
    const codeRef = useRef(code)
    const instanceSettingsChangeRef = useRef(onInstanceSettingsChange)
    const invalidateRef = useRef(onInvalidate)
    tappInstanceRef.current = tappInstance
    codeRef.current = code
    instanceSettingsChangeRef.current = onInstanceSettingsChange
    invalidateRef.current = onInvalidate

    // 稳定化核心 widgetProps；theme/primaryColor/locale 通过事件更新，
    // 不应仅因宿主外观或语言变化重建整个 iframe。
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
        // 初始主题和颜色仅用于首次渲染
        theme: latestThemeRef.current,
        primaryColor: latestColorRef.current,
      }),
      [
        widgetProps.size,
        widgetProps.isEditMode,
        widgetProps.isPreview,
        // 使用字符串比较稳定 config 依赖
        configString,
      ],
    )

    const handleReady = useCallback(() => {
      setIsReady(true)
      onReady?.()
    }, [onReady])

    // 🎯 共享订阅 hook：主题/主色调/页面可见性联动
    useSandboxSubscriptions(bridgeRef, isReady)

    // 同一个 Tapp 的 Page/其他 Widget 改写共享 storage 后，通知当前沙箱并刷新视图。
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
          invalidateRef.current?.('storage-changed')
        }),
      [tappInstance.id],
    )

    // 构建媒体状态对象（供 mediaStateChange 事件使用）
    const buildMediaState = useCallback((detail: Record<string, unknown>) => {
      const modeMap: Record<string, string> = {
        loop: 'loop',
        single: 'single',
        shuffle: 'shuffle',
      }
      const currentSong = detail.currentSong as Record<string, unknown> | null
      const currentTime = (detail.currentTime as number) || 0
      const audioDuration =
        (detail.audioDuration as number) ||
        (currentSong?.duration as number) ||
        0
      const volume = (detail.volume as number) ?? 0.7
      const playMode = (detail.playMode as string) || 'loop'
      return {
        isPlaying: detail.isPlaying || false,
        isPaused: !detail.isPlaying && currentSong !== null,
        currentTrack: currentSong
          ? {
              id: currentSong.id || '',
              title: currentSong.name || currentSong.title || '',
              name: currentSong.name || currentSong.title || '',
              artist: currentSong.artist || '',
              album: currentSong.album || '',
              cover: currentSong.cover || '',
              duration: currentSong.duration || 0,
            }
          : null,
        progress: {
          current: currentTime,
          duration: audioDuration,
          percentage:
            audioDuration > 0 ? (currentTime / audioDuration) * 100 : 0,
        },
        position: currentTime,
        volume: Math.round(volume * 100),
        mode: modeMap[playMode] || 'sequence',
        muted: volume === 0,
        lyrics: detail.lyrics || [],
        currentLyricIndex: (detail.currentLyricIndex as number) ?? -1,
        primaryColor: detail.musicColor || '#fc3c44',
        secondaryColor:
          (detail.musicColors as any)?.secondary ||
          detail.musicColor ||
          '#fc3c44',
        accentColor:
          (detail.musicColors as any)?.accent || detail.musicColor || '#fc3c44',
        lightColor: (detail.musicColors as any)?.light || '#ffffff',
        darkColor: (detail.musicColors as any)?.dark || '#000000',
      }
    }, [])

    // 🎵 媒体状态变化 — 转发给 Widget 沙箱
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
        // 部分派发（如 selectSong 只带曲目字段）缺 lyrics/currentTime——
        // 用全局状态兜底合并，避免编造「歌词清空/进度归零」传给 tapp
        const globalState =
          (window as { __musicPlayerState?: Record<string, unknown> })
            .__musicPlayerState || {}
        const merged: Record<string, unknown> = { ...globalState, ...detail }
        // 切歌时禁止沿用旧曲歌词/进度，避免标题与内容串曲
        const detailSong = detail.currentSong as
          | { id?: string | number }
          | null
          | undefined
        const globalSong = globalState.currentSong as
          | { id?: string | number }
          | null
          | undefined
        const detailId = detailSong?.id
        const globalId = globalSong?.id
        if (
          detailId != null &&
          globalId != null &&
          String(detailId) !== String(globalId)
        ) {
          if (!('lyrics' in detail)) {
            merged.lyrics = []
            merged.currentLyricIndex = -1
          }
          if (!('verbatimLyrics' in detail)) {
            merged.verbatimLyrics = []
            merged.hasVerbatimLyrics = false
            merged.verbatimLyricsSource = ''
          }
          if (!('currentTime' in detail)) {
            merged.currentTime = 0
          }
          if (!('audioDuration' in detail)) {
            merged.audioDuration =
              (detailSong as { duration?: number } | null)?.duration || 0
          }
        }
        bridgeRef.current.emit('mediaStateChange', buildMediaState(merged))
      }

      // 先注册监听，再触发同步（确保不会错过同步事件）
      window.addEventListener(
        'music-player-state-change',
        handleMusicStateChange,
      )

      // 🎯 Widget 就绪时立即推送当前音乐状态（解决初始化竞态）
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
        window.dispatchEvent(new CustomEvent('request-music-state-sync'))
      }

      // 🎯 延迟重推：确保 iframe SDK 消息监听器就绪后再推一次
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

        const { currentTime, audioDuration, songId } = (e as CustomEvent)
          .detail
        // 丢弃与当前曲目不一致的进度（快速切歌时旧 timeupdate 可能晚到）
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

    // 内容哈希避免等长代码/CSS 更新继续复用旧 iframe。
    const codeFingerprint = useMemo(
      () => getCodeStructureFingerprint(code, 'widget'),
      [code],
    )
    const runtimeFingerprint = getTappRuntimeFingerprint(tappInstance)

    // 初始化（不依赖 theme/primaryColor 变化）
    // 🎯 依赖优化：只使用稳定的 ID 和指纹，不使用对象引用
    // 🎯 Safari 兼容：使用 imperative iframe 创建，确保 srcdoc 在 DOM 插入前设置
    useEffect(() => {
      const container = containerRef.current
      if (!container) return

      // 🎯 从 ref 获取当前对象，避免闭包陈旧问题
      const currentTappInstance = tappInstanceRef.current
      const currentCode = codeRef.current

      // 使用 ref 中的初始值，避免闪烁
      const propsForHtml = {
        ...stableWidgetProps,
        theme: latestThemeRef.current,
        primaryColor: latestColorRef.current,
      }

      // 生成 session token（独立于 Bridge）
      const sessionToken = generateSessionToken()

      // 创建 iframe 元素（尚未插入 DOM）
      const iframe = document.createElement('iframe')
      iframe.className = 'tapp-widget-iframe'
      const pointerEvents =
        stableWidgetProps.isEditMode || stableWidgetProps.isPreview
          ? 'none'
          : 'auto'
      iframe.style.cssText = `position:absolute;top:0;left:0;width:100%;height:100%;border:none;background-color:transparent;display:block;border-radius:inherit;visibility:visible;opacity:1;pointer-events:${pointerEvents};`
      iframe.setAttribute('sandbox', IFRAME_SANDBOX_ATTRS)
      iframe.setAttribute('referrerpolicy', 'no-referrer')
      iframe.title = `${currentTappInstance.manifest.name} Widget`
      iframe.allowFullscreen = true
      iframeRef.current = iframe

      // 创建 Bridge（在 DOM 插入前设置消息监听）
      const bridge = new TappBridge()
      const runtimeGrant = new TappRuntimeGrant(
        currentTappInstance.id,
        `widget_${sessionToken.slice(0, 32)}`,
        'widget',
      )
      bridge.initialize(iframe, currentTappInstance, sessionToken, runtimeGrant)
      bridgeRef.current = bridge

      // 注册处理器（Widget 只需要基础 API）
      registerLifecycleHandlers(bridge, currentTappInstance, handleReady)
      registerUIHandlers(bridge, currentTappInstance)
      registerStorageHandlers(bridge, currentTappInstance.id)
      registerUserHandlers(bridge, currentTappInstance)
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
      registerFileHandlers(bridge)
      const closeAITaskStreams = registerAIHandlers(bridge)
      // Widget SDK 只暴露平台/报告读取能力，避免注册未暴露的写入 handler。
      registerPlatformHandlers(bridge, currentTappInstance, { readOnly: true })
      registerReportHandlers(bridge, currentTappInstance, { readOnly: true })
      // 🎯 注册 Context 处理器（包含 api.execute 和 context.getGeo）
      registerContextHandlers(bridge, currentTappInstance)
      const closeDataExchange = registerDataExchangeHandlers(
        bridge,
        currentTappInstance,
      )
      const closeEventStream = registerEventHandlers(
        bridge,
        currentTappInstance,
      )
      const closeAgentInteractions = registerAgentInteractionHandlers(
        bridge,
        currentTappInstance,
      )
      // 🎵 注册 Media 处理器（供音乐播放器 Tapp 使用）
      registerMediaHandlers(bridge, currentTappInstance)
      registerSpeechHandlers(bridge, currentTappInstance)
      registerAnimationHandlers(bridge)
      // 共享 core 在 Widget 模式同样会执行，必须能声明后台保活需求。
      registerBackgroundHandlers(bridge, currentTappInstance)
      // ⏰ 注册 Scheduler 处理器（定时任务，与 SDK Tapp.scheduler 对应）
      const closeScheduler = registerSchedulerHandlers(
        bridge,
        currentTappInstance,
      )

      // 监听 tapp.ready 事件（Widget HTML 发送的早期 ready 事件）
      const unsubscribeReady = bridge.on('tapp.ready', () => {
        handleReady()
      })

      // 生成 HTML（使用预生成的 session token）
      const html = generateWidgetHTML(
        currentTappInstance,
        currentCode,
        widgetId,
        propsForHtml,
        sessionToken,
      )

      // 🎯 关键：先设置 srcdoc，再插入 DOM
      // Safari 要求 srcdoc 在 iframe 插入 DOM 之前就设置好
      iframe.srcdoc = html
      container.appendChild(iframe)

      return () => {
        unsubscribeReady()
        closeScheduler()
        closeDataExchange()
        closeAITaskStreams()
        closeEventStream()
        closeAgentInteractions()
        iframeRef.current = null
        if (container.contains(iframe)) {
          container.removeChild(iframe)
        }
        bridge.destroy()
        bridgeRef.current = null
        setIsReady(false)
      }
      // 🎯 稳定依赖：只有这些真正改变时才重建 iframe
      // - tappInstance.id: Tapp 实例 ID
      // - widgetId: Widget ID
      // - codeFingerprint: 代码指纹（内容变化才会变）
      // - stableWidgetProps: 已稳定化的 props
    }, [
      tappInstance.id,
      runtimeFingerprint,
      widgetId,
      codeFingerprint,
      handleReady,
      stableWidgetProps,
    ])

    // 语言变化监听
    useEffect(() => {
      if (!isReady || !bridgeRef.current) return
      bridgeRef.current.emit('locale:change', widgetProps.locale)
    }, [widgetProps.locale, isReady])

    // 尺寸更新
    useEffect(() => {
      if (!isReady || !iframeRef.current) return

      const widgetDims = calculateWidgetDimensions(
        stableWidgetProps.size,
        dimensions.width,
        dimensions.height,
      )
      sendResizeMessage(iframeRef.current, widgetDims)
    }, [isReady, dimensions, stableWidgetProps.size])

    return (
      <div
        ref={containerRef}
        className={`tapp-widget-sandbox ${className || ''}`}
        style={{
          position: 'relative',
          width: '100%',
          height: '100%',
          borderRadius: 'inherit',
          ...style,
        }}
      >
        {/* iframe 在 useEffect 中 imperatively 创建，确保 srcdoc 在 DOM 插入前设置（Safari 兼容） */}
      </div>
    )
  },
)

export default TappWidgetSandbox
