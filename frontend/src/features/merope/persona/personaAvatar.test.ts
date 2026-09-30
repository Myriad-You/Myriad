import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'
import {
  PERSONA_STICKER_FALLBACK,
  personaStickerAvatarFromConfig,
} from './personaAvatar.ts'

test('reads the sticker address out of the public config', () => {
  assert.equal(
    personaStickerAvatarFromConfig({
      agentPersonaAvatarUrl: '/uploads/sticker.png',
    }),
    '/uploads/sticker.png',
  )
})

test('trims, because a padded value still has to match a real asset path', () => {
  assert.equal(
    personaStickerAvatarFromConfig({
      agentPersonaAvatarUrl: '  /uploads/sticker.png  ',
    }),
    '/uploads/sticker.png',
  )
})

/** 人设关掉时后端不给这个字段。少一个字段和给空串必须是同一个结果， 否则通知图标会去加载一个空 src，浏览器把它解析成当前页地址再画成裂图。 */
test('an absent, empty, or non-string field all read as no avatar', () => {
  for (const config of [
    {},
    { agentPersonaAvatarUrl: '' },
    { agentPersonaAvatarUrl: '   ' },
    { agentPersonaAvatarUrl: null },
    { agentPersonaAvatarUrl: 42 },
    null,
    undefined,
    'not an object',
  ]) {
    assert.equal(personaStickerAvatarFromConfig(config), null)
  }
})

test('only the agent notification source follows the persona', () => {
  const source = readFileSync(
    new URL('../../../components/notifications/NotificationIcons.tsx', import.meta.url),
    'utf8',
  )
  assert.match(source, /if \(source === 'agent'\) \{\s*return resolvedPersonaStickerAvatar\(\)/u)
  assert.match(source, /satisfies Record<NotificationSourceKey, string>/u)
})

test('settings Agent icon uses the same sticker fallback as notifications', () => {
  assert.equal(PERSONA_STICKER_FALLBACK, '/logo.webp')
  const icon = readFileSync(
    new URL('../../../components/config/MyriadConfigIcon.tsx', import.meta.url),
    'utf8',
  )
  assert.match(icon, /kind === 'agent' \? resolvedPersonaStickerAvatar\(\)/)
  assert.match(icon, /agent: PERSONA_STICKER_FALLBACK/)
  assert.doesNotMatch(icon, /agent: '\/logo\.webp'/)
})

test('persona updates refresh the sticker cache', () => {
  const source = readFileSync(new URL('./personaAvatar.ts', import.meta.url), 'utf8')
  assert.match(source, /PERSONA_UPDATED_EVENT/)
  assert.match(source, /refreshPersonaStickerAvatar/)
})

test('agent notification toasts use the source icon', () => {
  const panel = readFileSync(
    new URL('../../../components/ControlPanel/useControlPanelNotifications.tsx', import.meta.url),
    'utf8',
  )
  const toast = readFileSync(
    new URL('../../../components/Toast.tsx', import.meta.url),
    'utf8',
  )
  assert.match(panel, /icon: notificationSourceIconAsset\(source\)/)
  assert.match(toast, /typeof icon === 'string'/)
})
