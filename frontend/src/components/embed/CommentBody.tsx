import type { InlineToken, MarkdownBlock } from '../agent-panel/markdown'
import { Fragment, memo } from 'react'
import { parseMarkdownBlocks } from '../agent-panel/markdown'
import { BARE_URL, safeCommentUrl } from './commentMarkdown'

/**
 * 访客写的评论按 Markdown 显示，解析借助手面板那一份（agent-panel/markdown.ts），但只认评论用得上、
 * 放在别人文章下面也安全的那几样：粗体、斜体、行内代码、代码块、引用、列表、链接。
 * - 链接只认 http(s)，新窗口打开，带 nofollow ugc：不给评论里的地址传权重。
 * - 图片不加载，留成原文：访客贴的图会在每个读者的浏览器里请求，对方能借此知道谁读过。
 * - 标题、表格、分隔线在评论里没用，还会抢正文的视觉，降成普通段落。
 */

function Link({ href, children }: { href: string; children: string }) {
  return (
    <a className="cm-md-link" href={href} target="_blank" rel="nofollow ugc noopener noreferrer">
      {children}
    </a>
  )
}

/** 纯文字里裸贴的网址也认成链接。 */
function Text({ text }: { text: string }) {
  const parts: Array<string | { url: string }> = []
  let last = 0
  for (const match of text.matchAll(BARE_URL)) {
    if (match.index > last) parts.push(text.slice(last, match.index))
    parts.push({ url: match[0] })
    last = match.index + match[0].length
  }
  if (last < text.length) parts.push(text.slice(last))
  return (
    <>
      {parts.map((part, index) => {
        if (typeof part === 'string') return <Fragment key={index}>{part}</Fragment>
        const href = safeCommentUrl(part.url)
        return href ? <Link key={index} href={href}>{part.url}</Link> : <Fragment key={index}>{part.url}</Fragment>
      })}
    </>
  )
}

function Inline({ tokens }: { tokens: InlineToken[] }) {
  return (
    <>
      {tokens.map((token, index) => {
        switch (token.kind) {
          case 'code':
            return <code key={index} className="cm-md-code">{token.text}</code>
          case 'bold':
            return <strong key={index}>{token.text}</strong>
          case 'italic':
            return <em key={index}>{token.text}</em>
          case 'link': {
            const href = safeCommentUrl(token.url)
            return href ? <Link key={index} href={href}>{token.text}</Link> : <Fragment key={index}>{`[${token.text}](${token.url})`}</Fragment>
          }
          case 'image':
            return <Fragment key={index}>{`![${token.alt}](${token.url})`}</Fragment>
          default:
            return <Text key={index} text={token.text} />
        }
      })}
    </>
  )
}

function Block({ block }: { block: MarkdownBlock }) {
  switch (block.kind) {
    case 'code':
      return <pre className="cm-md-pre"><code>{block.text}</code></pre>
    case 'quote':
      return (
        <blockquote className="cm-md-quote">
          {block.lines.map((line, index) => <p key={index}><Inline tokens={line} /></p>)}
        </blockquote>
      )
    case 'list': {
      const Tag = block.ordered ? 'ol' : 'ul'
      return (
        <Tag className="cm-md-list">
          {block.items.map((item, index) => <li key={index}><Inline tokens={item} /></li>)}
        </Tag>
      )
    }
    case 'heading':
      return <p><strong><Inline tokens={block.inline} /></strong></p>
    case 'table':
      return (
        <>
          {[block.headers, ...block.rows].map((row, index) => (
            <p key={index}>
              {row.map((cell, cellIndex) => (
                <Fragment key={cellIndex}>
                  {cellIndex ? ' · ' : null}
                  <Inline tokens={cell} />
                </Fragment>
              ))}
            </p>
          ))}
        </>
      )
    case 'rule':
      return null
    default:
      return <p><Inline tokens={block.inline} /></p>
  }
}

export const CommentBody = memo(({ text }: { text: string }) => (
  <div className="cm-entry__body">
    {parseMarkdownBlocks(text).map((block, index) => <Block key={index} block={block} />)}
  </div>
))
