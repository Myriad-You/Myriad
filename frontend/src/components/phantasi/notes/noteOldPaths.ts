import { ApiError } from '../../../services/api'

/**
 * 搬家来的文章原来的地址。规则和后端 `crates/myriad-phantasi-notes/src/old_paths.rs`
 * 一字不差：只留路径、去掉结尾斜杠、百分号编码能解成 UTF-8 的就解开、站内地址不收。
 * 前端先按同样的规则规范化，存回来的和输入框里的才对得上，输入框不会被服务端改写。
 */

export const MAX_NOTE_OLD_PATHS = 20
export const MAX_NOTE_OLD_PATH_BYTES = 512
const MAX_RAW_BYTES = 4096

const RESERVED_FIRST_SEGMENTS = new Set([
  'api',
  'health',
  'healthz',
  'ready',
  'internal',
  '_updater',
  '_proxy',
  'sitemap.xml',
  'robots.txt',
  'llms.txt',
  '.well-known',
  'nodeinfo',
  'inbox',
  'users',
  'media',
  'phantasi',
  'assets',
  'embed',
  'library',
  'studio',
  'config',
  'agent',
  'login',
  'register',
  'setup',
  'tapp',
])

const KEEP_ENCODED = new Set([0x2F, 0x3F, 0x23, 0x25])
const encoder = new TextEncoder()

function hexValue(byte: number | undefined): number | null {
  if (byte == null) return null
  if (byte >= 0x30 && byte <= 0x39) return byte - 0x30
  if (byte >= 0x41 && byte <= 0x46) return byte - 0x41 + 10
  if (byte >= 0x61 && byte <= 0x66) return byte - 0x61 + 10
  return null
}

function hex2(byte: number): string {
  return `%${byte.toString(16).toUpperCase().padStart(2, '0')}`
}

function isControl(code: number): boolean {
  return code < 0x20 || (code >= 0x7F && code <= 0x9F)
}

function stripOrigin(raw: string): string {
  const lower = raw.slice(0, 8).toLowerCase()
  let rest: string
  if (lower.startsWith('https://')) rest = raw.slice(8)
  else if (lower.startsWith('http://')) rest = raw.slice(7)
  else if (raw.startsWith('//')) rest = raw.slice(2)
  else return raw
  const at = rest.search(/[/?#]/)
  return at < 0 ? '' : rest.slice(at)
}

function percentDecode(text: string): number[] | null {
  const bytes = encoder.encode(text)
  const out: number[] = []
  for (let index = 0; index < bytes.length;) {
    const byte = bytes[index]!
    if (byte !== 0x25) {
      out.push(byte)
      index += 1
      continue
    }
    const high = hexValue(bytes[index + 1])
    const low = hexValue(bytes[index + 2])
    if (high == null || low == null) {
      out.push(0x25, 0x32, 0x35)
      index += 1
      continue
    }
    const value = high * 16 + low
    if (KEEP_ENCODED.has(value)) {
      for (const char of hex2(value)) out.push(char.charCodeAt(0))
    } else if (value < 0x20 || value === 0x7F) {
      return null
    } else {
      out.push(value)
    }
    index += 3
  }
  return out
}

function decodeUtf8(bytes: number[]): string {
  try {
    return new TextDecoder('utf-8', { fatal: true }).decode(new Uint8Array(bytes))
  } catch {
    // 解不成 UTF-8 的（比如按 GBK 编码的旧地址）把高位字节编回去，和后端一样。
    return bytes.map((byte) => (byte < 0x80 ? String.fromCharCode(byte) : hex2(byte))).join('')
  }
}

type OldPathProblem = 'invalid' | 'reserved' | 'too-long'

function canonicalPath(raw: string): string | OldPathProblem {
  const trimmed = raw.trim()
  if (!trimmed) return 'invalid'
  if (encoder.encode(trimmed).length > MAX_RAW_BYTES) return 'too-long'
  const rest = stripOrigin(trimmed).split(/[?#]/)[0] ?? ''
  if (rest.includes('\\')) return 'invalid'
  for (const char of rest) {
    if (isControl(char.codePointAt(0)!)) return 'invalid'
  }
  const bytes = percentDecode(rest)
  if (!bytes) return 'invalid'
  const text = decodeUtf8(bytes)
  let out = ''
  for (const segment of text.split('/')) {
    if (!segment) continue
    if (segment === '.' || segment === '..') return 'invalid'
    out += `/${segment}`
  }
  if (!out) out = '/'
  if (encoder.encode(out).length > MAX_NOTE_OLD_PATH_BYTES) return 'too-long'
  return out
}

function isReserved(path: string): boolean {
  const lower = path.toLowerCase()
  if (lower === '/' || lower === '/blog' || lower === '/blog/posts' || lower.startsWith('/blog/posts/')) return true
  const first = lower.replace(/^\/+/, '').split('/')[0] ?? ''
  return RESERVED_FIRST_SEGMENTS.has(first)
}

/** 一条旧地址规范化后的样子；不收的返回原因。 */
export function normalizeOldPath(raw: string): string | OldPathProblem {
  const path = canonicalPath(raw)
  if (path === 'invalid' || path === 'too-long') return path
  return isReserved(path) ? 'reserved' : path
}

function isProblem(value: string): value is OldPathProblem {
  return value === 'invalid' || value === 'reserved' || value === 'too-long'
}

/**
 * 输入框里每行一条。合法时返回规范化、去重后的列表（空行跳过），
 * 有一行不合法或条数超了返回 undefined，调用方不要写进 PUT。
 */
export function noteOldPathsWire(text: string | null | undefined): string[] | undefined {
  const out: string[] = []
  for (const line of (text ?? '').split('\n')) {
    if (!line.trim()) continue
    const path = normalizeOldPath(line)
    if (isProblem(path)) return undefined
    if (!out.includes(path)) out.push(path)
  }
  return out.length > MAX_NOTE_OLD_PATHS ? undefined : out
}

export function noteOldPathsInvalid(text: string | null | undefined): boolean {
  return noteOldPathsWire(text) === undefined
}

/** 存在云端字段里的形状：规范化后一行一条，没有就是空串。 */
export function oldPathsField(paths: readonly string[] | null | undefined): string | null {
  return paths ? paths.join('\n') : null
}

export function oldPathsFromField(field: string | null | undefined): string[] {
  return field ? field.split('\n').filter(Boolean) : []
}

/** 409 里只有旧地址被占用的那条带 `old address`。 */
export function isOldPathTakenError(error: unknown): boolean {
  if (!(error instanceof ApiError) || error.status !== 409) return false
  return `${error.code ?? ''} ${error.message}`.toLowerCase().includes('old address')
}
