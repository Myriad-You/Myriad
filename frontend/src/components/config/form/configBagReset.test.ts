import type { Config, ConfigField } from './types'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { resetConfigBag } from './configBagReset'
import { createConfigDomain, executeConfigOperations } from './configDomain'

function fields(...keys: string[]): ConfigField[] {
  return keys.map((key) => ({
    key,
    value: 'custom',
    label: key,
    field_type: 'text',
    required: false,
    placeholder: '',
  }))
}
function config(): Config {
  return {
    platforms: [
      {
        name: 'Steam',
        enabled: true,
        has_token: true,
        config_fields: fields('username'),
        description: '',
        icon: '',
      },
    ],
    auto_fetch: { enabled: true, interval_hours: 1 },
    ui_config: {
      config_fields: fields(
        'base_url',
        'site_title',
        'proxy_url',
        'music_source',
        'merope_enabled',
        'analytics_enabled',
      ),
    },
    ai_config: { config_fields: fields('gemini_model', 'qq_bot_enabled') },
    tripo_config: { config_fields: fields('tripo_enabled', 'tripo_model') },
    report_config: { config_fields: [] },
  }
}
function value(
  bag: Config,
  group: 'ui_config' | 'ai_config' | 'tripo_config',
  key: string,
) {
  return bag[group].config_fields.find((field) => field.key === key)?.value
}

describe('config bag reset ownership', () => {
  it('lab resets only Tripo, while AI leaves Agent-owned fields alone', () => {
    const initial = config()
    const lab = resetConfigBag(initial, 'lab')!
    assert.ok(lab)
    assert.equal(value(lab, 'tripo_config', 'tripo_enabled'), 'false')
    assert.deepEqual(lab.ai_config, initial.ai_config)
    assert.deepEqual(lab.ui_config, initial.ui_config)
    const ai = resetConfigBag(initial, 'ai')!
    assert.ok(ai)
    assert.equal(value(ai, 'ai_config', 'gemini_model'), 'gemini-3.8-flash')
    assert.equal(value(ai, 'ai_config', 'qq_bot_enabled'), 'custom')
    assert.equal(resetConfigBag(initial, 'agent'), undefined)
  })
  it('restores usable AI defaults and clears credentials and source selections', () => {
    const expected = {
      provider: 'openai',
      gemini_model: 'gemini-3.8-flash',
      openai_model: 'minimax/minimax-m3',
      openai_base_url: 'https://openrouter.ai/api/v1',
      pro_enabled: 'false',
      pro_provider: 'openai',
      pro_gemini_model: 'gemini-3.1-pro-preview',
      pro_openai_model: 'anthropic/claude-opus-5.5',
      pro_openai_base_url: 'https://openrouter.ai/api/v1',
      lite_enabled: 'false',
      lite_provider: 'openai',
      lite_gemini_model: 'gemini-3.5-flash-lite',
      lite_openai_model: 'google/gemini-3.5-flash-lite',
      lite_openai_base_url: 'https://openrouter.ai/api/v1',
      ai_image_provider: 'openrouter',
      ai_image_model: 'openai/gpt-image-2.5-sunburst',
      ai_image_openai_base_url: 'https://api.openai.com/v1',
      ai_image_volcengine_base_url: 'https://ark.cn-beijing.volces.com/api/v3',
      tencent_region: 'ap-guangzhou',
      speech_provider: 'tencent',
      speech_openai_base_url: 'https://api.openai.com/v1',
      provider_openai_base_url: 'https://api.openai.com/v1',
      provider_volcengine_base_url: 'https://ark.cn-beijing.volces.com/api/v3',
      ai_vendor_sources: '[]',
      ai_source: '',
      lite_ai_source: '',
      pro_ai_source: '',
      ai_image_source: '',
      speech_source: '',
      speech_stt_model: '',
      speech_tts_model: '',
      speech_tts_voice: '',
      gemini_api_key: '',
      openai_api_key: '',
      pro_openai_api_key: '',
      lite_gemini_api_key: '',
      tencent_secret_key: '',
      provider_openrouter_api_key: '',
    }
    const initial = config()
    initial.ai_config.config_fields = fields(...Object.keys(expected))
    for (const scope of ['ai', 'all']) {
      const reset = resetConfigBag(initial, scope)!
      assert.ok(reset)
      assert.deepEqual(
        Object.fromEntries(
          reset.ai_config.config_fields.map(({ key, value }) => [key, value]),
        ),
        expected,
      )
    }
    assert.ok(
      initial.ai_config.config_fields.every(
        (field) => field.value === 'custom',
      ),
    )
  })
  it('full reset preserves the separately managed site URL and the input snapshot', () => {
    const initial = config()
    const reset = resetConfigBag(initial, 'all')!
    assert.ok(reset)
    assert.equal(value(reset, 'ui_config', 'base_url'), 'custom')
    assert.equal(value(reset, 'ui_config', 'merope_enabled'), 'custom')
    assert.equal(value(reset, 'ai_config', 'qq_bot_enabled'), 'custom')
    assert.equal(reset.platforms[0].enabled, false)
    assert.equal(reset.platforms[0].config_fields[0].value, '')
    assert.equal(value(initial, 'ui_config', 'site_title'), 'custom')
    assert.equal(initial.platforms[0].enabled, true)
  })
  it('actual bag arrays retain another page’s unsaved fields after page reset', async () => {
    const initial = config()
    const writes: Config[] = []
    const domain = createConfigDomain<Config | null>(() => ({
      id: 'bag',
      initial,
      ready: true,
      reset: resetConfigBag,
      persist: async (submitted) => {
        writes.push(submitted!)
        return submitted
      },
    }))
    domain.setDraft((draft) => ({
      ...draft!,
      ui_config: {
        config_fields: draft!.ui_config.config_fields.map((field) => ({
          ...field,
          value: field.key === 'proxy_url' ? 'unsaved proxy' : field.value,
        })),
      },
    }))
    await executeConfigOperations([domain], [domain.prepareReset('basic')!])
    assert.equal(value(writes[0], 'ui_config', 'proxy_url'), 'custom')
    assert.equal(
      value(domain.getSnapshot().draft!, 'ui_config', 'proxy_url'),
      'unsaved proxy',
    )
    assert.notEqual(
      value(domain.getSnapshot().draft!, 'ui_config', 'site_title'),
      'custom',
    )
    assert.equal(domain.getSnapshot().dirty, true)
  })
  it('pages with independent endpoints never acquire bag reset ownership', () => {
    for (const scope of [
      'oauth',
      'users',
      'permissions',
      'notifications',
      'federation',
      'about',
      'agent',
    ]) {
      assert.equal(resetConfigBag(config(), scope), undefined)
    }
  })
})
