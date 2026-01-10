/**
 * Tapp Page 沙箱组件
 *
 * 用于渲染 Tapp 的页面模式（全屏应用）
 */

import type { TappCodeStructure } from '../examples/tapps/types'
import type { TappInstance } from '../types'
import type { AnimationConfigRef, SafeInsets, TappNotificationOptions } from './sandbox'
import type { TappBridge } from './TappBridge'
import type { TappPermissionController } from './TappPermission'
import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { isPageVisible, onVisibility } from '../../hooks/animation/core'
import { useAnimationLevel } from '../../hooks/useAnimationLevel'
import { getPrimaryColor, subscribeToPrimaryColor } from '../../utils/colorSubscriber'
import { getIsDarkMode, subscribeToTheme } from '../../utils/themeSubscriber'
import { getCodeForMode } from '../examples/tapps/types'

import { sendResizeMessage, useIframeResize } from '../utils/iframeResize'
// 核心模块
import {

  generateCSP,
  generateFullSDK,
  generateNonce,
  generateSecurityWrapper,
  generateThemeCSS,
  IFRAME_SANDBOX_ATTRS,
  PAGE_STATIC_CSS,

} from './sandbox'
// 处理器
import {
  registerAdvancedHandlers,
  registerAIHandlers,
  registerAnimationHandlers,
  registerBackgroundHandlers,
  registerContextHandlers,
  registerDynamicContentHandlers,
  registerFileHandlers,
  registerLifecycleHandlers,
  registerMediaHandlers,
  registerPlatformHandlers,
  registerReportHandlers,
  registerStorageHandlers,
  registerUIHandlers,
  registerUserHandlers,
  registerWidgetHandlers,
} from './sandbox/handlers'

import { createTappBridge } from './TappBridge'
import { createPermissionController } from './TappPermission'

/**
 * 🎯 WebKit 浏览器检测
 * Safari/WebKit 在 iframe 渲染时存在特殊问题，需要特殊处理
 */
function isWebKit(): boolean {
  if (typeof navigator === 'undefined')
    return false
  const ua = navigator.userAgent
  // Safari (排除 Chrome/Firefox/Edge)
  if (ua.includes('Safari') && !ua.includes('Chrome') && !ua.includes('Firefox') && !ua.includes('Edg')) {
    return true
  }
  // iOS/iPadOS 上所有浏览器都使用 WebKit
  if (/iPad|iPhone|iPod/.test(ua) || (ua.includes('Mac') && 'ontouchend' in document)) {
    return true
  }
  return false
}

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
  /** 通知回调 */
  onNotification?: (options: TappNotificationOptions) => void
  /** 自定义类名 */
  className?: string
  /** 自定义样式 */
  style?: React.CSSProperties
  /** 安全区域内边距 */
  safeInsets?: SafeInsets
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
  safeInsets?: SafeInsets,
): string {
  const { manifest } = tappInstance
  const isDark = getIsDarkMode()
  const primaryColor = getComputedStyle(document.documentElement)
    .getPropertyValue('--color-primary')
    .trim() || '#94a3b8'

  // 🔒 生成唯一 nonce（每个沙箱实例独立）
  const nonce = generateNonce()
  const csp = generateCSP(nonce)
  const securityWrapper = generateSecurityWrapper(sessionToken)
  const sdkCode = generateFullSDK(tappInstance, sessionToken)
  const themeCSS = generateThemeCSS(isDark, primaryColor)

  // 自定义 CSS
  const customCSS = code.styles || ''

  // HTML 模板（如果有）
  const hasHtmlTemplate = !!code.pageHtml
  const pageHtmlContent = code.pageHtml || ''

  // 🎯 检测 pageHtml 是否已经包含分层结构
  // 如果包含 #tapp-background 或 #tapp-content，说明 Tapp 自己定义了分层
  const hasLayeredStructure = pageHtmlContent.includes('id="tapp-background"')
    || pageHtmlContent.includes('id=\'tapp-background\'')
    || pageHtmlContent.includes('id="tapp-content"')
    || pageHtmlContent.includes('id=\'tapp-content\'')

  // JS 代码 - 混合模式下也会加载
  const pageCode = getCodeForMode(code, 'page')

  // 🎯 使用安装时预编译的 CSS
  const tailwindCSS = code.pageCSS || ''

  // 是否需要调用 Tapp.pages.render()
  // 仅在没有 HTML 模板时才需要（纯 JS 模式）
  const needsJsRender = !hasHtmlTemplate

  // 初始安全区域 padding（确保首次渲染就有正确的间距）
  const initialPadding = `${safeInsets?.top ?? 0}px ${safeInsets?.right ?? 0}px ${safeInsets?.bottom ?? 0}px ${safeInsets?.left ?? 0}px`

  // 🎯 根据是否有分层结构决定 body 内容
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
  <title>${manifest.name}</title>
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
    window._TAPP_HAS_HTML = ${hasHtmlTemplate};
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
  
  <!-- JS 代码始终加载（用于事件绑定等） -->
  <script nonce="${nonce}">
    (function() {
      'use strict';
      try {
        ${pageCode}
      } catch (error) {
        console.error('[Page] Code error:', error);
        Tapp.lifecycle._notifyError(error);
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
          document.getElementById('tapp-content').innerHTML = 
            '<div class="tapp-empty tapp-text-error">Page Error: ' + error.message + '</div>';
        }
      }, 50);
    })();
  </script>
  `
    : '<!-- 混合/HTML 模式：HTML 已渲染，JS 用于交互 -->'}
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
  onNotification,
  className,
  style,
  safeInsets,
}) => {
  const iframeRef = useRef<HTMLIFrameElement>(null)
  const bridgeRef = useRef<TappBridge | null>(null)
  const permissionRef = useRef<TappPermissionController | null>(null)
  const [isReady, setIsReady] = useState(false)

  const { containerRef, dimensions } = useIframeResize<HTMLDivElement>()
  const { locale } = useI18n()
  const animationConfig = useAnimationLevel()

  // 🎯 性能优化：使用 ref 存储对象引用，避免依赖变化触发 iframe 重建
  const tappInstanceRef = useRef(tappInstance)
  const codeRef = useRef(code)
  const safeInsetsRef = useRef(safeInsets)
  tappInstanceRef.current = tappInstance
  codeRef.current = code
  safeInsetsRef.current = safeInsets

  // 🎯 集成动画调度器的页面可见性感知 + 通知 iframe 冻结/恢复
  const pageVisibleRef = useRef(isPageVisible())
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
  const codeFingerprint = useMemo(() => {
    const ph = code.pageHtml || ''
    const st = code.styles || ''
    const js = getCodeForMode(code, 'page') || ''
    return `${ph.length}:${st.length}:${js.length}`
  }, [code])

  const localeRef = useRef(locale)
  useEffect(() => { localeRef.current = locale }, [locale])

  const animationConfigRef = useRef<AnimationConfigRef>(animationConfig)
  useEffect(() => { animationConfigRef.current = animationConfig }, [animationConfig])

  // 尺寸更新
  useEffect(() => {
    if (!iframeRef.current || dimensions.width === 0)
      return
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
    if (!bridgeRef.current || !isReady)
      return
    bridgeRef.current.emit('locale:change', locale)
  }, [locale, isReady])

  // 主题变化
  // 🎯 优化：使用共享订阅器，确保所有组件都能响应变化
  useEffect(() => {
    if (!isReady)
      return
    return subscribeToTheme((isDark) => {
      const bridge = bridgeRef.current
      if (bridge) {
        bridge.emit('theme:change', isDark ? 'dark' : 'light')
      }
    })
  }, [isReady])

  // 主色调变化
  // 🎯 优化：使用共享的 colorSubscriber，避免每个组件都创建 MutationObserver
  // 🎯 修复：isReady 时立即发送当前颜色，确保多窗口场景下正确初始化
  useEffect(() => {
    if (!isReady)
      return

    // 立即发送当前主色调，确保新打开的窗口能获取到
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

  // 媒体状态变化 - 转发给 Tapp 沙箱
  useEffect(() => {
    if (!isReady)
      return

    const handleMusicStateChange = (e: Event) => {
      const detail = (e as CustomEvent).detail
      if (!detail)
        return

      const bridge = bridgeRef.current
      if (!bridge)
        return

      // 检查 Tapp 是否有 media:read 权限
      const tapp = tappInstanceRef.current
      if (!tapp?.grantedPermissions?.includes('media:read'))
        return

      const currentSong = detail.currentSong as Record<string, unknown> | null
      const currentTime = (detail.currentTime as number) || 0
      const audioDuration = (detail.audioDuration as number) || (currentSong?.duration as number) || 0
      const volume = (detail.volume as number) || 0.7
      const playMode = (detail.playMode as string) || 'loop'

      // 将内部 playMode 映射为 API 模式
      const modeMap: Record<string, string> = {
        loop: 'loop',
        single: 'single',
        shuffle: 'shuffle',
      }

      // 构建状态对象
      const mediaState = {
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
          percentage: audioDuration > 0 ? (currentTime / audioDuration) * 100 : 0,
        },
        position: currentTime,
        volume: Math.round(volume * 100), // 0-100
        mode: modeMap[playMode] || 'sequence',
        muted: volume === 0,
        // 歌词信息
        lyrics: detail.lyrics || [],
        currentLyricIndex: detail.currentLyricIndex ?? -1,
        // 动态主题色（完整颜色对象）
        primaryColor: detail.musicColor || '#fc3c44',
        secondaryColor: detail.musicColors?.secondary || detail.musicColor || '#fc3c44',
        accentColor: detail.musicColors?.accent || detail.musicColor || '#fc3c44',
        lightColor: detail.musicColors?.light || '#ffffff',
        darkColor: detail.musicColors?.dark || '#000000',
      }

      bridge.emit('mediaStateChange', mediaState)
    }

    window.addEventListener('music-player-state-change', handleMusicStateChange)
    return () => {
      window.removeEventListener('music-player-state-change', handleMusicStateChange)
    }
  }, [isReady])

  // 动画级别变化
  useEffect(() => {
    if (!isReady)
      return
    bridgeRef.current?.emit('animationLevel:change', animationConfig.level)
  }, [isReady, animationConfig.level])

  const handleReady = useCallback(() => {
    setIsReady(true)
    onReady?.()
  }, [onReady])

  // 初始化
  // 🎯 依赖优化：只使用稳定的 ID 和指纹，不使用对象引用
  useEffect(() => {
    if (!iframeRef.current)
      return

    // 🎯 从 ref 获取当前对象，避免闭包陈旧问题
    const currentTappInstance = tappInstanceRef.current
    const currentCode = codeRef.current

    const bridge = createTappBridge()
    bridgeRef.current = bridge

    const permission = createPermissionController(currentTappInstance)
    permissionRef.current = permission

    bridge.initialize(iframeRef.current, currentTappInstance)

    // 注册所有处理器
    registerLifecycleHandlers(bridge, currentTappInstance, handleReady)
    registerUIHandlers(bridge, () => localeRef.current, onNotification)
    registerStorageHandlers(bridge, currentTappInstance.id)
    registerUserHandlers(bridge, currentTappInstance)
    registerFileHandlers(bridge)
    registerWidgetHandlers(bridge, currentTappInstance)
    registerPlatformHandlers(bridge, currentTappInstance)
    registerAIHandlers(bridge, permission, currentTappInstance)
    registerReportHandlers(bridge, currentTappInstance)
    registerMediaHandlers(bridge, currentTappInstance)
    registerBackgroundHandlers(bridge, currentTappInstance)
    registerAnimationHandlers(bridge, animationConfigRef)
    registerDynamicContentHandlers(bridge, currentTappInstance)
    registerAdvancedHandlers(bridge, currentTappInstance)
    registerContextHandlers(bridge, currentTappInstance)

    // 获取 session token（Bridge 在 initialize 时已生成）
    const sessionToken = bridge.getSessionToken()

    // 生成 HTML（传递 session token 用于安全验证）
    // 🎯 使用 ref 获取 safeInsets，避免依赖变化重建 iframe
    const html = generatePageHTML(currentTappInstance, currentCode, sessionToken, safeInsetsRef.current)
    const blob = new Blob([html], { type: 'text/html' })
    const url = URL.createObjectURL(blob)
    iframeRef.current.src = url

    return () => {
      setIsReady(false)
      URL.revokeObjectURL(url)
      bridge.destroy()
      onDestroy?.()
    }
  // 🎯 稳定依赖：只有这些真正改变时才重建 iframe
  // - tappInstance.id: Tapp 实例 ID
  // - codeFingerprint: 代码指纹（内容变化才会变）
  // ⚠️ 注意：safeInsets 通过 ref 获取，不作为依赖（通过 postMessage 动态更新）
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tappInstance.id, codeFingerprint, handleReady])

  // 🎯 WebKit 检测
  const [isWebKitBrowser, setIsWebKitBrowser] = useState(false)
  useEffect(() => {
    setIsWebKitBrowser(isWebKit())
  }, [])

  // 🎯 WebKit 专用渲染：最简化的样式，避免任何可能影响 iframe 渲染的 CSS
  if (isWebKitBrowser) {
    return (
      <div
        ref={containerRef}
        className={`tapp-page-sandbox ${className || ''}`}
        style={{
          position: 'relative',
          width: '100%',
          height: '100%',
          // 🎯 WebKit: 移除所有可能影响渲染的 CSS
          // 不使用 overflow: hidden, isolation, contain 等
          ...style,
        }}
        data-no-ripple
      >
        <iframe
          ref={iframeRef}
          className="tapp-page-iframe"
          style={{
            position: 'absolute',
            top: 0,
            left: 0,
            width: '100%',
            height: '100%',
            border: 'none',
            display: 'block',
            // 🎯 WebKit: 确保 iframe 可见
            visibility: 'visible',
            opacity: 1,
          }}
          sandbox={IFRAME_SANDBOX_ATTRS}
          referrerPolicy="no-referrer"
          title={tappInstance.manifest.name}
          allowFullScreen
          // @ts-expect-error Safari webkit prefix
          webkitallowfullscreen="true"
        />
      </div>
    )
  }

  // 非 WebKit 浏览器：使用完整样式
  return (
    <div
      ref={containerRef}
      className={`tapp-page-sandbox ${className || ''}`}
      style={{
        position: 'relative',
        width: '100%',
        height: '100%',
        overflow: 'hidden',
        isolation: 'isolate',
        ...style,
      }}
      data-no-ripple
    >
      <iframe
        ref={iframeRef}
        className="tapp-page-iframe"
        style={{
          position: 'absolute',
          top: 0,
          left: 0,
          width: '100%',
          height: '100%',
          border: 'none',
          display: 'block',
        }}
        sandbox={IFRAME_SANDBOX_ATTRS}
        referrerPolicy="no-referrer"
        title={tappInstance.manifest.name}
        allowFullScreen
        // @ts-expect-error Safari webkit prefix
        webkitallowfullscreen="true"
      />
    </div>
  )
}

export default TappPageSandbox
