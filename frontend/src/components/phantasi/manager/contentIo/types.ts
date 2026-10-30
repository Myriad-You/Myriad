export interface ImportProgress {
  step: string
  current: number
  total: number
}

export type TransferNoteStatus = 'draft' | 'published'

export interface TransferNote {
  title: string
  content_md: string
  topic: string | null
  published_at: number | null
  status: TransferNoteStatus
  /** 旧站的 slug；这里用不了的样子不带，发布时自动生成。 */
  slug?: string
  /** 旧站上的文章地址（规范化后的路径），发布后访问会 301 到这篇。 */
  old_paths?: string[]
}

export type { NoteTransferKind } from '../../logic/board'
