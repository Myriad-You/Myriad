/**
 * 把 `markdown.ts` 认出来的那棵树画出来。
 *
 * 只画，不认字 —— 边界情况都在解析那一步，这里没有分支值得测，所以它可以是
 * 一个直白的组件。
 */

import type { InlineToken, MarkdownBlock } from './markdown'
import React from 'react'
import { parseMarkdownBlocks } from './markdown'

function Inline({ tokens }: { tokens: InlineToken[] }): React.ReactElement {
  return (
    <>
      {tokens.map((token, index) => {
        switch (token.kind) {
          case 'code':
            return (
              <code key={index} className="agent-md-code">
                {token.text}
              </code>
            )
          case 'image':
            return (
              <img
                key={index}
                className="agent-md-img"
                src={token.url}
                alt={token.alt}
                loading="lazy"
              />
            )
          case 'link':
            return (
              <a
                key={index}
                className="agent-md-link"
                href={token.url}
                target="_blank"
                rel="noopener noreferrer"
              >
                {token.text}
              </a>
            )
          case 'bold':
            return <strong key={index}>{token.text}</strong>
          case 'italic':
            return <em key={index}>{token.text}</em>
          default:
            return <React.Fragment key={index}>{token.text}</React.Fragment>
        }
      })}
    </>
  )
}

const Block = React.memo(({
  block,
}: {
  block: MarkdownBlock
}): React.ReactElement => {
  switch (block.kind) {
    case 'heading': {
      // 助手说的话嵌在气泡里，一级标题也不该有页面标题那么大
      const Tag = `h${block.level + 2}` as 'h3' | 'h4' | 'h5'
      return (
        <Tag className="agent-md-heading">
          <Inline tokens={block.inline} />
        </Tag>
      )
    }
    case 'code':
      return (
        <pre className="agent-md-pre" data-lang={block.lang ?? undefined}>
          <code>{block.text}</code>
        </pre>
      )
    case 'quote':
      return (
        <blockquote className="agent-md-quote">
          {block.lines.map((line, index) => (
            <p key={index} className="agent-md-p">
              <Inline tokens={line} />
            </p>
          ))}
        </blockquote>
      )
    case 'list': {
      const Tag = block.ordered ? 'ol' : 'ul'
      return (
        <Tag className="agent-md-list">
          {block.items.map((item, index) => (
            <li key={index}>
              <Inline tokens={item} />
            </li>
          ))}
        </Tag>
      )
    }
    case 'table':
      return (
        // 宽表自己横向滚，不把气泡撑破
        <div className="agent-md-table-scroll">
          <table className="agent-md-table">
            <thead>
              <tr>
                {block.headers.map((cell, index) => (
                  <th key={index}>
                    <Inline tokens={cell} />
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {block.rows.map((row, rowIndex) => (
                <tr key={rowIndex}>
                  {row.map((cell, cellIndex) => (
                    <td key={cellIndex}>
                      <Inline tokens={cell} />
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )
    case 'rule':
      return <hr className="agent-md-hr" />
    default:
      return (
        <p className="agent-md-p">
          <Inline tokens={block.inline} />
        </p>
      )
  }
})

export const AgentMarkdown: React.FC<{ text: string }> = ({ text }) => (
  <>
    {parseMarkdownBlocks(text).map((block, index) => (
      <Block key={index} block={block} />
    ))}
  </>
)
