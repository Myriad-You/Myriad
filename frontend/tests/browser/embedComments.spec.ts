import { expect, test } from '@playwright/test'

const now = Date.now()
function row(id: number, userId: number, name: string, text: string, extra = {}) {
  return {
  id, item_id: 7, user_id: userId, user_display_name: name, selected_text: '', comment: text,
  is_public: true, created_at: now - 3_600_000, updated_at: now - 3_600_000, ...extra,
}
}

const markdown = '**bold** and [evil](javascript:alert(1)) and [ok](https://example.com)'

async function mock(page: import('@playwright/test').Page, authed: boolean, deleted: number[] = []) {
  await page.addInitScript(() => localStorage.setItem('locale', 'en-US'))
  await page.route('**/api/**', (route) => {
    const request = route.request()
    const url = request.url()
    if (url.includes('/auth/me')) {
      return route.fulfill({ json: authed ? { authenticated: true, id: 1, username: 'owner', is_admin: true } : { authenticated: false } })
    }
    if (request.method() === 'DELETE') {
      deleted.push(Number(url.split('/').pop()))
      return route.fulfill({ json: { success: true } })
    }
    if (url.includes('/replies')) return route.fulfill({ json: { success: true, replies: [row(11, 1, 'Owner', 'Thanks!')] } })
    if (url.includes('/comments')) {
      return route.fulfill({ json: { success: true, has_comments: true, can_write: authed, comments: [
        row(1, 2, 'Alice', 'Great post.', { reply_count: 1 }),
        row(2, 1, 'Owner', 'A note from me.'),
        row(3, 3, 'Carol', markdown),
      ] } })
    }
    return route.fulfill({ json: url.includes('csrf') ? { csrf_token: null } : {} })
  })
}

test('guests see the thread and a login prompt, not a composer', async ({ page }) => {
  await mock(page, false)
  await page.goto('/embedCommentsHost.html')
  const frame = page.frameLocator('iframe')
  await expect(frame.getByText('Great post.')).toBeVisible()
  await expect(frame.getByRole('link', { name: /Log in/ })).toBeVisible()
  await expect(frame.locator('textarea')).toHaveCount(0)
  await frame.getByRole('button', { name: '1 reply' }).click()
  await expect(frame.getByText('Thanks!')).toBeVisible()
})

test('deleting your own comment asks first and then removes it', async ({ page }) => {
  const deleted: number[] = []
  await mock(page, true, deleted)
  await page.goto('/embedCommentsHost.html')
  const frame = page.frameLocator('iframe')
  await expect(frame.getByText('A note from me.')).toBeVisible()
  // Only the viewer's own comment offers delete.
  await expect(frame.getByRole('button', { name: 'Delete' })).toHaveCount(1)
  await frame.getByRole('button', { name: 'Delete' }).click()
  await expect(frame.getByText('Delete this comment?')).toBeVisible()
  expect(deleted).toEqual([])
  await frame.getByRole('alertdialog').getByRole('button', { name: 'Delete' }).click()
  await expect(frame.getByText('A note from me.')).toHaveCount(0)
  expect(deleted).toEqual([2])
})

test('comments render Markdown but only http(s) links', async ({ page }) => {
  await mock(page, false)
  await page.goto('/embedCommentsHost.html')
  const frame = page.frameLocator('iframe')
  await expect(frame.locator('strong', { hasText: 'bold' })).toBeVisible()
  await expect(frame.getByRole('link', { name: 'ok' })).toHaveAttribute('rel', /nofollow ugc/)
  await expect(frame.getByRole('link', { name: 'evil' })).toHaveCount(0)
  await expect(frame.getByText('[evil](javascript:alert(1))', { exact: false })).toBeVisible()
})
