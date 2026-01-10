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

import type { TappCodeStructure } from '../examples/tapps/types'
import type { TappInstance } from '../types'
import type { WidgetRenderProps } from './sandbox'
import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { isPageVisible, onVisibility } from '../../hooks/animation/core'
import { getPrimaryColor, subscribeToPrimaryColor } from '../../utils/colorSubscriber'
import { subscribeToTheme } from '../../utils/themeSubscriber'
import { getCodeForMode } from '../examples/tapps/types'
import { getQuotaManager } from '../services/QuotaManager'
import * as TappApiService from '../services/TappApiService'

import { calculateWidgetDimensions, sendResizeMessage, useIframeResize } from '../utils/iframeResize'
// 核心模块
import {
  generateCSP,
  generateNonce,
  generateThemeCSS,
  generateWidgetSDK,
  IFRAME_SANDBOX_ATTRS,
  WIDGET_STATIC_CSS,

} from './sandbox'

// 处理器（Widget 只需要基础处理器）
import {
  registerContextHandlers,
  registerFileHandlers,
  registerLifecycleHandlers,
  registerStorageHandlers,
  registerUIHandlers,
} from './sandbox/handlers'

import { TappBridge } from './TappBridge'
import { TappPermissionController } from './TappPermission'

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
  const sdkCode = generateWidgetSDK(tappInstance, sessionToken)
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
  <title>${manifest.name} Widget</title>
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
    window._TAPP_WIDGET_ID = '${widgetId}';
    window._TAPP_WIDGET_PROPS = ${JSON.stringify(widgetProps)};
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
    }, '*');
  </script>
  
  <!-- SDK 始终加载 -->
  <script nonce="${nonce}">${sdkCode}</script>
  
  <!-- JS 代码始终加载（用于事件绑定等） -->
  <script nonce="${nonce}">
    (function() {
      'use strict';
      try {
        ${widgetCode}
      } catch (error) {
        console.error('[Widget] Code error:', error);
      }
    })();
  </script>
  
  ${needsJsRender
    ? `
  <!-- 纯 JS 模式：调用 render 函数 -->
  <script nonce="${nonce}">
    (function() {
      'use strict';
      setTimeout(function() {
        try {
          var widgetId = '${widgetId}';
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
    : '<!-- 混合/HTML 模式：HTML 已渲染，JS 用于交互 -->'}
</body>
</html>`
}

/**
 * 注册 Widget 专用的 AI 处理器（精简版）
 *
 * 安全增强：添加配额检查
 */
function registerWidgetAIHandler(
  bridge: TappBridge,
  permission: TappPermissionController,
  tappId: string,
): void {
  const quotaManager = getQuotaManager()

  bridge.registerHandler('ai.chat', async (message) => {
    // 🔒 权限检查
    if (!permission.hasPermission('ai:chat')) {
      return { success: false, error: 'Permission denied: ai:chat' }
    }

    // 🔒 配额检查
    const quotaCheck = quotaManager.checkQuota(tappId, 'ai.generate')
    if (!quotaCheck.allowed) {
      return { success: false, error: quotaCheck.reason || 'Quota exceeded', code: 'QUOTA_EXCEEDED' }
    }

    const [params] = (message.payload as { args: unknown[] }).args || []
    const { messages, context, options } = (params || {}) as {
      messages?: Array<{ role: 'user' | 'assistant' | 'system', content: string }>
      context?: Record<string, unknown>
      options?: Record<string, unknown>
    }
    try {
      const result = await TappApiService.aiChat({
        tappId,
        messages: messages || [],
        context,
        options,
      })

      // 记录配额使用
      quotaManager.recordUsage(tappId, 'ai.generate')

      return { success: true, data: result }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'AI error' }
    }
  })
}

/**
 * Tapp Widget 沙箱组件
 */
export const TappWidgetSandbox = memo(({
  tappInstance,
  code,
  widgetId,
  widgetProps,
  onError,
  onReady,
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
  tappInstanceRef.current = tappInstance
  codeRef.current = code

  // 🎯 使用动画调度器的页面可见性感知，页面隐藏时跳过非必要更新
  const pageVisibleRef = useRef(isPageVisible())

  // 稳定化核心 widgetProps（不包含 theme 和 primaryColor，因为它们通过事件更新）
  // 这样主题/颜色变化不会触发整个沙箱重建
  const configString = JSON.stringify(widgetProps.config || {})
  const stableWidgetProps = useMemo(() => ({
    size: widgetProps.size,
    config: widgetProps.config,
    isEditMode: widgetProps.isEditMode,
    isPreview: widgetProps.isPreview,
    locale: widgetProps.locale,
    // 初始主题和颜色仅用于首次渲染
    theme: widgetProps.theme,
    primaryColor: widgetProps.primaryColor,
  }), [
    widgetProps.size,
    widgetProps.isEditMode,
    widgetProps.isPreview,
    widgetProps.locale,
    // 使用字符串比较稳定 config 依赖
    // eslint-disable-next-line react-hooks/exhaustive-deps
    configString,
  ])

  // 保存初始渲染用的主题/颜色
  const initialThemeRef = useRef(widgetProps.theme)
  const initialColorRef = useRef(widgetProps.primaryColor)

  const handleReady = useCallback(() => {
    setIsReady(true)
    onReady?.()
  }, [onReady])

  // 🎯 订阅页面可见性变化（用于优化后台渲染 + 通知 iframe 冻结/恢复）
  useEffect(() => {
    return onVisibility((visible) => {
      pageVisibleRef.current = visible
      // 🎯 通知 iframe 生命周期变化，让 Tapp 可以响应暂停/恢复
      if (isReady && bridgeRef.current) {
        bridgeRef.current.emit(visible ? 'lifecycle:resume' : 'lifecycle:pause', null)
      }
    })
  }, [isReady])

  // 🎯 生成稳定的代码指纹，只有代码实际变化时才重建 iframe
  // 使用 widgetHtml 长度 + styles 长度 + widgetCSS 长度作为简单指纹，避免大字符串比较
  const codeFingerprint = useMemo(() => {
    const wh = code.widgetHtml || ''
    const st = code.styles || ''
    const js = getCodeForMode(code, 'widget') || ''
    const css = code.widgetCSS || ''
    return `${wh.length}:${st.length}:${js.length}:${css.length}`
  }, [code])

  // 初始化（不依赖 theme/primaryColor 变化）
  // 🎯 依赖优化：只使用稳定的 ID 和指纹，不使用对象引用
  useEffect(() => {
    const iframe = iframeRef.current
    if (!iframe)
      return

    // 🎯 从 ref 获取当前对象，避免闭包陈旧问题
    const currentTappInstance = tappInstanceRef.current
    const currentCode = codeRef.current

    // 使用 ref 中的初始值，避免闪烁
    const propsForHtml = {
      ...stableWidgetProps,
      theme: initialThemeRef.current,
      primaryColor: initialColorRef.current,
    }

    // 创建 Bridge（在生成 HTML 之前，以获取 session token）
    const bridge = new TappBridge()
    bridge.initialize(iframe, currentTappInstance)
    bridgeRef.current = bridge

    // 获取 session token（用于消息验证）
    const sessionToken = bridge.getSessionToken()

    // 生成 HTML（传递 session token）
    const html = generateWidgetHTML(currentTappInstance, currentCode, widgetId, propsForHtml, sessionToken)

    const blob = new Blob([html], { type: 'text/html' })
    const url = URL.createObjectURL(blob)
    iframe.src = url

    // 注册处理器（Widget 只需要基础 API）
    const permission = new TappPermissionController(currentTappInstance)
    registerLifecycleHandlers(bridge, currentTappInstance, handleReady)
    registerUIHandlers(bridge)
    registerStorageHandlers(bridge, currentTappInstance.id)
    registerFileHandlers(bridge)
    registerWidgetAIHandler(bridge, permission, currentTappInstance.id)
    // 🎯 注册 Context 处理器（包含 api.execute 和 context.getGeo）
    registerContextHandlers(bridge, currentTappInstance)

    // 监听 tapp.ready 事件（Widget HTML 发送的早期 ready 事件）
    const unsubscribeReady = bridge.on('tapp.ready', () => {
      handleReady()
    })

    return () => {
      URL.revokeObjectURL(url)
      unsubscribeReady()
      bridge.destroy()
      bridgeRef.current = null
      setIsReady(false)
    }
  // 🎯 稳定依赖：只有这些真正改变时才重建 iframe
  // - tappInstance.id: Tapp 实例 ID
  // - widgetId: Widget ID
  // - codeFingerprint: 代码指纹（内容变化才会变）
  // - stableWidgetProps: 已稳定化的 props
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tappInstance.id, widgetId, codeFingerprint, handleReady, stableWidgetProps])

  // 主题变化监听（通过事件通知 iframe，而不是重建）
  // 🎯 优化：使用共享订阅器，确保所有 widget 都能响应变化
  useEffect(() => {
    if (!isReady)
      return

    return subscribeToTheme((isDark) => {
      // 直接使用 bridgeRef.current，确保获取最新的 bridge 实例
      const bridge = bridgeRef.current
      if (bridge) {
        bridge.emit('theme:change', isDark ? 'dark' : 'light')
      }
    })
  }, [isReady])

  // 主色调变化监听（通过事件通知 iframe，而不是重建）
  // 🎯 优化：使用共享的 colorSubscriber，避免每个组件都创建 MutationObserver
  // 🎯 修复：isReady 时立即发送当前颜色，确保多窗口/多组件场景下正确初始化
  useEffect(() => {
    if (!isReady)
      return

    // 立即发送当前主色调，确保新创建的组件能获取到
    const bridge = bridgeRef.current
    const currentColor = getPrimaryColor()
    if (bridge && currentColor) {
      bridge.emit('primaryColor:change', currentColor)
    }

    // 订阅后续变化
    return subscribeToPrimaryColor((color) => {
      const bridge = bridgeRef.current
      if (bridge && color) {
        bridge.emit('primaryColor:change', color)
      }
    })
  }, [isReady])

  // 语言变化监听
  useEffect(() => {
    if (!isReady || !bridgeRef.current)
      return
    bridgeRef.current.emit('locale:change', widgetProps.locale)
  }, [widgetProps.locale, isReady])

  // 尺寸更新
  useEffect(() => {
    if (!isReady || !iframeRef.current)
      return

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
        overflow: 'hidden',
        borderRadius: 'inherit',
        ...style,
      }}
    >
      <iframe
        ref={iframeRef}
        className="tapp-widget-iframe"
        style={{
          position: 'absolute',
          top: 0,
          left: 0,
          width: '100%',
          height: '100%',
          border: 'none',
          backgroundColor: 'transparent',
          display: 'block',
          borderRadius: 'inherit',
          // 编辑模式或预览模式下禁用 iframe 的鼠标事件
          pointerEvents: (widgetProps.isEditMode || widgetProps.isPreview) ? 'none' : 'auto',
        }}
        sandbox={IFRAME_SANDBOX_ATTRS}
        referrerPolicy="no-referrer"
        title={`${tappInstance.manifest.name} Widget`}
        // Safari/WebKit 全屏兼容性
        allowFullScreen
        // @ts-expect-error Safari webkit prefix
        webkitallowfullscreen="true"
      />
    </div>
  )
})

export default TappWidgetSandbox
