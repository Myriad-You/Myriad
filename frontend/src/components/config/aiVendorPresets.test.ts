import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  AI_VENDOR_PRESETS,
  apiFormatForSource,
  credentialModeForSource,
  findVendorPreset,
  isAgoraSource,
  isMiniMaxSpeechSource,
  resolveUsedVendorSlug,
  sharedKeyRefForSource,
  sourceFromCustom,
  sourceFromPreset,
  speechProviderKindFromSource,
  vendorSupports,
} from './aiVendorPresets'

describe('AI vendor presets', () => {
  it('lists the vendor picker presets', () => {
    assert.deepEqual(
      AI_VENDOR_PRESETS.map((item) => item.id),
      [
        'openrouter',
        'openai',
        'azureOpenAI',
        'gemini',
        'anthropic',
        'deepseek',
        'volcengine',
        'dashscope',
        'moonshot',
        'zhipu',
        'siliconflow',
        'groq',
        'xai',
        'mistral',
        'together',
        'fireworks',
        'perplexity',
        'minimax',
        'agora',
        'ollama',
        'cloudflare',
        'cohere',
        'nvidia',
        'tencentHunyuan',
        'openaiCompatible',
        'tencent',
      ],
    )
  })

  it('keeps named compatible vendors off image and speech pickers unless their API matches', () => {
    assert.equal(
      vendorSupports({ kind: 'openai_compatible', preset: 'deepseek' }, 'text'),
      true,
    )
    assert.equal(
      vendorSupports({ kind: 'openai_compatible', preset: 'deepseek' }, 'image'),
      false,
    )
    assert.equal(
      vendorSupports(
        { kind: 'openai_compatible', preset: 'openaiCompatible' },
        'speech',
      ),
      true,
    )
    assert.equal(
      vendorSupports({ kind: 'openai_compatible', preset: 'siliconflow' }, 'image'),
      true,
    )
    assert.equal(
      vendorSupports({ kind: 'openai_compatible', preset: 'groq' }, 'speech'),
      true,
    )
    assert.equal(
      vendorSupports({ kind: 'openai_compatible', preset: 'groq' }, 'image'),
      false,
    )
    assert.equal(
      vendorSupports({ kind: 'openai_compatible', preset: 'xai' }, 'image'),
      true,
    )
    assert.equal(
      vendorSupports({ kind: 'volcengine', preset: 'volcengine' }, 'text'),
      true,
    )
    assert.equal(vendorSupports({ kind: 'gemini', preset: 'gemini' }, 'image'), true)
    assert.equal(vendorSupports({ kind: 'gemini', preset: 'gemini' }, 'speech'), true)
    assert.equal(
      vendorSupports({ kind: 'openai_compatible', preset: 'minimax' }, 'speech'),
      true,
    )
    assert.equal(
      vendorSupports({ kind: 'openai_compatible', preset: 'minimax' }, 'image'),
      false,
    )
    assert.equal(
      vendorSupports({ kind: 'agora', preset: 'agora' }, 'realtime'),
      true,
    )
    assert.equal(vendorSupports({ kind: 'agora', preset: 'agora' }, 'speech'), false)
  })

  it('declares text, image, and speech from endpoints this stack can call', () => {
    const caps = Object.fromEntries(
      AI_VENDOR_PRESETS.map((item) => [item.id, item.capabilities.toSorted()]),
    )
    assert.deepEqual(caps, {
      openrouter: ['image', 'speech', 'text'],
      openai: ['image', 'speech', 'text'],
      azureOpenAI: ['image', 'speech', 'text'],
      gemini: ['image', 'speech', 'text'],
      anthropic: ['text'],
      deepseek: ['text'],
      volcengine: ['image', 'text'],
      dashscope: ['text'],
      moonshot: ['text'],
      zhipu: ['text'],
      siliconflow: ['image', 'speech', 'text'],
      groq: ['speech', 'text'],
      xai: ['image', 'text'],
      mistral: ['text'],
      together: ['image', 'text'],
      fireworks: ['text'],
      perplexity: ['text'],
      minimax: ['speech', 'text'],
      agora: ['realtime'],
      ollama: ['text'],
      cloudflare: ['text'],
      cohere: ['text'],
      nvidia: ['text'],
      tencentHunyuan: ['text'],
      openaiCompatible: ['image', 'speech', 'text'],
      tencent: ['speech'],
    })
  })

  it('resolves a second copy by slug suffix', () => {
    assert.equal(
      findVendorPreset({ slug: 'deepseek-2', kind: 'openai_compatible' })?.id,
      'deepseek',
    )
  })

  it('stamps preset id onto a new source', () => {
    const deepseek = AI_VENDOR_PRESETS.find((item) => item.id === 'deepseek')
    assert.ok(deepseek)
    const source = sourceFromPreset(deepseek, [])
    assert.equal(source.preset, 'deepseek')
    assert.equal(source.kind, 'openai_compatible')
    assert.equal(source.display_name, 'DeepSeek')
    assert.equal(source.base_url, 'https://api.deepseek.com/v1')
    const second = sourceFromPreset(deepseek, [source])
    assert.equal(second.slug, 'deepseek-2')
    assert.equal(second.display_name, 'DeepSeek 2')
  })

  it('tags usage by slug when the same vendor has two sources', () => {
    const first = sourceFromPreset(
      AI_VENDOR_PRESETS.find((item) => item.id === 'openai')!,
      [],
    )
    const second = sourceFromPreset(
      AI_VENDOR_PRESETS.find((item) => item.id === 'openai')!,
      [first],
    )
    assert.equal(resolveUsedVendorSlug(second.slug, [first, second]), 'openai-2')
    assert.equal(resolveUsedVendorSlug('openai', [first, second]), 'openai')
    assert.equal(resolveUsedVendorSlug('openai', [first]), 'openai')
    assert.equal(resolveUsedVendorSlug('openai', [second]), 'openai-2')
    const work = { ...first, slug: 'openai-work' }
    assert.equal(resolveUsedVendorSlug('openai', [work, second]), '')
  })

  it('maps MiniMax vendor sources onto the T2A speech provider', () => {
    assert.equal(
      isMiniMaxSpeechSource({
        kind: 'openai_compatible',
        preset: 'minimax',
        slug: 'minimax',
        base_url: 'https://api.minimaxi.com/v1',
      }),
      true,
    )
    assert.equal(
      speechProviderKindFromSource(
        {
          kind: 'openai_compatible',
          preset: 'minimax',
          slug: 'minimax',
        },
        'openai',
      ),
      'minimax',
    )
    assert.equal(
      speechProviderKindFromSource({ kind: 'tencent', slug: 'tencent' }, 'tencent'),
      'tencent',
    )
    assert.equal(
      isAgoraSource({ kind: 'agora', preset: 'agora', slug: 'agora' }),
      true,
    )
  })
})

// Protocol selection must survive creating and serializing a vendor source.
it('creates native Anthropic and Gemini sources with their wire protocols', () => {
  for (const [id, protocol] of [
    ['anthropic', 'anthropic'],
    ['gemini', 'gemini'],
  ]) {
    const preset = AI_VENDOR_PRESETS.find((item) => item.id === id)!
    const serialized = JSON.stringify(sourceFromPreset(preset, []))
    const source = JSON.parse(serialized)
    assert.equal(source.api_format, protocol)
  }
})

it('reads the stored credential mode and never guesses one', () => {
  assert.equal(apiFormatForSource({ kind: 'gemini' }), 'gemini')
  assert.equal(apiFormatForSource({ kind: 'openai' }), 'openai')
  assert.equal(apiFormatForSource({ kind: 'gemini', api_format: 'openai' }), 'openai')
  const custom = { kind: 'openai', base_url: 'https://proxy.example/v1' }
  assert.equal(credentialModeForSource(custom), 'none')
  assert.equal(sharedKeyRefForSource(custom), null)
  assert.equal(credentialModeForSource({ ...custom, credential_mode: 'own', api_key: 'key' }), 'own')
  assert.equal(sharedKeyRefForSource({ ...custom, credential_mode: 'shared', shared_key_ref: 'openai' }), 'openai')
  assert.equal(sharedKeyRefForSource({ kind: 'gemini' }), null)
})

it('creates independent custom sources with explicit unauthenticated mode', () => {
  const first = sourceFromCustom([], 'Local')
  const second = sourceFromCustom([first], 'Other')
  assert.notEqual(first.slug, second.slug)
  assert.equal(first.credential_mode, 'none')
  assert.equal(first.shared_key_ref, null)
  assert.equal(vendorSupports(first, 'text'), true)
  assert.equal(vendorSupports(first, 'image'), false)
})
