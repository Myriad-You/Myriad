import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'
import { personaStickerAvatarFromConfig } from './personaAvatar.ts'

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
    new URL('../../components/notifications/NotificationIcons.tsx', import.meta.url),
    'utf8',
  )
  assert.match(
    source,
    /if \(source === 'agent'\) \{\s*return personaStickerAvatarUrl\(\) \?\? NOTIFICATION_SOURCE_ICON_ASSETS\.agent/u,
  )
  assert.match(source, /satisfies Record<NotificationSourceKey, string>/u)
})
