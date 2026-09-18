import { API_URL } from '../config'
import { currentCopy } from '../i18n/localeCopy'
import apiService from '../services/api'
import { formatUserFacingError } from './formatUserFacingError'
import { httpStatusMessage } from './httpStatus'

export interface QuoteData {
  text: string
  author?: string
}

export interface HitokotoSource {
  id: string
  url: string
  textField: string
  authorField?: string
}

export const BUILTIN_HITOKOTO_SOURCES: Record<string, HitokotoSource> = {
  'hitokoto-cn': {
    id: 'hitokoto-cn',
    url: 'https://v1.hitokoto.cn/?c=d&c=i&c=k&encode=json',
    textField: 'hitokoto',
    authorField: 'from',
  },
  'hitokoto-anime': {
    id: 'hitokoto-anime',
    url: 'https://v1.hitokoto.cn/?c=a&c=b&encode=json',
    textField: 'hitokoto',
    authorField: 'from',
  },
  'quotable-en': {
    id: 'quotable-en',
    url: 'https://api.quotable.io/random',
    textField: 'content',
    authorField: 'author',
  },
  'meigen-ja': {
    id: 'meigen-ja',
    url: 'https://meigen.doodlenote.net/api/json.php',
    textField: 'meigen',
    authorField: 'auther',
  },
}

export const DEFAULT_HITOKOTO_SOURCE_ID = 'hitokoto-cn'

export interface HitokotoConfig {
  sourceId: string
  customUrl?: string
  customTextField?: string
  customAuthorField?: string
}

interface HitokotoConfigResponse {
  success: boolean
  config?: HitokotoConfig
  message?: string
}

export const DEFAULT_HITOKOTO_CONFIG: HitokotoConfig = {
  sourceId: DEFAULT_HITOKOTO_SOURCE_ID,
}

export const HITOKOTO_CONFIG_UPDATED_EVENT = 'hitokoto-config-updated'

export function normalizeHitokotoConfig(
  config?: Partial<HitokotoConfig>,
): HitokotoConfig {
  return {
    sourceId:
      typeof config?.sourceId === 'string'
        ? config.sourceId
        : DEFAULT_HITOKOTO_SOURCE_ID,
    customUrl: config?.customUrl,
    customTextField: config?.customTextField,
    customAuthorField: config?.customAuthorField,
  }
}

/* Do not persist hitokoto config in localStorage. */
const HITOKOTO_CONFIG_TTL = 5 * 60 * 1000
let cachedHitokotoConfig: HitokotoConfig | null = null
let cachedHitokotoConfigAt = 0
let hitokotoConfigInflight: Promise<HitokotoConfig> | null = null

export function clearHitokotoConfigCache(): void {
  cachedHitokotoConfig = null
  cachedHitokotoConfigAt = 0
  hitokotoConfigInflight = null
}

if (typeof window !== 'undefined') {
  window.addEventListener(HITOKOTO_CONFIG_UPDATED_EVENT, (event: Event) => {
    const detail = (event as CustomEvent<HitokotoConfig | undefined>).detail
    if (detail && typeof detail.sourceId === 'string') {
      cachedHitokotoConfig = normalizeHitokotoConfig(detail)
      cachedHitokotoConfigAt = Date.now()
      hitokotoConfigInflight = null
      return
    }
    clearHitokotoConfigCache()
  })
}

export async function fetchHitokotoConfig(
  options?: { force?: boolean },
): Promise<HitokotoConfig> {
  if (!options?.force) {
    if (
      cachedHitokotoConfig &&
      Date.now() - cachedHitokotoConfigAt < HITOKOTO_CONFIG_TTL
    ) {
      return cachedHitokotoConfig
    }
    if (hitokotoConfigInflight) return hitokotoConfigInflight
  }

  hitokotoConfigInflight = (async () => {
    try {
      const response = await apiService.get<HitokotoConfigResponse>(
        '/config/hitokoto',
      )
      const config = normalizeHitokotoConfig(response.config)
      cachedHitokotoConfig = config
      cachedHitokotoConfigAt = Date.now()
      return config
    } finally {
      hitokotoConfigInflight = null
    }
  })()

  return hitokotoConfigInflight
}

export async function updateHitokotoConfig(
  config: HitokotoConfig,
): Promise<HitokotoConfig> {
  const response = await apiService.put<HitokotoConfigResponse>(
    '/config/hitokoto',
    config,
  )
  if (!response.success) {
    throw new Error(
      await formatUserFacingError(
        response.message,
        currentCopy().config.hitokotoSaveFailed,
      ),
    )
  }
  const saved = normalizeHitokotoConfig(response.config)
  cachedHitokotoConfig = saved
  cachedHitokotoConfigAt = Date.now()
  hitokotoConfigInflight = null
  localStorage.removeItem('quote_cache')
  localStorage.removeItem('quote_cache_time')
  localStorage.removeItem('quote_cache_source')
  localStorage.removeItem('quote_data_cache')
  window.dispatchEvent(
    new CustomEvent(HITOKOTO_CONFIG_UPDATED_EVENT, { detail: saved }),
  )
  return saved
}

export function areHitokotoConfigsEqual(
  left: HitokotoConfig,
  right: HitokotoConfig,
): boolean {
  return (
    left.sourceId === right.sourceId &&
    (left.customUrl ?? '') === (right.customUrl ?? '') &&
    (left.customTextField ?? '') === (right.customTextField ?? '') &&
    (left.customAuthorField ?? '') === (right.customAuthorField ?? '')
  )
}

export async function resolveHitokotoSource(
  config?: HitokotoConfig,
): Promise<HitokotoSource | null> {
  const resolved = config ?? (await fetchHitokotoConfig())
  return resolveHitokotoSourceFromConfig(resolved)
}

function resolveHitokotoSourceFromConfig(
  config: HitokotoConfig,
): HitokotoSource | null {
  if (config.sourceId === 'custom') {
    const url = config.customUrl?.trim()
    if (!url) return null
    return {
      id: 'custom',
      url,
      textField: config.customTextField?.trim() || 'hitokoto',
      authorField: config.customAuthorField?.trim() || 'from',
    }
  }
  return (
    BUILTIN_HITOKOTO_SOURCES[config.sourceId] ??
    BUILTIN_HITOKOTO_SOURCES[DEFAULT_HITOKOTO_SOURCE_ID]
  )
}

export async function getRandomQuote(
  locale?: string,
): Promise<QuoteData | null> {
  let source: HitokotoSource | null
  try {
    source = await resolveHitokotoSource()
  } catch (error) {
    console.warn('Failed to load hitokoto config:', error)
    return getLocalQuote(locale)
  }
  if (!source) return getLocalQuote(locale)

  try {
    // Cache must match the current source URL.
    const cachedQuote = localStorage.getItem('quote_cache')
    const cacheTime = localStorage.getItem('quote_cache_time')
    const cacheSource = localStorage.getItem('quote_cache_source')

    if (cachedQuote && cacheTime && cacheSource === source.url) {
      const cacheAge = Date.now() - Number.parseInt(cacheTime)
      if (cacheAge < 10 * 60 * 1000) {
        return JSON.parse(cachedQuote)
      }
    }

    // Proxy (CORS).
    const proxyUrl =
      source.id === DEFAULT_HITOKOTO_SOURCE_ID
        ? `${API_URL}/api/proxy/hitokoto`
        : `${API_URL}/api/proxy/hitokoto?url=${encodeURIComponent(source.url)}`

    const response = await fetch(proxyUrl, {
      signal: AbortSignal.timeout(10000),
    })

    if (!response.ok) throw new Error(httpStatusMessage(response.status))

    const data = await response.json()
    const payload = Array.isArray(data) ? data[0] : data

    const text = payload?.[source.textField]
    if (typeof text !== 'string' || !text.trim()) {
      throw new Error('Hitokoto response missing text field')
    }
    const author = source.authorField
      ? payload?.[source.authorField]
      : undefined

    const quoteData: QuoteData = {
      text,
      author: typeof author === 'string' && author.trim() ? author : undefined,
    }

    localStorage.setItem('quote_cache', JSON.stringify(quoteData))
    localStorage.setItem('quote_cache_time', Date.now().toString())
    localStorage.setItem('quote_cache_source', source.url)

    return quoteData
  } catch (error) {
    console.warn('Failed to fetch quote:', error)
    return getLocalQuote(locale)
  }
}

function getLocalQuote(locale?: string): QuoteData {
  const quotesZhCN = [
    { text: '代码如诗，优雅至上', author: '程序员格言' },
    { text: '简洁是可靠的前提', author: 'Edsger Dijkstra' },
    { text: '过早优化是万恶之源', author: 'Donald Knuth' },
    {
      text: '任何可以被编写成 JavaScript 的程序，最终都会被编写成 JavaScript',
      author: 'Atwood 定律',
    },
    { text: '好的代码本身就是最好的文档', author: 'Steve McConnell' },
    { text: '先让它运行起来，再让它变得更好', author: 'Kent Beck' },
    { text: '代码是写给人看的，顺便让机器执行', author: 'Harold Abelson' },
    {
      text: '测试不能证明程序没有 bug，只能证明 bug 的存在',
      author: 'Edsger Dijkstra',
    },
  ]

  const quotesEnUS = [
    {
      text: "Code is like humor. When you have to explain it, it's bad.",
      author: 'Cory House',
    },
    { text: 'Simplicity is the soul of efficiency.', author: 'Austin Freeman' },
    { text: 'Make it work, make it right, make it fast.', author: 'Kent Beck' },
    { text: 'Talk is cheap. Show me the code.', author: 'Linus Torvalds' },
    { text: 'Software is eating the world.', author: 'Marc Andreessen' },
    {
      text: 'The best way to predict the future is to invent it.',
      author: 'Alan Kay',
    },
  ]

  const quotesJaJP = [
    { text: 'コードは詩のように、優雅であれ', author: 'プログラマーの格言' },
    { text: 'シンプルさは信頼性の前提条件である', author: 'Edsger Dijkstra' },
    { text: '早すぎる最適化は諸悪の根源', author: 'Donald Knuth' },
    {
      text: '動くようにしてから、正しくしてから、速くする',
      author: 'Kent Beck',
    },
    { text: '良いコードは最高のドキュメントである', author: 'Steve McConnell' },
    {
      text: '未来を予測する最良の方法は、それを発明することだ',
      author: 'Alan Kay',
    },
  ]

  let quotes: QuoteData[]
  switch (locale) {
    case 'en-US':
    case 'ko-KR':
    case 'fr-FR':
    case 'de-DE':
      quotes = quotesEnUS
      break
    case 'ja-JP':
      quotes = quotesJaJP
      break
    default:
      quotes = quotesZhCN
  }

  return quotes[Math.floor(Math.random() * quotes.length)]
}
