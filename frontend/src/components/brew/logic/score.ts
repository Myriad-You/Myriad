/**
 * Brew 磁贴墙的六因子评分。
 *
 * 纯函数、无 IO、`now` 必须注入 —— 排序结果要能在单测里锁死，
 * 读 `Date.now()` 会让同一份数据在不同时刻算出不同顺序。
 *
 * 角色差异是这里的重点，不是 UI 的重点：
 * - 游客没有已读态（后端对未登录一律返回 `unread_count = 0`），
 *   所以 unread 权重必须是 0，否则等于给所有源加同一个常数、白算一遍。
 * - 失败源对游客/成员是噪音（沉底），对管理员是待办（抬头），
 *   所以 fail 权重换号而不是换绝对值。
 */

import type { BrewSource } from '../../../types/brew'
import { isOwnBrewSource } from '../constants'

/** 观看者角色。与 `useAuth()` 的 isAuthenticated / isAdmin 两个布尔一一对应。 */
export type BrewViewerRole = 'guest' | 'member' | 'admin'

export interface ScoreFactors {
  /** 0 | 1 —— 站主自有内容（category 含「我」且非 admin_only） */
  own: number
  /** 0..1 —— 距最新一篇的衰减 exp(-days / 21) */
  recency: number
  /** 0..1 —— 语料规模 log1p(item_count) / log1p(2000) */
  corpus: number
  /** 0..1 —— 未读量 log1p(unread_count) / log1p(50) */
  unread: number
  /** 0 | 1 —— 用户手工排过序（sort_order 非空） */
  pin: number
  /** 0 | 1 —— 抓取失败过 */
  fail: number
}

export const SCORE_WEIGHTS: Record<BrewViewerRole, ScoreFactors> = {
  guest: { own: 0.4, recency: 0.28, corpus: 0.2, unread: 0, pin: 0.12, fail: -0.6 },
  member: { own: 0.18, recency: 0.22, corpus: 0.1, unread: 0.38, pin: 0.12, fail: -0.6 },
  admin: { own: 0.15, recency: 0.2, corpus: 0.08, unread: 0.32, pin: 0.1, fail: 0.45 },
}

/** 分档粒度。连续分数直接排会让每次刷新都微动，档内改按 id 定序。 */
export const SCORE_BUCKET = 0.05

/** recency 半衰参数（天）。 */
const RECENCY_TAU_DAYS = 21
/** corpus 饱和点（篇）。 */
const CORPUS_SATURATION = 2000
/** unread 饱和点（篇）。 */
const UNREAD_SATURATION = 50

const MS_PER_DAY = 86_400_000

export function roleFromAuth(
  isAuthenticated: boolean,
  isAdmin: boolean,
): BrewViewerRole {
  if (isAdmin) return 'admin'
  return isAuthenticated ? 'member' : 'guest'
}

/**
 * 最新一篇的发布时间（ms）；没有任何时间戳时返回 null。
 *
 * 优先 `recent_items[0].published_at`：`last_success_at` 是抓取时刻，
 * 健康源永远接近「现在」，用它算 recency 会让所有活源同分。
 */
export function lastPublishAt(s: BrewSource): number | null {
  const fromItems = s.recent_items?.[0]?.published_at
  if (typeof fromItems === 'number' && fromItems > 0) return fromItems
  if (typeof s.last_success_at === 'number' && s.last_success_at > 0) {
    return s.last_success_at
  }
  return null
}

/**
 * 距最新一篇的天数；没有时间戳返回 null（调用方必须据此跳过节律构图）。
 */
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
  // 友链没有更新时间，recency 一律 0 —— 否则会被 last_success_at 抬成「刚更新」
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

/** 落到 SCORE_BUCKET 的整数倍。同档内不再比较连续分数。 */
export function bucketScore(score: number): number {
  return Math.round(score / SCORE_BUCKET) * SCORE_BUCKET
}

/**
 * 智能排序比较器：分档降序，档内按 id 升序。
 *
 * 档内用 id 而不是分数，是为了让「未读 +1」这类微小变化不挪卡。
 */
export function compareByScore(
  a: BrewSource,
  b: BrewSource,
  role: BrewViewerRole,
  now: number,
): number {
  const bucketDelta =
    bucketScore(brewScore(b, role, now)) - bucketScore(brewScore(a, role, now))
  // 0.05 档的一半，避免浮点尾差被当成真实差异
  if (Math.abs(bucketDelta) > SCORE_BUCKET / 2) return bucketDelta > 0 ? 1 : -1
  return a.id - b.id
}

/** 按智能排序返回新数组（不改入参）。 */
export function sortByScore(
  sources: readonly BrewSource[],
  role: BrewViewerRole,
  now: number,
): BrewSource[] {
  return [...sources].sort((a, b) => compareByScore(a, b, role, now))
}
