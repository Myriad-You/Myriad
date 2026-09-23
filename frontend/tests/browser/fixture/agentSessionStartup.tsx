import { createRoot } from 'react-dom/client'
import { AgentOpenIntentCapture, AgentSessionHost } from '../../../src/components/agent-panel/AgentSessionHost'
import { initPageLoader, markDocumentReady } from '../../../src/utils/pageLoader'

export function mount() {
  initPageLoader()
  const root = createRoot(document.getElementById('app-root')!)
  // Same pairing as App's AgentAccessGate: capture queues opens, the host wakes on them.
  root.render(
    <>
      <AgentOpenIntentCapture />
      <AgentSessionHost><span id="session-ready">Session mounted</span></AgentSessionHost>
    </>,
  )
  return { ready: markDocumentReady, unmount: () => root.unmount() }
}
