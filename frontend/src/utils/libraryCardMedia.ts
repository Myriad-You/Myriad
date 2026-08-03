/**
 * Cap concurrent library cover decodes and help the browser drop bitmaps when
 * cards leave the virtualized viewport. Pan around a large canvas without
 * holding dozens of full-res decodes at once.
 */

const MAX_CONCURRENT_COVER_DECODES = 8

type Release = () => void

let activeDecodes = 0
const waitQueue: Array<() => void> = []

function pumpQueue() {
  // Loop condition uses only waitQueue (mutated by shift). Slot limit checked
  // inside — activeDecodes is updated by grant()/release, not in this body.
  while (waitQueue.length > 0) {
    if (activeDecodes >= MAX_CONCURRENT_COVER_DECODES) {
      return
    }
    const next = waitQueue.shift()
    if (!next) {
      return
    }
    next()
  }
}

/**
 * Acquire a decode slot. Call the returned release when the image loads, errors,
 * or the card unmounts. FIFO fairness when many cards mount after a pan.
 */
export function acquireCoverDecodeSlot(): Promise<Release> {
  return new Promise((resolve) => {
    const grant = () => {
      activeDecodes += 1
      let released = false
      resolve(() => {
        if (released) return
        released = true
        activeDecodes = Math.max(0, activeDecodes - 1)
        pumpQueue()
      })
    }
    if (activeDecodes < MAX_CONCURRENT_COVER_DECODES) {
      grant()
    } else {
      waitQueue.push(grant)
    }
  })
}

/** Test helper — not used in production paths. */
export function __coverDecodeSlotStatsForTest() {
  return { active: activeDecodes, waiting: waitQueue.length }
}

/** Drop decoded bitmap when a virtualized card unmounts. */
export function releaseCoverImageElement(img: HTMLImageElement | null) {
  if (!img) return
  try {
    img.removeAttribute('src')
    // Chromium treats empty src as current document in edge cases; blank data URI is safer.
    img.src =
      'data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw=='
  } catch {
    // ignore detach races
  }
}

/** Hint for card-sized display (~184–400 CSS px). */
export const LIBRARY_CARD_COVER_SIZES = '(max-width: 767px) 42vw, 220px'
