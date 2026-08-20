/**
 * 报告相关的TypeScript类型定义
 */

export interface PlatformReport {
  platform: string
  metadata: any
  summary: string
  insights: string[]
  card_visuals?: {
    danmaku?: string[]
    player_type?: string
    hardcore_score?: number
    top_genres?: string[]
    contribution_level?: string
    languages?: { name: string; percentage: number }[]
    soul_color?: string
    mood_keywords?: string[]
    status_counts?: Record<string, number>
    subject_type_distribution?: Record<string, number>
    collection_type_distribution?: Record<string, number>
    score_distribution?: Record<string, number>
    // 后端下发的是 { 标签: 次数 } 的分布对象，而非字符串数组
    favorite_tags?: Record<string, number>
    top_subjects?: Array<{
      subject_id?: number
      title?: string
      rate?: number
      subject_type?: string
      collection_type?: string
      cover?: string
    }>
    library_items?: Array<{
      title: string
      cover?: string
      type: string
      platform?: string
      rate?: number
    }>
  }
  created_at: string
}

export interface CrossPlatformReport {
  id?: number
  platform_reports: PlatformReport[]
  created_at: string
}

export interface PlatformConfig {
  id: string
  name: string
  icon: React.ComponentType<{ size?: number; className?: string }>
  color: string
}

/**
 * Report carousel card width — matches home WidgetGrid 4x2 at all breakpoints.
 *
 * Home columns (viewportBands): ≤767 → 4 cols, ≤1077 → 8 cols, ≥1078 → 16 cols.
 * Home shell: max-w-7xl (80rem) + p-2 (−1rem total) inside page padding.
 * --report-visible-cards is 1 / sm:2 / lg:4 (no md:3; tablet stays 50% like home).
 * gap-4 is spacing only and must not be baked into card width.
 */
export const REPORT_CARD_FLEX_BASIS =
  'calc((min(100dvw - 2 * var(--report-page-padding), 80rem) - 1rem) / var(--report-visible-cards))'

/**
 * First-card left edge inside the full-viewport strip.
 *
 * The strip breaks out to 100dvw, then reconstructs the page column:
 *   max(page gutter, centered max-w-7xl) + p-2 (0.5rem)
 * plus the status-bar rail's 0.25rem — same inset home uses on the user
 * card (`p-1`) and each widget. Without that last 0.25rem the GitHub/etc.
 * cards sit 4px left of the glass bar above them.
 */
export const REPORT_STRIP_ALIGN_PAD =
  'calc(max(var(--report-page-padding), (100dvw - 80rem) / 2) + 0.75rem)'

/**
 * Shared carousel strip CSS vars for the platform report strip.
 * Padding mirrors page gutters; visible-cards tracks home 4x2 fractions (1/2/4).
 */
export const REPORT_CAROUSEL_CSS_VARS =
  '[--report-page-padding:0.75rem] xs:[--report-page-padding:1rem] sm:[--report-page-padding:1.5rem] [--report-visible-cards:1] sm:[--report-visible-cards:2] lg:[--report-visible-cards:4]'
