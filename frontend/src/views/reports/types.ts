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
    languages?: { name: string, percentage: number }[]
    soul_color?: string
    mood_keywords?: string[]
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
  icon: React.ComponentType<{ size?: number, className?: string }>
  color: string
}
