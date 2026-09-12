import type { ReactNode } from 'react'

export interface ToggleSwitchPreview {
  on?: ReactNode
  off?: ReactNode
  disabled?: ReactNode
}

export interface ToggleSwitchPreviewResolved {
  body: ReactNode
  showKicker: boolean
}

function isUsableCopy(node: ReactNode): boolean {
  return node != null && node !== false && node !== ''
}

export function resolveToggleSwitchPreview(
  preview: ToggleSwitchPreview | undefined,
  checked: boolean,
  disabled: boolean,
): ToggleSwitchPreviewResolved {
  if (!preview) return { body: null, showKicker: false }

  if (disabled && isUsableCopy(preview.disabled)) {
    return { body: preview.disabled, showKicker: false }
  }

  const node = checked ? preview.off : preview.on
  if (!isUsableCopy(node)) return { body: null, showKicker: false }
  return { body: node, showKicker: true }
}
