import React from 'react'
import { createRoot } from 'react-dom/client'
import {
  MusicPlayerProvider,
  useMusicPlayerControl,
} from '../../../src/contexts/MusicPlayerContext'
import { sharedEventManager } from '../../../src/hooks/useSharedEventListener'

let renders = 0

function MusicConsumer() {
  const state = useMusicPlayerControl()
  renders++
  return <output id="music">{String(state.isPlaying)}:{state.currentLyricIndex}</output>
}

createRoot(document.getElementById('root')!).render(
  <MusicPlayerProvider><MusicConsumer /></MusicPlayerProvider>,
)

Object.assign(window, {
  performanceFixture: {
    renders: () => renders,
    publish: (detail: Record<string, unknown>) => {
      window.dispatchEvent(new CustomEvent('music-player-state-change', { detail }))
    },
    sharedEventManager,
  },
})
