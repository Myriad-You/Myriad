/**
 * 嵌入页（`/embed/*`）和外面那页之间的唯一入口。外面通常是博客页：它自己不跑脚本
 * （CSP 是 `script-src 'none'`），所以嵌入页同源时由嵌入页来碰它——量高度、画注释、
 * 挂创作台。外面那页的节点属于另一个 realm：不做 `instanceof`，只按属性和方法用。
 *
 * 和博客页约定的 iframe 属性（crates/myriad-blog 写，嵌入页读写）：
 *   data-embed-fit     这个框的高度由里面按内容定（评论、友链申请）
 *   data-embed-scheme  里面现在是深色还是浅色，第一帧之前由 utils/themeBootScript.ts 写上，
 *                      外面的样式按它给框上色
 *   data-embed-ready   里面画好了（小组件）。之前外面只显示占位底，框是透明的，
 *                      不在小框里露出半成品和加载圈
 */

export const EMBED_FIT_ATTR = 'data-embed-fit'
export const EMBED_SCHEME_ATTR = 'data-embed-scheme'
export const EMBED_READY_ATTR = 'data-embed-ready'

export interface EmbedHost {
  win: Window
  doc: Document
  /** 自己所在的 iframe（外面那页的节点）。 */
  frame: HTMLElement | null
}

/** 不在框里、或者外面跨源读不到时为 null。 */
export function embedHost(win: Window = window): EmbedHost | null {
  try {
    const parent = win.parent
    if (!parent || parent === win) return null
    return {
      win: parent,
      doc: parent.document,
      frame: (win.frameElement as HTMLElement | null) ?? null,
    }
  } catch {
    return null
  }
}
