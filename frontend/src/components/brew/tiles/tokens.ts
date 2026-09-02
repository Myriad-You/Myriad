/**
 * 磁贴排版 token。
 *
 * 字号乘 `fontScale`，间距乘 `scale` —— 两个缩放不是一回事，混用会让紧凑档
 * 的文字缩得比留白更狠（或反过来）。所有磁贴共用这一份，不要在组件里写字面量。
 */

import type { BrewTileSize } from '../logic/layout'

/** 站名 / 头条 / 主题名 */
export const T_TITLE = 14.5
/** 次条标题 */
export const T_MINOR = 11
/** 时间 / 标签 / 篇数说明 */
export const T_META = 8
/** 主题篇数、未读角标 */
export const T_NUM = 24
/** 数字型 4×4 的未读主数字 */
export const T_NUM_HERO_LARGE = 38
/** 数字型 4×2 的未读主数字 */
export const T_NUM_HERO_WIDE = 30

/** 站名行字标边长 */
export const MARK_SIZE = 18
/** 4×4 列表头条方图边长 */
export const LEAD_THUMB_SIZE = 52
/** icon 型的大字标边长 */
export const ICON_MARK_SIZE = 50

/** 4×4 feature 通栏封面高 */
export const COVER_H_FEATURE = 96
/** 4×4 主题拼贴高 */
export const COVER_H_TOPIC = 84

/** 4×2 左视觉栏占宽（feature / cadence） */
export const SPLIT_MEDIA_WIDTH = '40%'

/** px 取整并按几何缩放。间距、尺寸用这个。 */
export function sp(base: number, scale: number): number {
  return Math.round(base * scale)
}

/** 字号按 fontScale 缩放，保留一位小数（14.5 这类半 px 不能取整成 14）。 */
export function fs(base: number, fontScale: number): string {
  return `${Math.round(base * fontScale * 10) / 10}px`
}

/** 未读主数字的字号（只有数字型用）。 */
export function heroNumberSize(size: BrewTileSize): number {
  return size === '4x4' ? T_NUM_HERO_LARGE : T_NUM_HERO_WIDE
}
