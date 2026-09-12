import type { ReactNode } from 'react'
import type { GuideSectionLabels, SettingGuideEntry } from './types'
import React, { useMemo } from 'react'
import './SettingGuideBody.css'

export interface SettingGuideBodyProps {
  entry: SettingGuideEntry
  labels: GuideSectionLabels
}

const STEP_LINE =
  /^(?:[①②③④⑤⑥⑦⑧⑨⑩]|\d+[)）.、]|[（(]\d+[)）])\s*/

const URL_RE = /(https?:\/\/[^\s<>"'）】\]},;，。；]+)/g

function trimUrlTrailingPunct(raw: string): { href: string; trail: string } {
  let href = raw
  let trail = ''
  while (href.length > 0 && /[.,;:!?）】\]}>'"]$/u.test(href)) {
    trail = href.slice(-1) + trail
    href = href.slice(0, -1)
  }
  return { href, trail }
}

function linkifyLine(text: string): ReactNode {
  const parts = text.split(URL_RE)
  if (parts.length === 1) return text
  return parts.map((part, i) => {
    if (!part.startsWith('http://') && !part.startsWith('https://')) {
      return <React.Fragment key={i}>{part}</React.Fragment>
    }
    const { href, trail } = trimUrlTrailingPunct(part)
    if (!href) return <React.Fragment key={i}>{part}</React.Fragment>
    return (
      <React.Fragment key={i}>
        <a
          className="setting-guide-link"
          href={href}
          target="_blank"
          rel="noopener noreferrer"
        >
          {href}
        </a>
        {trail}
      </React.Fragment>
    )
  })
}

function renderText(text: string) {
  const lines = text
    .split('\n')
    .map((l) => l.trim())
    .filter(Boolean)
  const stepLines = lines.filter((l) => STEP_LINE.test(l))
  const isStepList =
    lines.length >= 2 && stepLines.length >= Math.ceil(lines.length * 0.6)

  if (!isStepList) {
    return (
      <p className="setting-guide-block-text">
        {lines.map((line, i) => (
          <React.Fragment key={i}>
            {i > 0 ? <br /> : null}
            {linkifyLine(line)}
          </React.Fragment>
        ))}
      </p>
    )
  }

  return (
    <ol className="setting-guide-steps">
      {lines.map((line, i) => (
        <li key={i} className="setting-guide-step">
          <span className="setting-guide-step-num" aria-hidden>
            {i + 1}
          </span>
          <span className="setting-guide-step-text">
            {linkifyLine(line.replace(STEP_LINE, ''))}
          </span>
        </li>
      ))}
    </ol>
  )
}

export const SettingGuideBody: React.FC<SettingGuideBodyProps> = ({
  entry,
  labels,
}) => {
  const blocks = useMemo(() => {
    const list: Array<{ key: string; label: string; text: string }> = []
    if (entry.what) {
      list.push({ key: 'what', label: labels.what, text: entry.what })
    }
    if (entry.chain) {
      list.push({ key: 'chain', label: labels.chain, text: entry.chain })
    }
    if (entry.frontend) {
      list.push({
        key: 'frontend',
        label: labels.frontend,
        text: entry.frontend,
      })
    }
    if (entry.notes) {
      list.push({ key: 'notes', label: labels.notes, text: entry.notes })
    }
    return list
  }, [entry, labels])

  if (blocks.length === 0) return null

  return (
    <div className="setting-guide-body">
      {blocks.map((b) => (
        <section key={b.key} className="setting-guide-block">
          <h4 className="setting-guide-block-label">{b.label}</h4>
          {renderText(b.text)}
        </section>
      ))}
    </div>
  )
}

SettingGuideBody.displayName = 'SettingGuideBody'

export default SettingGuideBody
