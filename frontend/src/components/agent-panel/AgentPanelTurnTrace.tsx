import React, { useEffect, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import {
  serializeTurnTrace,
  snapshotTurnTrace,
  subscribeTurnTrace,
} from '../../features/merope/turnTrace'

export function downloadTurnTrace(): void {
  const blob = new Blob([serializeTurnTrace()], { type: 'application/json' })
  const url = URL.createObjectURL(blob)
  const anchor = document.createElement('a')
  anchor.href = url
  anchor.download = 'merope-turn-trace.json'
  document.body.appendChild(anchor)
  anchor.click()
  anchor.remove()
  URL.revokeObjectURL(url)
}

export const AgentPanelTurnTrace: React.FC = () => {
  const { t, format } = useI18n()
  const [snap, setSnap] = useState(() => snapshotTurnTrace())

  useEffect(() => subscribeTurnTrace(setSnap), [])

  const idle = !snap.turnId && snap.marks.length === 0

  return (
    <div
      className="agent-panel-tag agent-panel-manage-row agent-panel-manage-trace"
      data-block="true"
    >
      <div className="agent-panel-manage-trace-copy">
        <span className="agent-panel-tag-text">
          {t.agentPanel.manage.turnTraceTitle}
        </span>
        <span className="agent-panel-manage-trace-meta">
          {idle
            ? t.agentPanel.manage.turnTraceIdle
            : format(t.agentPanel.manage.turnTraceTiming, {
                asr: `${snap.delays.asrMs}ms`,
                llm: `${snap.delays.llmFirstTokenMs}ms`,
                tts: `${snap.delays.ttsSynthMs}ms`,
                audio: `${snap.delays.firstAudioMs}ms`,
                e2e: `${snap.delays.requestToFirstAudioMs}ms`,
              })}
        </span>
      </div>
      <button
        type="button"
        className="agent-panel-manage-export"
        onClick={() => downloadTurnTrace()}
      >
        {t.agentPanel.manage.turnTraceExport}
      </button>
    </div>
  )
}
