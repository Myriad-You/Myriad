/**
 * TopicFeedMode - 主题跨源文章列表模式
 *
 * 形状抄 `CategoryFeedMode`：一个返回键 + 标题 + 篇数。区别是主题不是分类，
 * 没有「全部已读」—— 跨源批量标已读的语义不清楚（同一篇在源视图里也会变），
 * 想标已读回源视图做。
 */

import type { TopicFeedModeConfig } from './types'

import { LuChevronLeft as ChevronLeft, LuTag as Tag } from '@lib/icons'
import { IslandShell } from '../../../shared/control-island'
import { ISLAND_BTN } from './constants'

export interface TopicFeedModeProps {
  variant: 'mobile' | 'desktop'
  topicFeedMode: TopicFeedModeConfig
  /** 主题身份色；缺省用中性色 */
  hue?: string
  t: {
    backToAllSources: string
    totalArticles: string
  }
}

export function TopicFeedMode({
  variant,
  topicFeedMode,
  hue,
  t,
}: TopicFeedModeProps) {
  return (
    <IslandShell variant={variant} motionKey={`topic-feed-bar-${variant}`}>
      <button
        onClick={topicFeedMode.onBack}
        className={ISLAND_BTN}
        title={t.backToAllSources}
        aria-label={t.backToAllSources}
      >
        <ChevronLeft className="w-4.5 h-4.5" />
      </button>

      <div className="flex items-center gap-2 h-9 px-2 min-w-0 flex-1">
        <div
          className="w-7 h-7 rounded-lg flex items-center justify-center shrink-0"
          style={{ background: `${hue ?? '#6b7280'}26`, color: hue ?? '#6b7280' }}
        >
          <Tag className="w-4 h-4" />
        </div>
        <div className="min-w-0 flex-1">
          <h3 className="text-sm font-semibold text-gray-800 dark:text-gray-100 truncate">
            {topicFeedMode.topicLabel}
          </h3>
          <div className="text-[10px] text-gray-500 dark:text-gray-400">
            {t.totalArticles.replace('{count}', String(topicFeedMode.total))}
          </div>
        </div>
      </div>
    </IslandShell>
  )
}
