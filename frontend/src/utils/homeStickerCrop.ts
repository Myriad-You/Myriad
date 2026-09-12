import { widgetSizeSpan } from './widgetSizeScale'

export interface StickerCrop {
  x: number
  y: number
  zoom: number
}

export const STICKER_CROP_ZOOM_MIN = 1
export const STICKER_CROP_ZOOM_MAX = 4

export function defaultStickerCrop(): StickerCrop {
  return { x: 0.5, y: 0.5, zoom: 1 }
}

export function clampStickerCrop(crop: StickerCrop): StickerCrop {
  return {
    x: Math.min(1, Math.max(0, crop.x)),
    y: Math.min(1, Math.max(0, crop.y)),
    zoom: Math.min(
      STICKER_CROP_ZOOM_MAX,
      Math.max(STICKER_CROP_ZOOM_MIN, crop.zoom),
    ),
  }
}

export function parseStickerCrop(raw: unknown): StickerCrop | null {
  if (!raw || typeof raw !== 'object') return null
  const record = raw as Record<string, unknown>
  if (typeof record.x !== 'number' || typeof record.y !== 'number') return null
  const zoom = typeof record.zoom === 'number' ? record.zoom : 1
  return clampStickerCrop({ x: record.x, y: record.y, zoom })
}

export function stickerSlotAspect(size: string): number {
  const span = widgetSizeSpan(size)
  return span.w / Math.max(1, span.h)
}

export function stickerCropForSlot(
  imageWidth: number,
  imageHeight: number,
  size: string,
): StickerCrop | undefined {
  return stickerImageNeedsCrop(
    imageWidth,
    imageHeight,
    stickerSlotAspect(size),
  )
    ? defaultStickerCrop()
    : undefined
}

export function stickerImageNeedsCrop(
  imageWidth: number,
  imageHeight: number,
  slotAspect: number,
  epsilon = 0.04,
): boolean {
  if (imageWidth <= 0 || imageHeight <= 0 || slotAspect <= 0) return false
  const imageAspect = imageWidth / imageHeight
  return Math.abs(imageAspect - slotAspect) / slotAspect > epsilon
}

export function stickerCropObjectPosition(crop: StickerCrop): string {
  return `${crop.x * 100}% ${crop.y * 100}%`
}
