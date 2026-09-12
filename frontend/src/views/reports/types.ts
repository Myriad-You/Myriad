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
    /** { tag: count }, not string[]. */
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

/** Matches home 4x2: 1 / sm:2 / lg:4 (no md:3). gap-4 is spacing only, not width. */
export const REPORT_CARD_FLEX_BASIS =
  'calc((min(100dvw - 2 * var(--report-page-padding), 80rem) - 1rem) / var(--report-visible-cards))'

/** First-card left edge: page column + 0.75rem. */
export const REPORT_STRIP_ALIGN_PAD =
  'calc(max(var(--report-page-padding), (100dvw - 80rem) / 2) + 0.75rem)'

/** Page-gutter padding; visible-cards tracks home 4x2 fractions. */
export const REPORT_CAROUSEL_CSS_VARS =
  '[--report-page-padding:0.75rem] xs:[--report-page-padding:1rem] sm:[--report-page-padding:1.5rem] [--report-visible-cards:1] sm:[--report-visible-cards:2] lg:[--report-visible-cards:4]'
