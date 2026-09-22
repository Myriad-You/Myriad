import { expect, test } from '@playwright/test'

for (const mode of ['success', 'failure', 'preview']) {
  test(`friend links load only on display: ${mode}`, async ({ page }) => {
    const requests: URL[] = []
    await page.route('**/api/**', async (route) => {
      const url = new URL(route.request().url())
      if (url.pathname.endsWith('/phantasi/sources')) {
        requests.push(url)
        await route.fulfill({
          status: mode === 'failure' ? 500 : 200,
          json: {
            success: mode !== 'failure',
            sources: [
              {
                id: 1,
                name: 'Visible friend',
                url: 'https://friend.example',
                site_url: 'https://friend.example',
                category: '友情链接',
                source_type: 'site',
                enabled: true,
              },
            ],
          },
        })
      } else {
        await route.fulfill({ json: { success: true, data: {} } })
      }
    })
    await page.goto(`/friendLinks.html${mode === 'preview' ? '?preview' : ''}`)
    await expect(
      page.getByRole('button', { name: 'Mount widget' }),
    ).toBeVisible()
    expect(requests).toHaveLength(0)
    await page.getByRole('button', { name: 'Mount widget' }).click()
    await expect(page.locator('#widget > *')).toBeAttached()
    await page.waitForTimeout(150)
    expect(requests).toHaveLength(0)
    await page.locator('#widget').scrollIntoViewIfNeeded()
    if (mode === 'preview') {
      await page.waitForTimeout(150)
      expect(requests).toHaveLength(0)
    } else {
      await expect.poll(() => requests.length).toBe(1)
      expect(requests[0].searchParams.get('view')).toBe('catalog')
      expect(requests[0].searchParams.get('category')).toBe('friends')
      if (mode === 'success') {
        await expect(
          page.getByText('Visible friend', { exact: true }),
        ).toBeVisible()
}
      else {
        await expect(page.locator('#widget')).toContainText(
          /友情链接暂时不可用|Friend links are temporarily unavailable|相互リンクを一時的に利用できません/,
        )
}
      await page.evaluate(() => window.scrollTo(0, 0))
      await page.locator('#widget').scrollIntoViewIfNeeded()
      expect(requests).toHaveLength(1)
    }
  })
}

test('layout and ordinary source widgets do not preload the friend catalog', async ({
  page,
}) => {
  const catalogs: string[] = []
  let ordinarySources = 0
  await page.route('**/api/**', async (route) => {
    const url = new URL(route.request().url())
    if (url.pathname.endsWith('/phantasi/sources')) {
      if (url.searchParams.get('category') === 'friends')
        catalogs.push(url.href)
      else ordinarySources += 1
    }
    if (['/api/tapps/details', '/api/tapps/widgets'].includes(url.pathname)) {
      await route.fulfill({ json: { success: true, data: [] } })
      return
    }
    await route.fulfill({
      json: {
        success: true,
        sources: [],
        items: [],
        data: {},
        setup_required: false,
        preferences: {},
      },
    })
  })
  await page.goto('/friendLinks.html?layout')
  await expect(
    page.getByRole('button', { name: 'Mount widget', exact: true }),
  ).toBeVisible()
  await page
    .getByRole('button', { name: 'Mount ordinary sources', exact: true })
    .click()
  await expect.poll(() => ordinarySources).toBe(1)
  expect(catalogs).toHaveLength(0)
})
