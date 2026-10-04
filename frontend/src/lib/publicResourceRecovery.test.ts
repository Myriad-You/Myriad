import assert from 'node:assert/strict'
import { createRequire } from 'node:module'
import test from 'node:test'
import { compileFunction } from 'node:vm'

const require = createRequire(import.meta.url)
const { build } = createRequire(import.meta.resolve('tsx/package.json'))('esbuild')

async function loadResources(read: () => Promise<unknown>) {
  const bundle = await build({
    stdin: {
      contents: `export * as avatar from '../features/merope/persona/personaAvatar';
        export * as name from '../features/merope/persona/publicName';
        export * as social from '../components/widgets/reportCard/platformSocial';`,
      resolveDir: new URL('.', import.meta.url).pathname,
      loader: 'ts',
    },
    bundle: true, write: false, platform: 'node', format: 'cjs', packages: 'external',
    define: { 'import.meta.env': '{}' },
    plugins: [{ name: 'public-config-boundary', setup(builder) {
      builder.onResolve({ filter: /\/requestDedup$/ }, () => ({ path: 'config', namespace: 'test' }))
      builder.onLoad({ filter: /.*/, namespace: 'test' }, () => ({ contents: 'export const getPublicConfigDeduped = TestRead', loader: 'js' }))
    } }],
  })
  const module = { exports: {} as {
    avatar: typeof import('../features/merope/persona/personaAvatar')
    name: typeof import('../features/merope/persona/publicName')
    social: typeof import('../components/widgets/reportCard/platformSocial')
  } }
  compileFunction(bundle.outputFiles[0].text, ['require', 'module', 'exports', 'TestRead'])(require, module, module.exports, read)
  return module.exports
}

test('report account reads recover after failure and coalesce concurrent callers', async () => {
  let reads = 0
  let finish!: (value: unknown) => void
  const { social } = await loadResources(() => {
    reads++
    return reads === 1 ? Promise.reject(new Error('offline')) : new Promise(resolve => { finish = resolve })
  })
  assert.deepEqual(await social.fetchPlatformUserIds(), {})
  const first = social.fetchPlatformUserIds()
  const second = social.fetchPlatformUserIds()
  assert.equal(reads, 2)
  finish({ platforms: [{ name: 'Steam', config_fields: [{ key: 'steam_id', value: '123' }] }] })
  assert.deepEqual(await first, { steam: '123' })
  assert.deepEqual(await second, { steam: '123' })
  assert.deepEqual(await social.fetchPlatformUserIds(), { steam: '123' })
  assert.equal(reads, 2)
})

test('persona identity retains successful values on failure, but clears on a successful empty response', async () => {
  let fail = true
  let config: unknown = { agentPersonaAvatarUrl: '/portrait.png', agentPersonaName: 'Mira', meropeEnabled: true }
  const { avatar, name } = await loadResources(async () => {
    if (fail) throw new Error('offline')
    return config
  })
  const avatars: Array<string | null> = []
  const names: string[] = []
  const unwatchAvatar = avatar.onPersonaStickerAvatar(value => avatars.push(value))
  const unwatchName = name.onPersonaPublicName(value => names.push(value))
  try {
    await avatar.refreshPersonaStickerAvatar()
    await name.refreshPersonaPublicName()
    assert.deepEqual(avatars, [])
    assert.deepEqual(names, [])
    fail = false
    assert.equal(await avatar.refreshPersonaStickerAvatar(), '/portrait.png')
    assert.equal(await name.refreshPersonaPublicName(), 'Mira')
    fail = true
    assert.equal(await avatar.refreshPersonaStickerAvatar(), '/portrait.png')
    assert.equal(await name.refreshPersonaPublicName(), 'Mira')
    assert.equal(avatar.resolvedPersonaStickerAvatar(), '/portrait.png')
    assert.equal(name.personaPublicName(), 'Mira')
    assert.deepEqual(avatars, ['/portrait.png'])
    assert.deepEqual(names, ['Mira'])
    config = {}
    fail = false
    assert.equal(await avatar.refreshPersonaStickerAvatar(), null)
    assert.equal(await name.refreshPersonaPublicName(), 'Agent')
    assert.equal(avatar.resolvedPersonaStickerAvatar(), avatar.PERSONA_STICKER_FALLBACK)
    assert.deepEqual(avatars, ['/portrait.png', null])
    assert.deepEqual(names, ['Mira', 'Agent'])
  } finally {
    unwatchAvatar()
    unwatchName()
  }
})
