import { currentCopy } from '../i18n/localeCopy'
import {
  fetchGithubRepoCard,
  formatGithubCount,
  githubLanguageColor,
} from './githubRepo'
import { getNeteaseAudioUrlImmediate } from './musicPlayer'
import { proxyImageUrlOr } from './proxyImageUrl'
import { isTrustedIframeHost } from './rssContentProcessor'

const CACHE_TTL = 5 * 60 * 1000 // 5 min

interface CacheEntry<T> {
  data: T
  timestamp: number
}

const embedDataCache = new Map<string, CacheEntry<any>>()

function getCached<T>(key: string): T | null {
  const entry = embedDataCache.get(key)
  if (!entry) return null

  if (Date.now() - entry.timestamp > CACHE_TTL) {
    embedDataCache.delete(key)
    return null
  }

  return entry.data
}

function setCache<T>(key: string, data: T): void {
  embedDataCache.set(key, { data, timestamp: Date.now() })

  // Cap 100; drop oldest.
  if (embedDataCache.size > 100) {
    const entries = Iterator.from(embedDataCache.entries())
      .toArray()
      .toSorted((a, b) => a[1].timestamp - b[1].timestamp)
    for (let i = 0; i < 20; i++) {
      embedDataCache.delete(entries[i][0])
    }
  }
}

export type EmbedType =
  'netease-music' | 'steam-game' | 'bilibili-video' | 'github-repo'

function extractNeteaseSongId(iframeSrc: string): string | null {
  const match = iframeSrc.match(
    /music\.163\.com\/outchain\/player\?.*?id=(\d+)/,
  )
  return match ? match[1] : null
}

function extractNeteaseSongIdFromUrl(url: string): string | null {
  const queryMatch = url.match(
    /music\.163\.com\/(?:#\/)?(?:m\/)?song\?.*?id=(\d+)/,
  )
  if (queryMatch) return queryMatch[1]

  const pathMatch = url.match(/music\.163\.com\/(?:#\/)?(?:m\/)?song\/(\d+)/)
  if (pathMatch) return pathMatch[1]

  return null
}

function extractSteamAppId(url: string): string | null {
  const storeMatch = url.match(/store\.steampowered\.com\/app\/(\d+)/)
  if (storeMatch) return storeMatch[1]

  const steamMatch = url.match(/steam:\/\/store\/(\d+)/)
  if (steamMatch) return steamMatch[1]

  return null
}

function extractBilibiliVideoId(
  url: string,
): { type: 'bv' | 'av'; id: string } | null {
  const cleanUrl = url.replaceAll(/amp;/gi, '&')

  const bvMatch = cleanUrl.match(/(?:video\/|bvid=|[?&]bvid=)(BV[a-z0-9]+)/i)
  if (bvMatch) return { type: 'bv', id: bvMatch[1] }

  const avMatch = cleanUrl.match(/(?:video\/av|aid=|[?&]aid=)(\d+)/i)
  if (avMatch) return { type: 'av', id: avMatch[1] }

  const pureBvMatch = cleanUrl.match(/\b(BV[a-z0-9]{10,12})\b/i)
  if (pureBvMatch) return { type: 'bv', id: pureBvMatch[1] }

  const pureAvMatch = cleanUrl.match(/\bav(\d+)\b/i)
  if (pureAvMatch) return { type: 'av', id: pureAvMatch[1] }

  return null
}

function extractGithubRepo(
  url: string,
): { owner: string; repo: string } | null {
  const match = url.match(/github\.com\/([^/]+)\/([^/?#]+)/)
  if (match) return { owner: match[1], repo: match[2] }
  return null
}

function generateNeteaseMusicCard(songId: string): string {
  return `
    <div class="brew-embed-card brew-netease-music brew-embed-exempt not-prose block group cursor-pointer"
         data-embed-type="netease-music"
         data-song-id="${songId}"
         data-embed-exempt="true"
         style="width: 180px; height: 180px;">
      <div class="relative bg-white dark:bg-gray-800 rounded-xl shadow-md hover:shadow-2xl transition-all duration-300 transform hover:-translate-y-1 hover:scale-[1.02] overflow-hidden h-full">
        <div class="block w-full h-full relative">
          <!-- 封面容器 - 可被动态更新 -->
          <div class="brew-embed-cover w-full h-full bg-linear-to-br from-red-400 to-red-600 flex items-center justify-center">
            <svg class="w-16 h-16 text-white/80 brew-embed-placeholder" fill="currentColor" viewBox="0 0 24 24">
              <path d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm0 14.5c-2.49 0-4.5-2.01-4.5-4.5S9.51 7.5 12 7.5s4.5 2.01 4.5 4.5-2.01 4.5-4.5 4.5zm0-5.5c-.55 0-1 .45-1 1s.45 1 1 1 1-.45 1-1-.45-1-1-1z"/>
            </svg>
          </div>

          <!-- 悬停信息遮罩 - 与资料库一致 -->
          <div class="absolute inset-0 bg-linear-to-t from-black/95 via-black/60 to-transparent opacity-0 group-hover:opacity-100 transition-all duration-300 flex flex-col justify-end p-3 pointer-events-none">
            <div>
              <div class="flex items-start gap-1">
                <h3 class="brew-embed-title font-bold text-white text-xs leading-tight line-clamp-2 mb-1 flex-1">
                  ${currentCopy().common.loading}
                </h3>
              </div>
              <p class="brew-embed-artist text-[10px] text-white/75 line-clamp-1">
                ID: ${songId}
              </p>
            </div>
          </div>
        </div>
      </div>
    </div>
  `
}

function generateSteamGameCard(appId: string): string {
  const storeUrl = `https://store.steampowered.com/app/${appId}`
  const headerImg = `https://cdn.cloudflare.steamstatic.com/steam/apps/${appId}/header.jpg`

  return `
    <div class="brew-embed-card brew-steam-game brew-embed-exempt not-prose block group"
         data-embed-type="steam-game"
         data-app-id="${appId}"
         data-embed-exempt="true"
         style="max-width: 28rem;">
      <div class="bg-white dark:bg-gray-800 rounded-2xl shadow-lg hover:shadow-2xl transition-all duration-300 transform hover:-translate-y-1 overflow-hidden">
        <div class="relative overflow-hidden bg-linear-to-br from-gray-900 to-gray-800">
          <a href="${storeUrl}"
             target="_blank"
             rel="noopener noreferrer"
             class="block w-full relative no-underline">
            <!-- 游戏封面图 -->
            <img src="${headerImg}"
                 alt="Steam Game"
                 class="brew-embed-cover w-full block object-cover aspect-460/215 transition-all duration-500 group-hover:scale-110"
                 loading="lazy"/>
          </a>

          <!-- 悬停信息遮罩 -->
          <div class="absolute inset-0 bg-linear-to-t from-black/90 via-black/40 to-transparent opacity-0 group-hover:opacity-100 transition-opacity duration-300 flex flex-col justify-end p-4 pointer-events-none">
            <h3 class="brew-embed-title font-bold text-white text-base line-clamp-2 leading-snug mb-1">
              App ID: ${appId}
            </h3>
            <p class="brew-embed-desc text-sm text-white/80 line-clamp-1">
              ${currentCopy().brew.embedSteamStore}
            </p>
          </div>
        </div>
      </div>
    </div>
  `
}

function generateBilibiliIframe(videoId: {
  type: 'bv' | 'av'
  id: string
}): string {
  const playerUrl =
    videoId.type === 'bv'
      ? `//player.bilibili.com/player.html?bvid=${videoId.id}&autoplay=0`
      : `//player.bilibili.com/player.html?aid=${videoId.id}&autoplay=0`

  return `
    <div class="brew-embed-card brew-bilibili-embed brew-embed-exempt not-prose block"
         data-video-id="${videoId.id}"
         data-video-type="${videoId.type}"
         data-embed-exempt="true">
      <div class="aspect-video w-full rounded-xl overflow-hidden shadow-lg">
        <iframe
          src="${playerUrl}"
          class="w-full h-full border-0"
          scrolling="no"
          frameborder="0"
          allowfullscreen="true"
          loading="lazy"
          referrerpolicy="no-referrer">
        </iframe>
      </div>
    </div>
  `
}

function generateGithubRepoCard(repo: { owner: string; repo: string }): string {
  const repoUrl = `https://github.com/${repo.owner}/${repo.repo}`
  const ownerAvatar = `https://github.com/${repo.owner}.png?size=48`

  return `
    <div class="brew-embed-card brew-github-repo brew-embed-exempt not-prose block"
         data-embed-type="github-repo"
         data-owner="${repo.owner}"
         data-repo="${repo.repo}"
         data-embed-exempt="true">
      <a href="${repoUrl}"
         target="_blank"
         rel="noopener noreferrer"
         class="brew-github-card">
        <div class="brew-github-card-header">
          <span class="brew-github-card-avatar" aria-hidden="true">
            <img src="${ownerAvatar}" alt="" loading="lazy" />
          </span>
          <span class="brew-github-card-text">
            <span class="brew-github-card-title brew-embed-title">${repo.repo}</span>
            <span class="brew-github-card-owner brew-embed-owner">${repo.owner}</span>
          </span>
          <span class="brew-github-card-stars brew-embed-stars">
            <svg fill="currentColor" viewBox="0 0 16 16" aria-hidden="true">
              <path d="M8 .25a.75.75 0 0 1 .673.418l1.882 3.815 4.21.612a.75.75 0 0 1 .416 1.279l-3.046 2.97.719 4.192a.75.75 0 0 1-1.088.791L8 12.347l-3.766 1.98a.75.75 0 0 1-1.088-.79l.72-4.194L.818 6.374a.75.75 0 0 1 .416-1.28l4.21-.611L7.327.668A.75.75 0 0 1 8 .25z"/>
            </svg>
            <span>-</span>
          </span>
        </div>
        <p class="brew-github-card-desc brew-embed-desc"></p>
        <div class="brew-github-card-meta">
          <span class="brew-embed-lang">
            <span class="brew-embed-lang-dot"></span>
            <span>-</span>
          </span>
          <span class="brew-embed-forks">
            <svg fill="currentColor" viewBox="0 0 16 16" aria-hidden="true">
              <path d="M5 5.372v.878c0 .414.336.75.75.75h4.5a.75.75 0 0 0 .75-.75v-.878a2.25 2.25 0 1 1 1.5 0v.878a2.25 2.25 0 0 1-2.25 2.25h-1.5v2.128a2.251 2.251 0 1 1-1.5 0V8.5h-1.5A2.25 2.25 0 0 1 3.5 6.25v-.878a2.25 2.25 0 1 1 1.5 0ZM5 3.25a.75.75 0 1 0-1.5 0 .75.75 0 0 0 1.5 0Zm6.75.75a.75.75 0 1 0 0-1.5.75.75 0 0 0 0 1.5Zm-3 8.75a.75.75 0 1 0-1.5 0 .75.75 0 0 0 1.5 0Z"/>
            </svg>
            <span>-</span>
          </span>
        </div>
        <span class="brew-github-card-mark" aria-hidden="true">
          <svg viewBox="0 0 24 24">
            <path fill="currentColor" d="M12 0c-6.626 0-12 5.373-12 12 0 5.302 3.438 9.8 8.207 11.387.599.111.793-.261.793-.577v-2.234c-3.338.726-4.033-1.416-4.033-1.416-.546-1.387-1.333-1.756-1.333-1.756-1.089-.745.083-.729.083-.729 1.205.084 1.839 1.237 1.839 1.237 1.07 1.834 2.807 1.304 3.492.997.107-.775.418-1.305.762-1.604-2.665-.305-5.467-1.334-5.467-5.931 0-1.311.469-2.381 1.236-3.221-.124-.303-.535-1.524.117-3.176 0 0 1.008-.322 3.301 1.23.957-.266 1.983-.399 3.003-.404 1.02.005 2.047.138 3.006.404 2.291-1.552 3.297-1.23 3.297-1.23.653 1.653.242 2.874.118 3.176.77.84 1.235 1.911 1.235 3.221 0 4.609-2.807 5.624-5.479 5.921.43.372.823 1.102.823 2.222v3.293c0 .319.192.694.801.576 4.765-1.589 8.199-6.086 8.199-11.386 0-6.627-5.373-12-12-12z"/>
          </svg>
        </span>
      </a>
    </div>
  `
}

function generateGithubRepoChip(repo: { owner: string; repo: string }): string {
  const repoUrl = `https://github.com/${repo.owner}/${repo.repo}`
  const ownerAvatar = `https://github.com/${repo.owner}.png?size=48`
  return `<a class="github-project-badge brew-github-chip brew-embed-exempt"
       href="${repoUrl}"
       target="_blank"
       rel="noopener noreferrer"
       data-embed-type="github-repo"
       data-owner="${repo.owner}"
       data-repo="${repo.repo}"
       data-embed-exempt="true"
       title="${repo.repo} · GitHub">
    <span class="github-project-badge-mark brew-github-chip-avatar" aria-hidden="true">
      <img src="${ownerAvatar}" alt="" loading="lazy" />
    </span>
    <span class="github-project-badge-name">${repo.repo}</span>
  </a>`
}

export function processEmbeds(content: string): string {
  let result = content

  const neteaseIframeRegex =
    /<iframe[^>]*src=["']([^"']*music\.163\.com\/outchain\/player[^"']*)["'][^>]*>[\s\S]*?<\/iframe>/gi
  result = result.replace(neteaseIframeRegex, (match, src) => {
    const songId = extractNeteaseSongId(src)
    if (songId) {
      return generateNeteaseMusicCard(songId)
    }
    return match // 无法解析则保留原样
  })

  // 1.1 处理网易云音乐链接（<a> 标签形式）
  // 匹配: <a href="https:
  const neteaseLinkRegex =
    /<a[^>]*href=["'](https?:\/\/(?:y\.)?music\.163\.com\/(?:#\/)?(?:m\/)?song(?:\?[^"']*id=\d|\/\d)[^"']*)["'][^>]*>[\s\S]*?<\/a>/gi
  result = result.replace(neteaseLinkRegex, (match, url) => {
    const songId = extractNeteaseSongIdFromUrl(url)
    if (songId) {
      return generateNeteaseMusicCard(songId)
    }
    return match
  })

  // 2. 处理 Steam 链接（不在已有链接标签内的纯 URL）
  // 只处理独立的链接，避免重复处理
  const steamLinkRegex =
    /<a[^>]*href=["'](https?:\/\/store\.steampowered\.com\/app\/\d[^"']*)["'][^>]*>[\s\S]*?<\/a>/gi
  result = result.replace(steamLinkRegex, (match, url) => {
    const appId = extractSteamAppId(url)
    if (appId) {
      return generateSteamGameCard(appId)
    }
    return match
  })

  // 3. Bilibili 处理：不再处理官方 iframe，只把 AV/BV 号和链接转为官方 iframe

  // 3.1 处理 Bilibili 视频链接 -> 转为官方 iframe
  // www / m / b23.tv 短链（路径里带 BV/av 时）
  const bilibiliLinkRegex =
    /<a[^>]*href=["'](https?:\/\/(?:(?:www|m)\.)?bilibili\.com\/video\/(?:BV[a-z0-9]|av\d)[^"']*|https?:\/\/b23\.tv\/[^"']+)["'][^>]*>[\s\S]*?<\/a>/gi
  result = result.replace(bilibiliLinkRegex, (match, url) => {
    const videoId = extractBilibiliVideoId(url)
    if (videoId) {
      return generateBilibiliIframe(videoId)
    }
    return match
  })

  // 3.1b 纯文本 URL（非 <a>）：m.bilibili / www / b23.tv
  const bilibiliBareUrlRegex =
    /(?<!["'=])(https?:\/\/(?:(?:www|m)\.)?bilibili\.com\/video\/(?:BV[a-z0-9]|av\d)[^\s<]*|https?:\/\/b23\.tv\/[A-Z0-9]+)/gi
  result = result.replace(bilibiliBareUrlRegex, (url) => {
    const videoId = extractBilibiliVideoId(url)
    if (videoId) {
      return generateBilibiliIframe(videoId)
    }
    return url
  })

  const bilibiliPlainTextRegex =
    /(?<!<[^>]*|href=["'][^"']*|>)\b(BV[a-z0-9]{10,12}|av\d{1,12})\b(?![^<]*<\/a>)/gi
  result = result.replace(bilibiliPlainTextRegex, (match) => {
    const videoId = extractBilibiliVideoId(match)
    if (videoId) {
      return generateBilibiliIframe(videoId)
    }
    return match
  })

  // 4. 处理 GitHub 仓库链接（仅处理指向仓库首页的链接）
  // 同一仓库第一次出完整卡，后文再用设置页小标签。
  const seenGithubRepos = new Set<string>()
  const githubLinkRegex =
    /<a[^>]*href=["'](https?:\/\/github\.com\/[^/]+\/[^/?#"']+)["'][^>]*>[\s\S]*?<\/a>/gi
  result = result.replace(githubLinkRegex, (match, url) => {
    const cleanUrl = url.split('?')[0].split('#')[0]
    const parts = cleanUrl.replaceAll(/^https?:\/\/github\.com\//g, '').split('/')
    if (parts.length === 2 && parts[0] && parts[1]) {
      const repo = extractGithubRepo(url)
      if (repo) {
        const key = `${repo.owner}/${repo.repo}`.toLowerCase()
        if (seenGithubRepos.has(key)) {
          return generateGithubRepoChip(repo)
        }
        seenGithubRepos.add(key)
        return generateGithubRepoCard(repo)
      }
    }
    return match
  })

  // Strip iframes not on the shared host allowlist.
  result = result.replaceAll(/<iframe\b[\s\S]*?<\/iframe>/gi, (match) => {
    const srcMatch =
      match.match(/\bsrc\s*=\s*(["'])([^"']*)\1/i) ??
      match.match(/\bsrc\s*=\s*([^\s>]+)/i)
    if (!srcMatch) return ''
    const rawSrc = (srcMatch[2] || srcMatch[1] || '').trim()
    try {
      const href = rawSrc.startsWith('//') ? `https:${rawSrc}` : rawSrc
      const host = new URL(href, 'https://example.invalid').hostname
      return isTrustedIframeHost(host) ? match : ''
    } catch {
      return ''
    }
  })

  return result
}

export async function loadEmbedData(container: HTMLElement): Promise<void> {
  await Promise.all([
    loadNeteaseMusicData(container),
    loadSteamGameData(container),
    loadGithubRepoData(container),
  ])
}

async function loadNeteaseMusicData(container: HTMLElement): Promise<void> {
  const unloadedCards = Iterator.from(
    container.querySelectorAll('.brew-netease-music[data-song-id]'),
  )
    .filter(
      (card) =>
        card.getAttribute('data-song-id') &&
        card.getAttribute('data-loaded') !== 'true',
    )
    .toArray()

  if (unloadedCards.length === 0) return

  await Promise.all(
    unloadedCards.map(async (card) => {
      const songId = card.getAttribute('data-song-id')
      if (!songId) return

      card.setAttribute('data-loaded', 'loading')

      try {
        const response = await fetch(`/api/proxy/music/netease/song/${songId}`)
        if (!response.ok) {
          card.setAttribute('data-loaded', 'true')
          return
        }

        const songData = await response.json()
        if (!songData || !songData.name) {
          card.setAttribute('data-loaded', 'true')
          return
        }

        const coverContainer = card.querySelector('.brew-embed-cover')
        if (coverContainer && songData.album?.picUrl) {
          const coverUrl = songData.album.picUrl || songData.al?.picUrl
          if (coverUrl) {
            // DOM APIs only; no innerHTML.
            const img = document.createElement('img')
            img.src = coverUrl
            img.alt = songData.name || ''
            img.className =
              'w-full h-full object-cover transition-all duration-500 group-hover:scale-110'
            img.loading = 'lazy'
            coverContainer.innerHTML = ''
            coverContainer.appendChild(img)
          }
        }

        const titleEl = card.querySelector('.brew-embed-title')
        if (titleEl) {
          titleEl.textContent = songData.name
        }

        const artistEl = card.querySelector('.brew-embed-artist')
        if (artistEl) {
          const artists = songData.artists || songData.ar || []
          const artistText =
            artists.map((a: any) => a.name).join(', ') ||
            currentCopy().library.unknownArtist
          artistEl.textContent = artistText
        }

        if (songData.isVip || songData.fee === 1 || songData.fee === 4) {
          const titleContainer =
            card.querySelector('.brew-embed-title')?.parentElement
          if (titleContainer && !titleContainer.querySelector('.vip-badge')) {
            const vipBadge = document.createElement('span')
            vipBadge.className =
              'vip-badge inline-flex items-center px-1.5 py-0.5 rounded-md bg-linear-to-r from-yellow-500 to-amber-600 text-[10px] font-semibold text-white shadow-md select-none ml-1'
            vipBadge.textContent = 'VIP'
            titleContainer.appendChild(vipBadge)
          }
        }

        card.setAttribute('data-loaded', 'true')
      } catch (error) {
        console.warn(`[embedProcessor] 加载网易云音乐 ${songId} 失败:`, error)
        card.setAttribute('data-loaded', 'true')
      }
    }),
  )
}

/** Proxy (CORS). */
async function loadSteamGameData(container: HTMLElement): Promise<void> {
  const unloadedCards = Iterator.from(
    container.querySelectorAll('.brew-steam-game[data-app-id]'),
  )
    .filter(
      (card) =>
        card.getAttribute('data-app-id') &&
        card.getAttribute('data-loaded') !== 'true',
    )
    .toArray()

  if (unloadedCards.length === 0) return

  await Promise.all(
    unloadedCards.map(async (card) => {
      const appId = card.getAttribute('data-app-id')
      if (!appId) return

      card.setAttribute('data-loaded', 'loading')

      try {
        // Cache key includes locale.
        let steamLang = 'english'
        try {
          const locale =
            localStorage.getItem('locale') ||
            (typeof navigator !== 'undefined' ? navigator.language : '') ||
            'en'
          steamLang = locale
        } catch {
          /* ignore */
        }
        const cacheKey = `steam:${appId}:${steamLang}`
        let gameData = getCached<any>(cacheKey)

        if (!gameData) {
          const response = await fetch(
            `/api/steam/game/${appId}?lang=${encodeURIComponent(steamLang)}`,
            {
              headers: {
                'Accept-Language': steamLang,
              },
            },
          )
          if (!response.ok) {
            card.setAttribute('data-loaded', 'true')
            return
          }

          const result = await response.json()
          if (result.success && result.data) {
            gameData = result.data
            setCache(cacheKey, gameData)
          }
        }

        if (gameData) {
          const titleEl = card.querySelector('.brew-embed-title')
          if (titleEl && gameData.name) {
            titleEl.textContent = gameData.name
          }

          const descEl = card.querySelector('.brew-embed-desc')
          if (descEl && gameData.short_description) {
            // DOMParser text; no innerHTML.
            try {
              const parser = new DOMParser()
              const doc = parser.parseFromString(
                gameData.short_description,
                'text/html',
              )
              const plainText = doc.body.textContent || ''
              descEl.textContent = plainText
            } catch {
              descEl.textContent = gameData.short_description.replaceAll(
                /<[^>]*>/g,
                '',
              )
            }
          }
        }

        card.setAttribute('data-loaded', 'true')
      } catch (error) {
        console.warn(`[embedProcessor] 加载 Steam 游戏 ${appId} 失败:`, error)
        card.setAttribute('data-loaded', 'true')
      }
    }),
  )
}

async function loadGithubRepoData(container: HTMLElement): Promise<void> {
  const unloadedCards = Iterator.from(
    container.querySelectorAll('.brew-github-repo[data-owner][data-repo]'),
  )
    .filter((card) => {
      const owner = card.getAttribute('data-owner')
      const repo = card.getAttribute('data-repo')
      return owner && repo && card.getAttribute('data-loaded') !== 'true'
    })
    .toArray()

  if (unloadedCards.length === 0) return

  await Promise.all(
    unloadedCards.map(async (card) => {
      const owner = card.getAttribute('data-owner')
      const repo = card.getAttribute('data-repo')
      if (!owner || !repo) return

      card.setAttribute('data-loaded', 'loading')

      try {
        const repoData = await fetchGithubRepoCard({ owner, repo })

        const descEl = card.querySelector('.brew-embed-desc')
        if (descEl) {
          const text = repoData.description
          descEl.textContent = text || ''
        }

        const starsWrap = card.querySelector(
          '.brew-embed-stars',
        ) as HTMLElement | null
        const starsEl = starsWrap?.querySelector('span')
        if (starsWrap && repoData.stars != null) {
          if (starsEl) starsEl.textContent = formatGithubCount(repoData.stars)
          starsWrap.hidden = false
        } else if (starsWrap) {
          starsWrap.hidden = true
        }

        const forksWrap = card.querySelector(
          '.brew-embed-forks',
        ) as HTMLElement | null
        const forksEl = forksWrap?.querySelector('span:last-child')
        if (forksWrap && repoData.forks != null) {
          if (forksEl) forksEl.textContent = formatGithubCount(repoData.forks)
        } else if (forksWrap) {
          forksWrap.hidden = true
        }

        const langEl = card.querySelector(
          '.brew-embed-lang',
        ) as HTMLElement | null
        if (langEl && repoData.language) {
          const langColor = githubLanguageColor(repoData.language)
          const colorDot = langEl.querySelector(
            '.brew-embed-lang-dot',
          ) as HTMLElement
          const langText = langEl.querySelector('span:last-child')
          if (colorDot) {
            colorDot.style.backgroundColor = langColor
          }
          if (langText) {
            langText.textContent = repoData.language
          }
        } else if (langEl) {
          langEl.hidden = true
        }

        const meta = card.querySelector(
          '.brew-github-card-meta',
        ) as HTMLElement | null
        if (meta) {
          const visible = Iterator.from(meta.children).some(
            (el) => el instanceof HTMLElement && !el.hidden,
          )
          meta.hidden = !visible
        }

        card.setAttribute('data-loaded', 'true')
      } catch (error) {
        console.warn(
          `[embedProcessor] 加载 GitHub 仓库 ${owner}/${repo} 失败:`,
          error,
        )

        const descEl = card.querySelector('.brew-embed-desc')
        if (descEl) {
          descEl.textContent = currentCopy().brew.repoLoadFailed
        }
        const stars = card.querySelector(
          '.brew-embed-stars',
        ) as HTMLElement | null
        const meta = card.querySelector(
          '.brew-github-card-meta',
        ) as HTMLElement | null
        if (stars) stars.hidden = true
        if (meta) meta.hidden = true
        card.setAttribute('data-loaded', 'true')
      }
    }),
  )
}

export async function playNeteaseSong(songId: string): Promise<void> {
  try {
    const fallbackCover =
      'https://p1.music.126.net/UeTuwE7pvjBpypWLudqukA==/3132508627578625.jpg'

    // Do not await detail/geo on click.
    const song = {
      id: songId,
      name: `${currentCopy().widgets.reportNetease} #${songId}`,
      artist: currentCopy().library.unknownArtist,
      album: currentCopy().library.unknownAlbum,
      cover: proxyImageUrlOr(fallbackCover, fallbackCover),
      url: getNeteaseAudioUrlImmediate(songId),
      duration: 0,
      source: 'netease' as const,
      isVip: false,
    }

    window.dispatchEvent(new CustomEvent('open-control-panel'))
    window.dispatchEvent(
      new CustomEvent('play-song', {
        detail: { song },
      }),
    )

    try {
      const detailResponse = await fetch(
        `/api/proxy/music/netease/song/${songId}`,
      )
      if (!detailResponse.ok) return
      const songData = await detailResponse.json()
      const rawCover =
        songData?.album?.picUrl || songData?.al?.picUrl || fallbackCover
      const g = (window as { __musicPlayerState?: Record<string, unknown> })
        .__musicPlayerState
      const cur = g?.currentSong as
        | { id?: string; url?: string; [k: string]: unknown }
        | undefined
      if (!cur || cur.id !== songId) return

      const nextSong = {
        ...cur,
        name: songData?.name || cur.name,
        artist:
          songData?.artists?.map((a: { name?: string }) => a.name).join(', ') ||
          songData?.ar?.map((a: { name?: string }) => a.name).join(', ') ||
          cur.artist,
        album: songData?.album?.name || songData?.al?.name || cur.album,
        cover: proxyImageUrlOr(rawCover, rawCover),
        duration: songData?.duration
          ? Math.floor(songData.duration / 1000)
          : cur.duration,
        isVip: !!(
          songData?.isVip ||
          songData?.fee === 1 ||
          songData?.fee === 4
        ),
        // Keep the playing url; do not reload.
        url: cur.url || song.url,
      }
      g!.currentSong = nextSong
      // Patch enrichment; a full broadcast would clobber it.
      window.dispatchEvent(
        new CustomEvent('music-player-patch-current-song', {
          detail: { song: nextSong },
        }),
      )
      window.dispatchEvent(
        new CustomEvent('music-player-state-change', {
          detail: { currentSong: nextSong },
        }),
      )
    } catch (e) {
      console.warn('[embedProcessor] 获取歌曲详情失败:', e)
    }
  } catch (error) {
    console.error('[embedProcessor] 播放网易云音乐失败:', error)
    throw error
  }
}
