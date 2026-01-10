/**
 * 示例 Tapp 类型定义
 */

import type { TappManifest } from '../../types'

/**
 * Tapp 代码结构（分离架构）
 *
 * 支持三种渲染方式：
 * 1. 纯 JS 模式：传统的 Tapp.widgets.render() / Tapp.pages.render()
 * 2. 纯 HTML 模式：只用 widgetHtml/pageHtml，无需 JS（适合纯静态展示）
 * 3. 混合模式（推荐）：HTML 定义结构 + JS 处理交互（性能最优）
 *
 * 混合模式示例：
 * - widgetHtml: '<div class="tapp-container"><button id="send-btn">发送</button></div>'
 * - core: 'document.getElementById("send-btn").onclick = function() { ... }'
 */
export interface TappCodeStructure {
  /** 核心代码 - 共享逻辑，所有模式都会加载 */
  core: string
  /** Widget 代码 - 仅 widget 模式加载（JS） */
  widget?: string
  /** 页面代码 - 仅 page 模式加载（JS） */
  page?: string
  /** 自定义 CSS 样式 */
  styles?: string
  /** Widget HTML 模板 - 直接注入到容器，与 JS 配合使用 */
  widgetHtml?: string
  /** Page HTML 模板 - 直接注入到容器，与 JS 配合使用 */
  pageHtml?: string
  /** Widget 专用编译后的 Tailwind CSS */
  widgetCSS?: string
  /** Page 专用编译后的 Tailwind CSS */
  pageCSS?: string
}

/** 示例 Tapp 数据 */
export interface ExampleTapp {
  manifest: TappManifest
  /** 代码结构（分离架构） */
  code: TappCodeStructure
  category: 'widget' | 'tool' | 'platform' | 'demo' | 'test'
  tags: string[]
}

/** 获取指定模式的完整代码 */
export function getCodeForMode(
  code: TappCodeStructure,
  mode: 'widget' | 'page' | 'background',
): string {
  switch (mode) {
    case 'widget':
      return code.widget
        ? `${code.core}\n\n// ========== Widget Code ==========\n${code.widget}`
        : code.core
    case 'page':
      return code.page
        ? `${code.core}\n\n// ========== Page Code ==========\n${code.page}`
        : code.core
    case 'background':
      return code.core
    default:
      return code.core
  }
}
