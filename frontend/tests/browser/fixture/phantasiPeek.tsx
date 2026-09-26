import { StrictMode, useState, useSyncExternalStore } from 'react'
import { createRoot } from 'react-dom/client'
import { makePreview, makeSource } from '../../../src/components/phantasi/logic/fixtures'
import { PhantasiViewLane } from '../../../src/components/phantasi/skin/PhantasiChip'
import PhantasiFeeds from '../../../src/components/phantasi/skin/PhantasiFeeds'
import { PhantasiPeekAir, readPeekFace, subscribePeekFace } from '../../../src/components/phantasi/ui/PhantasiPeekAir'
import { PhantasiSearch } from '../../../src/components/phantasi/ui/PhantasiSearch'
import { usePeekSession } from '../../../src/components/phantasi/ui/usePeekSession'
import { I18nNamespace, I18nProvider } from '../../../src/contexts/I18nContext'
import '../../../src/styles/tailwind.css'
import '../../../src/styles/spa-document.css'
import '../../../src/styles/theme.css'
import '../../../src/styles/animations.css'
import '../../../src/styles/utility.css'
import '../../../src/styles/performance.css'
import '../../../src/components/phantasi/ui/phantasi.css'

const stories = Array.from({ length: 8 }, (_, i) => makePreview({ id: i + 1, title: `Story ${i + 1}`, summary: 'A visible hover summary', image: i === 1 ? null : `https://test.invalid/cover-${i}.svg` }))
const sources = [makeSource({ recent_items: stories })]
function Harness() {
  const [route, setRoute] = useState('feeds')
  const [blocked, setBlocked] = useState(false)
  const [search, setSearch] = useState('')
  const session = usePeekSession(route, blocked)
  const face = useSyncExternalStore(subscribePeekFace, readPeekFace)
  return <div className="phantasi-skin">
    <button style={{ position: 'relative', zIndex: 10 }} id="route" onClick={() => setRoute(value => value === 'feeds' ? 'notes' : 'feeds')}>switch route</button>
    <button style={{ position: 'relative', zIndex: 10 }} id="reader" onClick={() => setBlocked(value => !value)}>reader</button>
    <output id="face">{face?.title ?? 'none'}</output>
    <PhantasiPeekAir face={face} />
    <PhantasiSearch value={search} onChange={setSearch} />
    <div style={{ height: 'calc(100vh - 70px)', display: 'flex', padding: 16 }}>
      <PhantasiViewLane wave={route} onDisplayed={session.resumePeekAfterLane} suspended={blocked}>
        <PhantasiFeeds
          key={route}
          sources={sources}
          stories={stories}
          onPeekItem={session.handlePeekItem}
          onPeekEnd={session.handlePeekEnd}
        />
      </PhantasiViewLane>
    </div>
  </div>
}
createRoot(document.getElementById('root')!).render(<StrictMode><I18nProvider><I18nNamespace names={['phantasi']}><Harness /></I18nNamespace></I18nProvider></StrictMode>)
