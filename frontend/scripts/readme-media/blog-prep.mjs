// Puts the instance's blog into the state one language's blog recording starts from:
// `node blog-prep.mjs <zh|ja|en>`. Deletes every note on the instance.
import { execFileSync } from 'node:child_process'
import process from 'node:process'
import { BLOG, OWNER } from './content.mjs'
import { START } from './render.mjs'
import { ownerApi } from './session.mjs'

const L = process.argv[2] || 'zh'
const db = process.env.README_MEDIA_DATABASE_URL
if (!db) throw new Error('README_MEDIA_DATABASE_URL is not set')
const call = await ownerApi()
// The blog byline is the owner's display name, which has no owner-facing endpoint.
execFileSync('psql', ['-q', db, '-c', `UPDATE users SET display_name = '${OWNER[L].name.replace(/'/g, "''")}' WHERE id = 1`])
for (const d of (await call('GET', '/api/phantasi/notes/docs')).docs) {
  if (d.item_id) await call('DELETE', `/api/phantasi/notes/${d.item_id}`).catch((e) => console.log(e.message))
  await call('DELETE', `/api/phantasi/notes/docs/${d.id}`).catch((e) => console.log(e.message))
}
await call('PUT', '/api/blog/settings', { theme_id: 'myriad-default', site_title: 'Myriad', description: BLOG[L].description, params: {} })
for (const [title, content_md, daysAgo] of BLOG[L].posts) {
  await call('POST', '/api/phantasi/notes', { title, content_md, published_at: START.getTime() - daysAgo * 86400000 })
}
const before = (await call('GET', '/api/blog/settings')).build?.last_built_at
await call('POST', '/api/blog/rebuild')
for (let i = 0; i < 60; i++) {
  await new Promise((r) => setTimeout(r, 1000))
  const b = (await call('GET', '/api/blog/settings')).build
  if (b?.status === 'ok' && b.last_built_at !== before) break
  if (i === 59) throw new Error('blog rebuild did not finish')
}
console.log('blog ready:', (await call('GET', '/api/phantasi/notes/docs')).docs.map((d) => d.title).join(' | '))
