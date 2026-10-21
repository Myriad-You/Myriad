import apiService from '../services/api'

/** 报告风格：只管文字怎么写。只有站长能读写，接口都在 `/api/reports/styles`。 */
export type ReportStyleVoice = 'narrator' | 'owner'
export type ReportStyleLength = 'short' | 'standard' | 'long'

export interface ReportStyle {
  id: string
  name: string
  voice: ReportStyleVoice
  tone: string
  length: ReportStyleLength
  /** 固定六个章节键里挑的几个，按播放顺序。 */
  chapters: string[]
  avoid: string[]
  sample?: string
}

export interface ReportStyleLimits {
  custom_styles: number
  name_chars: number
  tone_chars: number
  sample_chars: number
  avoid_items: number
  avoid_chars: number
}

export interface ReportStylesState {
  current: string
  current_hash: string
  /** 整份风格配置的版本。保存时带回去，别处先改过就会被拒（409 report_styles_conflict）。 */
  revision: string
  builtin: ReportStyle[]
  custom: ReportStyle[]
  chapter_keys: { key: string; meaning: string }[]
  limits: ReportStyleLimits
}

export interface ReportStylePreview {
  headline?: string
  chapter?: { key: string; title: string; beats: { text: string; fact?: string }[] } | null
  facts?: unknown
}

export function fetchReportStyles(): Promise<ReportStylesState> {
  return apiService.get<ReportStylesState>('/reports/styles')
}

/** 整表替换：当前用哪套 + 全部自定义风格，基于读到的那个版本。 */
export function saveReportStyles(
  baseRevision: string,
  current: string,
  custom: ReportStyle[],
): Promise<ReportStylesState> {
  return apiService.put<ReportStylesState>('/reports/styles', { base_revision: baseRevision, current, custom })
}

/** 按草稿写一幕看看，不落库；会调用一次模型。 */
export function previewReportStyle(
  platform: string,
  style: ReportStyle,
  chapter?: string,
): Promise<ReportStylePreview> {
  return apiService.post<ReportStylePreview>('/reports/styles/preview', { platform, style, chapter })
}

/** 自定义风格的编号：小写字母、数字、`-`，不撞名。 */
export function newCustomStyleId(taken: Iterable<string>): string {
  const used = new Set(taken)
  for (let n = 0; ; n += 1) {
    const id = `custom-${Date.now().toString(36)}${n ? `-${n}` : ''}`
    if (!used.has(id)) return id
  }
}
