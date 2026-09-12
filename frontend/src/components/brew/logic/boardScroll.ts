export interface BoardScroll {
  x: number
  y: number
  rails?: Record<string, number>
}

const RAIL_TRACK = '[data-brew-rail-track]'

function railKey(value: string | undefined): string | null {
  return value && /^[a-z]+$/.test(value) ? value : null
}

function readRails(root: ParentNode | null | undefined): Record<string, number> {
  const rails: Record<string, number> = {}
  if (!root || !('querySelectorAll' in root)) return rails
  root.querySelectorAll<HTMLElement>(RAIL_TRACK).forEach((el) => {
    const key = railKey(el.dataset.brewRailTrack)
    const value = Number(el.dataset.brewRailScroll)
    if (key && Number.isFinite(value)) rails[key] = value
  })
  return rails
}

function writeRails(
  root: ParentNode | null | undefined,
  rails: Record<string, number> | undefined,
): void {
  if (!root || !rails) return
  for (const [key, value] of Object.entries(rails)) {
    if (!railKey(key) || !Number.isFinite(value)) continue
    const el = root.querySelector<HTMLElement>(
      `[data-brew-rail-track="${key}"]`,
    )
    if (el) el.dataset.brewRailRestore = String(value)
  }
}

export function captureBoardScroll(
  view: Pick<Window, 'scrollX' | 'scrollY'> = window,
  root: ParentNode | null = typeof document === 'undefined' ? null : document,
): BoardScroll {
  const rails = readRails(root)
  return {
    x: view.scrollX || 0,
    y: view.scrollY || 0,
    rails: Object.keys(rails).length ? rails : undefined,
  }
}

export function restoreBoardScroll(
  pos: BoardScroll | null | undefined,
  view: Pick<Window, 'scrollTo'> = window,
  root: ParentNode | null = typeof document === 'undefined' ? null : document,
): void {
  if (!pos) return
  view.scrollTo(pos.x, pos.y)
  writeRails(root, pos.rails)
}
