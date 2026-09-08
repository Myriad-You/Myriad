/**
 * GitHub 仓库卡共用：解析输入、打站点 /api/github/repo、语言色。
 * 阅读器嵌入卡和首页小组件走同一条出站链。
 */

import type { GithubRepoRef } from '../components/settings/githubProject'
import { parseGithubRepoUrl } from '../components/settings/githubProject'

export type { GithubRepoRef }

export const GITHUB_REPO_LIST_LIMIT = 8

export interface GithubRepoCardData {
  stars: number | null
  forks: number | null
  description: string | null
  language: string | null
}

const CACHE_TTL_MS = 5 * 60 * 1000
const cache = new Map<string, { data: GithubRepoCardData; at: number }>()
const inflight = new Map<string, Promise<GithubRepoCardData>>()

export function githubRepoPageUrl(ref: GithubRepoRef): string {
  return `https://github.com/${ref.owner}/${ref.repo}`
}

export function githubOwnerAvatarUrl(owner: string, size = 48): string {
  return `https://github.com/${owner}.png?size=${size}`
}

export function parseGithubRepoInput(raw: string): GithubRepoRef | null {
  const trimmed = raw.trim()
  if (!trimmed) return null
  const candidate =
    /github\.com/i.test(trimmed) || trimmed.includes('://')
      ? trimmed
      : `https://github.com/${trimmed.replace(/^\/+/, '')}`
  return parseGithubRepoUrl(candidate)
}

export function parseGithubRepoList(
  text: string,
  limit = GITHUB_REPO_LIST_LIMIT,
): GithubRepoRef[] {
  const seen = new Set<string>()
  const out: GithubRepoRef[] = []
  for (const chunk of text.split(/[\n,]+/)) {
    const ref = parseGithubRepoInput(chunk)
    if (!ref) continue
    const key = `${ref.owner}/${ref.repo}`.toLowerCase()
    if (seen.has(key)) continue
    seen.add(key)
    out.push(ref)
    if (out.length >= limit) break
  }
  return out
}

export function readGithubRepoCard(body: {
  stars?: unknown
  forks?: unknown
  description?: unknown
  language?: unknown
}): GithubRepoCardData {
  const asCount = (value: unknown): number | null =>
    typeof value === 'number' && Number.isFinite(value) && value >= 0
      ? Math.round(value)
      : null
  return {
    stars: asCount(body.stars),
    forks: asCount(body.forks),
    description:
      typeof body.description === 'string' && body.description.trim()
        ? body.description.trim()
        : null,
    language:
      typeof body.language === 'string' && body.language.trim()
        ? body.language.trim()
        : null,
  }
}

export function formatGithubCount(count: number): string {
  if (count >= 1_000_000) {
    return `${(count / 1_000_000).toFixed(1).replace(/\.0$/, '')}M`
  }
  if (count >= 1000) {
    return `${(count / 1000).toFixed(1).replace(/\.0$/, '')}k`
  }
  return String(count)
}

export function githubLanguageColor(language: string): string {
  const colors: Record<string, string> = {
    JavaScript: '#f1e05a',
    TypeScript: '#3178c6',
    Python: '#3572A5',
    Java: '#b07219',
    Go: '#00ADD8',
    Rust: '#dea584',
    C: '#555555',
    'C++': '#f34b7d',
    'C#': '#178600',
    PHP: '#4F5D95',
    Ruby: '#701516',
    Swift: '#F05138',
    Kotlin: '#A97BFF',
    Dart: '#00B4AB',
    Vue: '#41b883',
    HTML: '#e34c26',
    CSS: '#563d7c',
    Shell: '#89e051',
    Lua: '#000080',
  }
  return colors[language] || '#6b7280'
}

export async function fetchGithubRepoCard(
  ref: GithubRepoRef,
  fetchImpl: typeof fetch = fetch,
): Promise<GithubRepoCardData> {
  const key = `${ref.owner}/${ref.repo}`.toLowerCase()
  const now = Date.now()
  const hit = cache.get(key)
  if (hit && now - hit.at < CACHE_TTL_MS) return hit.data
  const pending = inflight.get(key)
  if (pending) return pending

  const request = (async () => {
    const params = new URLSearchParams({ owner: ref.owner, repo: ref.repo })
    const response = await fetchImpl(`/api/github/repo?${params.toString()}`, {
      credentials: 'include',
    })
    if (!response.ok) {
      throw new Error(`github repo ${key}: HTTP ${response.status}`)
    }
    const data = readGithubRepoCard(
      (await response.json()) as {
        stars?: unknown
        forks?: unknown
        description?: unknown
        language?: unknown
      },
    )
    cache.set(key, { data, at: Date.now() })
    return data
  })()

  inflight.set(key, request)
  try {
    return await request
  } finally {
    inflight.delete(key)
  }
}
