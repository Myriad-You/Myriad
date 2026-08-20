/**
 * 示例 Tapp 类型定义
 */

import type { TappPlaygroundCode } from '../../services/TappPlaygroundService'
import type { TappManifest } from '../../types'

export type { TappPlaygroundCode } from '../../services/TappPlaygroundService'

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
/** 示例 Tapp 数据 */
export interface ExampleTapp {
  manifest: TappManifest
  /** 代码结构（分离架构） */
  code: TappPlaygroundCode
  tags: string[]
}
