import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { GamePresenceWidget } from '../../../src/components/widgets/GamePresenceWidget'
import { useWidgetRotation } from '../../../src/components/widgets/shared/useWidgetRotation'
import { I18nProvider } from '../../../src/contexts/I18nContext'
import { ensureMotionReady } from '../../../src/lib/lazyMotion'
import './gamePresence.css'

function Fixture() {
  const [editing, setEditing] = useState(false)
  const [outerPage, setOuterPage] = useState(0)
  const outer = useWidgetRotation({
    count: 3,
    interactive: true,
    delay: null,
    autoplay: false,
    onStep: delta => setOuterPage(value => value + delta),
  })
  const width = Number(new URLSearchParams(location.search).get('width') || 400)
  return <>
    <button onClick={() => setEditing(value => !value)}>Toggle editing</button>
    <output data-outer-page>{outerPage}</output>
    <div ref={outer.rootRef} {...outer.rootProps} style={{ padding: 60 }}>
      <div id="widget" style={{ width, height: 220 }}>
        <GamePresenceWidget
          config={{
            id: 'game',
            type: 'game-presence',
            size: '4x2',
            position: { x: 0, y: 0 },
            config: { accountId: '800123456', game: 'genshin' },
          }}
          isEditMode={editing}
        />
      </div>
    </div>
    <div style={{ height: 2000 }} />
  </>
}

void ensureMotionReady().then(() => {
  createRoot(document.getElementById('root')!).render(<I18nProvider><Fixture /></I18nProvider>)
})
