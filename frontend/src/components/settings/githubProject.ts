/**
 * 设置里第三方 GitHub 仓库：识别 URL、star 缓存、数字缩写。
 *
 * GitHub 产品页（settings / login / 光组织页）不是仓库。
 * 仓库摘要走站点数据平台同一条出站链（GitHubApiUrl / 代理 / token），
 * 浏览器只打 `/api/github/repo`；star 数字在本机再缓存 7 天。
 */

export const GITHUB_STAR_TTL_MS = 7 * 24 * 60 * 60 * 1000
export const GITHUB_STAR_CACHE_PREFIX = 'myriad:github-stars:v1:'

/** GitHub.com 上不是「用户/仓库」的第一段。 */
const GITHUB_SYSTEM_ROOTS = new Set([
  'about',
  'account',
  'apps',
  'auth',
  'blog',
  'codespaces',
  'collections',
  'contact',
  'copilot',
  'customer-stories',
  'dashboard',
  'enterprises',
  'events',
  'explore',
  'features',
  'issues',
  'login',
  'marketplace',
  'new',
  'notifications',
  'organizations',
  'orgs',
  'pricing',
  'pulls',
  'search',
  'security',
  'sessions',
  'settings',
  'signup',
  'site',
  'sponsors',
  'stars',
  'topics',
  'trending',
  'users',
  'watching',
])

const OWNER_REPO_RE = /^[A-Za-z0-9](?:[A-Za-z0-9._-]*[A-Za-z0-9])?$/

export interface GithubRepoRef {
  owner: string
  repo: string
}

const memoryStars = new Map<string, { count: number; fetchedAt: number }>()
const inflightStars = new Map<string, Promise<number | null>>()

export function githubStarCacheKey(owner: string, repo: string): string {
  return `${GITHUB_STAR_CACHE_PREFIX}${owner}/${repo}`.toLowerCase()
}

export function parseGithubRepoUrl(url: string): GithubRepoRef | null {
  const trimmed = url.trim()
  if (!trimmed) return null
  let parsed: URL
  try {
    parsed = new URL(trimmed.includes('://') ? trimmed : `https://${trimmed}`)
  } catch {
    return null
  }
  const host = parsed.hostname.replace(/^www\./i, '').toLowerCase()
  if (host !== 'github.com') return null
  const parts = parsed.pathname.split('/').filter(Boolean)
  if (parts.length < 2) return null
  const owner = parts[0]
  const repo = parts[1].replace(/\.git$/i, '')
  if (GITHUB_SYSTEM_ROOTS.has(owner.toLowerCase())) return null
  if (!OWNER_REPO_RE.test(owner) || !OWNER_REPO_RE.test(repo)) return null
  return { owner, repo }
}

export function isGithubRepoUrl(url: string | null | undefined): boolean {
  return typeof url === 'string' && parseGithubRepoUrl(url) !== null
}

export function githubRepoUrl(ref: GithubRepoRef): string {
  return `https://github.com/${ref.owner}/${ref.repo}`
}

/** GitHub 主站同款缩写：1.2k / 10.5k / 1.2m。 */
export function formatStarCount(count: number): string {
  if (!Number.isFinite(count) || count < 0) return '0'
  const n = Math.round(count)
  if (n < 1000) return String(n)
  if (n < 1_000_000) {
    return `${trimDecimal(n / 1000)}k`
  }
  return `${trimDecimal(n / 1_000_000)}m`
}

function trimDecimal(value: number): string {
  return value.toFixed(1).replace(/\.0$/, '')
}

function storageOf(storage?: Storage | null): Storage | null {
  if (storage !== undefined) return storage
  try {
    return globalThis.localStorage ?? null
  } catch {
    return null
  }
}

export function readStarCache(
  owner: string,
  repo: string,
  now = Date.now(),
  storage?: Storage | null,
): number | null {
  const key = githubStarCacheKey(owner, repo)
  const mem = memoryStars.get(key)
  if (mem && now - mem.fetchedAt < GITHUB_STAR_TTL_MS) return mem.count

  const store = storageOf(storage)
  if (!store) return null
  try {
    const raw = store.getItem(key)
    if (!raw) return null
    const parsed = JSON.parse(raw) as { count?: unknown; fetchedAt?: unknown }
    if (
      typeof parsed.count !== 'number' ||
      !Number.isFinite(parsed.count) ||
      parsed.count < 0 ||
      typeof parsed.fetchedAt !== 'number'
    ) {
      store.removeItem(key)
      return null
    }
    if (now - parsed.fetchedAt >= GITHUB_STAR_TTL_MS) {
      store.removeItem(key)
      memoryStars.delete(key)
      return null
    }
    memoryStars.set(key, { count: parsed.count, fetchedAt: parsed.fetchedAt })
    return parsed.count
  } catch {
    return null
  }
}

export function writeStarCache(
  owner: string,
  repo: string,
  count: number,
  fetchedAt = Date.now(),
  storage?: Storage | null,
): void {
  const key = githubStarCacheKey(owner, repo)
  const entry = { count, fetchedAt }
  memoryStars.set(key, entry)
  const store = storageOf(storage)
  if (!store) return
  try {
    store.setItem(key, JSON.stringify(entry))
  } catch {
    // quota / private mode
  }
}

export async function fetchGithubStarCount(
  owner: string,
  repo: string,
  options: {
    now?: number
    storage?: Storage | null
    fetchImpl?: typeof fetch
  } = {},
): Promise<number | null> {
  const now = options.now ?? Date.now()
  const cached = readStarCache(owner, repo, now, options.storage)
  if (cached != null) return cached

  const key = githubStarCacheKey(owner, repo)
  const pending = inflightStars.get(key)
  if (pending) return pending

  const fetchImpl = options.fetchImpl ?? globalThis.fetch
  if (typeof fetchImpl !== 'function') return null

  const request = (async () => {
    try {
      const params = new URLSearchParams({ owner, repo })
      const path = `/api/github/repo?${params.toString()}`
      const response = await fetchImpl(path, { credentials: 'include' })
      if (!response.ok) return null
      const body = (await response.json()) as { stars?: unknown }
      if (
        typeof body.stars !== 'number' ||
        !Number.isFinite(body.stars) ||
        body.stars < 0
      ) {
        return null
      }
      const count = Math.round(body.stars)
      writeStarCache(owner, repo, count, now, options.storage)
      return count
    } catch {
      return null
    } finally {
      inflightStars.delete(key)
    }
  })()

  inflightStars.set(key, request)
  return request
}

export function resetGithubStarMemoryForTests(): void {
  memoryStars.clear()
  inflightStars.clear()
}
