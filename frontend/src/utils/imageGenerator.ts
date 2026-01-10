// AI Image Generation with Pollinations.ai
// API Docs: https://github.com/pollinations/pollinations/blob/master/APIDOCS.md

import { API_URL } from '../config'
import { getCSRFToken } from './csrf'

interface ImageGenOptions {
  model?: 'flux' | 'flux-realism' | 'flux-anime' | 'flux-3d' | 'turbo'
  width?: number
  height?: number
  seed?: number
  nologo?: boolean
  private?: boolean
  enhance?: boolean
}

interface CardIllustration {
  url: string
  prompt: string
  timestamp: number
  cardId: number
}

const IMAGE_CACHE_KEY = 'myriad_card_illustrations'
const CACHE_EXPIRY_DAYS = 30

// 调用后端 AI 生成提示词
async function generatePromptFromAPI(title: string, summary: string, category: string): Promise<string> {
  try {
    // 输入验证
    if (!title || typeof title !== 'string') {
      throw new Error('Invalid title')
    }

    // 获取 CSRF Token
    const csrfToken = await getCSRFToken(true)
    if (!csrfToken) {
      throw new Error('无法获取 CSRF Token')
    }

    const controller = new AbortController()
    const timeoutId = setTimeout(() => controller.abort(), 10000)

    const response = await fetch(`${API_URL}/api/prompt/generate`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'X-CSRF-Token': csrfToken,
      },
      credentials: 'include',
      body: JSON.stringify({
        title: title.substring(0, 500),
        summary: summary.substring(0, 1000),
        category: category.substring(0, 100),
      }),
      signal: controller.signal,
    })

    clearTimeout(timeoutId)

    if (!response.ok) {
      throw new Error(`API error: ${response.status}`)
    }

    const data = await response.json()

    if (!data.prompt || typeof data.prompt !== 'string') {
      throw new Error('Invalid response format')
    }

    return data.prompt
  }
  catch (error) {
    // 降级方案：使用简单的默认提示词
    return `A cute chibi character, ${title}, in Studio Ghibli art style, transparent background, PNG format, no background, isolated subject, masterpiece, highest quality, detailed character design, soft lighting, hand-drawn animation style, Hayao Miyazaki inspired, watercolor texture, gentle colors, whimsical atmosphere, professional illustration, 8K resolution, ultra detailed, cute kawaii style`
  }
}

// 根据卡片内容生成吉卜力风格插画提示词
export async function generateGhibliPrompt(title: string, summary: string, category: string): Promise<string> {
  // 直接调用后端 AI 生成提示词
  const prompt = await generatePromptFromAPI(title, summary, category)
  return prompt
}

// 生成插画 URL（Pollinations API）
export function generateImageUrl(prompt: string, options: ImageGenOptions = {}): string {
  const {
    model = 'flux',
    width = 512,
    height = 512,
    seed,
    nologo = true,
    private: isPrivate = false,
    enhance = true,
  } = options

  // URL encode the prompt
  const encodedPrompt = encodeURIComponent(prompt)

  // Build query parameters
  const params = new URLSearchParams()
  params.set('width', width.toString())
  params.set('height', height.toString())
  params.set('model', model)
  params.set('nologo', nologo.toString())
  params.set('private', isPrivate.toString())
  params.set('enhance', enhance.toString())
  if (seed !== undefined) {
    params.set('seed', seed.toString())
  }

  return `https://image.pollinations.ai/prompt/${encodedPrompt}?${params.toString()}`
}

// 从 localStorage 获取缓存的插画
export function getCachedIllustrations(): Map<number, CardIllustration> {
  try {
    const cached = localStorage.getItem(IMAGE_CACHE_KEY)
    if (!cached)
      return new Map()

    const data = JSON.parse(cached)
    const illustrations = new Map<number, CardIllustration>()

    // 检查过期
    const now = Date.now()
    const expiryTime = CACHE_EXPIRY_DAYS * 24 * 60 * 60 * 1000

    for (const [cardId, illustration] of Object.entries(data)) {
      const ill = illustration as CardIllustration
      if (now - ill.timestamp < expiryTime) {
        illustrations.set(Number(cardId), ill)
      }
    }

    return illustrations
  }
  catch (e) {
    return new Map()
  }
}

// 保存插画到 localStorage
export function saveIllustration(cardId: number, illustration: CardIllustration): void {
  try {
    const cached = getCachedIllustrations()
    cached.set(cardId, illustration)

    const data: Record<number, CardIllustration> = {}
    cached.forEach((value, key) => {
      data[key] = value
    })

    localStorage.setItem(IMAGE_CACHE_KEY, JSON.stringify(data))
  }
  catch (e) {
    // 静默失败,缓存不可用不影响功能
  }
}

// 为卡片生成插画
export async function generateCardIllustration(
  cardId: number,
  title: string,
  summary: string,
  category: string,
): Promise<string> {
  // 检查缓存
  const cached = getCachedIllustrations()
  const cachedIll = cached.get(cardId)

  if (cachedIll) {
    return cachedIll.url
  }

  // 从公开配置读取参数 - 注意：图片生成配置目前不在公开API中
  // 这里暂时使用默认值，如果需要从配置读取，需要将相关配置添加到 /api/config/ui
  let config: any = {}
  try {
    const response = await fetch(`${API_URL}/api/config/ui`)
    const data = await response.json()
    // 注意：目前 /api/config/ui 不包含 image_gen 配置，使用默认值
    config = data || {}
  }
  catch (e) {
    // 使用默认配置
  }

  // 默认启用图片生成（因为公开配置中没有此字段）
  const enabled = true
  if (!enabled) {
    return '' // 返回空字符串表示不生成
  }

  // 使用默认配置参数（公开API不包含这些配置）
  const model = 'flux'
  const width = 512
  const height = 512

  // 验证尺寸参数
  const validWidth = Math.max(256, Math.min(width, 2048))
  const validHeight = Math.max(256, Math.min(height, 2048))

  // 生成提示词（调用后端 AI）
  const prompt = await generateGhibliPrompt(title, summary, category)

  // 使用 cardId 作为 seed 确保每张卡片生成不同的图片
  const seed = cardId * 1000 + Date.now() % 1000

  // 生成图片 URL
  const imageUrl = generateImageUrl(prompt, {
    model: model as any,
    width: validWidth,
    height: validHeight,
    seed,
    nologo: true,
    enhance: true,
  })

  // 保存到缓存
  const illustration: CardIllustration = {
    url: imageUrl,
    prompt,
    timestamp: Date.now(),
    cardId,
  }
  saveIllustration(cardId, illustration)

  return imageUrl
}

// 清除过期缓存
export function clearExpiredIllustrations(): void {
  const cached = getCachedIllustrations()
  if (cached.size > 0) {
    const data: Record<number, CardIllustration> = {}
    cached.forEach((value, key) => {
      data[key] = value
    })
    localStorage.setItem(IMAGE_CACHE_KEY, JSON.stringify(data))
  }
}
