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

export const AgentMarkdown = React.memo(({ text }: { text: string }) => (
  <>
    {parseMarkdownBlocks(text).map((block, index) => (
      <Block key={index} block={block} />
    ))}
  </>
))
