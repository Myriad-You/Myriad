import type { Config, ConfigField } from './types'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  agentSlicePersistPayload,
  omitAgentOwnedFields,
  pickAgentSlice,
  resetAgentSlice,
} from './agentBagSlice.ts'
import { DEFAULT_AUTO_FETCH_CONFIG } from './defaults.ts'

function field(key: string, value = 'custom'): ConfigField {
  return {
    key,
    value,
    label: key,
    field_type: 'text',
    required: false,
    placeholder: '',
  }
}

function config(): Config {
  return {
    platforms: [],
    auto_fetch: DEFAULT_AUTO_FETCH_CONFIG,
    ai_config: {
      config_fields: [
        field('gemini_model'),
        field('lite_ai_model', 'lite/model'),
        field('pro_enabled', 'true'),
        field('qq_bot_enabled', 'true'),
        field('telegram_bot_token', 'secret'),
      ],
    },
    tripo_config: { config_fields: [] },
    ui_config: {
      config_fields: [
        field('site_title', 'Myriad'),
        field('merope_enabled', 'true'),
        field('merope_speech_enabled', 'true'),
      ],
    },
  }
}

describe('agent bag slice', () => {
  it('picks Agent-owned keys plus read-only model gates', () => {
    const slice = pickAgentSlice(config())
    assert.deepEqual(
      slice.aiFields.map((item) => item.key),
      [
        'lite_ai_model',
        'pro_enabled',
        'qq_bot_enabled',
        'telegram_bot_token',
      ],
    )
    assert.deepEqual(
      slice.uiFields.map((item) => item.key),
      ['merope_enabled', 'merope_speech_enabled'],
    )
  })

  it('persists only Agent-owned keys so site title and models stay put', () => {
    const payload = agentSlicePersistPayload(pickAgentSlice(config()))
    assert.deepEqual(payload.platforms, [])
    assert.equal('auto_fetch' in payload, false)
    assert.deepEqual(
      payload.ai_config.config_fields.map((item) => item.key),
      ['qq_bot_enabled', 'telegram_bot_token'],
    )
    assert.deepEqual(
      payload.ui_config.config_fields.map((item) => item.key),
      ['merope_enabled', 'merope_speech_enabled'],
    )
  })

  it('resets Agent keys and leaves Lite/Pro gates alone', () => {
    const reset = resetAgentSlice(pickAgentSlice(config()))
    assert.equal(
      reset.aiFields.find((item) => item.key === 'qq_bot_enabled')?.value,
      'false',
    )
    assert.equal(
      reset.aiFields.find((item) => item.key === 'telegram_bot_token')?.value,
      '',
    )
    assert.equal(
      reset.aiFields.find((item) => item.key === 'lite_ai_model')?.value,
      'lite/model',
    )
    assert.equal(
      reset.uiFields.find((item) => item.key === 'merope_enabled')?.value,
      'false',
    )
  })

  it('strips Agent keys from the site bag so /config cannot rewrite them', () => {
    const next = omitAgentOwnedFields(config())
    assert.deepEqual(
      next.ai_config.config_fields.map((item) => item.key),
      ['gemini_model', 'lite_ai_model', 'pro_enabled'],
    )
    assert.deepEqual(
      next.ui_config.config_fields.map((item) => item.key),
      ['site_title'],
    )
    assert.equal(next.ui_config.config_fields[0]?.value, 'Myriad')
  })
})
