/** 用户从输入框附上的文件。预览留在前端，发给后端的是名字、类型和文字摘录。 */

export const AGENT_ATTACH_MAX_COUNT = 4
export const AGENT_ATTACH_MAX_BYTES = 8 * 1024 * 1024
export const AGENT_ATTACH_TEXT_CHARS = 8000
export const AGENT_ATTACH_ACCEPT =
  'image/*,text/plain,text/markdown,text/csv,application/json,application/xml,text/xml,.txt,.md,.markdown,.csv,.json,.xml'

export interface AgentAttachment {
  id: string
  name: string
  mime: string
  size: number
  /** 图片预览，只给界面用 */
  previewUrl?: string
  /** 文本摘录，给后端看 */
  text?: string
}

export type AttachError = 'tooMany' | 'tooLarge' | 'unsupported'

const TEXT_TYPES = new Set([
  'text/plain',
  'text/markdown',
  'text/csv',
  'application/json',
  'application/xml',
  'text/xml',
])

export function isAttachableFile(file: File): boolean {
  if (file.type.startsWith('image/')) return true
  if (TEXT_TYPES.has(file.type)) return true
  return /\.(txt|md|markdown|csv|json|xml)$/i.test(file.name)
}

export function attachErrorFor(
  file: File,
  currentCount: number,
): AttachError | null {
  if (currentCount >= AGENT_ATTACH_MAX_COUNT) return 'tooMany'
  if (file.size > AGENT_ATTACH_MAX_BYTES) return 'tooLarge'
  if (!isAttachableFile(file)) return 'unsupported'
  return null
}

export async function fileToAttachment(file: File): Promise<AgentAttachment> {
  const id = `att_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`
  const mime = file.type || 'application/octet-stream'
  const base: AgentAttachment = {
    id,
    name: file.name,
    mime,
    size: file.size,
  }
  if (file.type.startsWith('image/')) {
    const previewUrl = await readAsDataUrl(file)
    return { ...base, previewUrl }
  }
  const raw = await file.text()
  const text = raw.slice(0, AGENT_ATTACH_TEXT_CHARS)
  return { ...base, text }
}

function readAsDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = () => resolve(String(reader.result || ''))
    reader.onerror = () => reject(reader.error)
    reader.readAsDataURL(file)
  })
}

export function attachmentsForRequest(
  attachments: readonly AgentAttachment[],
): Array<{ name: string; mime: string; size: number; text?: string }> {
  return attachments.map(({ name, mime, size, text }) => ({
    name,
    mime,
    size,
    ...(text ? { text } : {}),
  }))
}

export async function collectAttachments(
  files: Iterable<File>,
  current: readonly AgentAttachment[],
): Promise<{ attachments: AgentAttachment[]; error: AttachError | null }> {
  const next = [...current]
  let error: AttachError | null = null
  for (const file of files) {
    const err = attachErrorFor(file, next.length)
    if (err) {
      error ??= err
      continue
    }
    next.push(await fileToAttachment(file))
  }
  return { attachments: next, error }
}
