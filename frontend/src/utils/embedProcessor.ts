/**
 * 嵌入内容处理器
 * 将文章中的 iframe 和特定链接转换为精美卡片
 *
 * 支持：
 * - 网易云音乐 iframe → 音乐播放卡片（触发临时播放）
 * - Steam 链接 → 游戏卡片
 * - Bilibili 嵌入/链接 → 视频卡片
 * - GitHub 链接 → 仓库卡片
 *
 * 样式与资料库卡片风格一致
 */

// ==================== 缓存系统 ====================
// 简单的内存缓存，避免重复请求相同资源（尤其是 GitHub API 有速率限制）
const CACHE_TTL = 5 * 60 * 1000 // 5分钟缓存

interface CacheEntry<T> {
  data: T
  timestamp: number
}

const embedDataCache = new Map<string, CacheEntry<any>>()

function getCached<T>(key: string): T | null {
  const entry = embedDataCache.get(key)
  if (!entry)
    return null

  // 检查是否过期
  if (Date.now() - entry.timestamp > CACHE_TTL) {
    embedDataCache.delete(key)
    return null
  }

  return entry.data
}

function setCache<T>(key: string, data: T): void {
  embedDataCache.set(key, { data, timestamp: Date.now() })

  // 简单的缓存清理：超过100条时清理最旧的
  if (embedDataCache.size > 100) {
    const entries = Array.from(embedDataCache.entries())
    entries.sort((a, b) => a[1].timestamp - b[1].timestamp)
    // 删除最旧的 20 条
    for (let i = 0; i < 20; i++) {
      embedDataCache.delete(entries[i][0])
    }
  }
}

// ==================== 类型定义 ====================
// 嵌入类型
export type EmbedType = 'netease-music' | 'steam-game' | 'bilibili-video' | 'github-repo'

// 嵌入信息
export interface EmbedInfo {
  type: EmbedType
  id: string
  url?: string
  originalHtml: string
}

/**
 * 从网易云音乐 iframe 提取歌曲 ID
 * 支持格式：
 * - https://music.163.com/outchain/player?type=2&id=2114614108&auto=1&height=66
 */
function extractNeteaseSongId(iframeSrc: string): string | null {
  const match = iframeSrc.match(/music\.163\.com\/outchain\/player\?.*?id=(\d+)/)
  return match ? match[1] : null
}

/**
 * 从网易云音乐链接提取歌曲 ID
 * 支持格式：
 * - https://music.163.com/song?id=33911781
 * - https://music.163.com/#/song?id=33911781
 * - https://y.music.163.com/m/song?id=33911781
 * - https://music.163.com/song/33911781
 */
function extractNeteaseSongIdFromUrl(url: string): string | null {
  // 处理 ?id=xxx 格式
  const queryMatch = url.match(/music\.163\.com\/(?:#\/)?(?:m\/)?song\?.*?id=(\d+)/)
  if (queryMatch)
    return queryMatch[1]

  // 处理 /song/xxx 格式
  const pathMatch = url.match(/music\.163\.com\/(?:#\/)?(?:m\/)?song\/(\d+)/)
  if (pathMatch)
    return pathMatch[1]

  return null
}

/**
 * 从 Steam 链接提取 AppID
 * 支持格式：
 * - https://store.steampowered.com/app/1234567
 * - steam://store/1234567
 */
function extractSteamAppId(url: string): string | null {
  const storeMatch = url.match(/store\.steampowered\.com\/app\/(\d+)/)
  if (storeMatch)
    return storeMatch[1]

  const steamMatch = url.match(/steam:\/\/store\/(\d+)/)
  if (steamMatch)
    return steamMatch[1]

  return null
}

/**
 * 从 Bilibili 链接/嵌入/纯文本提取视频 ID
 * 支持格式：
 * - https://www.bilibili.com/video/BV1xx411c7XW
 * - https://player.bilibili.com/player.html?bvid=BV1xx411c7XW
 * - https://www.bilibili.com/blackboard/html5mobileplayer.html?bvid=BV1xx411c7XW
 * - av号：https://www.bilibili.com/video/av170001 或 aid=170001
 * - 纯 BV 号：BV1xx411c7XW
 * - 纯 av 号：av170001
 */
function extractBilibiliVideoId(url: string): { type: 'bv' | 'av', id: string } | null {
  // 处理可能被破坏的 URL（RSS 有时会丢失分隔符）
  // 例如：amp;bvid=BV... 应该是 &bvid=BV...
  const cleanUrl = url.replace(/amp;/gi, '&')

  // 优先匹配 BV 号（各种格式）
  const bvMatch = cleanUrl.match(/(?:video\/|bvid=|[?&]bvid=)(BV[a-z0-9]+)/i)
  if (bvMatch)
    return { type: 'bv', id: bvMatch[1] }

  // 匹配 AV 号（各种格式）
  const avMatch = cleanUrl.match(/(?:video\/av|aid=|[?&]aid=)(\d+)/i)
  if (avMatch)
    return { type: 'av', id: avMatch[1] }

  // 匹配纯 BV 号（BV + 10-12 位字母数字）
  const pureBvMatch = cleanUrl.match(/\b(BV[a-z0-9]{10,12})\b/i)
  if (pureBvMatch)
    return { type: 'bv', id: pureBvMatch[1] }

  // 匹配纯 av 号（av + 数字）
  const pureAvMatch = cleanUrl.match(/\bav(\d+)\b/i)
  if (pureAvMatch)
    return { type: 'av', id: pureAvMatch[1] }

  return null
}

/**
 * 从 GitHub 链接提取仓库信息
 * 支持格式：
 * - https://github.com/owner/repo
 */
function extractGithubRepo(url: string): { owner: string, repo: string } | null {
  const match = url.match(/github\.com\/([^/]+)\/([^/?#]+)/)
  if (match)
    return { owner: match[1], repo: match[2] }
  return null
}

/**
 * 生成网易云音乐卡片 HTML - 与资料库音乐卡片风格一致
 * 正方形封面，悬停显示信息，平台图标气泡
 * 使用 my-4 作为默认margin（小尺寸卡片）
 */
function generateNeteaseMusicCard(songId: string, isDark: boolean): string {
  return `
    <div class="brew-embed-card brew-netease-music brew-embed-exempt not-prose block group cursor-pointer"
         data-embed-type="netease-music"
         data-song-id="${songId}"
         data-embed-exempt="true"
         style="width: 180px; height: 180px;">
      <div class="relative bg-white dark:bg-gray-800 rounded-xl shadow-md hover:shadow-2xl transition-all duration-300 transform hover:-translate-y-1 hover:scale-[1.02] overflow-hidden h-full">
        <div class="block w-full h-full relative">
          <!-- 封面容器 - 可被动态更新 -->
          <div class="brew-embed-cover w-full h-full bg-gradient-to-br from-red-400 to-red-600 flex items-center justify-center">
            <svg class="w-16 h-16 text-white/80 brew-embed-placeholder" fill="currentColor" viewBox="0 0 24 24">
              <path d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm0 14.5c-2.49 0-4.5-2.01-4.5-4.5S9.51 7.5 12 7.5s4.5 2.01 4.5 4.5-2.01 4.5-4.5 4.5zm0-5.5c-.55 0-1 .45-1 1s.45 1 1 1 1-.45 1-1-.45-1-1-1z"/>
            </svg>
          </div>

          <!-- 悬停信息遮罩 - 与资料库一致 -->
          <div class="absolute inset-0 bg-gradient-to-t from-black/95 via-black/60 to-transparent opacity-0 group-hover:opacity-100 transition-all duration-300 flex flex-col justify-end p-3 pointer-events-none">
            <div>
              <div class="flex items-start gap-1">
                <h3 class="brew-embed-title font-bold text-white text-xs leading-tight line-clamp-2 mb-1 flex-1">
                  加载中...
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

/**
 * 生成 Steam 游戏卡片 HTML - 与GitHub卡片尺寸一致
 * 横版卡片，悬停显示信息遮罩，无平台图标
 * 使用 my-6 作为默认margin（大尺寸卡片）
 */
function generateSteamGameCard(appId: string, isDark: boolean): string {
  const storeUrl = `https://store.steampowered.com/app/${appId}`
  const headerImg = `https://cdn.cloudflare.steamstatic.com/steam/apps/${appId}/header.jpg`

  return `
    <div class="brew-embed-card brew-steam-game brew-embed-exempt not-prose block group"
         data-embed-type="steam-game"
         data-app-id="${appId}"
         data-embed-exempt="true"
         style="max-width: 28rem;">
      <div class="bg-white dark:bg-gray-800 rounded-2xl shadow-lg hover:shadow-2xl transition-all duration-300 transform hover:-translate-y-1 overflow-hidden">
        <div class="relative overflow-hidden bg-gradient-to-br from-gray-900 to-gray-800">
          <a href="${storeUrl}"
             target="_blank"
             rel="noopener noreferrer"
             class="block w-full relative no-underline">
            <!-- 游戏封面图 -->
            <img src="${headerImg}"
                 alt="Steam Game"
                 class="brew-embed-cover w-full block object-cover aspect-[460/215] transition-all duration-500 group-hover:scale-110"
                 loading="lazy"/>
          </a>

          <!-- 悬停信息遮罩 -->
          <div class="absolute inset-0 bg-gradient-to-t from-black/90 via-black/40 to-transparent opacity-0 group-hover:opacity-100 transition-opacity duration-300 flex flex-col justify-end p-4 pointer-events-none">
            <h3 class="brew-embed-title font-bold text-white text-base line-clamp-2 leading-snug mb-1">
              App ID: ${appId}
            </h3>
            <p class="brew-embed-desc text-sm text-white/80 line-clamp-1">
              点击访问 Steam 商店
            </p>
          </div>
        </div>
      </div>
    </div>
  `
}

/**
 * 生成 Bilibili 官方嵌入 iframe
 * 使用 B站官方播放器，响应式容器
 * margin 由 BrewReader 统一控制
 */
function generateBilibiliIframe(videoId: { type: 'bv' | 'av', id: string }): string {
  // 构建官方播放器 URL
  const playerUrl = videoId.type === 'bv'
    ? `//player.bilibili.com/player.html?bvid=${videoId.id}&autoplay=0`
    : `//player.bilibili.com/player.html?aid=${videoId.id}&autoplay=0`

  return `
    <div class="brew-embed-card brew-bilibili-embed brew-embed-exempt not-prose block"
         data-video-id="${videoId.id}"
         data-video-type="${videoId.type}"
         data-embed-exempt="true">
      <div class="relative w-full rounded-xl overflow-hidden shadow-lg" style="padding-bottom: 56.25%;">
        <iframe
          src="${playerUrl}"
          class="absolute inset-0 w-full h-full border-0"
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

/**
 * 生成 Bilibili 视频卡片 HTML - 与资料库视频卡片风格一致
 * 横版封面，悬停显示信息（保留但不再使用）
 */
function generateBilibiliVideoCard(videoId: { type: 'bv' | 'av', id: string }, isDark: boolean): string {
  const videoUrl = videoId.type === 'bv'
    ? `https://www.bilibili.com/video/${videoId.id}`
    : `https://www.bilibili.com/video/av${videoId.id}`

  // 使用与其他卡片一致的结构：外层 div 作为 brew-embed-card
  return `
    <div class="brew-embed-card brew-bilibili-video brew-embed-exempt not-prose block group"
         data-embed-type="bilibili-video" 
         data-video-id="${videoId.id}"
         data-video-type="${videoId.type}"
         data-embed-exempt="true"
         style="width: 240px; height: 150px;">
      <a href="${videoUrl}" target="_blank" rel="noopener noreferrer" class="block no-underline h-full">
        <div class="relative bg-white dark:bg-gray-800 rounded-xl shadow-md hover:shadow-2xl transition-all duration-300 transform hover:-translate-y-1 hover:scale-[1.02] overflow-hidden h-full">
          <div class="block w-full h-full relative">
            <!-- 视频占位封面 -->
            <div class="w-full h-full bg-gradient-to-br from-[#00A1D6] to-[#0086B3] flex items-center justify-center">
              <svg class="w-16 h-16 text-white/80" fill="currentColor" viewBox="0 0 24 24">
                <path d="M8 5v14l11-7z"/>
              </svg>
            </div>
            
            <!-- 播放按钮遮罩 -->
            <div class="absolute inset-0 bg-black/20 flex items-center justify-center opacity-0 group-hover:opacity-100 transition-opacity duration-300">
              <div class="w-14 h-14 rounded-full bg-white/90 flex items-center justify-center shadow-lg">
                <svg class="w-7 h-7 text-[#00A1D6] ml-1" fill="currentColor" viewBox="0 0 24 24">
                  <path d="M8 5v14l11-7z"/>
                </svg>
              </div>
            </div>
            
            <!-- 底部信息栏 - 与资料库视频卡片一致 -->
            <div class="absolute bottom-3 left-3 right-3">
              <div class="inline-flex items-start max-w-full">
                <div class="bg-white/95 dark:bg-gray-800/95 backdrop-blur-sm rounded-lg px-3 py-2 shadow-lg">
                  <h3 class="font-bold text-gray-900 dark:text-white text-sm line-clamp-1 leading-snug">
                    ${videoId.type.toUpperCase()}: ${videoId.id}
                  </h3>
                  <p class="text-xs text-gray-600 dark:text-gray-400 mt-0.5">
                    点击查看视频
                  </p>
                </div>
              </div>
            </div>
          </div>
        </div>
      </a>
    </div>
  `
}

/**
 * 生成 GitHub 仓库卡片 HTML - 简洁风格
 * 显示仓库基本信息，与阅读器风格统一
 * margin 由 BrewReader 统一控制
 */
function generateGithubRepoCard(repo: { owner: string, repo: string }, isDark: boolean): string {
  const repoUrl = `https://github.com/${repo.owner}/${repo.repo}`
  const ownerAvatar = `https://github.com/${repo.owner}.png?size=32`

  // 使用与 Steam 卡片一致的结构：外层 div 作为 brew-embed-card，内部包含可视样式和链接
  return `
    <div class="brew-embed-card brew-github-repo brew-embed-exempt not-prose block group"
         data-embed-type="github-repo"
         data-owner="${repo.owner}"
         data-repo="${repo.repo}"
         data-embed-exempt="true"
         style="max-width: 28rem;">
      <a href="${repoUrl}"
         target="_blank"
         rel="noopener noreferrer"
         class="block no-underline">
        <div class="relative w-full bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded-xl p-4 shadow-sm hover:shadow-lg transition-all duration-300 transform hover:-translate-y-0.5">

          <!-- GitHub 图标 -->
          <div class="absolute top-4 right-4">
            <svg viewBox="0 0 24 24" class="w-5 h-5 text-gray-300 dark:text-gray-600 group-hover:text-gray-400 dark:group-hover:text-gray-500 transition-colors">
              <path fill="currentColor" d="M12 0c-6.626 0-12 5.373-12 12 0 5.302 3.438 9.8 8.207 11.387.599.111.793-.261.793-.577v-2.234c-3.338.726-4.033-1.416-4.033-1.416-.546-1.387-1.333-1.756-1.333-1.756-1.089-.745.083-.729.083-.729 1.205.084 1.839 1.237 1.839 1.237 1.07 1.834 2.807 1.304 3.492.997.107-.775.418-1.305.762-1.604-2.665-.305-5.467-1.334-5.467-5.931 0-1.311.469-2.381 1.236-3.221-.124-.303-.535-1.524.117-3.176 0 0 1.008-.322 3.301 1.23.957-.266 1.983-.399 3.003-.404 1.02.005 2.047.138 3.006.404 2.291-1.552 3.297-1.23 3.297-1.23.653 1.653.242 2.874.118 3.176.77.84 1.235 1.911 1.235 3.221 0 4.609-2.807 5.624-5.479 5.921.43.372.823 1.102.823 2.222v3.293c0 .319.192.694.801.576 4.765-1.589 8.199-6.086 8.199-11.386 0-6.627-5.373-12-12-12z"/>
            </svg>
          </div>

          <!-- 所有者信息 -->
          <div class="flex items-center gap-2 mb-2">
            <img src="${ownerAvatar}"
                 alt="${repo.owner}"
                 class="w-5 h-5 rounded-full"
                 loading="lazy"/>
            <span class="brew-embed-owner text-sm text-gray-500 dark:text-gray-400">
              ${repo.owner}
            </span>
          </div>

          <!-- 仓库名称 -->
          <h3 class="brew-embed-title text-base font-semibold text-gray-900 dark:text-white mb-1 group-hover:text-blue-600 dark:group-hover:text-blue-400 transition-colors">
            ${repo.repo}
          </h3>

          <!-- 仓库描述 -->
          <p class="brew-embed-desc text-sm text-gray-600 dark:text-gray-400 mb-3 line-clamp-2">
            加载中...
          </p>

          <!-- 统计信息 -->
          <div class="flex items-center gap-4 text-xs text-gray-500 dark:text-gray-400">
            <!-- Stars -->
            <span class="brew-embed-stars flex items-center gap-1">
              <svg class="w-3.5 h-3.5" fill="currentColor" viewBox="0 0 16 16">
                <path d="M8 .25a.75.75 0 0 1 .673.418l1.882 3.815 4.21.612a.75.75 0 0 1 .416 1.279l-3.046 2.97.719 4.192a.75.75 0 0 1-1.088.791L8 12.347l-3.766 1.98a.75.75 0 0 1-1.088-.79l.72-4.194L.818 6.374a.75.75 0 0 1 .416-1.28l4.21-.611L7.327.668A.75.75 0 0 1 8 .25z"/>
              </svg>
              <span>-</span>
            </span>

            <!-- Forks -->
            <span class="brew-embed-forks flex items-center gap-1">
              <svg class="w-3.5 h-3.5" fill="currentColor" viewBox="0 0 16 16">
                <path d="M5 5.372v.878c0 .414.336.75.75.75h4.5a.75.75 0 0 0 .75-.75v-.878a2.25 2.25 0 1 1 1.5 0v.878a2.25 2.25 0 0 1-2.25 2.25h-1.5v2.128a2.251 2.251 0 1 1-1.5 0V8.5h-1.5A2.25 2.25 0 0 1 3.5 6.25v-.878a2.25 2.25 0 1 1 1.5 0ZM5 3.25a.75.75 0 1 0-1.5 0 .75.75 0 0 0 1.5 0Zm6.75.75a.75.75 0 1 0 0-1.5.75.75 0 0 0 0 1.5Zm-3 8.75a.75.75 0 1 0-1.5 0 .75.75 0 0 0 1.5 0Z"/>
              </svg>
              <span>-</span>
            </span>

            <!-- 编程语言 -->
            <span class="brew-embed-lang flex items-center gap-1">
              <span class="inline-block w-2.5 h-2.5 rounded-full bg-gray-300 dark:bg-gray-600"></span>
              <span>-</span>
            </span>
          </div>
        </div>
      </a>
    </div>
  `
}

/**
 * 处理内容中的嵌入
 * @param content HTML 内容
 * @param isDark 是否为暗色主题
 * @returns 处理后的 HTML 内容
 */
export function processEmbeds(content: string, isDark: boolean): string {
  let result = content

  // 1. 处理网易云音乐 iframe
  // 匹配: <iframe ... src="https://music.163.com/outchain/player?..." ...></iframe>
  const neteaseIframeRegex = /<iframe[^>]*src=["']([^"']*music\.163\.com\/outchain\/player[^"']*)["'][^>]*>[\s\S]*?<\/iframe>/gi
  result = result.replace(neteaseIframeRegex, (match, src) => {
    const songId = extractNeteaseSongId(src)
    if (songId) {
      return generateNeteaseMusicCard(songId, isDark)
    }
    return match // 无法解析则保留原样
  })

  // 1.1 处理网易云音乐链接（<a> 标签形式）
  // 匹配: <a href="https://music.163.com/song?id=xxx">...</a>
  // 支持多种格式：/song?id=xxx, /#/song?id=xxx, /m/song?id=xxx, /song/xxx
  const neteaseLinkRegex = /<a[^>]*href=["'](https?:\/\/(?:y\.)?music\.163\.com\/(?:#\/)?(?:m\/)?song(?:\?[^"']*id=\d|\/\d)[^"']*)["'][^>]*>[\s\S]*?<\/a>/gi
  result = result.replace(neteaseLinkRegex, (match, url) => {
    const songId = extractNeteaseSongIdFromUrl(url)
    if (songId) {
      return generateNeteaseMusicCard(songId, isDark)
    }
    return match
  })

  // 2. 处理 Steam 链接（不在已有链接标签内的纯 URL）
  // 只处理独立的链接，避免重复处理
  const steamLinkRegex = /<a[^>]*href=["'](https?:\/\/store\.steampowered\.com\/app\/\d[^"']*)["'][^>]*>[\s\S]*?<\/a>/gi
  result = result.replace(steamLinkRegex, (match, url) => {
    const appId = extractSteamAppId(url)
    if (appId) {
      return generateSteamGameCard(appId, isDark)
    }
    return match
  })

  // 3. Bilibili 处理：不再处理官方 iframe，只把 AV/BV 号和链接转为官方 iframe

  // 3.1 处理 Bilibili 视频链接 -> 转为官方 iframe
  const bilibiliLinkRegex = /<a[^>]*href=["'](https?:\/\/(?:www\.)?bilibili\.com\/video\/(?:BV[a-z0-9]|av\d)[^"']*)["'][^>]*>[\s\S]*?<\/a>/gi
  result = result.replace(bilibiliLinkRegex, (match, url) => {
    const videoId = extractBilibiliVideoId(url)
    if (videoId) {
      return generateBilibiliIframe(videoId)
    }
    return match
  })

  // 3.2 处理纯文本中的 BV 号或 AV 号（不在链接内的）-> 转为官方 iframe
  // 匹配独立的 BV1xxxxxxxxx 或 av12345678 格式
  const bilibiliPlainTextRegex = /(?<!<[^>]*|href=["'][^"']*|>)\b(BV[a-z0-9]{10,12}|av\d{1,12})\b(?![^<]*<\/a>)/gi
  result = result.replace(bilibiliPlainTextRegex, (match) => {
    const videoId = extractBilibiliVideoId(match)
    if (videoId) {
      return generateBilibiliIframe(videoId)
    }
    return match
  })

  // 4. 处理 GitHub 仓库链接（仅处理指向仓库首页的链接）
  const githubLinkRegex = /<a[^>]*href=["'](https?:\/\/github\.com\/[^/]+\/[^/?#"']+)["'][^>]*>[\s\S]*?<\/a>/gi
  result = result.replace(githubLinkRegex, (match, url) => {
    // 排除 github.com/user/repo/xxx 这种子页面链接
    const cleanUrl = url.split('?')[0].split('#')[0]
    const parts = cleanUrl.replace(/^https?:\/\/github\.com\//, '').split('/')
    if (parts.length === 2 && parts[0] && parts[1]) {
      const repo = extractGithubRepo(url)
      if (repo) {
        return generateGithubRepoCard(repo, isDark)
      }
    }
    return match
  })

  return result
}

/**
 * 加载所有嵌入卡片的真实数据
 * 在内容渲染后调用此函数，异步获取数据并更新 DOM
 * @param container 包含嵌入卡片的容器元素
 */
export async function loadEmbedData(container: HTMLElement): Promise<void> {
  // 并行加载所有类型的嵌入数据
  await Promise.all([
    loadNeteaseMusicData(container),
    loadSteamGameData(container),
    loadGithubRepoData(container),
  ])
}

/**
 * 加载网易云音乐卡片数据
 * 使用并行加载提升性能
 */
async function loadNeteaseMusicData(container: HTMLElement): Promise<void> {
  const cards = Array.from(container.querySelectorAll('.brew-netease-music[data-song-id]'))

  // 过滤出未加载的卡片
  const unloadedCards = cards.filter(card =>
    card.getAttribute('data-song-id') && card.getAttribute('data-loaded') !== 'true',
  )

  if (unloadedCards.length === 0)
    return

  // 并行加载所有卡片数据
  await Promise.all(unloadedCards.map(async (card) => {
    const songId = card.getAttribute('data-song-id')
    if (!songId)
      return

    // 立即标记为加载中，防止重复请求
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

      // 更新封面
      const coverContainer = card.querySelector('.brew-embed-cover')
      if (coverContainer && songData.album?.picUrl) {
        const coverUrl = songData.album.picUrl || songData.al?.picUrl
        if (coverUrl) {
          // 安全：使用 DOM API 创建元素，避免 XSS
          const img = document.createElement('img')
          img.src = coverUrl
          img.alt = songData.name || ''
          img.className = 'w-full h-full object-cover transition-all duration-500 group-hover:scale-110'
          img.loading = 'lazy'
          coverContainer.innerHTML = ''
          coverContainer.appendChild(img)
        }
      }

      // 更新标题
      const titleEl = card.querySelector('.brew-embed-title')
      if (titleEl) {
        titleEl.textContent = songData.name
      }

      // 更新艺术家
      const artistEl = card.querySelector('.brew-embed-artist')
      if (artistEl) {
        const artists = songData.artists || songData.ar || []
        const artistText = artists.map((a: any) => a.name).join(', ') || '未知艺术家'
        artistEl.textContent = artistText
      }

      // 添加 VIP 标记
      if (songData.isVip || songData.fee === 1 || songData.fee === 4) {
        const titleContainer = card.querySelector('.brew-embed-title')?.parentElement
        if (titleContainer && !titleContainer.querySelector('.vip-badge')) {
          const vipBadge = document.createElement('span')
          vipBadge.className = 'vip-badge inline-flex items-center px-1.5 py-0.5 rounded-md bg-gradient-to-r from-yellow-500 to-amber-600 text-[10px] font-semibold text-white shadow-md select-none ml-1'
          vipBadge.textContent = 'VIP'
          titleContainer.appendChild(vipBadge)
        }
      }

      // 标记已加载
      card.setAttribute('data-loaded', 'true')
    }
    catch (error) {
      console.warn(`[embedProcessor] 加载网易云音乐 ${songId} 失败:`, error)
      card.setAttribute('data-loaded', 'true')
    }
  }))
}

/**
 * 加载 Steam 游戏卡片数据
 * 通过后端API获取游戏详情（避免CORS问题）
 * 使用并行加载 + 缓存提升性能
 */
async function loadSteamGameData(container: HTMLElement): Promise<void> {
  const cards = Array.from(container.querySelectorAll('.brew-steam-game[data-app-id]'))

  // 过滤出未加载的卡片
  const unloadedCards = cards.filter(card =>
    card.getAttribute('data-app-id') && card.getAttribute('data-loaded') !== 'true',
  )

  if (unloadedCards.length === 0)
    return

  // 并行加载所有卡片数据
  await Promise.all(unloadedCards.map(async (card) => {
    const appId = card.getAttribute('data-app-id')
    if (!appId)
      return

    // 立即标记为加载中
    card.setAttribute('data-loaded', 'loading')

    try {
      // 检查缓存
      const cacheKey = `steam:${appId}`
      let gameData = getCached<any>(cacheKey)

      if (!gameData) {
        // 使用后端代理API获取游戏详情
        const response = await fetch(`/api/steam/game/${appId}`)
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
        // 更新游戏名称
        const titleEl = card.querySelector('.brew-embed-title')
        if (titleEl && gameData.name) {
          titleEl.textContent = gameData.name
        }

        // 更新游戏描述
        const descEl = card.querySelector('.brew-embed-desc')
        if (descEl && gameData.short_description) {
          // 安全：使用 DOMParser 提取纯文本，避免 innerHTML 触发脚本
          try {
            const parser = new DOMParser()
            const doc = parser.parseFromString(gameData.short_description, 'text/html')
            const plainText = doc.body.textContent || ''
            descEl.textContent = plainText
          }
          catch {
            // 回退：直接移除所有 HTML 标签
            descEl.textContent = gameData.short_description.replace(/<[^>]*>/g, '')
          }
        }
      }

      card.setAttribute('data-loaded', 'true')
    }
    catch (error) {
      console.warn(`[embedProcessor] 加载 Steam 游戏 ${appId} 失败:`, error)
      card.setAttribute('data-loaded', 'true')
    }
  }))
}

/**
 * 加载 GitHub 仓库卡片数据
 * 使用 GitHub API 获取仓库信息（star、fork、描述、语言）
 * 使用并行加载 + 缓存提升性能（GitHub API 有 60次/小时 的速率限制）
 */
async function loadGithubRepoData(container: HTMLElement): Promise<void> {
  const cards = Array.from(container.querySelectorAll('.brew-github-repo[data-owner][data-repo]'))

  // 过滤出未加载的卡片
  const unloadedCards = cards.filter((card) => {
    const owner = card.getAttribute('data-owner')
    const repo = card.getAttribute('data-repo')
    return owner && repo && card.getAttribute('data-loaded') !== 'true'
  })

  if (unloadedCards.length === 0)
    return

  // 并行加载所有卡片数据
  await Promise.all(unloadedCards.map(async (card) => {
    const owner = card.getAttribute('data-owner')
    const repo = card.getAttribute('data-repo')
    if (!owner || !repo)
      return

    // 立即标记为加载中
    card.setAttribute('data-loaded', 'loading')

    try {
      // 检查缓存（GitHub API 有速率限制，缓存很重要）
      const cacheKey = `github:${owner}/${repo}`
      let repoData = getCached<any>(cacheKey)

      if (!repoData) {
        // 使用 GitHub API（无需认证的公开接口，有速率限制）
        const response = await fetch(`https://api.github.com/repos/${owner}/${repo}`)
        if (!response.ok) {
          card.setAttribute('data-loaded', 'true')
          return
        }

        repoData = await response.json()
        setCache(cacheKey, repoData)
      }

      // 更新仓库描述
      const descEl = card.querySelector('.brew-embed-desc')
      if (descEl) {
        descEl.textContent = repoData.description || '暂无描述'
      }

      // 更新 Star 数
      const starsEl = card.querySelector('.brew-embed-stars span')
      if (starsEl) {
        const stars = repoData.stargazers_count || 0
        starsEl.textContent = formatCount(stars)
      }

      // 更新 Fork 数
      const forksEl = card.querySelector('.brew-embed-forks span')
      if (forksEl) {
        const forks = repoData.forks_count || 0
        forksEl.textContent = formatCount(forks)
      }

      // 更新语言
      const langEl = card.querySelector('.brew-embed-lang')
      if (langEl && repoData.language) {
        const langColor = getLanguageColor(repoData.language)
        const colorDot = langEl.querySelector('span:first-child') as HTMLElement
        const langText = langEl.querySelector('span:last-child')
        if (colorDot) {
          colorDot.style.backgroundColor = langColor
        }
        if (langText) {
          langText.textContent = repoData.language
        }
      }
      else if (langEl) {
        // 没有语言信息，隐藏该元素
        (langEl as HTMLElement).style.display = 'none'
      }

      // 标记已加载
      card.setAttribute('data-loaded', 'true')
    }
    catch (error) {
      console.warn(`[embedProcessor] 加载 GitHub 仓库 ${owner}/${repo} 失败:`, error)

      // 即使失败也标记，避免重复请求
      const descEl = card.querySelector('.brew-embed-desc')
      if (descEl) {
        descEl.textContent = '仓库信息加载失败'
      }
      card.setAttribute('data-loaded', 'true')
    }
  }))
}

/**
 * 获取编程语言对应的颜色
 */
function getLanguageColor(language: string): string {
  const colors: Record<string, string> = {
    'JavaScript': '#f1e05a',
    'TypeScript': '#3178c6',
    'Python': '#3572A5',
    'Java': '#b07219',
    'Go': '#00ADD8',
    'Rust': '#dea584',
    'C': '#555555',
    'C++': '#f34b7d',
    'C#': '#178600',
    'PHP': '#4F5D95',
    'Ruby': '#701516',
    'Swift': '#F05138',
    'Kotlin': '#A97BFF',
    'Dart': '#00B4AB',
    'Vue': '#41b883',
    'HTML': '#e34c26',
    'CSS': '#563d7c',
    'Shell': '#89e051',
    'Lua': '#000080',
  }
  return colors[language] || '#6b7280'
}

/**
 * 格式化数字显示（如 1.2k, 3.5M）
 */
function formatCount(count: number): string {
  if (count >= 1000000) {
    return `${(count / 1000000).toFixed(1).replace(/\.0$/, '')}M`
  }
  if (count >= 1000) {
    return `${(count / 1000).toFixed(1).replace(/\.0$/, '')}k`
  }
  return count.toString()
}

/**
 * 从网易云音乐卡片获取歌曲信息并触发播放
 * @param songId 歌曲 ID
 */
export async function playNeteaseSong(songId: string): Promise<void> {
  try {
    let songData: any = null

    // 通过后端代理 API 获取歌曲详情（避免跨域问题）
    try {
      const detailResponse = await fetch(`/api/proxy/music/netease/song/${songId}`)

      if (detailResponse.ok) {
        songData = await detailResponse.json()
      }
    }
    catch (e) {
      console.warn('[embedProcessor] 获取歌曲详情失败:', e)
    }

    // 构建歌曲对象（即使没有详情也能播放）
    const song = {
      id: songId,
      name: songData?.name || `网易云音乐 #${songId}`,
      artist: songData?.artists?.map((a: any) => a.name).join(', ')
        || songData?.ar?.map((a: any) => a.name).join(', ') || '未知艺术家',
      album: songData?.album?.name || songData?.al?.name || '未知专辑',
      cover: songData?.album?.picUrl || songData?.al?.picUrl
        || `https://p1.music.126.net/UeTuwE7pvjBpypWLudqukA==/3132508627578625.jpg`,
      url: `/api/proxy/music/netease/audio/${songId}`,
      duration: songData?.duration ? Math.floor(songData.duration / 1000) : 0,
      source: 'netease',
      isVip: songData?.isVip || songData?.fee === 1 || songData?.fee === 4,
    }

    // 触发播放事件
    window.dispatchEvent(new CustomEvent('play-song', {
      detail: { song },
    }))

    // 打开控制面板
    window.dispatchEvent(new CustomEvent('open-control-panel'))
  }
  catch (error) {
    console.error('[embedProcessor] 播放网易云音乐失败:', error)
    throw error
  }
}
