import type { ReaderCopy } from '../../../src/components/phantasi/reader/types'
import { useRef, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { useReaderControls } from '../../../src/components/phantasi/reader/hooks/useReaderControls'
import { ReaderProgressPercent } from '../../../src/components/phantasi/reader/ReaderProgress'
import { phantasiSubject } from '../../../src/utils/phantasiSubject'

const copy = { phantasi: { readingSyncConflict: 'conflict' } } as ReaderCopy
function noop() {}

function Harness({ saved, short }: { saved: number; short: boolean }) {
  const articleRef = useRef<HTMLElement>(null)
  const contentRef = useRef<HTMLDivElement>(null)
  const [ready, setReady] = useState(false)
  const controls = useReaderControls({
    articleRef,
    contentRef,
    itemId: 23,
    readProgress: saved,
    stateRevision: 1,
    contentReady: ready,
    isAuthenticated: true,
    adjustFontSize: noop,
    showToastMessage: noop,
    t: copy,
  })
  return (
    <>
      <button onClick={() => setReady(true)}>load content</button>
      <button onClick={() => void controls.recoverRemoteProgress()}>
        adopt remote
      </button>
      <div data-testid="progress">
        <ReaderProgressPercent
          progress={controls.readingProgress}
          className=""
        />
      </div>
      <article
        ref={articleRef}
        data-testid="article"
        style={{ height: 300, overflowY: 'auto', scrollBehavior: 'smooth' }}
      >
        <div
          ref={contentRef}
          style={{ height: ready ? (short ? 100 : 2000) : 0 }}
        >
          {ready && 'Article content'}
        </div>
      </article>
    </>
  )
}

export function mountReaderProgress(saved: number, short = false) {
  phantasiSubject.change('user:638:member')
  createRoot(document.getElementById('root')!).render(
    <Harness saved={saved} short={short} />,
  )
}
