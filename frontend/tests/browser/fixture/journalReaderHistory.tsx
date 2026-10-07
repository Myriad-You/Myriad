import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { BrowserRouter, useLocation, useNavigate } from 'react-router-dom'
import {
  makeItem,
  makeSource,
} from '../../../src/components/phantasi/logic/fixtures'
import { usePhantasiItemRoute } from '../../../src/components/phantasi/usePhantasiItemRoute'
import * as phantasiApi from '../../../src/services/phantasiApi'

const sources = [makeSource(), makeSource({ id: 2, source_type: 'note' })]

function Harness() {
  const location = useLocation()
  const navigate = useNavigate()
  const [error, setError] = useState('')
  const param = /^\/journal\/articles\/(.+)$/.exec(location.pathname)?.[1]
  const listPath = param ? '/journal/friends' : location.pathname
  const item = usePhantasiItemRoute(
    param,
    sources,
    true,
    navigate,
    setError,
    'failed',
    listPath,
    location,
  )
  return (
    <>
      <button onClick={() => navigate('/journal/notes')}>notes</button>
      <button onClick={() => navigate('/journal/friends')}>friends</button>
      <button
        onClick={() =>
          void item.openArticle((signal) =>
            phantasiApi.getItem(1, undefined, { signal }),
          )
        }
      >
        external
      </button>
      <button
        onClick={() => void item.openArticle(makeItem({ id: 2, source_id: 2 }))}
      >
        own
      </button>
      <button onClick={() => void item.openArticle(makeItem({ id: 3 }))}>
        next external
      </button>
      <button
        onClick={() =>
          void item.openArticle(
            makeItem({ id: -1, source_id: 0, fromWebSearch: true }),
          )
        }
      >
        search result
      </button>
      <button onClick={() => navigate('/journal/articles/1')}>
        external deep link
      </button>
      <output data-testid="error">{error}</output>
      <output data-testid="opening">{String(item.opening)}</output>
      {item.selectedItem && (
        <div role="dialog" aria-label="reader">
          <output data-testid="item">{item.selectedItem.id}</output>
          <button onClick={item.closeReader}>close reader</button>
        </div>
      )}
    </>
  )
}

export function mountReaderHistory() {
  createRoot(document.getElementById('root')!).render(
    <BrowserRouter>
      <Harness />
    </BrowserRouter>,
  )
}
