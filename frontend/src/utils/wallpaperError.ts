export interface WallpaperErrorCopy {
  unsafeUrl: string
  imageLoadFailed: string
  unknown: string
}

export function wallpaperUnknownMessage(
  reason: unknown,
  copy: WallpaperErrorCopy,
): string {
  if (reason instanceof Error && reason.message.trim()) {
    const message = reason.message.trim()
    if (!/^API Error:\s*\d+$/i.test(message)) return message
  }
  return copy.unknown
}
