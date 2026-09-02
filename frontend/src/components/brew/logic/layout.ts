/**
 * 磁贴构图与尺寸的派生规则。
 *
 * 两条硬约束：
 * 1. 窄屏降档**只在这里**做。组件内部禁止再写第二套窄屏判断，
 *    否则同一个 `4x4` 会在两处得出不同结论。
 * 2. 生产只有 `2x2` / `4x2` / `4x4` 三档。`WidgetSize` 枚举里没有 `8x4`。
 */

import type { BrewSource } from '../../../types/brew'
import type { ViewportBand } from '../../../utils/viewportBands'
import type { WidgetSize } from '../../WidgetGrid'
import type { BrewViewerRole } from './score'
import { daysSinceLastPublish } from './score'

/** 生产用的三档尺寸。是 `WidgetSize` 的子集（下方 satisfies 守住这点）。 */
export type BrewTileSize = '2x2' | '4x2' | '4x4'

export const BREW_TILE_SIZES = ['2x2', '4x2', '4x4'] as const satisfies
  readonly WidgetSize[]

/** 构图型。按源的状态派生，不是一种模子刻到底。 */
export type BrewTileLayout = 'feature' | 'list' | 'cadence' | 'numeric' | 'icon'

/** 进入 numeric 构图的未读阈值（仅登录角色）。 */
export const NUMERIC_UNREAD_MIN = 20
/** 进入 cadence 构图的沉寂天数。 */
export const CADENCE_QUIET_DAYS = 60
/** cadence 构图至少需要的 pulses 根数；不足则降级 feature。 */
export const CADENCE_MIN_PULSES = 6
/** 条目少到这个数就走 feature（列表撑不起来）。 */
export const FEATURE_MAX_ITEMS = 2
/** 没有封面的源要有这么多条才配得上 4x4：少于一页列表的量，中间必然是空的。 */
export const FULL_TILE_MIN_ITEMS = 5

/** 源少于这个数时一律撑满，不留空格子。 */
export const FULL_BLEED_SOURCE_COUNT = 6
/** 分数上界：≥ 走 4x4 */
export const SIZE_SCORE_LARGE = 0.55
/** 分数中界：≥ 走 4x2 */
export const SIZE_SCORE_MEDIUM = 0.3

/**
 * 构图决策树。按顺序，命中即停：
 *
 * ```
 * source_type === 'link'                         → icon
 * role !== 'guest' && unread_count >= 20         → numeric
 * daysSinceLastPublish > 60 && pulses.length ≥ 6 → cadence
 * 展示条目数 ≤ 2                                  → feature
 * 其余                                            → list
 * ```
 *
 * `now` 必须注入：沉寂判断依赖当前时间，纯函数不自己读表。组件侧一次性取
 * `now`（会话内冻结布局），单测注入固定值。
 *
 * @param itemCount 实际可展示的条目数。默认用 `recent_items` 的长度；网格补拉过
 *   `getItems` 时传补齐后的条数，否则「条目 ≤ 2 → feature」会对只回了 3 条
 *   预览的源误判。
 */
export function tileLayout(
  s: BrewSource,
  role: BrewViewerRole,
  now: number,
  itemCount: number = s.recent_items?.length ?? 0,
): BrewTileLayout {
  if (s.source_type === 'link') return 'icon'

  // 游客的 unread_count 恒为 0，这一支天然不会命中；显式判角色是为了让规则
  // 可读，也避免后端某天开始给游客回聚合数时静默进入数字型。
  if (role !== 'guest' && s.unread_count >= NUMERIC_UNREAD_MIN) return 'numeric'

  // 没有时间戳时不走 cadence —— 无法判断沉寂
  const quietDays = daysSinceLastPublish(s, now)
  if (quietDays !== null && quietDays > CADENCE_QUIET_DAYS) {
    // 只有三根线的节律图没有信息量：pulses 不足就整支降级为 feature，
    // 而不是掉进 list —— 沉寂源的列表全是几年前的标题，读不出任何东西。
    return (s.pulses?.length ?? 0) >= CADENCE_MIN_PULSES ? 'cadence' : 'feature'
  }

  if (itemCount <= FEATURE_MAX_ITEMS) return 'feature'
  return 'list'
}

/**
 * 按 band 降档。desktop 完整三档；tablet / phone 没有 4x4 的容身之处。
 *
 * phone 是 4 列 × 4 行，`4x2` 正好满宽。
 */
export function downgradeForBand(
  size: BrewTileSize,
  band: ViewportBand,
): BrewTileSize {
  if (band === 'desktop') return size
  return size === '4x4' ? '4x2' : size
}

/** `card_size`（旧的手工尺寸）现在的语义是「用户锁定」。零 migration。 */
const CARD_SIZE_TO_TILE: Record<'tiny' | 'mini' | 'full', BrewTileSize> = {
  tiny: '2x2',
  mini: '4x2',
  full: '4x4',
}

/**
 * 尺寸派生。
 *
 * `card_size` 非空即视为用户锁定：映射后只套 band 降档，不再看分数，
 * 也不被「源太少一律撑满」覆盖 —— 显式意图优先于启发式。
 */
export function tileSize(
  score: number,
  s: BrewSource,
  band: ViewportBand,
  sourceCount: number,
): BrewTileSize {
  const locked = s.card_size ? CARD_SIZE_TO_TILE[s.card_size] : null
  if (locked) return downgradeForBand(locked, band)

  if (sourceCount < FULL_BLEED_SOURCE_COUNT) {
    return downgradeForBand('4x4', band)
  }
  // 友链是入口不是内容，给不了 4x4 的信息量
  if (s.source_type === 'link') return downgradeForBand('4x2', band)

  // 内容撑不起来的源不给 4x4：一张封面都没有、条目又不够铺满一页列表时，
  // 4x4 的下半张必然是空的（管理员视图里被 fail 权重抬上来的失败源、只有三四
  // 条纯文字的小源都是这样）。有封面的 4x4 靠图撑得住。
  // 「源太少一律撑满」走在前面不受影响 —— 那是有意留白，不是没排完。
  const count = s.recent_items?.length ?? 0
  const hasCover = Boolean(s.recent_items?.some((i) => i.image))
  // 4x4 只有两种填法：满一页列表（≥5 条），或 feature 构图的通栏大图（≤2 条
  // 且有封面）。三四条配一张 52px 小方图撑不起 320px，那是最空的一种卡。
  const thin = count < FULL_TILE_MIN_ITEMS && !(count <= FEATURE_MAX_ITEMS && hasCover)
  if (score >= SIZE_SCORE_LARGE) {
    return downgradeForBand(thin ? '4x2' : '4x4', band)
  }
  if (score >= SIZE_SCORE_MEDIUM) return downgradeForBand('4x2', band)
  return downgradeForBand('2x2', band)
}

/** 智能模式下前几个主题卡吃 4x4。 */
export const TOPIC_LARGE_COUNT_SMART = 2
/** 主题模式下前几个主题卡吃 4x4。 */
export const TOPIC_LARGE_COUNT_TOPIC_MODE = 4

/**
 * 主题磁贴尺寸。主题卡永远是通栏的（最小 4x2），不进 2x2。
 */
export function topicTileSize(
  index: number,
  mode: 'smart' | 'topic',
  band: ViewportBand,
): BrewTileSize {
  const largeCount =
    mode === 'topic' ? TOPIC_LARGE_COUNT_TOPIC_MODE : TOPIC_LARGE_COUNT_SMART
  const size: BrewTileSize = index < largeCount ? '4x4' : '4x2'
  return downgradeForBand(size, band)
}
