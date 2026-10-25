import type { InlineToken } from '../agent-panel/markdown'
import { parseMarkdownBlocks } from '../agent-panel/markdown'

/** 评论 Markdown 的纯逻辑部分：显示在 CommentBody.tsx，创作台列表只要摘要，不必带上 React 组件。 */

const SAFE_URL = /^https?:\/\//i
export const BARE_URL = /https?:\/\/[^\s<>()[\]]+[^\s<>()[\].,;:!?'"，。；：！？）】」』]/gi

export function safeCommentUrl(url: string): string | null {
  const value = url.trim()
  if (!SAFE_URL.test(value)) return null
  try {
    return new URL(value).href
  } catch {
    return null
  }
}

function inlineText(tokens: InlineToken[]): string {
  return tokens.map((token) => (token.kind === 'image' ? token.alt : token.text)).join('')
}

/** 一行摘要（创作台评论列表的标题）：去掉 Markdown 记号，只留字。 */
export function commentPlainText(text: string): string {
  return parseMarkdownBlocks(text)
    .map((block) => {
      switch (block.kind) {
        case 'code':
          return block.text
        case 'quote':
          return block.lines.map(inlineText).join(' ')
        case 'list':
          return block.items.map(inlineText).join(' ')
        case 'table':
          return [block.headers, ...block.rows].map((row) => row.map(inlineText).join(' ')).join(' ')
        case 'rule':
          return ''
        default:
          return inlineText(block.inline)
      }
    })
    .filter(Boolean)
    .join(' ')
    .replace(/\s+/g, ' ')
    .trim()
}
