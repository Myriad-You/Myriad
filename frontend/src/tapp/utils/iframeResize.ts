/**
 * Tapp iframe 自适应工具
 * 统一处理 iframe 在不同场景下的尺寸适配
 *
 * 🚀 性能优化:
 * - 集成统一动画调度系统 (animation/core.ts)
 * - 复用全局 ResizeObserver，避免重复创建
 * - RAF 批量更新，防止布局抖动
 * - 自动节流，低帧率时跳过更新
 *
 * 支持场景:
 * - Widget 模式: 小组件尺寸 (1x1 到 4x4)
 * - Page 模式: 全屏/嵌入页面
 * - 预览模式: 库中拖拽预览
 *
 * 🎯 开发者零配置:
 * - 自动注入 CSS 变量和响应式工具类
 * - 自动发送尺寸消息到 iframe
 * - Tapp 代码可直接使用 CSS 变量或监听 tapp:resize 事件
 */

import { useLayoutEffect, useRef, useState } from 'react'
import {
  isPageVisible,
  observeResizeAtomic as observeResize,
} from '../../hooks/animation'
import { STANDARD_CELL_SIZE } from '../../hooks/useWidgetSize'

/** iframe 容器尺寸信息 */
export interface IframeDimensions {
  width: number
  height: number
  scale: number
  fontScale: number
  /** 是否为紧凑模式 (width < 150 或 height < 150) */
  isCompact: boolean
  /** 是否为迷你模式 (width < 100 或 height < 100) */
  isMini: boolean
  /** 安全区域内边距 - 顶部 (避免与控制条重叠) */
  safeInsetTop?: number
  /** 安全区域内边距 - 右侧 */
  safeInsetRight?: number
  /** 安全区域内边距 - 底部 */
  safeInsetBottom?: number
  /** 安全区域内边距 - 左侧 */
  safeInsetLeft?: number
}

/** 与 useWidgetSize 同一基准，保证库预览 / 网格实装比例一致 */
const BASE_CELL_SIZE = STANDARD_CELL_SIZE

/**
 * 尺寸变化阈值（像素）
 * 只有当宽度或高度变化超过此值时才触发更新
 * 这可以过滤掉 F12 开发工具打开时的微小尺寸变化
 */
const RESIZE_THRESHOLD = 10

/** 默认尺寸 */
const DEFAULT_DIMENSIONS: IframeDimensions = {
  width: 0,
  height: 0,
  scale: 1,
  fontScale: 1,
  isCompact: false,
  isMini: false,
  safeInsetTop: 0,
  safeInsetRight: 0,
  safeInsetBottom: 0,
  safeInsetLeft: 0,
}

/** 计算尺寸信息（优化：避免多次 Math 调用） */
function calculateDimensions(width: number, height: number): IframeDimensions {
  const minSize = width < height ? width : height
  const rawScale = minSize / BASE_CELL_SIZE
  const scale = rawScale < 0.1 ? 0.1 : rawScale

  // 字体缩放：clamp(0.6, 0.2 + scale * 0.8, 1.2)
  const rawFontScale = 0.2 + scale * 0.8
  const fontScale =
    rawFontScale < 0.6 ? 0.6 : rawFontScale > 1.2 ? 1.2 : rawFontScale

  return {
    width,
    height,
    scale,
    fontScale,
    isCompact: width < 150 || height < 150,
    isMini: width < 100 || height < 100,
    safeInsetTop: 0,
    safeInsetRight: 0,
    safeInsetBottom: 0,
    safeInsetLeft: 0,
  }
}

/**
 * Hook: 监听容器尺寸变化
 *
 * 🚀 性能特性:
 * - 复用全局 ResizeObserver (animation/core.ts)
 * - 自动节流，页面不可见时暂停
 * - 使用 batchWrite 批量更新，避免布局抖动
 *
 * @example
 * ```tsx
 * function TappContainer() {
 *   const { containerRef, dimensions } = useIframeResize()
 *   // dimensions 包含 width, height, scale, fontScale, isCompact, isMini
 *   return <div ref={containerRef}>...</div>
 * }
 * ```
 */
export function useIframeResize<T extends HTMLElement = HTMLDivElement>(): {
  containerRef: React.RefObject<T>
  dimensions: IframeDimensions
} {
  const containerRef = useRef<T>(null!)
  const [dimensions, setDimensions] =
    useState<IframeDimensions>(DEFAULT_DIMENSIONS)

  // 用于追踪是否已经完成初始化，避免 F12 等微小变化触发更新
  const initializedRef = useRef(false)
  const lastDimensionsRef = useRef<IframeDimensions>(DEFAULT_DIMENSIONS)

  // useLayoutEffect 确保在 DOM 更新后、浏览器绘制前执行
  // 这样可以确保 ref 已经绑定到元素
  useLayoutEffect(() => {
    const element = containerRef.current
    if (!element) return

    // 立即计算初始尺寸
    // 注意：页面入场动画期间 getBoundingClientRect 可能返回动画中间状态的尺寸
    // 但这没关系，ResizeObserver 会在动画结束后提供正确的尺寸
    const rect = element.getBoundingClientRect()
    if (rect.width > 0 || rect.height > 0) {
      const initial = calculateDimensions(rect.width, rect.height)
      setDimensions(initial)
      lastDimensionsRef.current = initial
      initializedRef.current = true
    }

    // 设置 ResizeObserver 监听后续变化
    const unsubscribe = observeResize(element, (entry: ResizeObserverEntry) => {
      // 页面不可见时跳过更新
      if (!isPageVisible()) return

      const { width, height } = entry.contentRect

      // 跳过无效尺寸
      if (width === 0 && height === 0) return

      const prev = lastDimensionsRef.current

      // 初始化时（prev 为默认值 0,0）无条件更新
      // 后续更新时使用阈值过滤 F12 等微小变化
      const isInitial = !initializedRef.current
      const widthChanged = Math.abs(prev.width - width) > RESIZE_THRESHOLD
      const heightChanged = Math.abs(prev.height - height) > RESIZE_THRESHOLD

      if (isInitial || widthChanged || heightChanged) {
        const newDimensions = calculateDimensions(width, height)
        lastDimensionsRef.current = newDimensions
        initializedRef.current = true
        setDimensions(newDimensions)
      }
    })

    return () => {
      unsubscribe()
      initializedRef.current = false
    }
  }, [])

  return { containerRef, dimensions }
}

/** 消息去重缓存 */
const lastSentDimensions = new WeakMap<HTMLIFrameElement, string>()

/**
 * 向 iframe 发送尺寸更新消息
 *
 * 🚀 优化:
 * - 消息去重：相同尺寸不重复发送
 * - 使用整数 key 避免字符串拼接
 * - postMessage 本身是异步的，无需额外批量处理
 */
export function sendResizeMessage(
  iframe: HTMLIFrameElement | null,
  dimensions: IframeDimensions,
): void {
  if (!iframe?.contentWindow) return

  // 消息去重：包含尺寸和安全区域信息
  // 使用字符串 key 确保所有相关属性都被考虑
  const key = `${dimensions.width | 0},${dimensions.height | 0},${dimensions.safeInsetTop || 0},${dimensions.safeInsetRight || 0},${dimensions.safeInsetBottom || 0},${dimensions.safeInsetLeft || 0}`
  const lastKey = lastSentDimensions.get(iframe)
  if (lastKey === key) return
  lastSentDimensions.set(iframe, key)

  try {
    iframe.contentWindow.postMessage(
      {
        type: 'event',
        action: 'container:resize',
        payload: dimensions,
      },
      '*',
    )
  } catch {
    // iframe 可能未加载完成或已销毁
  }
}

/**
 * 计算 Widget 模式下的最佳尺寸
 * 根据 widget size (如 '2x2') 和容器尺寸计算
 */
export function calculateWidgetDimensions(
  widgetSize: string,
  containerWidth: number,
  containerHeight: number,
): IframeDimensions {
  // 解析 widget size
  const [cols, rows] = widgetSize.split('x').map(Number)
  const validCols = Number.isNaN(cols) ? 1 : cols
  const validRows = Number.isNaN(rows) ? 1 : rows

  // 计算期望尺寸
  const expectedWidth = validCols * BASE_CELL_SIZE
  const expectedHeight = validRows * BASE_CELL_SIZE

  // 计算实际缩放比例（取较小值保持宽高比）
  const scaleX = containerWidth / expectedWidth
  const scaleY = containerHeight / expectedHeight
  const scale = Math.min(scaleX, scaleY, 2) // 最大放大 2 倍

  // 字体缩放使用更平滑的曲线
  const fontScale = Math.max(0.5, Math.min(1.5, 0.3 + scale * 0.7))

  return {
    width: containerWidth,
    height: containerHeight,
    scale: Math.max(0.1, scale),
    fontScale,
    isCompact: containerWidth < 150 || containerHeight < 150,
    isMini: containerWidth < 100 || containerHeight < 100,
  }
}

/**
 * 计算 Page 模式下的尺寸
 * 页面模式通常填满容器
 */
export function calculatePageDimensions(
  containerWidth: number,
  containerHeight: number,
): IframeDimensions {
  return {
    width: containerWidth,
    height: containerHeight,
    scale: 1,
    fontScale: 1,
    isCompact: false,
    isMini: false,
  }
}

/**
 * iframe 样式生成器
 * 根据模式和尺寸生成 CSS 样式
 */
export function getIframeStyles(
  mode: 'widget' | 'page',
  _dimensions: IframeDimensions,
  _isPreview?: boolean,
): React.CSSProperties {
  const baseStyles: React.CSSProperties = {
    width: '100%',
    height: '100%',
    border: 'none',
    backgroundColor: 'transparent',
    display: 'block',
  }

  if (mode === 'widget') {
    return {
      ...baseStyles,
      // Widget 模式：确保完全填满容器
      position: 'absolute',
      top: 0,
      left: 0,
      right: 0,
      bottom: 0,
    }
  }

  if (mode === 'page') {
    return {
      ...baseStyles,
      // Page 模式：填满容器，可能需要滚动
      position: 'absolute',
      top: 0,
      left: 0,
      right: 0,
      bottom: 0,
      overflow: 'hidden',
    }
  }

  return baseStyles
}

/**
 * 沙箱 HTML 中注入的自适应脚本
 *
 * 🎯 开发者零配置:
 * - 自动接收父窗口尺寸消息
 * - 自动更新 CSS 变量
 * - 自动触发 tapp:resize 事件
 * - 支持容器查询 (Container Queries)
 *
 * 🚀 性能优化:
 * - CSS 变量缓存避免重复设置
 * - RAF 节流事件派发
 * - 被动事件监听
 * - 避免强制布局
 */
export const IFRAME_RESIZE_SCRIPT = `
(function() {
  'use strict';

  // 读取预设的安全区域值（由框架注入）
  var initialInsets = window._TAPP_INITIAL_SAFE_INSETS || {};

  // 使用 Object.create(null) 避免原型链查找
  var dims = Object.create(null);
  dims.width = window.innerWidth;
  dims.height = window.innerHeight;
  dims.scale = 1;
  dims.fontScale = 1;
  dims.isCompact = false;
  dims.isMini = false;
  // 安全区域内边距（使用预设值或默认 0）
  dims.safeInsetTop = initialInsets.top || 0;
  dims.safeInsetRight = initialInsets.right || 0;
  dims.safeInsetBottom = initialInsets.bottom || 0;
  dims.safeInsetLeft = initialInsets.left || 0;

  // 暴露给 Tapp SDK
  window._TAPP_DIMENSIONS = dims;

  // CSS 变量缓存（避免重复设置）
  var cssCache = Object.create(null);
  var root = document.documentElement;
  var rootStyle = root.style;

  // 批量更新 CSS 变量（性能优化）
  function updateCSS(d) {
    var w = d.width + 'px';
    var h = d.height + 'px';
    var s = String(d.scale);
    var fs = String(d.fontScale);
    var bf = Math.max(10, 14 * d.fontScale) + 'px';
    var ic = d.isCompact ? '1' : '0';
    var im = d.isMini ? '1' : '0';
    // 安全区域内边距
    var sit = (d.safeInsetTop || 0) + 'px';
    var sir = (d.safeInsetRight || 0) + 'px';
    var sib = (d.safeInsetBottom || 0) + 'px';
    var sil = (d.safeInsetLeft || 0) + 'px';

    // 仅更新变化的变量
    if (cssCache.w !== w) { cssCache.w = w; rootStyle.setProperty('--tapp-container-width', w); }
    if (cssCache.h !== h) { cssCache.h = h; rootStyle.setProperty('--tapp-container-height', h); }
    if (cssCache.s !== s) { cssCache.s = s; rootStyle.setProperty('--tapp-scale', s); }
    if (cssCache.fs !== fs) { cssCache.fs = fs; rootStyle.setProperty('--tapp-font-scale', fs); }
    if (cssCache.bf !== bf) { cssCache.bf = bf; rootStyle.setProperty('--tapp-base-font-size', bf); }
    if (cssCache.ic !== ic) { cssCache.ic = ic; rootStyle.setProperty('--tapp-is-compact', ic); }
    if (cssCache.im !== im) { cssCache.im = im; rootStyle.setProperty('--tapp-is-mini', im); }
    // 安全区域 CSS 变量
    if (cssCache.sit !== sit) { cssCache.sit = sit; rootStyle.setProperty('--tapp-safe-inset-top', sit); }
    if (cssCache.sir !== sir) { cssCache.sir = sir; rootStyle.setProperty('--tapp-safe-inset-right', sir); }
    if (cssCache.sib !== sib) { cssCache.sib = sib; rootStyle.setProperty('--tapp-safe-inset-bottom', sib); }
    if (cssCache.sil !== sil) { cssCache.sil = sil; rootStyle.setProperty('--tapp-safe-inset-left', sil); }

    // 使用 classList 批量操作（比 toggle 更快）
    var cl = document.body.classList;
    if (d.isCompact && !cl.contains('tapp-compact')) cl.add('tapp-compact');
    else if (!d.isCompact && cl.contains('tapp-compact')) cl.remove('tapp-compact');
    if (d.isMini && !cl.contains('tapp-mini')) cl.add('tapp-mini');
    else if (!d.isMini && cl.contains('tapp-mini')) cl.remove('tapp-mini');
  }

  // RAF 节流的事件派发
  var eventQueued = false;
  function queueResizeEvent() {
    if (eventQueued) return;
    eventQueued = true;
    requestAnimationFrame(function() {
      eventQueued = false;
      window.dispatchEvent(new CustomEvent('tapp:resize', { detail: dims, bubbles: false }));
    });
  }

  // 消息处理（优化分支）
  function onMessage(e) {
    var msg = e.data;
    if (!msg || msg.type !== 'event' || msg.action !== 'container:resize') return;

    var p = msg.payload;
    dims.width = p.width;
    dims.height = p.height;
    dims.scale = p.scale;
    dims.fontScale = p.fontScale;
    dims.isCompact = p.isCompact;
    dims.isMini = p.isMini;
    // 安全区域内边距
    dims.safeInsetTop = p.safeInsetTop || 0;
    dims.safeInsetRight = p.safeInsetRight || 0;
    dims.safeInsetBottom = p.safeInsetBottom || 0;
    dims.safeInsetLeft = p.safeInsetLeft || 0;

    updateCSS(dims);
    queueResizeEvent();
  }

  window.addEventListener('message', onMessage, false);

  // 备用：监听 iframe resize（节流 100ms）
  var resizeTimer;
  window.addEventListener('resize', function() {
    clearTimeout(resizeTimer);
    resizeTimer = setTimeout(function() {
      dims.width = window.innerWidth;
      dims.height = window.innerHeight;
      queueResizeEvent();
    }, 100);
  }, { passive: true });

  // 初始化
  updateCSS(dims);

  // 通知父窗口就绪
  // 注意：消息格式需要符合 TappBridge 规范，包含 id、type、action、_sessionToken
  // action 只能包含字母数字下划线和点号；token 由宿主注入到 window._TAPP_SESSION_TOKEN
  if (window.parent !== window) {
    try {
      window.parent.postMessage({
        type: 'event',
        id: 'tapp-ready-' + Date.now(),
        action: 'tapp.ready',
        payload: null,
        timestamp: Date.now(),
        _sessionToken: typeof window._TAPP_SESSION_TOKEN === 'string'
          ? window._TAPP_SESSION_TOKEN
          : undefined
      }, '*');
    } catch(e) {}
  }
})();
`

/**
 * 沙箱 HTML 中注入的自适应 CSS
 *
 * 🎯 开发者零配置响应式:
 * - CSS 变量自动更新
 * - 响应式工具类（类似 Tailwind）
 * - 紧凑/迷你模式自动切换
 * - 容器查询支持
 *
 * 🚀 性能优化:
 * - 使用 CSS 层叠 (@layer) 控制优先级
 * - GPU 加速动画 (transform, opacity)
 * - 减少选择器复杂度
 * - will-change 提示
 */
export const IFRAME_RESIZE_CSS = `
/* CSS 层叠定义：确保正确的样式优先级 */
@layer tapp-reset, tapp-base, tapp-utilities, tapp-responsive;

/* 重置层 */
@layer tapp-reset {
  *, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }
}

/* 基础层 */
@layer tapp-base {
  :root {
    --tapp-container-width: 100vw;
    --tapp-container-height: 100vh;
    --tapp-scale: 1;
    --tapp-font-scale: 1;
    --tapp-base-font-size: 14px;
    --tapp-is-compact: 0;
    --tapp-is-mini: 0;
    --tapp-spacing-unit: calc(4px * var(--tapp-scale));
    --tapp-radius-unit: calc(4px * var(--tapp-scale));
    /* 安全区域内边距 - 用于避免与控制条/控制岛重叠 */
    --tapp-safe-inset-top: 0px;
    --tapp-safe-inset-right: 0px;
    --tapp-safe-inset-bottom: 0px;
    --tapp-safe-inset-left: 0px;
    color-scheme: light dark;
  }

  html {
    height: 100%;
    width: 100%;
    font-size: var(--tapp-base-font-size);
    -webkit-text-size-adjust: 100%;
    -webkit-font-smoothing: antialiased;
    text-rendering: optimizeLegibility;
  }

  body {
    width: 100%;
    height: 100%;
    min-height: 100%;
    overflow: hidden;
    font-family: system-ui, -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
    font-size: inherit;
    line-height: 1.5;
    background: transparent;
  }

  /* 根容器 */
  #tapp-root {
    position: absolute;
    inset: 0;
    overflow: hidden;
    container-type: size;
    container-name: tapp;
    contain: layout style;
  }

  /* 背景层 - 填满整个屏幕，无安全区域限制 */
  #tapp-background {
    position: absolute;
    inset: 0;
    z-index: 0;
    pointer-events: none;
    overflow: hidden;
  }

  /* 内容层 - 自动应用安全区域 padding */
  #tapp-content {
    position: absolute;
    inset: 0;
    z-index: 1;
    overflow: auto;
    box-sizing: border-box;
    -webkit-overflow-scrolling: touch;
    overscroll-behavior: contain;
    /* 允许纵向滚动，同时让点击更快被识别为 tap 而非 scroll 手势 */
    touch-action: pan-y pinch-zoom;
    /* 安全区域内边距 - 开发者无需手动处理 */
    padding-top: var(--tapp-safe-inset-top, 0px);
    padding-right: var(--tapp-safe-inset-right, 0px);
    padding-bottom: var(--tapp-safe-inset-bottom, 0px);
    padding-left: var(--tapp-safe-inset-left, 0px);
  }

  /* 移动端控件：避免 300ms 语义延迟、保证可点 */
  button,
  a,
  input,
  textarea,
  select,
  [role='button'],
  [onclick] {
    touch-action: manipulation;
    -webkit-tap-highlight-color: color-mix(
      in srgb,
      var(--color-primary, #6366f1) 18%,
      transparent
    );
  }

  /* Widget 模式下不需要分层和安全区域 */
  .tapp-mode-widget #tapp-root {
    overflow: hidden;
    contain: strict;
  }
  .tapp-mode-widget #tapp-background,
  .tapp-mode-widget #tapp-content {
    padding: 0;
  }
}

/* 工具类层 */
@layer tapp-utilities {
  /* 文本 */
  .tapp-text-xs { font-size: calc(.75rem * var(--tapp-font-scale)); }
  .tapp-text-sm { font-size: calc(.875rem * var(--tapp-font-scale)); }
  .tapp-text-base { font-size: calc(1rem * var(--tapp-font-scale)); }
  .tapp-text-lg { font-size: calc(1.125rem * var(--tapp-font-scale)); }
  .tapp-text-xl { font-size: calc(1.25rem * var(--tapp-font-scale)); }
  .tapp-text-2xl { font-size: calc(1.5rem * var(--tapp-font-scale)); }
  .tapp-text-3xl { font-size: calc(1.875rem * var(--tapp-font-scale)); }

  /* 间距 - 使用 CSS 变量减少计算 */
  .tapp-p-0 { padding: 0; }
  .tapp-p-1 { padding: var(--tapp-spacing-unit); }
  .tapp-p-2 { padding: calc(var(--tapp-spacing-unit) * 2); }
  .tapp-p-3 { padding: calc(var(--tapp-spacing-unit) * 3); }
  .tapp-p-4 { padding: calc(var(--tapp-spacing-unit) * 4); }

  .tapp-px-1 { padding-inline: var(--tapp-spacing-unit); }
  .tapp-px-2 { padding-inline: calc(var(--tapp-spacing-unit) * 2); }
  .tapp-px-3 { padding-inline: calc(var(--tapp-spacing-unit) * 3); }
  .tapp-px-4 { padding-inline: calc(var(--tapp-spacing-unit) * 4); }

  .tapp-py-1 { padding-block: var(--tapp-spacing-unit); }
  .tapp-py-2 { padding-block: calc(var(--tapp-spacing-unit) * 2); }
  .tapp-py-3 { padding-block: calc(var(--tapp-spacing-unit) * 3); }
  .tapp-py-4 { padding-block: calc(var(--tapp-spacing-unit) * 4); }

  .tapp-m-0 { margin: 0; }
  .tapp-m-1 { margin: var(--tapp-spacing-unit); }
  .tapp-m-2 { margin: calc(var(--tapp-spacing-unit) * 2); }
  .tapp-m-auto { margin: auto; }

  .tapp-gap-1 { gap: var(--tapp-spacing-unit); }
  .tapp-gap-2 { gap: calc(var(--tapp-spacing-unit) * 2); }
  .tapp-gap-3 { gap: calc(var(--tapp-spacing-unit) * 3); }
  .tapp-gap-4 { gap: calc(var(--tapp-spacing-unit) * 4); }

  /* 圆角 */
  .tapp-rounded { border-radius: var(--tapp-radius-unit); }
  .tapp-rounded-lg { border-radius: calc(var(--tapp-radius-unit) * 2); }
  .tapp-rounded-xl { border-radius: calc(var(--tapp-radius-unit) * 3); }
  .tapp-rounded-full { border-radius: 9999px; }

  /* 阴影 - 使用 filter 替代 box-shadow 获得更好的性能 */
  .tapp-shadow-sm { filter: drop-shadow(0 1px 1px rgb(0 0 0 / .05)); }
  .tapp-shadow { filter: drop-shadow(0 1px 2px rgb(0 0 0 / .1)); }
  .tapp-shadow-lg { filter: drop-shadow(0 4px 6px rgb(0 0 0 / .1)); }

  /* 布局 */
  .tapp-flex { display: flex; }
  .tapp-flex-col { flex-direction: column; }
  .tapp-flex-center { display: flex; place-items: center; place-content: center; }
  .tapp-flex-between { display: flex; align-items: center; justify-content: space-between; }
  .tapp-grid { display: grid; }
  .tapp-absolute-fill { position: absolute; inset: 0; }
  .tapp-relative { position: relative; }
  .tapp-overflow-hidden { overflow: hidden; }
  .tapp-overflow-auto { overflow: auto; }
  .tapp-w-full { width: 100%; }
  .tapp-h-full { height: 100%; }

  /* 安全区域工具类 - 使用安全区域内边距 */
  .tapp-safe-p {
    padding-top: var(--tapp-safe-inset-top, 0px);
    padding-right: var(--tapp-safe-inset-right, 0px);
    padding-bottom: var(--tapp-safe-inset-bottom, 0px);
    padding-left: var(--tapp-safe-inset-left, 0px);
  }
  .tapp-safe-pt { padding-top: var(--tapp-safe-inset-top, 0px); }
  .tapp-safe-pr { padding-right: var(--tapp-safe-inset-right, 0px); }
  .tapp-safe-pb { padding-bottom: var(--tapp-safe-inset-bottom, 0px); }
  .tapp-safe-pl { padding-left: var(--tapp-safe-inset-left, 0px); }
  .tapp-safe-px {
    padding-left: var(--tapp-safe-inset-left, 0px);
    padding-right: var(--tapp-safe-inset-right, 0px);
  }
  .tapp-safe-py {
    padding-top: var(--tapp-safe-inset-top, 0px);
    padding-bottom: var(--tapp-safe-inset-bottom, 0px);
  }
  /* 安全区域 + 额外边距 */
  .tapp-safe-p-4 {
    padding-top: calc(var(--tapp-safe-inset-top, 0px) + var(--tapp-spacing-unit) * 4);
    padding-right: calc(var(--tapp-safe-inset-right, 0px) + var(--tapp-spacing-unit) * 4);
    padding-bottom: calc(var(--tapp-safe-inset-bottom, 0px) + var(--tapp-spacing-unit) * 4);
    padding-left: calc(var(--tapp-safe-inset-left, 0px) + var(--tapp-spacing-unit) * 4);
  }
  .tapp-safe-p-6 {
    padding-top: calc(var(--tapp-safe-inset-top, 0px) + var(--tapp-spacing-unit) * 6);
    padding-right: calc(var(--tapp-safe-inset-right, 0px) + var(--tapp-spacing-unit) * 6);
    padding-bottom: calc(var(--tapp-safe-inset-bottom, 0px) + var(--tapp-spacing-unit) * 6);
    padding-left: calc(var(--tapp-safe-inset-left, 0px) + var(--tapp-spacing-unit) * 6);
  }

  /* 动画 - GPU 加速 */
  .tapp-transition {
    transition: color .15s, background-color .15s, border-color .15s, opacity .15s, transform .15s;
    will-change: transform, opacity;
  }

  .tapp-animate-fade-in {
    animation: tapp-fade .2s ease-out;
  }

  .tapp-animate-scale-in {
    animation: tapp-scale .2s ease-out;
  }
}

/* 响应式层 */
@layer tapp-responsive {
  /* 紧凑模式 */
  .tapp-compact .tapp-hide-compact { display: none; }

  /* 迷你模式 */
  .tapp-mini .tapp-hide-mini { display: none; }
  .tapp-mini .tapp-hide-compact { display: none; }

  /* 容器查询 */
  @container tapp (width < 150px) { .tapp-cq-hide-sm { display: none; } }
  @container tapp (width < 100px) { .tapp-cq-hide-xs { display: none; } }
  @container tapp (width >= 200px) { .tapp-cq-show-md { display: block; } }
  @container tapp (width >= 300px) { .tapp-cq-show-lg { display: block; } }
}

/* 动画关键帧（在层外定义） */
@keyframes tapp-fade { from { opacity: 0; } }
@keyframes tapp-scale { from { opacity: 0; transform: scale(.95); } }

/* 减少动画偏好 */
@media (prefers-reduced-motion: reduce) {
  .tapp-transition { transition: none; }
  .tapp-animate-fade-in, .tapp-animate-scale-in { animation: none; }
}
`

export default {
  useIframeResize,
  sendResizeMessage,
  calculateWidgetDimensions,
  calculatePageDimensions,
  getIframeStyles,
  IFRAME_RESIZE_SCRIPT,
  IFRAME_RESIZE_CSS,
}
