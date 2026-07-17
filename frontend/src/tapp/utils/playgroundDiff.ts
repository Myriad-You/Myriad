/**
 * Lightweight line-level text diff for Playground version compare.
 * Pure helpers — no React, no DOM.
 */

export type DiffOp = 'equal' | 'add' | 'remove'

export interface DiffLine {
  op: DiffOp
  text: string
  /** 1-based line number in the "before" text (removes + equals). */
  oldLine?: number
  /** 1-based line number in the "after" text (adds + equals). */
  newLine?: number
}

export interface SideBySideRow {
  left: { text: string; op: 'equal' | 'remove' | 'empty'; line?: number }
  right: { text: string; op: 'equal' | 'add' | 'empty'; line?: number }
}

function splitLines(text: string): string[] {
  if (!text) return []
  // Preserve empty trailing line semantics of split; drop a single trailing empty
  // only when the source ends with a newline (common editor display).
  const lines = text.split('\n')
  if (lines.length > 0 && lines[lines.length - 1] === '' && text.endsWith('\n')) {
    lines.pop()
  }
  return lines
}

/**
 * Myers-inspired O(ND) line diff; falls back to naive for huge inputs.
 * Returns a unified sequence of equal/add/remove ops.
 */
export function computeLineDiff(before: string, after: string): DiffLine[] {
  const a = splitLines(before)
  const b = splitLines(after)

  if (a.length === 0 && b.length === 0) return []
  if (a.length === 0) {
    return b.map((text, i) => ({ op: 'add' as const, text, newLine: i + 1 }))
  }
  if (b.length === 0) {
    return a.map((text, i) => ({ op: 'remove' as const, text, oldLine: i + 1 }))
  }

  // Cap to keep UI responsive on huge generated files.
  const MAX = 4_000
  if (a.length > MAX || b.length > MAX) {
    return naiveHeadTailDiff(a, b)
  }

  const n = a.length
  const m = b.length
  const max = n + m
  const offset = max
  const v = new Int32Array(2 * max + 1).fill(-1)
  v[offset + 1] = 0
  const trace: Int32Array[] = []

  for (let d = 0; d <= max; d++) {
    const vCopy = new Int32Array(v)
    trace.push(vCopy)
    for (let k = -d; k <= d; k += 2) {
      let x: number
      if (k === -d || (k !== d && v[offset + k - 1] < v[offset + k + 1])) {
        x = v[offset + k + 1]
      } else {
        x = v[offset + k - 1] + 1
      }
      let y = x - k
      while (x < n && y < m && a[x] === b[y]) {
        x++
        y++
      }
      v[offset + k] = x
      if (x >= n && y >= m) {
        return backtrack(trace, a, b, offset)
      }
    }
  }

  return naiveHeadTailDiff(a, b)
}

function backtrack(
  trace: Int32Array[],
  a: string[],
  b: string[],
  offset: number,
): DiffLine[] {
  const ops: Array<{ op: DiffOp; text: string }> = []
  let x = a.length
  let y = b.length

  for (let d = trace.length - 1; d >= 0; d--) {
    const v = trace[d]
    const k = x - y
    let prevK: number
    if (k === -d || (k !== d && v[offset + k - 1] < v[offset + k + 1])) {
      prevK = k + 1
    } else {
      prevK = k - 1
    }
    const prevX = v[offset + prevK]
    const prevY = prevX - prevK

    while (x > prevX && y > prevY) {
      ops.push({ op: 'equal', text: a[x - 1] })
      x--
      y--
    }
    if (d === 0) break
    if (x === prevX) {
      ops.push({ op: 'add', text: b[prevY] })
      y--
    } else {
      ops.push({ op: 'remove', text: a[prevX] })
      x--
    }
  }

  ops.reverse()

  let oldLine = 0
  let newLine = 0
  return ops.map((row) => {
    if (row.op === 'equal') {
      oldLine++
      newLine++
      return { ...row, oldLine, newLine }
    }
    if (row.op === 'add') {
      newLine++
      return { ...row, newLine }
    }
    oldLine++
    return { ...row, oldLine }
  })
}

/** Cheap fallback: shared prefix/suffix, middle as remove-then-add. */
function naiveHeadTailDiff(a: string[], b: string[]): DiffLine[] {
  let start = 0
  while (start < a.length && start < b.length && a[start] === b[start]) {
    start++
  }
  let endA = a.length - 1
  let endB = b.length - 1
  while (endA >= start && endB >= start && a[endA] === b[endB]) {
    endA--
    endB--
  }

  const result: DiffLine[] = []
  let oldLine = 0
  let newLine = 0
  for (let i = 0; i < start; i++) {
    oldLine++
    newLine++
    result.push({ op: 'equal', text: a[i], oldLine, newLine })
  }
  for (let i = start; i <= endA; i++) {
    oldLine++
    result.push({ op: 'remove', text: a[i], oldLine })
  }
  for (let i = start; i <= endB; i++) {
    newLine++
    result.push({ op: 'add', text: b[i], newLine })
  }
  for (let i = endA + 1; i < a.length; i++) {
    oldLine++
    newLine++
    result.push({ op: 'equal', text: a[i], oldLine, newLine })
  }
  return result
}

/** Pair unified ops into side-by-side rows (remove/add aligned). */
export function toSideBySide(lines: DiffLine[]): SideBySideRow[] {
  const rows: SideBySideRow[] = []
  let i = 0
  while (i < lines.length) {
    const line = lines[i]
    if (line.op === 'equal') {
      rows.push({
        left: { text: line.text, op: 'equal', line: line.oldLine },
        right: { text: line.text, op: 'equal', line: line.newLine },
      })
      i++
      continue
    }

    const removes: DiffLine[] = []
    const adds: DiffLine[] = []
    while (i < lines.length && lines[i].op === 'remove') {
      removes.push(lines[i])
      i++
    }
    while (i < lines.length && lines[i].op === 'add') {
      adds.push(lines[i])
      i++
    }
    const count = Math.max(removes.length, adds.length)
    for (let j = 0; j < count; j++) {
      const rem = removes[j]
      const add = adds[j]
      rows.push({
        left: rem
          ? { text: rem.text, op: 'remove', line: rem.oldLine }
          : { text: '', op: 'empty' },
        right: add
          ? { text: add.text, op: 'add', line: add.newLine }
          : { text: '', op: 'empty' },
      })
    }
  }
  return rows
}

export function countDiffChanges(lines: DiffLine[]): {
  added: number
  removed: number
} {
  let added = 0
  let removed = 0
  for (const line of lines) {
    if (line.op === 'add') added++
    else if (line.op === 'remove') removed++
  }
  return { added, removed }
}
