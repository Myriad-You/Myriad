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
  TAPP_WIDGET_SKELETON,
  WidgetSkeletonCover,
} from '../../components/widgets/shared/WidgetSkeleton'
import { useI18n } from '../../contexts/I18nContext'
import {
  buildTappMediaState,
  mergeMusicPlayerEventDetail,
} from '../../utils/musicPlayerState'
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
// 核心模块
import {
  cspOptionsFromPermissions,
  escapeSandboxHtmlText,
  escapeSandboxScriptSource,
  generateCSP,
  generateNonce,
  generateSecurityWrapper,
  generateSessionToken,
  generateThemeCSS,
  generateWidgetSDK,
  IFRAME_SANDBOX_ATTRS,
  serializeSandboxScriptValue,
  WIDGET_STATIC_CSS,
} from './sandbox'
// 处理器
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
  registerPlatformHandlers,
  registerReportHandlers,
  registerSchedulerHandlers,
  registerSpeechHandlers,
  registerStorageHandlers,
  registerUIHandlers,
  registerUserHandlers,
} from './sandbox/handlers'
import { registerPlaygroundPreviewHandlers } from './sandbox/handlers/playgroundPreviewHandlers'
import { TappBridge } from './TappBridge'
import { TappRuntimeGrant } from './TappRuntimeGrant'
import { useSandboxSubscriptions } from './useSandboxSubscriptions'
import { widgetPerfMark } from './WidgetLoadPerf'
import { onTappSharedChange, onTappStorageChange } from './WidgetRuntimeSignals'

export interface TappWidgetSandboxProps {
  /** Tapp 实例 */
  tappInstance: TappInstance
  /** Tapp 代码 */
  code: TappCodeStructure
  widgetId: string
  /** Widget 渲染属性 */
  widgetProps: WidgetRenderProps
  /**
   * Playground temporary preview (MYR-024).
   * No Runtime Grant; only in-memory preview handlers — never treat manifest
   * declarations as real installed host grants.
   */
  previewMode?: boolean
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
 * 1. 纯 JS 模式：Tapp.widgets[id].render(container, props) 填满容器
 * 2. 纯 HTML 模式：widgetHtml 直接渲染（无 render 时保持静态）
 * 3. 混合模式：widgetHtml 定义结构，宿主仍调用 render(container) 绑定数据
 *    （旧逻辑在有 HTML 时跳过 render，导致 storage 有数但 UI 永远是 "--"）
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

  // 生成唯一 nonce（每个沙箱实例独立）
  const nonce = generateNonce()
  const cspOptions = cspOptionsFromPermissions(tappInstance.grantedPermissions)
  const csp = generateCSP(nonce, cspOptions)
  // 与 Page 对齐的深度防御包装（真正边界仍是 sandbox + CSP + Bridge）
  const securityWrapper = escapeSandboxScriptSource(
    generateSecurityWrapper(sessionToken, cspOptions.allowRemoteMedia),
  )
  const sdkCode = escapeSandboxScriptSource(
    generateWidgetSDK(tappInstance, sessionToken),
  )
  const themeCSS = generateThemeCSS(isDark, primaryColor)

  // 自定义 CSS
  const customCSS = code.styles || ''

  // HTML 模板（如果有）
  const hasHtmlTemplate = !!code.widgetHtml
  const widgetHtmlContent = code.widgetHtml || ''

  // JS 代码 - 混合模式下也会加载。只装这一个 widget 的入口。
  const widgetCode = buildLayerScript(code, 'widget', widgetId).source

  // 使用安装时预编译的 CSS
  const tailwindCSS = code.widgetCSS || ''

  // Always invoke Tapp.widgets[id].render when registered.
  // Hybrid mode (HTML template + JS) used to skip render entirely — templates
  // only showed static placeholders ("--") while data was written to storage
  // and never painted (see cn.xciy.xingji.dashboard). HTML seeds structure;
  // render() binds data/events. Pure-HTML widgets without a render() still work.
  const hasHtmlTemplateLiteral = hasHtmlTemplate ? 'true' : 'false'

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
    window._TAPP_LOCALE = ${serializeSandboxScriptValue(widgetProps.locale)};
    window._TAPP_I18N = ${serializeSandboxScriptValue(code.i18n || {})};
    window._TAPP_SESSION_TOKEN = ${serializeSandboxScriptValue(sessionToken)};
    window._TAPP_DIMENSIONS = { width: 0, height: 0, scale: 1, fontScale: 1, isCompact: false, isMini: false };
    window._TAPP_HAS_HTML = ${hasHtmlTemplateLiteral};

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

    // iframe → host: early ready 必须带 session token（与 Bridge 校验对齐）
    window.parent.postMessage({
      type: 'event',
      id: 'widget-ready-' + Date.now(),
      action: 'tapp.ready',
      payload: null,
      timestamp: Date.now(),
      _sessionToken: window._TAPP_SESSION_TOKEN
    }, document.referrer ? new URL(document.referrer).origin : '*');
  </script>

  <!-- 安全包装（冻结危险 API；边界仍以 CSP/sandbox 为准） -->
  <script nonce="${nonce}">${securityWrapper}</script>

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

  <!-- Always try render(): pure-JS fills container; hybrid paints data into template.
       Microtask (not setTimeout 16ms) so first paint is not artificially delayed after
       widget code registration; prior scripts already defined Tapp.widgets. -->
  <script nonce="${nonce}">
    (function() {
      'use strict';
      var runRender = function() {
        try {
          var widgetId = ${serializeSandboxScriptValue(widgetId)};
          var widgetDef = Tapp.widgets && Tapp.widgets[widgetId];
          var container = document.getElementById('widget-root');
          if (!container) return;

          if (!widgetDef || typeof widgetDef.render !== 'function') {
            // Pure HTML static widget is fine; only error when there is no HTML either.
            if (!window._TAPP_HAS_HTML) {
              console.warn('[Widget] Not found:', widgetId);
              container.innerHTML = '<div class="tapp-empty">Widget not found: ' + widgetId + '</div>';
            }
            return;
          }

          var props = window._TAPP_WIDGET_PROPS || {};
          props.scale = window._TAPP_DIMENSIONS.scale;
          props.fontScale = window._TAPP_DIMENSIONS.fontScale;

          widgetDef.render(container, props);

        } catch (error) {
          console.error('[Widget] Render error:', error);
          var root = document.getElementById('widget-root');
          if (root) {
            root.innerHTML =
              '<div class="tapp-empty tapp-text-error">Error: ' + (error && error.message ? error.message : error) + '</div>';
          }
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

/**
 * Tapp Widget 沙箱组件
 */
export const TappWidgetSandbox = memo(
  ({
    tappInstance,
    code,
    widgetId,
    widgetProps,
    previewMode = false,
    onReady,
    onInstanceSettingsChange,
    onInvalidate,
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
    /** iframe 长时间不 ready 时展示 stall 文案（优于无限骨架） */
    const [readyStalled, setReadyStalled] = useState(false)
    /** Bumps when host identity settles after login/logout so iframe remounts. */
    const [subjectEpoch, setSubjectEpoch] = useState(0)

    useEffect(() => {
      const onSubjectReady = () => setSubjectEpoch((n) => n + 1)
      window.addEventListener('tapp-subject-ready', onSubjectReady)
      return () =>
        window.removeEventListener('tapp-subject-ready', onSubjectReady)
    }, [])

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

    // 使用 ref 存储对象引用，避免依赖变化触发 iframe 重建
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
      widgetPerfMark(
        tappInstance.id,
        widgetId,
        'iframe-ready',
        widgetProps.size,
      )
      onReady?.()
    }, [onReady, tappInstance.id, widgetId, widgetProps.size])

    // 共享订阅 hook：主题/主色调/页面可见性联动
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
          invalidateRef.current?.('shared-changed')
        }),
      [tappInstance.id],
    )

    // 构建媒体状态对象（供 mediaStateChange 事件使用）— 与 Page 共用纯函数
    const buildMediaState = useCallback((detail: Record<string, unknown>) => {
      return buildTappMediaState(detail)
    }, [])

    // 媒体状态变化 — 转发给 Widget 沙箱
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

      // 先注册监听，再触发同步（确保不会错过同步事件）
      window.addEventListener(
        'music-player-state-change',
        handleMusicStateChange,
      )

      // Widget 就绪时立即推送当前音乐状态（解决初始化竞态）
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

      // 延迟重推：确保 iframe SDK 消息监听器就绪后再推一次
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
      () => getCodeStructureFingerprint(code, 'widget', widgetId),
      [code, widgetId],
    )
    const runtimeFingerprint = getTappRuntimeFingerprint(tappInstance)

    // 初始化（不依赖 theme/primaryColor 变化）
    // 依赖优化：只使用稳定的 ID 和指纹，不使用对象引用
    // Safari 兼容：使用 imperative iframe 创建，确保 srcdoc 在 DOM 插入前设置
    useEffect(() => {
      const container = containerRef.current
      if (!container) return

      // 从 ref 获取当前对象，避免闭包陈旧问题
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
      // Playground preview: no Runtime Grant (uninstalled id + no real host grants).
      // Installed widgets: share host Runtime Grant (refcount) across same-Tapp iframes.
      if (previewMode) {
        bridge.initialize(iframe, currentTappInstance, sessionToken, undefined)
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
        // 与 iframe 解析并行预热 Runtime Grant（仍 host-only，不进 srcdoc）
        void runtimeGrant.getToken().catch(() => {
          /* 首次 API 调用时会重试；此处失败不阻塞沙箱启动 */
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
      registerAnimationHandlers(bridge)

      let closeAITaskStreams: () => void = () => {}
      let closeDataExchange: () => void = () => {}
      let closeEventStream: () => void = () => {}
      let closeAgentInteractions: () => void = () => {}
      let closeScheduler: () => void = () => {}

      if (previewMode) {
        // MYR-024: ephemeral handlers only — no real storage/API/host surfaces.
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
        // 注册处理器：始终挂载 Widget 热路径；按 grantedPermissions 惰性挂载重型能力。
        // Bridge 仍会做权限校验；这里少注册可降低每个 iframe 的启动成本，并缩小攻击面。
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
        const hasPlatform = hasExact('platform:read')
        const hasAnalytics = hasExact('analytics:read')
        const hasReport = hasExact('report:read')

        registerStorageHandlers(bridge, currentTappInstance.id)
        registerUserHandlers(bridge, currentTappInstance)
        registerFileHandlers(bridge)
        registerAssetHandlers(bridge, currentTappInstance)
        // Context（含 api.execute）始终需要：声明式 HTTP/API 与公开上下文查询。
        registerContextHandlers(bridge, currentTappInstance)
        // 共享 core 在 Widget 模式同样会执行，必须能声明后台保活需求。
        registerBackgroundHandlers(bridge, currentTappInstance)

        // 可选能力 — 仅在已授权时挂载（后端仍强制 Runtime Grant + 权限）
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
        if (hasMedia) {
          registerMediaHandlers(bridge, currentTappInstance)
        }
        if (hasSpeech) {
          registerSpeechHandlers(bridge, currentTappInstance)
        }
        closeScheduler = hasScheduler
          ? registerSchedulerHandlers(bridge, currentTappInstance)
          : () => {}
      }

      // 监听 tapp.ready：必须先 allowSandboxEvent（显式 inbound 白名单）
      bridge.allowSandboxEvent('tapp.ready')
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

      // 关键：先设置 srcdoc，再插入 DOM
      // Safari 要求 srcdoc 在 iframe 插入 DOM 之前就设置好
      iframe.srcdoc = html
      container.appendChild(iframe)
      // srcdoc 解析后 contentWindow 稳定，注册到集中式 message 路由
      bridge.attachSource()
      // 部分引擎在 load 后替换 browsing context — 再挂一次
      const onIframeLoad = () => bridge.attachSource()
      iframe.addEventListener('load', onIframeLoad)

      return () => {
        unsubscribeReady()
        closeScheduler()
        closeDataExchange()
        closeAITaskStreams()
        closeEventStream()
        closeAgentInteractions()
        iframe.removeEventListener('load', onIframeLoad)
        iframeRef.current = null
        if (container.contains(iframe)) {
          container.removeChild(iframe)
        }
        bridge.destroy()
        bridgeRef.current = null
        setIsReady(false)
      }
      // 稳定依赖：只有这些真正改变时才重建 iframe
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
      subjectEpoch,
      previewMode,
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

    const themeAccent =
      tappInstance.manifest.themeColor?.trim() ||
      widgetProps.primaryColor ||
      'var(--color-primary, #6366f1)'

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
        {/* iframe 由 effect 创建；ready 前骨架盖住，ready 后淡出（内容在下） */}
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
