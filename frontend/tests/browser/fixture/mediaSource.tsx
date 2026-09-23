import React from 'react'
import { flushSync } from 'react-dom'
import { createRoot } from 'react-dom/client'
import { AuthenticatedMedia } from '../../../src/components/phantasi/skin/AuthenticatedMedia'

const requests: Array<{ url: string; finish: () => void }> = []
const created: string[] = []
const revoked: string[] = []
const originalFetch = window.fetch.bind(window)
window.fetch = ((url, init) => {
  if (!String(url).includes('/api/media/')) return originalFetch(url, init)
  // Deliberately finish body reads after abort to reproduce cancellation races.
  let finish!: () => void
  const body = new Promise<Blob>((resolve) => { finish = () => resolve(new Blob([Uint8Array.from(atob('iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAACXBIWXMAAAPoAAAD6AG1e1JrAAAADklEQVQImWNw6fj/H4QBFnsFlbfmtiMAAAAASUVORK5CYII='), c => c.charCodeAt(0))], { type: 'image/png' })) })
  requests.push({ url: String(url), finish })
  return Promise.resolve({ ok: true, blob: () => body } as Response)
}) as typeof fetch
const create = URL.createObjectURL.bind(URL)
const revoke = URL.revokeObjectURL.bind(URL)
URL.createObjectURL = (blob) => {
  const url = create(blob)
  created.push(url)
  return url
}
URL.revokeObjectURL = (url) => { revoked.push(url); revoke(url) }
const root = createRoot(document.getElementById('root')!)
window.mediaSourceTest = {
  show(src?: string) { flushSync(() => root.render(<AuthenticatedMedia src={src} />)) },
  requests, created, revoked,
  unmount() { flushSync(() => root.unmount()) },
}

declare global {
  interface Window {
    mediaSourceTest: {
      show: (src?: string) => void
      requests: typeof requests
      created: string[]
      revoked: string[]
      unmount: () => void
    }
  }
}
