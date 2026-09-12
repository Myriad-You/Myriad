import spec from '../../../shared/error_codes.json' with { type: 'json' }

interface CodeSpec {
  aliases: Record<string, string>
  labels: Record<string, string>
  leftovers: Record<string, string>
  prefixes: Record<string, string>
}

const CODES = spec as CodeSpec

const EXACT = new Map<string, string>()
for (const [label, code] of Object.entries(CODES.labels)) {
  EXACT.set(label, code)
  EXACT.set(label.toLowerCase(), code)
}
for (const [label, code] of Object.entries(CODES.leftovers)) {
  EXACT.set(label, code)
}

const PREFIXES = Object.entries(CODES.prefixes).toSorted(
  (a, b) => b[0].length - a[0].length,
)

/** Map a public label or leftover string onto a stable machine code. */
export function inferErrorCode(raw: string): string | undefined {
  const text = raw.trim()
  if (!text) return undefined
  const exact = EXACT.get(text) ?? EXACT.get(text.toLowerCase())
  if (exact) return exact
  const lower = text.toLowerCase()
  for (const [prefix, code] of PREFIXES) {
    if (text.startsWith(prefix) || lower.startsWith(prefix.toLowerCase())) {
      return code
    }
  }
  return undefined
}

/** Prefer an explicit API code; otherwise infer from leftover text. */
export function resolveErrorCode(
  explicit: string | undefined,
  raw: string,
): string | undefined {
  const aliased =
    explicit && explicit !== 'unmapped'
      ? (CODES.aliases[explicit] ?? explicit)
      : undefined
  if (aliased) return aliased
  return inferErrorCode(raw) ?? (explicit === 'unmapped' ? 'unmapped' : undefined)
}
