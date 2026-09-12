interface SavedStyles {
  htmlOverflow: string
  bodyOverflow: string
  bodyPaddingRight: string
}

let lockCount = 0
let saved: SavedStyles | null = null

export function lockScroll(): () => void {
  if (typeof document === 'undefined') return () => {}

  const html = document.documentElement
  const body = document.body

  if (lockCount === 0) {
    saved = {
      htmlOverflow: html.style.overflow,
      bodyOverflow: body.style.overflow,
      bodyPaddingRight: body.style.paddingRight,
    }

    // Pad by scrollbar width; overlay scrollbars measure 0.
    const scrollbarWidth = window.innerWidth - html.clientWidth
    if (scrollbarWidth > 0) {
      body.style.paddingRight = `${scrollbarWidth}px`
    }

    html.style.overflow = 'hidden'
    body.style.overflow = 'hidden'
  }

  lockCount += 1

  let released = false
  return () => {
    if (released) return
    released = true
    lockCount = Math.max(0, lockCount - 1)
    if (lockCount === 0 && saved) {
      html.style.overflow = saved.htmlOverflow
      body.style.overflow = saved.bodyOverflow
      body.style.paddingRight = saved.bodyPaddingRight
      saved = null
    }
  }
}
