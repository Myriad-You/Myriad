import assert from 'node:assert/strict'
import { readdirSync, readFileSync } from 'node:fs'
import { dirname, join, relative } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

const HERE = dirname(fileURLToPath(import.meta.url))
const FRONTEND = join(HERE, '..')

const SKIP_DIR = new Set(['node_modules', 'dist', '.astro', 'vendor'])
const EXTS = new Set([
  '.ts',
  '.tsx',
  '.js',
  '.jsx',
  '.mjs',
  '.cjs',
  '.css',
  '.astro',
])

const HISTORY_PHRASES = [
  '曾经',
  '原来是',
  '原来那个',
  '原实现',
  '已经不存在',
  '历史兼容',
  '为了兼容性',
  'used to be',
  'used to return',
  'used to assert',
  'formerly',
  'was the one exception',
  'they were only ever',
] as const

const SPECULATION_PHRASES = [
  '大概',
  '也许是',
  '好像是',
  '猜测',
] as const

const RESTATEMENT_STUBS = [
  '类型定义',
  '辅助方法',
  '单例导出',
  '导出类型',
  '内容管理',
  '提供者管理',
  '监听器管理',
  'Tapp 集成',
  'Tapp Page 沙箱组件',
  '渲染 Tapp 图标',
  'Tapp',
  '全局控制面板',
  '计算匹配的关键词数量',
  '将 SVG 转换为 data URI（使用 useMemo 避免重复计算）',
  '1. 优先使用内联 SVG（通过 img + data URI 渲染）',
  '2. 检查 icon 是否为 URL 或 Myriad 图标 token',
  '3. 使用 emoji',
  '4. Fallback：显示名称首字母',
] as const

function inScope(rel: string): boolean {
  if (rel === 'public/sw.js') return true
  if (rel.startsWith('src/')) return true
  if (rel.startsWith('tests/')) return true
  if (rel.startsWith('scripts/')) return true
  return (
    rel === 'eslint.config.js'
    || rel === 'astro.config.mjs'
    || rel === 'tailwind.config.mjs'
    || rel === 'playwright.config.ts'
    || rel === 'taze.config.js'
  )
}

function walk(dir: string, acc: string[] = []): string[] {
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    const p = join(dir, e.name)
    const rel = relative(FRONTEND, p).replaceAll('\\', '/')
    if (e.isDirectory()) {
      if (SKIP_DIR.has(e.name)) continue
      if (e.name === 'tapp-runtime' && rel.startsWith('public/')) continue
      walk(p, acc)
      continue
    }
    const dot = e.name.lastIndexOf('.')
    const ext = dot >= 0 ? e.name.slice(dot) : ''
    if (!EXTS.has(ext) && e.name !== 'sw.js') continue
    if (`/${rel}/`.includes('/vendor/')) continue
    if (rel.startsWith('public/tapp-runtime/')) continue
    if (!inScope(rel)) continue
    acc.push(p)
  }
  return acc
}

function isIdentChar(c: string): boolean {
  return /[\w$]/.test(c)
}

function prevNonWs(text: string, idx: number): string {
  let j = idx - 1
  while (j >= 0 && (text[j] === ' ' || text[j] === '\t')) j--
  return j >= 0 ? text[j]! : ''
}

function canStartRegex(text: string, idx: number): boolean {
  const p = prevNonWs(text, idx)
  if (p === '' || p === '<' || p === '>') return false
  if (/[([{;,:!?=~&|*%^+]/.test(p)) return true
  let j = idx - 1
  while (j >= 0 && /\s/.test(text[j]!)) j--
  if (j < 0) return true
  if (!isIdentChar(text[j]!)) return false
  let k = j
  while (k >= 0 && isIdentChar(text[k]!)) k--
  const word = text.slice(k + 1, j + 1)
  return /^(return|case|throw|new|delete|void|typeof|in|of|instanceof|else|do|yield|await)$/.test(
    word,
  )
}

export function extractJsComments(
  text: string,
): { line: number; raw: string }[] {
  const out: { line: number; raw: string }[] = []
  const n = text.length
  let i = 0
  let line = 1
  let inSingle = false
  let inDouble = false
  let inTemplate = false
  let templateExprDepth = 0
  let inLineComment = false
  let inBlockComment = false
  let inRegex = false
  let regexCharClass = false
  let commentStart = 0
  let commentStartLine = 1

  while (i < n) {
    const c = text[i]!
    const next = i + 1 < n ? text[i + 1]! : ''

    if (inLineComment) {
      if (c === '\n') {
        out.push({ line: commentStartLine, raw: text.slice(commentStart, i) })
        inLineComment = false
        line++
      }
      i++
      continue
    }
    if (inBlockComment) {
      if (c === '*' && next === '/') {
        out.push({
          line: commentStartLine,
          raw: text.slice(commentStart, i + 2),
        })
        inBlockComment = false
        i += 2
        continue
      }
      if (c === '\n') line++
      i++
      continue
    }
    if (inRegex) {
      if (c === '\n') {
        line++
        inRegex = false
        regexCharClass = false
        i++
        continue
      }
      if (c === '\\') {
        i += 2
        continue
      }
      if (regexCharClass) {
        if (c === ']') regexCharClass = false
        i++
        continue
      }
      if (c === '[') {
        regexCharClass = true
        i++
        continue
      }
      if (c === '/') {
        inRegex = false
        i++
        continue
      }
      i++
      continue
    }
    if (inSingle) {
      if (c === '\\') {
        i += 2
        continue
      }
      if (c === "'") inSingle = false
      if (c === '\n') line++
      i++
      continue
    }
    if (inDouble) {
      if (c === '\\') {
        i += 2
        continue
      }
      if (c === '"') inDouble = false
      if (c === '\n') line++
      i++
      continue
    }
    if (inTemplate) {
      if (c === '\\') {
        i += 2
        continue
      }
      if (c === '`') {
        inTemplate = false
        i++
        continue
      }
      if (c === '$' && next === '{') {
        templateExprDepth++
        inTemplate = false
        i += 2
        continue
      }
      if (c === '\n') line++
      i++
      continue
    }

    if (c === "'") {
      inSingle = true
      i++
      continue
    }
    if (c === '"') {
      inDouble = true
      i++
      continue
    }
    if (c === '`') {
      inTemplate = true
      i++
      continue
    }
    if (c === '}' && templateExprDepth > 0) {
      templateExprDepth--
      inTemplate = true
      i++
      continue
    }
    if (c === '/' && next === '/') {
      inLineComment = true
      commentStart = i
      commentStartLine = line
      i += 2
      continue
    }
    if (c === '/' && next === '*') {
      inBlockComment = true
      commentStart = i
      commentStartLine = line
      i += 2
      continue
    }
    if (c === '/' && next !== '>' && canStartRegex(text, i)) {
      inRegex = true
      i++
      continue
    }
    if (c === '\n') line++
    i++
  }
  if (inLineComment) {
    out.push({ line: commentStartLine, raw: text.slice(commentStart) })
  }
  return out
}

export function extractCssComments(
  text: string,
): { line: number; raw: string }[] {
  const out: { line: number; raw: string }[] = []
  const n = text.length
  let i = 0
  let line = 1
  let inSingle = false
  let inDouble = false
  let inBlock = false
  let commentStart = 0
  let commentStartLine = 1
  while (i < n) {
    const c = text[i]!
    const next = i + 1 < n ? text[i + 1]! : ''
    if (inBlock) {
      if (c === '*' && next === '/') {
        out.push({
          line: commentStartLine,
          raw: text.slice(commentStart, i + 2),
        })
        inBlock = false
        i += 2
        continue
      }
      if (c === '\n') line++
      i++
      continue
    }
    if (inSingle) {
      if (c === '\\') {
        i += 2
        continue
      }
      if (c === "'") inSingle = false
      if (c === '\n') line++
      i++
      continue
    }
    if (inDouble) {
      if (c === '\\') {
        i += 2
        continue
      }
      if (c === '"') inDouble = false
      if (c === '\n') line++
      i++
      continue
    }
    if (c === "'") {
      inSingle = true
      i++
      continue
    }
    if (c === '"') {
      inDouble = true
      i++
      continue
    }
    if (c === '/' && next === '*') {
      inBlock = true
      commentStart = i
      commentStartLine = line
      i += 2
      continue
    }
    if (c === '\n') line++
    i++
  }
  return out
}

function extractHtmlComments(
  text: string,
): { line: number; raw: string }[] {
  const out: { line: number; raw: string }[] = []
  const re = /<!--([\s\S]*?)-->/g
  let m = re.exec(text)
  while (m) {
    const before = text.slice(0, m.index)
    const line = 1 + (before.match(/\n/g) || []).length
    out.push({ line, raw: m[0] })
    m = re.exec(text)
  }
  return out
}

function extractFile(path: string, text: string): { line: number; raw: string }[] {
  if (path.endsWith('.css')) return extractCssComments(text)
  if (path.endsWith('.astro')) {
    const seen = new Set<string>()
    const merged: { line: number; raw: string }[] = []
    for (const c of [...extractJsComments(text), ...extractHtmlComments(text)]) {
      const k = `${c.line}:${c.raw}`
      if (seen.has(k)) continue
      seen.add(k)
      merged.push(c)
    }
    return merged.toSorted((a, b) => a.line - b.line)
  }
  return extractJsComments(text)
}

function stripComment(raw: string): string {
  let s = raw.trim()
  if (s.startsWith('<!--')) {
    s = s.slice(4, s.endsWith('-->') ? s.length - 3 : s.length)
  } else if (s.startsWith('///')) {
    s = s.slice(3)
  } else if (s.startsWith('//')) {
    s = s.slice(2)
  } else if (s.startsWith('/*')) {
    s = s.slice(2, s.endsWith('*/') ? s.length - 2 : s.length)
  }
  return s.replaceAll(/^\s*\* ?/gm, '').trim()
}

describe('authored frontend comments', () => {
  const files = walk(FRONTEND)
  const extracted: { rel: string; line: number; body: string }[] = []
  for (const p of files) {
    const rel = relative(FRONTEND, p).replaceAll('\\', '/')
    const text = readFileSync(p, 'utf8')
    for (const c of extractFile(p, text)) {
      extracted.push({ rel, line: c.line, body: stripComment(c.raw) })
    }
  }

  it('walks authored trees and skips string/template contents', () => {
    assert.ok(files.length > 100, `expected a real walk, got ${files.length} files`)
    assert.ok(
      extracted.length > 0,
      'extractor must find comments (directives remain)',
    )
    const runtimeTest = extracted.filter(
      (c) => c.rel === 'src/tapp/runtime/moduleRuntime.test.ts',
    )
    assert.equal(
      runtimeTest.some((c) => c.body.includes("require('./commented.js')")),
      false,
      'template-literal fixture comments must not be extracted',
    )
    const eslint = extracted.filter((c) => c.rel === 'eslint.config.js')
    assert.ok(
      eslint.some((c) => c.body.includes('@ts-check')),
      'eslint.config.js @ts-check must still extract',
    )
  })

  it('remaining comments have no history-rationale phrases', () => {
    const hits: string[] = []
    for (const c of extracted) {
      for (const phrase of HISTORY_PHRASES) {
        if (c.body.includes(phrase)) {
          hits.push(`${c.rel}:${c.line} ${phrase}`)
        }
      }
    }
    assert.deepEqual(hits, [])
  })

  it('remaining comments have no speculation phrasing', () => {
    const hits: string[] = []
    for (const c of extracted) {
      for (const phrase of SPECULATION_PHRASES) {
        if (c.body.includes(phrase)) {
          hits.push(`${c.rel}:${c.line} ${phrase}`)
        }
      }
    }
    assert.deepEqual(hits, [])
  })

  it('remaining comments are not restatement/section stubs', () => {
    const hits: string[] = []
    for (const c of extracted) {
      const compact = c.body.replaceAll(/\s+/g, ' ').trim()
      const firstLine = c.body.split('\n')[0]!.replaceAll(/\s+/g, ' ').trim()
      for (const stub of RESTATEMENT_STUBS) {
        if (compact === stub || firstLine === stub) {
          hits.push(`${c.rel}:${c.line} ${stub}`)
        }
      }
    }
    assert.deepEqual(hits, [])
  })
})
