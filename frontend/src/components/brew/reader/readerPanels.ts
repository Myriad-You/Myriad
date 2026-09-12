export type ReaderToolPanel = 'toc' | 'annotations' | 'podcast'

/** Same toolbar action on desktop and mobile: one panel, or none. */
export function nextExclusivePanel<T extends string>(
  current: T | null,
  toggle: T,
): T | null {
  return current === toggle ? null : toggle
}

export function readerPanelFlags(panel: ReaderToolPanel | null): {
  toc: boolean
  brewlia: boolean
  podcast: boolean
} {
  return {
    toc: panel === 'toc',
    brewlia: panel === 'annotations',
    podcast: panel === 'podcast',
  }
}

export function currentReaderPanel(state: {
  toc: boolean
  brewlia: boolean
  podcast: boolean
}): ReaderToolPanel | null {
  if (state.toc) return 'toc'
  if (state.brewlia) return 'annotations'
  if (state.podcast) return 'podcast'
  return null
}

export function applyExclusivePanel(
  current: ReaderToolPanel | null,
  toggle: ReaderToolPanel,
): ReturnType<typeof readerPanelFlags> {
  return readerPanelFlags(nextExclusivePanel(current, toggle))
}

export function readerDialogTrigger(
  expanded: boolean,
  controls: string,
): {
  'aria-expanded': boolean
  'aria-haspopup': 'dialog'
  'aria-controls': string
} {
  return {
    'aria-expanded': expanded,
    'aria-haspopup': 'dialog',
    'aria-controls': controls,
  }
}

export function readerPopupTrigger(
  expanded: boolean,
  controls: string,
): {
  'aria-expanded': boolean
  'aria-haspopup': true
  'aria-controls': string
} {
  return {
    'aria-expanded': expanded,
    'aria-haspopup': true,
    'aria-controls': controls,
  }
}

export type ReaderChromeLayer =
  | 'lightbox'
  | 'popup'
  | 'comments'
  | 'toc'
  | 'annotations'
  | 'voice'
  | 'podcast'
  | 'controls'

/** Escape closes the top overlay first, then the reader. */
export function dismissReaderChrome(state: {
  lightbox: boolean
  popup: boolean
  comments: boolean
  toc: boolean
  brewlia: boolean
  voice: boolean
  podcast: boolean
  controls: boolean
}): ReaderChromeLayer | null {
  if (state.lightbox) return 'lightbox'
  if (state.popup) return 'popup'
  if (state.comments) return 'comments'
  if (state.toc) return 'toc'
  if (state.brewlia) return 'annotations'
  if (state.voice) return 'voice'
  if (state.podcast) return 'podcast'
  if (state.controls) return 'controls'
  return null
}

/** Fields swallow Escape unless it is closing a composer or dialog. */
export function escapeWhileTyping(
  layer: ReaderChromeLayer | null,
): ReaderChromeLayer | null {
  if (
    layer === 'lightbox' ||
    layer === 'popup' ||
    layer === 'comments' ||
    layer === 'voice'
  ) {
    return layer
  }
  return null
}

/** Tab wraps inside a modal. null means the browser should move normally. */
export function nextReaderDialogTab(
  nodes: readonly HTMLElement[],
  active: EventTarget | null,
  shift: boolean,
): HTMLElement | null {
  if (nodes.length === 0) return null
  const first = nodes[0]
  const last = nodes.at(-1)!
  if (shift) {
    if (active === first || !nodes.includes(active as HTMLElement)) return last
    return null
  }
  if (active === last || !nodes.includes(active as HTMLElement)) return first
  return null
}
