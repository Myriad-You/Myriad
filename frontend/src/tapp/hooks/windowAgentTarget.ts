import type { WindowTarget } from '../../services/agent'

export interface WindowRef {
  windowId: string
  tappId: string
  position?: { x: number; y: number }
  tapp?: { name?: string; manifest?: { name?: string } } | null
}

export function resolveWindowTarget(
  target: WindowTarget,
  windows: WindowRef[] | undefined,
  activeWindowId: string | null,
): string | null {
  if (target.windowId) {
    return target.windowId
  }
  const list = windows ?? []
  if (target.tappId) {
    return list.find((win) => win.tappId === target.tappId)?.windowId ?? null
  }
  if (target.tappName) {
    const needle = target.tappName.toLowerCase()
    const match = list.find((win) => {
      const name = win.tapp?.name || win.tapp?.manifest?.name
      return name?.toLowerCase().includes(needle)
    })
    return match?.windowId ?? null
  }
  if (list.length === 0) {
    return target.position === 'active' ? activeWindowId : null
  }
  switch (target.position) {
    case 'active':
      return activeWindowId
    case 'left':
      return [...list].sort(
        (a, b) => (a.position?.x ?? 0) - (b.position?.x ?? 0),
      )[0]?.windowId ?? null
    case 'right':
      return [...list].sort(
        (a, b) => (b.position?.x ?? 0) - (a.position?.x ?? 0),
      )[0]?.windowId ?? null
    case 'next': {
      const idx = list.findIndex((win) => win.windowId === activeWindowId)
      if (idx < 0) return list[0]?.windowId ?? null
      return list[(idx + 1) % list.length]?.windowId ?? null
    }
    case 'previous': {
      const idx = list.findIndex((win) => win.windowId === activeWindowId)
      if (idx < 0) return list[0]?.windowId ?? null
      const prev = idx === 0 ? list.length - 1 : idx - 1
      return list[prev]?.windowId ?? null
    }
    default:
      return null
  }
}

export function resolveCloseWindowIds(
  target: WindowTarget,
  windows: WindowRef[] | undefined,
  activeWindowId: string | null,
): string[] {
  if (target.position === 'all') {
    return (windows ?? []).map((win) => win.windowId)
  }
  const id = resolveWindowTarget(target, windows, activeWindowId)
  return id ? [id] : []
}
