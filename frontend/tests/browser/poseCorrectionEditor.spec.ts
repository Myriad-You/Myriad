import { fileURLToPath } from 'node:url'
import { expect, test } from '@playwright/test'

test('optional pose editor leaves playback alone until expanded and preserves collapsed drafts', async ({ page }) => {
  const fixture = `/@fs${fileURLToPath(new URL('./fixture/poseCorrectionEditor.tsx', import.meta.url))}`
  await page.route('**/pose-editor-probe', route => route.fulfill({ contentType: 'text/html', body: '<div id="root"></div>' }))
  await page.goto('/pose-editor-probe')
  await page.evaluate(async fixture => {
    const { mountPoseEditor } = await import(fixture)
    window.poseEditorProbe = await mountPoseEditor()
  }, fixture)
  const group = page.locator('#merope-pose-corrections')
  const toggle = group.locator('.setting-group-header-toggle')
  const add = group.locator('.merope-pose__step').nth(1).getByRole('button', { name: 'Left eye', exact: true })
  const remove = group.getByRole('button', { name: 'Delete this spot', exact: true })
  const save = group.getByRole('button', { name: 'Save', exact: true })
  await expect(toggle).toHaveAttribute('aria-expanded', 'false')
  await expect(add).toHaveCount(0)
  expect(await page.evaluate(() => window.poseEditorProbe.calls)).toEqual([])
  await toggle.click()
  await expect(add).toBeVisible()
  await add.click()
  await expect(remove).toBeVisible()
  await expect(save).toBeEnabled()
  await expect.poll(() => page.evaluate(() => window.poseEditorProbe.calls.at(-1)?.length)).toBe(1)
  await toggle.click()
  await expect(add).toHaveCount(0)
  await expect.poll(() => page.evaluate(() => window.poseEditorProbe.calls.at(-1))).toBeNull()
  expect(await page.evaluate(() => window.poseEditorProbe.saves)).toEqual([])
  await toggle.click()
  await expect(remove).toBeVisible()
  await expect(save).toBeEnabled()
  await expect.poll(() => page.evaluate(() => window.poseEditorProbe.calls.at(-1)?.length)).toBe(1)
  await save.click()
  await expect.poll(() => page.evaluate(() => window.poseEditorProbe.saves.length)).toBe(1)
  await page.evaluate(() => window.poseEditorProbe.render('second-outfit'))
  await expect(toggle).toHaveAttribute('aria-expanded', 'false')
  await expect.poll(() => page.evaluate(() => window.poseEditorProbe.calls.at(-1))).toBeNull()
  await toggle.click()
  await expect(add).toBeVisible()
  await expect(remove).toHaveCount(0)
  await expect(save).toBeDisabled()
  await page.evaluate(() => window.poseEditorProbe.unmount())
  await expect.poll(() => page.evaluate(() => window.poseEditorProbe.calls.at(-1))).toBeNull()
})

test('a version 7 package\'s corrections are shown and previewed in the player\'s axes and saved back in its own', async ({ page }) => {
  const fixture = `/@fs${fileURLToPath(new URL('./fixture/poseCorrectionEditor.tsx', import.meta.url))}`
  const stored = [{
    surface: 'head',
    at: { angleX: -0.7, angleY: 0.5 },
    patches: [{ x: 0.2, y: -0.1, radiusX: 0.5, radiusY: 0.4, dx: 0.03, dy: -0.02 }],
  }]
  await page.route('**/pose-editor-probe', route => route.fulfill({ contentType: 'text/html', body: '<div id="root"></div>' }))
  await page.goto('/pose-editor-probe')
  await page.evaluate(async ({ fixture, stored }) => {
    const { mountPoseEditor } = await import(fixture)
    window.poseEditorProbe = await mountPoseEditor(stored)
  }, { fixture, stored })
  const group = page.locator('#merope-pose-corrections')
  await group.locator('.setting-group-header-toggle').click()
  await expect(group.locator('.merope-pose__corner-name b')).toHaveText(['Turn left 70% + Look up 50%'])
  await expect.poll(() => page.evaluate(() => window.poseEditorProbe.calls.at(-1))).toEqual([
    { ...stored[0], at: { headYaw: -0.7, headPitch: 0.5 } },
  ])
  // The driver turns right and looks down: the new spot is a pose of its own.
  await group.locator('.merope-pose__step').nth(1).getByRole('button', { name: 'Left eye', exact: true }).click()
  await expect(group.locator('.merope-pose__corner-name b')).toHaveCount(2)
  await group.getByRole('button', { name: 'Save', exact: true }).click()
  await expect.poll(() => page.evaluate(() => window.poseEditorProbe.saves.length)).toBe(1)
  const [saved] = await page.evaluate(() => window.poseEditorProbe.saves)
  expect(saved).toHaveLength(2)
  expect(saved.find((c: { at: object }) => 'angleX' in c.at && (c.at as { angleX: number }).angleX < 0)).toEqual(stored[0])
  for (const correction of saved) expect(Object.keys(correction.at).every(axis => ['angleX', 'angleY', 'eyeCloseL', 'eyeCloseR', 'mouthOpen'].includes(axis))).toBe(true)
  await page.evaluate(() => window.poseEditorProbe.unmount())
})
