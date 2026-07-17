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

export interface BackgroundElement {
  type: 'circle' | 'rect' | 'gradient' | 'pattern' | 'svg'
  style?: React.CSSProperties
  className?: string
  animate?: any
  transition?: any
  svgPath?: string
  content?: string
}

export interface ComprehensiveAnalysis {
  [key: string]: any
  theme_color: string
  visual_style: string
  decorative_emojis: string[]
  card_subtitle: string
  key_metric: string
  background_elements?: BackgroundElement[]
  icon_image_url?: string
  icon_prompt?: string
  theme_icon?: string
}

export interface CrossPlatformReport {
  id?: number
  platform_reports: PlatformReport[]
  综合分析?: ComprehensiveAnalysis | null
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
 * Home columns: <640 → 4 cols (4x2 = 100%), <1024 → 8 cols (4x2 = 50%), else 16 cols (4x2 = 25%).
 * Home shell: max-w-7xl (80rem) + p-2 (−1rem total) inside page padding.
 * --report-visible-cards is 1 / sm:2 / lg:4 (no md:3; tablet stays 50% like home).
 * gap-4 is spacing only and must not be baked into card width.
 */
export const REPORT_CARD_FLEX_BASIS =
  'calc((min(100dvw - 2 * var(--report-page-padding), 80rem) - 1rem) / var(--report-visible-cards))'

/**
 * Shared carousel strip CSS vars for platform + comprehensive strips.
 * Padding mirrors page gutters; visible-cards tracks home 4x2 fractions (1/2/4).
 */
export const REPORT_CAROUSEL_CSS_VARS =
  '[--report-page-padding:0.75rem] xs:[--report-page-padding:1rem] sm:[--report-page-padding:1.5rem] [--report-visible-cards:1] sm:[--report-visible-cards:2] lg:[--report-visible-cards:4]'
