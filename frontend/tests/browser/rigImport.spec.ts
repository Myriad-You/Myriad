import { expect, test } from '@playwright/test'

for (const event of ['blur', 'auth-state-changed']) {
test(`${event} cancels a completed tap tail immediately and the next press starts fresh`, async ({ page }) => {
  const point = await page.evaluate(() => (window as any).rigImportTest.touchSurface('face'))
  for (let i = 0; i < 3; i++) await page.mouse.click(point.x, point.y, { delay: 40 })
  const cancelled = await page.evaluate(event => {
    const state = (window as any).touchSurfaceState
    const before = state.current()
    window.dispatchEvent(new Event(event))
    return { before: before.form, active: state.current().active }
  }, event)
  expect(cancelled.before).toBe('withdraw')
  expect(cancelled.active).toBe(false)
  await page.mouse.move(point.x, point.y)
  await page.mouse.down()
  expect(await page.evaluate(() => (window as any).touchSurfaceState.current().form)).toBe('notice')
  await page.mouse.up()
  await page.evaluate(() => (window as any).touchSurfaceState.dispose())
})
}

test('identity change releases a held pointer and ignores its later pointer-up', async ({ page }) => {
  const point = await page.evaluate(() => (window as any).rigImportTest.touchSurface('hair'))
  await page.mouse.move(point.x, point.y)
  await page.mouse.down()
  await expect(page.locator('#touch-character')).toHaveAttribute('data-merope-touch-active', 'true')
  const cancelled = await page.evaluate(() => {
    window.dispatchEvent(new Event('auth-state-changed'))
    return (window as any).touchSurfaceState.current().active
  })
  expect(cancelled).toBe(false)
  await expect(page.locator('#touch-character')).not.toHaveAttribute('data-merope-touch-active', 'true')
  await page.mouse.up()
  expect(await page.evaluate(() => (window as any).touchSurfaceState.events.includes('end:tap'))).toBe(false)
  await page.mouse.click(point.x, point.y)
  expect(await page.evaluate(() => (window as any).touchSurfaceState.current().form)).toBe('accept')
  await page.evaluate(() => (window as any).touchSurfaceState.dispose())
})

test('repeated hair clicks never reopen the settled happy eyes between presses', async ({ page }) => {
  const point = await page.evaluate(() => (window as any).rigImportTest.touchSurface('hair'))
  await page.mouse.click(point.x, point.y)
  await expect.poll(() => page.evaluate(() => (window as any).touchSurfaceState.current().pose.eyeOpenL)).toBeLessThan(0.65)
  await page.evaluate(() => {
    const state = (window as any).touchSurfaceState
    state.eyeSamples = []
    state.monitor = setInterval(() => state.eyeSamples.push(state.current().pose.eyeOpenL), 8)
  })
  for (let i = 0; i < 8; i++) await page.mouse.click(point.x, point.y, { delay: 90 })
  const samples = await page.evaluate(() => {
    const state = (window as any).touchSurfaceState
    clearInterval(state.monitor)
    return state.eyeSamples as number[]
  })
  expect(samples.length).toBeGreaterThan(20)
  const diagnostics = await page.evaluate(() => ({ events: (window as any).touchSurfaceState.events, current: (window as any).touchSurfaceState.current() }))
  expect(Math.max(...samples), JSON.stringify(diagnostics)).toBeLessThan(0.68)
  expect(Math.max(...samples) - Math.min(...samples)).toBeLessThan(0.15)
  await page.evaluate(() => (window as any).touchSurfaceState.dispose())
})

for (const region of ['hair', 'face']) {
  test(`completed ${region} click produces a held facial expression, not just head motion`, async ({ page }) => {
    const point = await page.evaluate(region => (window as any).rigImportTest.touchSurface(region), region)
    expect(point).not.toBeNull()
    await page.mouse.click(point.x, point.y)
    await expect.poll(() => page.evaluate(() => (window as any).touchSurfaceState.current().form)).toBe(region === 'hair' ? 'accept' : 'hesitate')
    await expect.poll(() => page.evaluate(() => (window as any).touchSurfaceState.current().pose.eyeOpenL)).toBeLessThan(region === 'hair' ? 0.7 : 0.85)
    expect(await page.evaluate(() => (window as any).touchSurfaceState.current().pose.eyeSqueeze)).toBe(0)
    await expect.poll(() => page.evaluate(() => Math.abs((window as any).touchSurfaceState.current().pose.browAngSym))).toBeGreaterThan(region === 'hair' ? 0.15 : 0.35)
    await expect.poll(() => page.evaluate(() => (window as any).touchSurfaceState.current().active), { timeout: 3000 }).toBe(false)
    await expect.poll(() => page.evaluate(() => (window as any).touchSurfaceState.current().pose.eyeOpenL), { timeout: 3000 }).toBeGreaterThan(0.95)
    await page.evaluate(() => (window as any).touchSurfaceState.dispose())
  })
}

test('three completed face clicks change hesitation into a visible frown', async ({ page }) => {
  const point = await page.evaluate(() => (window as any).rigImportTest.touchSurface('face'))
  for (let i = 0; i < 3; i++) await page.mouse.click(point.x, point.y, { delay: 40 })
  await expect.poll(() => page.evaluate(() => (window as any).touchSurfaceState.current().form)).toBe('withdraw')
  await page.mouse.move(point.x, point.y)
  await page.mouse.down()
  await expect.poll(() => page.evaluate(() => (window as any).touchSurfaceState.current().form)).toBe('withdraw')
  await expect.poll(() => page.evaluate(() => (window as any).touchSurfaceState.events.includes('update:hold'))).toBe(true)
  expect(await page.evaluate(() => (window as any).touchSurfaceState.current().form)).toBe('withdraw')
  await expect.poll(() => page.evaluate(() => (window as any).touchSurfaceState.current().pose.browAngSym)).toBeGreaterThan(0.45)
  await page.mouse.up()
  await page.evaluate(() => (window as any).touchSurfaceState.dispose())
})

test('real pointer contact reaches the shared director and releases on blur', async ({ page }) => {
  const point = await page.evaluate(() => (window as any).rigImportTest.touchSurface())
  expect(point).not.toBeNull()
  await page.mouse.move(point.x, point.y)
  await page.mouse.down()
  await expect.poll(() => page.evaluate(() => (window as any).touchSurfaceState.current().active)).toBe(true)
  await expect(page.locator('#touch-character')).toHaveAttribute('data-merope-touch-active', 'true')
  await expect.poll(() => page.evaluate(() => (window as any).touchSurfaceState.events.includes('update:hold'))).toBe(true)
  // Face contact expresses hesitation; its head answer is transient, not a
  // permanently positive nod. Assert the sustained rendered reaction instead.
  await expect.poll(() => page.evaluate(() => (window as any).touchSurfaceState.current().form)).toBe('hesitate')
  await expect.poll(() => page.evaluate(() => (window as any).touchSurfaceState.current().pose.browAngSym)).toBeLessThan(-0.35)
  await expect.poll(() => page.evaluate(() => (window as any).touchSurfaceState.current().pose.body)).toBeLessThan(-0.02)
  await page.evaluate(() => window.dispatchEvent(new Event('blur')))
  await expect.poll(() => page.evaluate(() => (window as any).touchSurfaceState.current().active)).toBe(false)
  await expect(page.locator('#touch-character')).not.toHaveAttribute('data-merope-touch-active', 'true')
  await page.mouse.up()
  expect(await page.evaluate(() => (window as any).touchSurfaceState.events.includes('end:tap'))).toBe(false)
  await page.evaluate(() => (window as any).touchSurfaceState.dispose())
})

test('covering the captured character cancels contact rather than touching through the overlay', async ({ page }) => {
  const point = await page.evaluate(() => (window as any).rigImportTest.touchSurface())
  await page.mouse.move(point.x, point.y)
  await page.mouse.down()
  await expect.poll(() => page.evaluate(() => (window as any).touchSurfaceState.current().active)).toBe(true)
  await page.evaluate(() => {
    const cover = document.createElement('div')
    cover.style.cssText = 'position:fixed;inset:0;z-index:99999;background:white'
    document.body.append(cover)
  })
  await expect.poll(() => page.evaluate(() => (window as any).touchSurfaceState.current().active)).toBe(false)
  await page.mouse.up()
  await page.evaluate(() => (window as any).touchSurfaceState.dispose())
})

for (const expression of ['tense', 'withdrawn']) {
  test(`petting preserves ${expression} bearing while the real player speaks over music`, async ({ page }) => {
    test.setTimeout(60_000)
    const result = await page.evaluate(expression => (window as any).rigImportTest.directorReplay(true, false, 'accept', 60, expression), expression)
    const control = await page.evaluate(expression => (window as any).rigImportTest.directorReplay(true, false, 'control', 60, expression), expression)
    const held = result.frames.filter((f: any) => f.at > 0.7 && f.at < 1.25)
    expect(held.every((f: any) => f.owners.mouth === 'speech')).toBe(true)
    expect(Math.max(...held.map((f: any) => f.mouthOpen))).toBeGreaterThan(0.05)
    for (const f of held) {
      const original = control.frames[Math.round(f.at * 60)].browAngSym
      expect(Math.sign(original) * f.browAngSym).toBeGreaterThan(Math.abs(original) * 0.7)
    }
    expect(Math.max(...held.map((f: any) => Math.abs(f.eyeX - control.frames[Math.round(f.at * 60)].eyeX)))).toBeGreaterThan(0.02)
    expect(result.glError).toBe(0)
    expect(result.rejected).toBe(0)
  })
}

for (const [fps, decision] of [[30, 'withdraw'], [60, 'withdraw'], [120, 'withdraw'], [60, 'accept'], [60, 'late'], [60, 'changed'], [60, 'fail']] as const) {
  test(`touch handoff through full player at ${fps} fps with ${decision} appraisal`, async ({ page }) => {
    test.setTimeout(60_000)
    const result = await page.evaluate(({ fps, decision }) =>
      (window as any).rigImportTest.directorReplay(true, false, decision, fps), { fps, decision })
    const control = await page.evaluate(fps =>
      (window as any).rigImportTest.directorReplay(true, false, 'control', fps), fps)
    const frames = result.frames as Array<{
      at: number; angleX: number; angleY: number; angleZ: number; body: number; eyeX: number
      mouthOpen: number; touchForm: string | null; presentedTouch: string | null; touchApplied: number
      owners: { mouth: string; headBody: string }; active: string[]
    }>
    expect(result.rejected).toBe(0)
    expect(result.duplicateWrites).toBe(0)
    expect(result.glError).toBe(0)
    expect(result.touchWriteMutation).toBe(false)
    expect(result.touchRequests).toBe(1)
    expect(result.touchApplied).toBe(['late', 'changed', 'fail'].includes(decision) ? 0 : 1)
    if (decision === 'withdraw') expect(frames.some(f => f.touchForm === 'withdraw')).toBe(true)
    if (decision === 'withdraw') expect(frames.some(f => f.presentedTouch === 'withdraw')).toBe(true)
    expect(frames.filter(f => f.at > 3).every(f => f.presentedTouch === null)).toBe(true)
    expect(frames.filter(f => f.at < 2).every(f => f.owners.mouth === 'speech')).toBe(true)
    expect(Math.max(...frames.filter(f => f.at > 0.6 && f.at < 1.6).map(f => f.mouthOpen))).toBeGreaterThan(0.05)
    expect(frames.filter(f => f.at > 3 && f.at < 4).every(f => f.owners.headBody === 'music')).toBe(true)
    expect(frames.filter(f => f.at > 2.4).every(f => f.touchForm === null)).toBe(true)
    expect(frames.at(-1)!.active.some(f => ['accept:', 'withdraw:', 'hesitate:', 'notice:'].some(prefix => f.startsWith(prefix)))).toBe(false)
    const metrics: Record<string, { speed: number; acceleration: number; peakAt: number }> = {}
    for (const key of ['angleX', 'angleY', 'angleZ', 'body', 'eyeX'] as const) {
      let lastVelocity = 0
      metrics[key] = { speed: 0, acceleration: 0, peakAt: 0 }
      for (let i = 2; i < frames.length; i++) {
        // Touch handoffs end before 3.2s. The independently phased music
        // shutdown at 4s is covered by the music replay, not this differential.
        if (frames[i].at > 3.2) break
        const delta = (index: number) => frames[index][key] - control.frames[index][key]
        const velocity = (delta(i) - delta(i - 1)) * fps
        if (i > 2 && Math.abs(velocity - lastVelocity) * fps > metrics[key].acceleration) {
          metrics[key].acceleration = Math.abs(velocity - lastVelocity) * fps
          metrics[key].peakAt = frames[i].at
        }
        metrics[key].speed = Math.max(metrics[key].speed, Math.abs(velocity))
        lastVelocity = velocity
      }
      expect(metrics[key].speed, `${key}: ${JSON.stringify(metrics)}`).toBeLessThan(6)
      // Normalized driver units/s²: gaze uses the existing 16Hz response,
      // head/body 7–9.5Hz. These are regression budgets, not human-motion claims.
      expect(metrics[key].acceleration, `${key}: ${JSON.stringify(metrics)}`).toBeLessThan(key === 'eyeX' ? 180 : 100)
    }
  })
}

for (const kind of ['ordinary', 'collar', 'necklace']) {
  test(`touch picking matches real rendered alpha for ${kind} while turning`, async ({ page }) => {
    const result = await page.evaluate((kind) => (window as any).rigImportTest.touchPicking(kind), kind)
    expect(result.hits).toBeGreaterThan(50)
    expect(result.falseHits).toBe(0)
    expect(result.misses).toBe(0)
    expect(result.glError).toBe(0)
    if (kind === 'necklace') expect(result.regions).toContain('accessory')
  })
}

test('stream replacement rejects stale events and cancellation preserves music', async ({
  page,
}) => {
  test.setTimeout(60_000)
  const result = await page.evaluate(() =>
    (window as any).rigImportTest.directorReplay(true, true),
  )
  expect(result.raceChecks).toEqual({
    chunkKeepsDirector: true,
    staleKeptDirector: true,
    staleKeptSpeech: true,
    reconnectDeduplicated: true,
    cancelledPlanAbsent: true,
  })
  expect(result.deliveredText).toEqual([
    '第一句。',
    '后半句继续。',
    '新回复正在说话。',
  ])
  expect(
    result.frames.slice(18, 60).every((f: any) => f.owners.mouth === 'speech'),
  ).toBe(true)
  expect(
    result.frames
      .slice(120, 240)
      .some((f: any) => f.owners.headBody === 'music'),
  ).toBe(true)
  expect(
    Math.max(...result.frames.slice(12, 60).map((f: any) => f.maniac)),
  ).toBeGreaterThan(0.2)
  expect(result.frames.at(-1).maniac).toBeLessThan(0.01)
  expect(result.duplicateWrites).toBe(0)
  expect(result.rejected).toBe(0)
  expect(result.glError).toBe(0)
})

test('speech and music share the director outlet and release their channels', async ({
  page,
}) => {
  test.setTimeout(60_000)
  const result = await page.evaluate(() =>
    (window as any).rigImportTest.directorReplay(true),
  )
  const frames = result.frames as Array<{
    mouthOpen: number
    angleX: number
    angleY: number
    maniac: number
    owners: { mouth: string; headBody: string }
    active: string[]
    musicBehaviors: number
  }>
  expect(frames.slice(0, 120).every((f) => f.owners.mouth === 'speech')).toBe(
    true,
  )
  expect(
    Math.max(...frames.slice(0, 120).map((f) => f.mouthOpen)),
  ).toBeGreaterThan(0.05)
  expect(frames.some((f) => f.musicBehaviors > 0)).toBe(true)
  expect(Math.max(...frames.map((f) => f.maniac))).toBeGreaterThan(0.2)
  expect(
    frames.slice(180, 240).some((f) => f.owners.headBody === 'music'),
  ).toBe(true)
  const musicPoses = frames.slice(180, 240).map((f) => f.angleY)
  expect(Math.max(...musicPoses) - Math.min(...musicPoses)).toBeGreaterThan(
    0.001,
  )
  expect(frames.at(-1)!.owners.mouth).not.toBe('speech')
  expect(frames.at(-1)!.owners.headBody).not.toBe('music')
  expect(result.rejected).toBe(0)
  expect(result.duplicateWrites).toBe(0)
  expect(result.glError).toBe(0)
})

test('director source reaches rendered replacement and recovery without replaying revisions', async ({
  page,
}) => {
  test.setTimeout(60_000)
  const result = await page.evaluate(() =>
    (window as any).rigImportTest.directorReplay(),
  )
  expect(result.accepted).toBeGreaterThan(0)
  expect(result.rejected).toBe(0)
  expect(result.replacementMutation).toBe(false)
  expect(result.duplicateWrites).toBe(0)
  const frames = result.frames as Array<{
    angleX: number
    angleY: number
    angleZ: number
    maniac: number
    active: string[]
    pixelSum: number
  }>
  expect(frames.slice(0, 12).some((f) => Math.abs(f.angleY) > 0.001)).toBe(true)
  expect(frames.slice(12, 30).some((f) => f.maniac > 0.01)).toBe(true)
  expect(frames.some((f) => f.active.includes('respond:recovering'))).toBe(true)
  expect(Math.max(...frames.map((f) => f.maniac))).toBeGreaterThan(0.2)
  expect(frames.at(-1)!.maniac).toBeLessThan(0.01)
  expect(
    frames
      .at(-1)!
      .active.some(
        (id) => id.startsWith('respond:') || id.startsWith('maniac:'),
      ),
  ).toBe(false)
  expect(new Set(frames.map((f) => f.pixelSum)).size).toBeGreaterThan(20)
  for (let i = 1; i < frames.length; i++) {
    for (const key of ['angleX', 'angleY', 'angleZ'] as const)
      expect(Math.abs(frames[i][key] - frames[i - 1][key])).toBeLessThan(0.2)
  }
  expect(result.glError).toBe(0)
})

for (const kind of ['ordinary', 'collar', 'necklace']) {
  for (const fps of [30, 60]) {
    test(`${kind} body replay preserves geometry and visible clothing at ${fps} fps`, async ({
      page,
    }) => {
      test.setTimeout(60_000)
      const result = await page.evaluate(
        ({ kind, fps }) => (window as any).rigImportTest.bodyReplay(kind, fps),
        { kind, fps },
      )
      expect(result.hairLayers).toBeGreaterThan(0)
      expect(result.warmupError).toBe(0)
      expect(result.rootEdges).toBeGreaterThan(0)
      // Gross root collapse/stretch guard, not a claim of artistic acceptance.
      expect(result.minRootStretch).toBeGreaterThan(0.5)
      expect(result.maxRootStretch).toBeLessThan(1.5)
      expect(result.checkedFrames).toBe(2.5 * fps)
      expect(result.invalid).toBe(0)
      expect(result.excursion).toBeGreaterThan(1)
      // No single frame may consume half the replay's full excursion. This
      // detects gross jumps without imposing a new absolute movement-speed cap.
      expect(result.maxStep, JSON.stringify(result)).toBeLessThan(
        result.excursion * 0.5,
      )
      expect(result.targetMutation).toBe(0)
      expect(result.idempotenceError).toBeLessThan(0.00001)
      expect(result.pixelMismatch).toBe(0)
      expect(result.minClothingPixels).toBeGreaterThan(20)
      if (kind === 'collar') {
        expect(result.roles).toContain('collar-front')
        expect(result.collarClip).toBe(true)
      }
      if (kind === 'necklace') {
        expect(result.roles).toContain('neckwear')
        expect(result.roles).not.toContain('collar-front')
        expect(result.minAccessoryPixels).toBeGreaterThan(5)
      }
      expect(result.glError).toBe(0)
    })
  }
}

for (const fps of [30, 60]) {
  test(`eye pixels preserve closure, hidden-white clipping and fade coverage at ${fps} fps`, async ({
    page,
  }) => {
    test.setTimeout(60_000)
    const result = await page.evaluate(
      (fps) => (window as any).rigImportTest.eyePixels(fps),
      fps,
    )
    const { open, wink, closed, special, reopen, turn, reverse } =
      result.endpoints
    expect(open[0]).toBeGreaterThan(20)
    expect(open.slice(1)).toEqual([0, 0])
    expect(wink[0]).toBeGreaterThan(10)
    expect(wink[1]).toBeGreaterThan(10)
    expect(wink[2]).toBe(0)
    expect(closed[0]).toBe(0)
    expect(closed[1]).toBeGreaterThan(10)
    expect(closed[2]).toBeGreaterThan(10)
    // No crying art in this fixture: all ordinary eye art must yield.
    expect(special).toEqual([0, 0, 0])
    expect(reopen).toEqual(open)
    expect(turn[0]).toBeGreaterThan(0)
    expect(reverse[0]).toBeGreaterThan(0)
    expect(result.outsideMask).toBe(0)
    expect(result.invalidVertices).toBe(0)
    // Fast closure crosses the art-fade window in a few frames. Verify coverage
    // and direction, not an arbitrary opacity speed that would slow blinking.
    // Residual special-expression filtering may remain below one alpha byte.
    expect(result.maxCoverageError).toBeLessThan(1 / 255)
    expect(result.reversals).toBe(0)
    expect(result.blinkColors[2]).toBeGreaterThan(10)
    expect(result.blinkColors[1]).toBe(0)
    expect(result.samples).toBe(8 * fps)
    expect(result.glError).toBe(0)
  })
}

test('relative source assets resolve against the page and preserve fetch errors', async ({
  page,
}) => {
  const requests: string[] = []
  await page.route('**/assets/master.png', (route) => {
    requests.push(new URL(route.request().url()).pathname)
    return route.fulfill({ status: 404, body: '' })
  })
  const result = await page.evaluate(() =>
    (window as any).rigImportTest.relativeSourceFailure(),
  )
  expect(requests).toEqual(['/assets/master.png'])
  expect(result.error).toBe(result.expected)
})

test.beforeEach(async ({ page }) => {
  // A self-contained fixture server only; never talk to an actual backend.
  await page.route('**/api/**', (route) => route.abort())
  await page.goto('/rigImport.html')
  await page.waitForFunction(() => 'rigImportTest' in window)
})

for (const kind of ['ordinary', 'collar', 'necklace', 'alternate-eyes']) {
  test(`real worker import preserves ${kind} manifest and PNG pixels`, async ({
    page,
  }) => {
    const result = await page.evaluate(async (kind) => {
      const harness = (window as any).rigImportTest
      return harness.run(kind)
    }, kind)
    expect(result.error).toBeUndefined()
    expect(result.stages).toEqual(['validated', 'packing'])
    expect(result.sourceEqual).toBe(true)
    expect(result.partCount).toBeGreaterThan(15)
    expect(result.atlas).toEqual(result.expectedAtlas)
    expect(result.reference).toEqual(result.expectedReference)
    if (kind === 'collar') expect(result.roles).toContain('collar-front')
    if (kind === 'necklace') {
      expect(result.roles).toContain('neckwear')
      expect(result.roles).not.toContain('collar-front')
    }
  })
}

test('real player distinguishes automatic blinks, deliberate closure and special eyes', async ({
  page,
}) => {
  const result = await page.evaluate(() =>
    (window as any).rigImportTest.eyeRuntime(),
  )
  expect(result.ordinaryBlink).toBeGreaterThan(0.9)
  expect(result.alternateBlink).toBe(0)
  expect(result.wink.left).toBeGreaterThan(0.95)
  expect(result.wink.ordinaryLeft).toBeLessThan(0.01)
  expect(result.wink.right).toBeLessThan(0.01)
  expect(result.both.left).toBeGreaterThan(0.95)
  expect(result.both.right).toBeGreaterThan(0.95)
  expect(result.cry).toBeLessThan(0.01)
  expect(result.rebound).toBeGreaterThan(0.003)
  expect(result.rebound).toBeLessThan(0.045)
  expect(result.glError).toBe(0)
})

test('packing cancellation rejects and a fresh import still succeeds', async ({
  page,
}) => {
  const result = await page.evaluate(async () => {
    const harness = (window as any).rigImportTest
    return {
      cancelled: await harness.run('ordinary', true),
      next: await harness.run('ordinary'),
    }
  })
  expect(result.cancelled.stages).toEqual(['validated', 'packing'])
  expect(result.cancelled.name).toBe('AbortError')
  expect(result.next.error).toBeUndefined()
  expect(result.next.sourceEqual).toBe(true)
})

test('worker compile failures keep the selected UI language', async ({
  page,
}) => {
  const result = await page.evaluate(() =>
    (window as any).rigImportTest.localizedFailure(),
  )
  expect(result.expected).toMatch(/[\u4E00-\u9FFF]/)
  expect(result.result.error).toBe(result.expected)
})
