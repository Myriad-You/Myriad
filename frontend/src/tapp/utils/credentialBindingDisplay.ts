/** Compact display helpers for write-only credential bindings. */

export interface CredentialBindingLike {
  api: string
  method: string
  endpoint: string
  access: string
}

export interface CredentialBindingDisplayRow {
  api: string
  method: string
  path: string
  endpoint: string
  access: string
}

export interface CredentialBindingSummaryModel {
  count: number
  methods: string[]
  accesses: string[]
  rows: CredentialBindingDisplayRow[]
}

/** Drop query/hash so settings copy stays on the path, not field lists. */
export function compactCredentialEndpoint(endpoint: string): string {
  const trimmed = endpoint.trim()
  if (!trimmed) return ''
  const withoutHash = trimmed.split('#', 1)[0] ?? trimmed
  const withoutQuery = withoutHash.split('?', 1)[0] ?? withoutHash
  try {
    const url = new URL(withoutQuery)
    if (withoutQuery.startsWith(url.origin)) {
      return withoutQuery.slice(url.origin.length) || '/'
    }
    return url.pathname || '/'
  } catch {
    return withoutQuery
  }
}

export function uniqueNonEmpty(values: Iterable<string>): string[] {
  const seen = new Set<string>()
  const out: string[] = []
  for (const value of values) {
    if (!value || seen.has(value)) continue
    seen.add(value)
    out.push(value)
  }
  return out
}

export function summarizeCredentialBindings(
  bindings: CredentialBindingLike[],
): CredentialBindingSummaryModel {
  const rows = bindings.map((binding) => ({
    api: binding.api,
    method: binding.method,
    path: compactCredentialEndpoint(binding.endpoint),
    endpoint: binding.endpoint,
    access: binding.access,
  }))
  return {
    count: rows.length,
    methods: uniqueNonEmpty(rows.map((row) => row.method)),
    accesses: uniqueNonEmpty(rows.map((row) => row.access)),
    rows,
  }
}
