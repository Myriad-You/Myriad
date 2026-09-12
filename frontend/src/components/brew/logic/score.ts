/** `now` 必须注入。游客 unread 权重必须为 0。fail 权重按角色换号。 */

import type { BrewSource } from '../../../types/brew'
import { isOwnBrewSource } from '../constants'

export type BrewViewerRole = 'guest' | 'member' | 'admin'

export interface ScoreFactors {
  own: number
  recency: number
  corpus: number
  unread: number
  pin: number
  fail: number
}

export const SCORE_WEIGHTS: Record<BrewViewerRole, ScoreFactors> = {
  guest: { own: 0.4, recency: 0.28, corpus: 0.2, unread: 0, pin: 0.12, fail: -0.6 },
  member: { own: 0.18, recency: 0.22, corpus: 0.1, unread: 0.38, pin: 0.12, fail: -0.6 },
  admin: { own: 0.15, recency: 0.2, corpus: 0.08, unread: 0.32, pin: 0.1, fail: 0.45 },
}

/** 连续分直接排会微动；档内按 id。 */
export const SCORE_BUCKET = 0.05

const RECENCY_TAU_DAYS = 21
const CORPUS_SATURATION = 2000
const UNREAD_SATURATION = 50

const MS_PER_DAY = 86_400_000

export function roleFromAuth(
  isAuthenticated: boolean,
  isAdmin: boolean,
): BrewViewerRole {
  if (isAdmin) return 'admin'
  return isAuthenticated ? 'member' : 'guest'
}

/** 优先 published_at；用 last_success_at 会让活源 recency 同分。 */
export function lastPublishAt(s: BrewSource): number | null {
  const fromItems = s.recent_items?.[0]?.published_at
  if (typeof fromItems === 'number' && fromItems > 0) return fromItems
  if (typeof s.last_success_at === 'number' && s.last_success_at > 0) {
    return s.last_success_at
  }
  return null
}

/** 没有时间戳返回 null，调用方必须跳过节律构图。 */
export function daysSinceLastPublish(
  s: BrewSource,
  now: number,
): number | null {
  const at = lastPublishAt(s)
  if (at === null) return null
  return Math.max(0, (now - at) / MS_PER_DAY)
}

function clamp01(v: number): number {
  if (!Number.isFinite(v)) return 0
  if (v < 0) return 0
  return v > 1 ? 1 : v
}

export function scoreFactors(s: BrewSource, now: number): ScoreFactors {
  // 友链 recency 一律 0，避免 last_success_at 抬成刚更新。
  const isLink = s.source_type === 'link'
  const days = daysSinceLastPublish(s, now)

  return {
    own: isOwnBrewSource(s) ? 1 : 0,
    recency:
      isLink || days === null ? 0 : clamp01(Math.exp(-days / RECENCY_TAU_DAYS)),
    corpus: clamp01(
      Math.log1p(Math.max(0, s.item_count)) / Math.log1p(CORPUS_SATURATION),
    ),
    unread: clamp01(
      Math.log1p(Math.max(0, s.unread_count)) / Math.log1p(UNREAD_SATURATION),
    ),
    pin: s.sort_order !== null && s.sort_order !== undefined ? 1 : 0,
    fail: s.error_count > 0 ? 1 : 0,
  }
}

export function brewScore(
  s: BrewSource,
  role: BrewViewerRole,
  now: number,
): number {
  const f = scoreFactors(s, now)
  const w = SCORE_WEIGHTS[role]
  return (
    f.own * w.own +
    f.recency * w.recency +
    f.corpus * w.corpus +
    f.unread * w.unread +
    f.pin * w.pin +
    f.fail * w.fail
  )
}

/** 同档内不再比较连续分数。 */
export function bucketScore(score: number): number {
  return Math.round(score / SCORE_BUCKET) * SCORE_BUCKET
}

/** 档内按 id，未读 +1 不挪卡。 */
export function compareByScore(
  a: BrewSource,
  b: BrewSource,
  role: BrewViewerRole,
  now: number,
): number {
  const bucketDelta =
    bucketScore(brewScore(b, role, now)) - bucketScore(brewScore(a, role, now))
  // 半档阈值，避免浮点尾差。
  if (Math.abs(bucketDelta) > SCORE_BUCKET / 2) return bucketDelta > 0 ? 1 : -1
  return a.id - b.id
}

export function sortByScore(
  sources: readonly BrewSource[],
  role: BrewViewerRole,
  now: number,
): BrewSource[] {
  return sources.toSorted((a, b) => compareByScore(a, b, role, now))
}
