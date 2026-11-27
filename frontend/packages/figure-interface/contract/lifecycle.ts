import type { FigureEntries } from './index'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { countCalls, installHeadlessPage } from './headless'

/** A legal bust manifest with its playback package, as an importer makes one. */
export function bustManifest(): Record<string, any> {
  return JSON.parse(readFileSync(new URL('fixtures/bust-manifest.json', import.meta.url), 'utf8'))
}

const face = (ready: boolean, mood: number, capabilities: readonly string[]) => ({ ready, mood, arousal: 48, activity: 'idle' as const, capabilities })

const draws = (gl: Map<string, number>) => countCalls(gl, name => name.startsWith('draw'))
const deletes = (gl: Map<string, number>) => countCalls(gl, name => name.startsWith('delete'))

function pointer(type: string, x: number, y: number): Event {
  return Object.assign(new Event(type), { pointerId: 7, isPrimary: true, button: 0, clientX: x, clientY: y })
}

/**
 * What the site's pages do with a live figure, run without a browser: a motion runtime held and let go,
 * live faces attached, frames subscribed to, touch bound and unbound, and the player built, ticked,
 * re-dressed and disposed.
 */
export function lifecycleContract(figure: FigureEntries): void {
  const { anime25drig, interaction, motion, root } = figure

  test('figure contract: a held runtime leases its channels and lets every one go when the last hold ends', () => {
    const coordinator = new motion.RigMotionCoordinator()
    const runtime = motion.createLiveMotionRuntime(coordinator, null, new root.FigureChannel())
    assert.equal(coordinator.leaseCount(), 0)
    assert.equal(runtime.summaryFacts().faceVisible, false)
    const release = runtime.retain()
    const held = coordinator.leaseCount()
    assert.ok(held > 0, 'a held runtime holds leases')
    assert.equal(runtime.summaryFacts().faceVisible, true)
    const second = runtime.retain()
    second()
    second()
    assert.equal(coordinator.leaseCount(), held, 'one of two holds ending keeps the leases')
    release()
    assert.equal(coordinator.leaseCount(), 0)
    assert.equal(runtime.summaryFacts().faceVisible, false)
    release()
    assert.equal(coordinator.leaseCount(), 0)
  })

  test('figure contract: live faces set what she can do and who she is, and leave when released', () => {
    const runtime = motion.createPreviewMotionRuntime(new root.FigureChannel())
    const release = runtime.retain()
    try {
      runtime.setCapabilities(['blink'])
      assert.deepEqual(runtime.summaryFacts().capabilities, ['blink'])
      const frames: { mood: number | undefined }[] = []
      const stop = runtime.subscribe(frame => frames.push({ mood: frame.mood?.mood }))
      const consumer = runtime.attachLiveFaceConsumer(face(true, 22, ['independent-eyes', 'independent-eyes']))
      assert.deepEqual(runtime.summaryFacts().capabilities, ['independent-eyes'])
      assert.equal(runtime.summaryFacts().faceVisible, true)
      assert.equal(frames.at(-1)?.mood, 22)
      consumer.update(face(false, 80, ['independent-eyes']))
      assert.equal(runtime.summaryFacts().faceVisible, false)
      assert.deepEqual(runtime.summaryFacts().capabilities, [])
      consumer.release()
      consumer.update(face(true, 90, ['dizzy-eye']))
      assert.deepEqual(runtime.summaryFacts().capabilities, [], 'a released face no longer counts')
      stop()
    }
    finally {
      release()
    }
  })

  test('figure contract: a subscriber gets the frame at once and every change until it unsubscribes', () => {
    const runtime = motion.createPreviewMotionRuntime(new root.FigureChannel())
    const release = runtime.retain()
    try {
      const moods: (number | undefined)[] = []
      const stop = runtime.subscribe(frame => moods.push(frame.mood?.mood))
      assert.equal(moods.length, 1)
      runtime.mood.set(15, 'thinking', 30)
      assert.ok(moods.includes(15), 'a mood change reaches the subscriber')
      stop()
      const seen = moods.length
      runtime.mood.set(85, 'talking', 70)
      assert.equal(moods.length, seen, 'nothing reaches it after unsubscribing')
    }
    finally {
      release()
    }
  })

  test('figure contract: a bound canvas reports a touch on her and nothing once unbound', () => {
    const page = installHeadlessPage()
    try {
      const phases: string[] = []
      let cancelled = 0
      const binding = interaction.bindCharacterTouch(page.canvas as unknown as HTMLCanvasElement, () => ({ region: 'hair' }), touch => phases.push(touch.phase), () => { cancelled += 1 })
      page.canvas.dispatchEvent(pointer('pointerdown', 150, 100))
      page.canvas.dispatchEvent(pointer('pointerup', 150, 100))
      assert.deepEqual([phases[0], phases.at(-1)], ['start', 'end'])
      assert.equal(page.canvas.dataset.meropeTouchActive, undefined)
      binding.unbind()
      assert.ok(cancelled >= 1, 'unbinding drops the reaction in progress')
      const before = phases.length
      page.canvas.dispatchEvent(pointer('pointerdown', 150, 100))
      page.canvas.dispatchEvent(pointer('pointerup', 150, 100))
      assert.equal(phases.length, before)
    }
    finally {
      page.restore()
    }
  })

  test('figure contract: the player draws as it ticks, keeps its outfit when a new one fails, and lets go of the GPU', async () => {
    const page = installHeadlessPage()
    try {
      const manifest = bustManifest()
      const atlas = manifest.textures[0].url as string
      const player = new anime25drig.Anime25DPlayer(page.canvas as unknown as HTMLCanvasElement, manifest.anime25dPlayback, manifest as never)
      await player.replaceLivePackage(manifest.anime25dPlayback, manifest as never, atlas)
      player.resize(300, 400, 1)
      const before = draws(page.gl)
      for (let frame = 0; frame < 3; frame += 1) {
        player.setTarget({ headYaw: frame / 3 })
        player.tick(1 / 30)
      }
      assert.ok(draws(page.gl) > before, 'ticking draws')
      const layers = player.debugSnapshot().layerCount
      assert.ok(layers > 0)

      await assert.rejects(player.replaceLivePackage({ kind: 'not-a-package' } as never, manifest as never, atlas))
      await assert.rejects(player.replaceLivePackage(manifest.anime25dPlayback, manifest as never, '/api/merope/rig/assets/unreachable.png'))
      assert.equal(player.debugSnapshot().layerCount, layers, 'the last outfit stays')
      const kept = draws(page.gl)
      player.tick(1 / 30)
      assert.ok(draws(page.gl) > kept, 'the last outfit still draws')

      const freed = deletes(page.gl)
      player.dispose()
      assert.ok(deletes(page.gl) > freed, 'disposing frees what it gave the GPU')
    }
    finally {
      page.restore()
    }
  })
}
