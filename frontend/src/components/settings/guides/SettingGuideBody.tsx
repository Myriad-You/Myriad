/**
 * 结构化选项指南正文（概述 / 关联 / 位置 / 提示）
 * 支持多行步骤（① / 1) 开头）自动拆成 1 2 3 列表。
 */

import type { GuideSectionLabels, SettingGuideEntry } from './types'
import React, { useMemo } from 'react'
import './SettingGuideBody.css'

export interface SettingGuideBodyProps {
  entry: SettingGuideEntry
  labels: GuideSectionLabels
}

const STEP_LINE =
  /^(?:[①②③④⑤⑥⑦⑧⑨⑩]|\d+[)）.、]|[（(]\d+[)）])\s*/

function renderText(text: string) {
  const lines = text
    .split('\n')
    .map((l) => l.trim())
    .filter(Boolean)
  const stepLines = lines.filter((l) => STEP_LINE.test(l))
  const isStepList =
    lines.length >= 2 && stepLines.length >= Math.ceil(lines.length * 0.6)

  if (!isStepList) {
    return <p className="setting-guide-block-text">{text}</p>
  }

  return (
    <ol className="setting-guide-steps">
      {lines.map((line, i) => (
        <li key={i} className="setting-guide-step">
          <span className="setting-guide-step-num" aria-hidden>
            {i + 1}
          </span>
          <span className="setting-guide-step-text">
            {line.replace(STEP_LINE, '')}
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
