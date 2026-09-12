import assert from 'node:assert/strict'
import { existsSync, readdirSync } from 'node:fs'
import { it } from 'node:test'

it('keeps the lazy Motion implementation out of the built home static graph', async (t) => {
  if (!existsSync(new URL('../../dist/index.html', import.meta.url))) {
    t.skip('Run the production build to inspect its actual chunk graph')
    return
  }
  const { measureHomeBudget } = await import('../../scripts/home-budget.mjs')
  const measured = await measureHomeBudget()
  // Application settings/motion.ts contains tiny shared timing constants;
  // only the explicitly named third-party implementation must remain lazy.
  const vendorFiles = readdirSync(new URL('../../dist/assets/', import.meta.url))
    .filter(file => /^motion-vendor-[^/]+\.js$/.test(file))
  assert.ok(vendorFiles.length > 0, 'The production build must emit the Motion vendor chunk')
  const motionChunks = measured.files.filter(({ file }: { file: string }) =>
    vendorFiles.some(vendor => file.endsWith(`/assets/${vendor}`) || file === `assets/${vendor}`),
  )
  assert.deepEqual(motionChunks, [], 'A shared runtime must not pull in the lazy Motion implementation')
})
