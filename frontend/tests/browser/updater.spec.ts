import type { Page } from '@playwright/test'
import { expect, test } from '@playwright/test'

test.use({ locale: 'en-US' })

async function setup(page: Page) {
  let reads = 0
  let posts = 0
  const status = {
    schema_version: 1, current_version: 'v0.5.3', updater_version: 'v0.5.3',
    channel: 'stable', update_mode: 'release', maintenance_active: false, maintenance_phase: 'idle',
    job_in_flight: null, latest_available: null, requires_self_update: false, last_checked_at: new Date().toISOString(),
    self_update_last: { status: 'succeeded', queued: false, error: '', target_tag: 'v0.5.3', previous_tag: 'v0.5.2', at: 'before' },
  }
  await page.addInitScript(() => {
    sessionStorage.setItem('csrf_token', `v1.${'a'.repeat(16)}.${'b'.repeat(43)}`)
    sessionStorage.setItem('csrf_token_stored_at', String(Date.now()))
  })
  await page.route('**/api/admin/updater/**', async (route) => {
    const path = new URL(route.request().url()).pathname
    if (path.endsWith('/status')) {
      reads++
      await route.fulfill({ json: status })
    } else if (path.endsWith('/self-update')) {
      posts++
      status.self_update_last = { status: 'pending', queued: true, error: '', target_tag: '', previous_tag: 'v0.5.3', at: 'queued' }
      await route.fulfill({ json: { scheduled: true, helper_container_id: 'docker-guard', new_updater_tag: '', previous_updater_tag: 'v0.5.3' } })
    } else if (path.endsWith('/snapshots')) {
      await route.fulfill({ json: { schema_version: 1, items: [] } })
    } else {
      await route.fulfill({ json: null })
    }
  })
  await page.route('**/health', route => route.fulfill({ json: { version: 'v0.5.3' } }))
  page.on('dialog', dialog => dialog.accept())
  await page.goto('/updater.html')
  return { status, reads: () => reads, posts: () => posts }
}

test('self-update discovery failure ends waiting and allows retry without reloading', async ({ page }) => {
  const app = await setup(page)
  const button = page.getByRole('button', { name: 'Upgrade updater', exact: true })
  await expect(button).toBeEnabled()
  await page.clock.install()
  await button.click()
  await expect(page.getByText('Updater upgrade scheduled; confirming result…')).toBeVisible()
  app.status.self_update_last = { status: 'failed', queued: false, error: 'Registry unavailable', target_tag: '', previous_tag: 'v0.5.3', at: 'failed' }
  await page.clock.runFor(4100)
  await expect(button).toBeEnabled()
  await expect(page.getByText('Updater upgrade failed: Registry unavailable', { exact: true })).toBeVisible()
  expect(app.posts()).toBe(1)
})

test('self-update recovery stays pending until Guard publishes the result', async ({ page }) => {
  const app = await setup(page)
  const button = page.getByRole('button', { name: 'Upgrade updater', exact: true })
  await expect(button).toBeEnabled()
  await page.clock.install()
  await button.click()
  app.status.self_update_last = { status: 'pending', queued: false, error: 'Recovering previous image', target_tag: 'v0.5.4', previous_tag: 'v0.5.3', at: 'recovering' }
  await page.clock.runFor(4100)
  await expect(button).toBeDisabled()
  await expect(page.getByText('Updater upgrade scheduled; confirming result…')).toBeVisible()
  app.status.self_update_last = { ...app.status.self_update_last, status: 'failed', error: 'Previous image restored', at: 'finished' }
  await page.clock.runFor(4100)
  await expect(button).toBeEnabled()
  await expect(page.getByText('Updater upgrade failed: Previous image restored', { exact: true })).toBeVisible()
})
