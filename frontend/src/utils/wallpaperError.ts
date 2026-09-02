import { isUselessErrorText, userFacingError } from './userFacingError'

export interface WallpaperErrorCopy {
  unsafeUrl: string
  imageLoadFailed: string
  unknown: string
}

export function wallpaperUnknownMessage(
  reason: unknown,
  copy: WallpaperErrorCopy,
): string {
  const raw =
    reason instanceof Error
      ? reason.message.trim()
      : typeof reason === 'string'
        ? reason.trim()
        : ''
  if (!raw || isUselessErrorText(raw)) return copy.unknown
  if (/timeout|decode|failed to (load|fetch)|http\s*\d|network/i.test(raw)) {
    return copy.imageLoadFailed
  }
  const mapped = userFacingError(reason, copy.unknown)
  return mapped === raw ? copy.unknown : mapped
}
