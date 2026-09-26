import React from 'react'
import { flushSync } from 'react-dom'
import { createRoot } from 'react-dom/client'
import { MediaPreview } from '../../../src/components/phantasi/skin/MediaPreview'

const root = createRoot(document.getElementById('root')!)
window.mediaSourceTest = {
  show(src?: string) {
    flushSync(() => root.render(<MediaPreview src={src} />))
  },
  unmount() {
    flushSync(() => root.unmount())
  },
}

declare global {
  interface Window {
    mediaSourceTest: {
      show: (src?: string) => void
      unmount: () => void
    }
  }
}
