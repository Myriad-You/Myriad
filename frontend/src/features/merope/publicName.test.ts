import assert from 'node:assert/strict'
import test from 'node:test'
import {
  PERSONA_DEFAULT_NAME,
  PERSONA_OFF_NAME,
  publicPersonaName,
  publicPersonaNameFromConfig,
} from './publicName'

test('public persona name matches the site face rule', () => {
  assert.equal(publicPersonaName(false, '瞳'), PERSONA_OFF_NAME)
  assert.equal(publicPersonaName(false, ''), PERSONA_OFF_NAME)
  assert.equal(publicPersonaName(true, '  瞳  '), '瞳')
  assert.equal(publicPersonaName(true, '   '), PERSONA_DEFAULT_NAME)
  assert.equal(publicPersonaName(true, null), PERSONA_DEFAULT_NAME)
})

test('public config name wins; otherwise fall back by meropeEnabled', () => {
  assert.equal(
    publicPersonaNameFromConfig({
      meropeEnabled: false,
      agentPersonaName: 'Agent',
    }),
    PERSONA_OFF_NAME,
  )
  assert.equal(
    publicPersonaNameFromConfig({
      meropeEnabled: true,
      agentPersonaName: '瞳',
    }),
    '瞳',
  )
  assert.equal(
    publicPersonaNameFromConfig({ meropeEnabled: false }),
    PERSONA_OFF_NAME,
  )
  assert.equal(
    publicPersonaNameFromConfig({ meropeEnabled: true }),
    PERSONA_DEFAULT_NAME,
  )
})
