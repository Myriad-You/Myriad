export type AiVendorKind =
  | 'openrouter'
  | 'openai'
  | 'openai_compatible'
  | 'gemini'
  | 'volcengine'
  | 'tencent'
  | 'agora'
  | 'anthropic'
  | 'custom'

export type AiApiFormat =
  | 'openai'
  | 'openai_responses'
  | 'anthropic'
  | 'gemini'

export type AiVendorCapability = 'text' | 'image' | 'speech' | 'realtime'

export interface AiVendorSource {
  slug: string
  kind: AiVendorKind | string
  display_name: string
  enabled: boolean
  preset?: string
  api_format?: AiApiFormat | string
  api_key?: string | null
  base_url?: string
  secret_id?: string | null
  secret_key?: string | null
  region?: string | null
  app_id?: string | null
}

export interface AiVendorPreset {
  id: string
  defaultSlug: string
  kind: AiVendorKind
  api_format?: AiApiFormat
  display_name: string
  base_url: string
  docs_url?: string
  capabilities: AiVendorCapability[]
  keyPlaceholder?: string
  defaultTextModel?: string
  defaultImageModel?: string
  defaultSttModel?: string
  defaultTtsModel?: string
  defaultVoice?: string
}

export const AI_VENDOR_PRESETS: AiVendorPreset[] = [
  {
    id: 'openrouter',
    defaultSlug: 'openrouter',
    kind: 'openrouter',
    api_format: 'openai',
    display_name: 'OpenRouter',
    base_url: 'https://openrouter.ai/api/v1',
    docs_url: 'https://openrouter.ai/docs',
    capabilities: ['text', 'image', 'speech'],
    keyPlaceholder: 'sk-or-v1-...',
    defaultTextModel: 'minimax/minimax-m3',
    defaultImageModel: 'openai/gpt-image-2',
    defaultSttModel: 'openai/gpt-transcribe',
    defaultTtsModel: '',
    defaultVoice: 'marin',
  },
  {
    id: 'openai',
    defaultSlug: 'openai',
    kind: 'openai',
    api_format: 'openai_responses',
    display_name: 'OpenAI',
    base_url: 'https://api.openai.com/v1',
    docs_url: 'https://platform.openai.com/docs',
    capabilities: ['text', 'image', 'speech'],
    keyPlaceholder: 'sk-...',
    defaultTextModel: 'gpt-5.6-terra',
    defaultImageModel: 'gpt-image-2',
    defaultSttModel: 'gpt-transcribe',
    defaultTtsModel: 'gpt-4o-mini-tts',
    defaultVoice: 'marin',
  },
  {
    id: 'azureOpenAI',
    defaultSlug: 'azure-openai',
    kind: 'openai_compatible',
    api_format: 'openai',
    display_name: 'Azure OpenAI',
    base_url: '',
    docs_url: 'https://learn.microsoft.com/azure/ai-services/openai/',
    capabilities: ['text', 'image', 'speech'],
    keyPlaceholder: '',
    defaultImageModel: 'gpt-image-1',
    defaultSttModel: 'gpt-transcribe',
    defaultTtsModel: 'gpt-4o-mini-tts',
    defaultVoice: 'marin',
  },
  {
    id: 'gemini',
    defaultSlug: 'gemini',
    kind: 'gemini',
    api_format: 'gemini',
    display_name: 'Gemini',
    base_url: '',
    docs_url: 'https://ai.google.dev/gemini-api/docs',
    capabilities: ['text', 'image', 'speech'],
    keyPlaceholder: 'AIza...',
    defaultTextModel: 'gemini-3.6-flash',
    defaultImageModel: 'gemini-3.1-flash-image',
    defaultSttModel: 'gemini-3.6-flash',
    defaultTtsModel: 'gemini-2.5-flash-preview-tts',
    defaultVoice: 'Kore',
  },
  {
    id: 'anthropic',
    defaultSlug: 'anthropic',
    kind: 'anthropic',
    api_format: 'anthropic',
    display_name: 'Anthropic',
    base_url: 'https://api.anthropic.com/v1',
    docs_url: 'https://docs.anthropic.com/',
    capabilities: ['text'],
    keyPlaceholder: 'sk-ant-...',
  },
  {
    id: 'deepseek',
    defaultSlug: 'deepseek',
    kind: 'openai_compatible',
    display_name: 'DeepSeek',
    base_url: 'https://api.deepseek.com/v1',
    docs_url: 'https://api-docs.deepseek.com/',
    capabilities: ['text'],
    defaultTextModel: 'deepseek-chat',
  },
  {
    id: 'volcengine',
    defaultSlug: 'volcengine',
    kind: 'volcengine',
    display_name: 'Volcengine',
    base_url: 'https://ark.cn-beijing.volces.com/api/v3',
    docs_url: 'https://www.volcengine.com/docs/82379',
    capabilities: ['text', 'image'],
    defaultImageModel: 'doubao-seedream-5-0-260128',
  },
  {
    id: 'dashscope',
    defaultSlug: 'dashscope',
    kind: 'openai_compatible',
    display_name: 'DashScope',
    base_url: 'https://dashscope.aliyuncs.com/compatible-mode/v1',
    docs_url: 'https://help.aliyun.com/zh/model-studio/',
    capabilities: ['text'],
    defaultTextModel: 'qwen-plus',
  },
  {
    id: 'moonshot',
    defaultSlug: 'moonshot',
    kind: 'openai_compatible',
    display_name: 'Moonshot',
    base_url: 'https://api.moonshot.cn/v1',
    docs_url: 'https://platform.moonshot.cn/docs',
    capabilities: ['text'],
  },
  {
    id: 'zhipu',
    defaultSlug: 'zhipu',
    kind: 'openai_compatible',
    display_name: 'Zhipu',
    base_url: 'https://open.bigmodel.cn/api/paas/v4',
    docs_url: 'https://docs.bigmodel.cn/',
    capabilities: ['text'],
    defaultTextModel: 'glm-4-flash',
  },
  {
    id: 'siliconflow',
    defaultSlug: 'siliconflow',
    kind: 'openai_compatible',
    display_name: 'SiliconFlow',
    base_url: 'https://api.siliconflow.cn/v1',
    docs_url: 'https://docs.siliconflow.cn/',
    capabilities: ['text', 'image', 'speech'],
    defaultImageModel: 'Kwai-Kolors/Kolors',
    defaultSttModel: 'FunAudioLLM/SenseVoiceSmall',
    defaultTtsModel: 'FunAudioLLM/CosyVoice2-0.5B',
    defaultVoice: 'FunAudioLLM/CosyVoice2-0.5B:alex',
  },
  {
    id: 'groq',
    defaultSlug: 'groq',
    kind: 'openai_compatible',
    display_name: 'Groq',
    base_url: 'https://api.groq.com/openai/v1',
    docs_url: 'https://console.groq.com/docs',
    capabilities: ['text', 'speech'],
    defaultTextModel: 'llama-3.3-70b-versatile',
    defaultSttModel: 'whisper-large-v3-turbo',
    defaultTtsModel: 'canopylabs/orpheus-v1-english',
    defaultVoice: 'troy',
  },
  {
    id: 'xai',
    defaultSlug: 'xai',
    kind: 'openai_compatible',
    display_name: 'xAI',
    base_url: 'https://api.x.ai/v1',
    docs_url: 'https://docs.x.ai/',
    capabilities: ['text', 'image'],
    defaultTextModel: 'grok-3',
    defaultImageModel: 'grok-imagine-image-2.0',
  },
  {
    id: 'mistral',
    defaultSlug: 'mistral',
    kind: 'openai_compatible',
    display_name: 'Mistral',
    base_url: 'https://api.mistral.ai/v1',
    docs_url: 'https://docs.mistral.ai/',
    capabilities: ['text'],
    defaultTextModel: 'mistral-large-latest',
  },
  {
    id: 'together',
    defaultSlug: 'together',
    kind: 'openai_compatible',
    display_name: 'Together',
    base_url: 'https://api.together.xyz/v1',
    docs_url: 'https://docs.together.ai/',
    capabilities: ['text', 'image'],
    defaultImageModel: 'black-forest-labs/FLUX.1-schnell',
  },
  {
    id: 'fireworks',
    defaultSlug: 'fireworks',
    kind: 'openai_compatible',
    display_name: 'Fireworks',
    base_url: 'https://api.fireworks.ai/inference/v1',
    docs_url: 'https://docs.fireworks.ai/',
    capabilities: ['text'],
  },
  {
    id: 'perplexity',
    defaultSlug: 'perplexity',
    kind: 'openai_compatible',
    display_name: 'Perplexity',
    base_url: 'https://api.perplexity.ai',
    docs_url: 'https://docs.perplexity.ai/',
    capabilities: ['text'],
    defaultTextModel: 'sonar',
  },
  {
    id: 'minimax',
    defaultSlug: 'minimax',
    kind: 'openai_compatible',
    display_name: 'MiniMax',
    base_url: 'https://api.minimaxi.com/v1',
    docs_url: 'https://platform.minimaxi.com/document/',
    capabilities: ['text', 'speech'],
    defaultTtsModel: 'speech-2.8-turbo',
    defaultVoice: 'female-shaonv',
  },
  {
    id: 'agora',
    defaultSlug: 'agora',
    kind: 'agora',
    display_name: 'Shengwang / Agora',
    base_url: 'https://api.agora.io/cn',
    docs_url: 'https://www.shengwang.cn/ConversationalAI/',
    capabilities: ['realtime'],
  },
  {
    id: 'ollama',
    defaultSlug: 'ollama',
    kind: 'openai_compatible',
    display_name: 'Ollama',
    base_url: 'http://127.0.0.1:11434/v1',
    docs_url: 'https://github.com/ollama/ollama',
    capabilities: ['text'],
    keyPlaceholder: 'ollama',
    defaultTextModel: 'llama3.2',
  },
  {
    id: 'cloudflare',
    defaultSlug: 'cloudflare',
    kind: 'openai_compatible',
    display_name: 'Cloudflare',
    base_url: '',
    docs_url: 'https://developers.cloudflare.com/workers-ai/',
    capabilities: ['text'],
  },
  {
    id: 'cohere',
    defaultSlug: 'cohere',
    kind: 'openai_compatible',
    display_name: 'Cohere',
    base_url: 'https://api.cohere.ai/compatibility/v1',
    docs_url: 'https://docs.cohere.com/',
    capabilities: ['text'],
  },
  {
    id: 'nvidia',
    defaultSlug: 'nvidia',
    kind: 'openai_compatible',
    display_name: 'NVIDIA',
    base_url: 'https://integrate.api.nvidia.com/v1',
    docs_url: 'https://docs.nvidia.com/nim/',
    capabilities: ['text'],
  },
  {
    id: 'tencentHunyuan',
    defaultSlug: 'hunyuan',
    kind: 'openai_compatible',
    display_name: 'Tencent Hunyuan',
    base_url: 'https://api.hunyuan.cloud.tencent.com/v1',
    docs_url: 'https://cloud.tencent.com/document/product/1729',
    capabilities: ['text'],
  },
  {
    id: 'openaiCompatible',
    defaultSlug: 'openai-compatible',
    kind: 'openai_compatible',
    display_name: 'OpenAI Compatible',
    base_url: '',
    capabilities: ['text', 'image', 'speech'],
    keyPlaceholder: 'sk-...',
    defaultSttModel: 'gpt-transcribe',
    defaultTtsModel: 'gpt-4o-mini-tts',
    defaultVoice: 'marin',
  },
  {
    id: 'tencent',
    defaultSlug: 'tencent',
    kind: 'tencent',
    display_name: 'Tencent Cloud',
    base_url: '',
    docs_url: 'https://cloud.tencent.com/document/product/1073',
    capabilities: ['speech'],
  },
]

function slugBase(slug: string): string {
  return slug.trim().replaceAll(/-\d+$/g, '')
}

export function findVendorPreset(source: {
  kind?: string
  slug?: string
  preset?: string | null
}): AiVendorPreset | undefined {
  const presetId = source.preset?.trim()
  if (presetId) {
    const byId = AI_VENDOR_PRESETS.find((item) => item.id === presetId)
    if (byId) return byId
  }
  const slug = source.slug?.trim() ?? ''
  if (slug) {
    const exact = AI_VENDOR_PRESETS.find(
      (item) => item.id === slug || item.defaultSlug === slug,
    )
    if (exact) return exact
    const base = slugBase(slug)
    const prefixed = AI_VENDOR_PRESETS.find(
      (item) => item.id === base || item.defaultSlug === base,
    )
    if (prefixed) return prefixed
  }
  return undefined
}

export function isAgoraSource(source: {
  kind?: string
  slug?: string
  preset?: string | null
}): boolean {
  const preset = findVendorPreset(source)
  if (preset?.id === 'agora') return true
  const kind = source.kind?.trim().toLowerCase() ?? ''
  if (kind === 'agora') return true
  const slug = source.slug?.trim().toLowerCase() ?? ''
  return slug === 'agora' || slug.startsWith('agora-')
}

export function isMiniMaxSpeechSource(source: {
  kind?: string
  slug?: string
  preset?: string | null
  base_url?: string
}): boolean {
  const preset = findVendorPreset(source)
  if (preset?.id === 'minimax') return true
  const kind = source.kind?.trim().toLowerCase() ?? ''
  if (kind === 'minimax') return true
  const slug = source.slug?.trim().toLowerCase() ?? ''
  if (slug === 'minimax' || slug.startsWith('minimax-')) return true
  const host = source.base_url?.trim().toLowerCase() ?? ''
  return (
    host.includes('minimaxi.com') ||
    host.includes('minimax.io') ||
    host.includes('minimax.chat')
  )
}

export function speechProviderKindFromSource(
  source:
    | {
        kind?: string
        slug?: string
        preset?: string | null
        base_url?: string
      }
    | undefined,
  fallback: string,
): string {
  if (
    isMiniMaxSpeechSource(
      source ?? { kind: fallback, slug: fallback, preset: fallback },
    )
  ) {
    return 'minimax'
  }
  const kind = source?.kind || fallback
  if (kind === 'tencent') return 'tencent'
  if (kind === 'openrouter') return 'openrouter'
  if (kind === 'gemini') return 'gemini'
  return 'openai'
}

export function vendorSupports(
  kindOrSource:
    | string
    | {
        kind: string
        slug?: string
        preset?: string | null
        api_format?: string
      },
  capability: AiVendorCapability,
): boolean {
  const source =
    typeof kindOrSource === 'string' ? { kind: kindOrSource } : kindOrSource
  const preset = findVendorPreset(source)
  if (preset) return preset.capabilities.includes(capability)
  if (source.api_format && capability === 'text') return true
  switch (source.kind) {
    case 'openrouter':
    case 'openai':
      return true
    case 'openai_compatible':
      return capability === 'text'
    case 'gemini':
      return true
    case 'volcengine':
      return capability === 'text' || capability === 'image'
    case 'tencent':
      return capability === 'speech'
    case 'agora':
      return capability === 'realtime'
    default:
      return false
  }
}

export function resolveUsedVendorSlug(
  raw: string,
  sources: AiVendorSource[],
): string {
  const value = raw.trim()
  if (!value) return ''
  if (sources.some((item) => item.slug === value)) return value
  const kindMatches = sources.filter(
    (item) => item.preset === value || item.kind === value,
  )
  return kindMatches.length === 1 ? kindMatches[0].slug : ''
}

export function parseVendorSources(raw: string): AiVendorSource[] {
  if (!raw.trim()) return []
  try {
    const parsed = JSON.parse(raw) as unknown
    if (!Array.isArray(parsed)) return []
    return parsed.filter((item): item is AiVendorSource =>
      Boolean(
        item && typeof item === 'object' && typeof item.slug === 'string',
      ),
    )
  } catch {
    return []
  }
}

export function sourceFromPreset(
  preset: AiVendorPreset,
  existing: AiVendorSource[],
): AiVendorSource {
  const slug = uniqueVendorSlug(preset.defaultSlug, existing)
  const copy =
    slug === preset.defaultSlug
      ? ''
      : slug.slice(preset.defaultSlug.length).replaceAll(/^-+/g, '')
  return {
    slug,
    kind: preset.kind,
    display_name: copy ? `${preset.display_name} ${copy}` : preset.display_name,
    enabled: true,
    preset: preset.id,
    api_format: preset.api_format ?? defaultApiFormat(preset),
    api_key: '',
    base_url: preset.base_url,
    secret_id: '',
    secret_key: '',
    region: preset.kind === 'tencent' ? 'ap-guangzhou' : '',
    app_id: '',
  }
}

export function sourceFromCustom(
  existing: AiVendorSource[],
  displayName: string,
): AiVendorSource {
  return {
    slug: uniqueVendorSlug('custom', existing),
    kind: 'custom',
    display_name: displayName,
    enabled: true,
    preset: '',
    api_format: 'openai',
    api_key: '',
    base_url: '',
    secret_id: '',
    secret_key: '',
    region: '',
    app_id: '',
  }
}

export function apiFormatForSource(
  source: Pick<AiVendorSource, 'kind' | 'preset' | 'api_format'>,
): AiApiFormat {
  const explicit = source.api_format?.trim()
  if (
    explicit === 'openai' ||
    explicit === 'openai_responses' ||
    explicit === 'anthropic' ||
    explicit === 'gemini'
  ) {
    return explicit
  }
  if (source.kind === 'gemini') return 'gemini'
  if (source.kind === 'anthropic' || source.preset === 'anthropic') return 'anthropic'
  return 'openai'
}

function defaultApiFormat(preset: AiVendorPreset): AiApiFormat {
  if (preset.kind === 'gemini') return 'gemini'
  if (preset.id === 'anthropic') return 'anthropic'
  return 'openai'
}

function uniqueVendorSlug(base: string, existing: AiVendorSource[]): string {
  const seed = base.trim() || 'source'
  if (!existing.some((item) => item.slug === seed)) return seed
  let index = 2
  while (existing.some((item) => item.slug === `${seed}-${index}`)) {
    index += 1
  }
  return `${seed}-${index}`
}

export function defaultModelsForSource(
  source: { kind: string; slug?: string; preset?: string | null },
  capability: AiVendorCapability,
): {
  stt?: string
  tts?: string
  voice?: string
  text?: string
  image?: string
} {
  const preset = findVendorPreset(source)
  if (preset) {
    if (capability === 'image') return { image: preset.defaultImageModel }
    if (capability === 'speech') {
      return {
        stt: preset.defaultSttModel,
        tts: preset.defaultTtsModel,
        voice: preset.defaultVoice,
      }
    }
    return { text: preset.defaultTextModel }
  }
  if (capability === 'image') {
    if (source.kind === 'openai') return { image: 'gpt-image-2' }
    if (source.kind === 'volcengine')
      return { image: 'doubao-seedream-5-0-260128' }
    if (source.kind === 'gemini') return { image: 'gemini-3.1-flash-image' }
    return { image: 'openai/gpt-image-2' }
  }
  if (capability === 'speech') {
    if (source.kind === 'openrouter') {
      return { stt: 'openai/gpt-transcribe', tts: '', voice: 'marin' }
    }
    if (source.kind === 'gemini') {
      return {
        stt: 'gemini-3.6-flash',
        tts: 'gemini-2.5-flash-preview-tts',
        voice: 'Kore',
      }
    }
    if (source.kind === 'openai' || source.kind === 'openai_compatible') {
      return { stt: 'gpt-transcribe', tts: 'gpt-4o-mini-tts', voice: 'marin' }
    }
    return {}
  }
  if (source.kind === 'gemini') return { text: 'gemini-3.6-flash' }
  if (source.kind === 'openai') return { text: 'gpt-5.6-terra' }
  return { text: 'minimax/minimax-m3' }
}
