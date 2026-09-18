/** 字号乘 fontScale，间距乘 scale，不要混用。 */

import type { PhantasiTileSize } from '../logic/layout'

export const T_TITLE = 14.5
export const T_MINOR = 11
export const T_META = 8
export const T_FEATURED_TITLE = 17
export const T_FEATURED_BODY = 13.5
export const T_FEATURED_META = 12
export const T_NUM = 24
const T_NUM_HERO_LARGE = 38
const T_NUM_HERO_WIDE = 30

export const MARK_SIZE = 18
export const LEAD_THUMB_SIZE = 52
export const ICON_MARK_SIZE = 50

export const COVER_H_FEATURE = 96
export const COVER_H_FEATURED = 112
export const COVER_H_TOPIC = 84
export const COVER_H_TOPIC_WIDE = 56

export const SPLIT_MEDIA_WIDTH = '40%'

export function sp(base: number, scale: number): number {
  return Math.round(base * scale)
}

/** 字号保留一位小数，不能取整成 14。 */
export function fs(base: number, fontScale: number): string {
  return `${Math.round(base * fontScale * 10) / 10}px`
}

export function heroNumberSize(size: PhantasiTileSize): number {
  return size === '4x4' ? T_NUM_HERO_LARGE : T_NUM_HERO_WIDE
}
