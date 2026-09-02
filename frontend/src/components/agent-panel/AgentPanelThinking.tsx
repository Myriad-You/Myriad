/**
 * 思考过程 —— 气泡里、正文还没来的时候。
 *
 * 有思考正文就只出字，扫一层高亮。工具调用另排一行，不和思考挤在一起。
 * 正文一到，整块卸掉。
 */

import type { AgentMessageStep } from './agentThinking'
import React, { useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import {
  formatStepDuration,
  stepsWorthShowing,
  summarizeAgentSteps,
  thinkingVisible,
} from './agentThinking'
import { AgentPresence, AgentSwap } from './useAgentPresence'

function ThinkingDots() {
  return (
    <span className="agent-panel-thinking-dots" aria-hidden="true">
      <span />
      <span />
      <span />
    </span>
  )
}

function ToolList({ steps }: { steps: AgentMessageStep[] }) {
  return (
    <ol className="agent-panel-tools">
      {steps.map((step) => (
        <li key={step.id} data-status={step.status}>
          <span className="agent-panel-tool-status" aria-hidden="true" />
          <span className="agent-panel-tool-copy">
            <span className="agent-panel-tool-name">{step.name}</span>
            {step.note ? (
              <span className="agent-panel-tool-note">{step.note}</span>
            ) : null}
          </span>
          {typeof step.durationMs === 'number' ? (
            <span className="agent-panel-tool-elapsed">
              {formatStepDuration(step.durationMs)}
            </span>
          ) : null}
        </li>
      ))}
    </ol>
  )
}

export const AgentPanelThinking: React.FC<{
  steps: AgentMessageStep[]
  live?: boolean
  thought?: string
}> = ({ steps, live = false, thought }) => {
  const { t, format } = useI18n()
  const summary = summarizeAgentSteps(steps)
  const [userOpen, setUserOpen] = useState<boolean | null>(null)
  const expanded = userOpen ?? summary.failed
  const note = thought?.trim() || undefined
  const showsSteps = steps.length > 0

  if (!thinkingVisible(steps, live, note)) return null

  if (live) {
    return (
      <div className="agent-panel-thinking" data-live="true">
        {note ? (
          <p className="agent-panel-thinking-thought" data-live="true">
            <span className="agent-panel-thinking-sweep">{note}</span>
          </p>
        ) : showsSteps ? null : (
          <ThinkingDots />
        )}
        {showsSteps ? <ToolList steps={steps} /> : null}
      </div>
    )
  }

  const showsStepList = stepsWorthShowing(steps)
  const headline = summary.failed
    ? t.agentPanel.thinking.failed
    : format(t.agentPanel.thinking.doneSteps, { count: summary.total })

  return (
    <div
      className="agent-panel-thinking"
      data-failed={summary.failed ? 'true' : 'false'}
    >
      {note ? <p className="agent-panel-thinking-thought">{note}</p> : null}
      {showsStepList ? (
        <>
          <button
            type="button"
            className="agent-panel-thinking-head"
            onClick={() => setUserOpen(!(userOpen ?? summary.failed))}
            aria-expanded={expanded}
          >
            <span className="agent-panel-thinking-headline">
              <AgentSwap id={headline} from="self">
                <span>{headline}</span>
              </AgentSwap>
            </span>
            {summary.elapsedMs !== null && (
              <span className="agent-panel-thinking-elapsed">
                {formatStepDuration(summary.elapsedMs)}
              </span>
            )}
            <svg
              className="agent-panel-thinking-arrow"
              data-open={expanded ? 'true' : 'false'}
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
              strokeLinejoin="round"
              aria-hidden="true"
            >
              <path d="m9 18 6-6-6-6" />
            </svg>
          </button>

          <AgentPresence open={expanded} kind="swap" from="self">
            <ToolList steps={steps} />
          </AgentPresence>
        </>
      ) : null}
    </div>
  )
}
