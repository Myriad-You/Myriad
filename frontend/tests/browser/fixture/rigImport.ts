import type { CollarClipMesh } from '../../../src/features/merope/anime25drig/collarRuntime'
import type { Anime25DGpuLayer } from '../../../src/features/merope/anime25drig/layerGpuBinding'
import { writePsd } from 'ag-psd'
import { realizeAnime25DBehaviorPlan } from '../../../src/features/merope/anime25drig/behaviorRealizer'
import { Anime25DPlayer } from '../../../src/features/merope/anime25drig/player'
import {
  speechArticulationDriverPatch,
  speechEnergyDriverPatch,
} from '../../../src/features/merope/anime25drig/speechDriver'
import { bindCharacterTouch } from '../../../src/features/merope/interaction/bindTouch'
import { TouchAppraisal } from '../../../src/features/merope/interaction/touchAppraisal'
import { TouchGestureTracker } from '../../../src/features/merope/interaction/touchGesture'
import {
  applyMotionFrame,
  createMotionApplyState,
} from '../../../src/features/merope/motion/applyFrame'
import { RigMotionCoordinator } from '../../../src/features/merope/motion/coordinator'
import {
  liveMotionGeneration,
  setLiveMotionGeneration,
} from '../../../src/features/merope/motion/liveGeneration'
import { MusicMotionSource } from '../../../src/features/merope/motion/musicSource'
import { MotionRuntime } from '../../../src/features/merope/motion/runtime'
import { dispatchMeropePerformance } from '../../../src/features/merope/performanceEvents'
import { anime25DImportCopy } from '../../../src/features/merope/rig/anime25dImportCopy'
import { prepareAnime25DRigPsd } from '../../../src/features/merope/rig/anime25dImporter'
import { syntheticSeeThroughPsd } from '../../../src/features/merope/rig/anime25dImporter.fixture'
import { decodeRigPsd } from '../../../src/features/merope/rig/psdDecode'
import { prepareRigPsdImport } from '../../../src/features/merope/rig/psdImporter'
import { dispatchMeropeSpeech } from '../../../src/features/merope/speechEvents'
import { loadLocale } from '../../../src/i18n/loadLocale'
import { currentCopy } from '../../../src/i18n/localeCopy'

function fixture(kind: string) {
  const psd = syntheticSeeThroughPsd()
  if (kind === 'alternate-eyes') {
    const closed = psd.children!.find((l) => l.name === 'eyelash_c')!
    const data = new Uint8ClampedArray(closed.imageData!.data)
    for (let y = 0; y < 256; y++)
      data.fill(0, (y * 256 + 128) * 4, (y + 1) * 256 * 4)
    psd.children!.push({
      ...closed,
      name: 'eye_close2',
      imageData: { ...closed.imageData!, data },
    })
  }
  if (kind === 'missing-face')
    psd.children = psd.children!.filter((l) => l.name !== 'face')
  if (kind === 'collar' || kind === 'necklace') {
    // Non-square PSDs do not ask for a See-through square reference.
    psd.height = 300
    const data = new Uint8ClampedArray(40 * 50 * 4)
    for (let i = 0; i < data.length; i += 4) data.set([245, 205, 190, 255], i)
    psd.children!.push({
      name: 'neck',
      left: 108,
      top: 138,
      imageData: { width: 40, height: 50, data },
    })
    if (kind === 'necklace') {
      const topwear = psd.children!.find((l) => l.name === 'topwear')!
      const pixels = topwear.imageData!.data
      pixels.fill(0, 0, 180 * 256 * 4)
      psd.children!.push({
        name: 'neckwear_1',
        left: 124,
        top: 144,
        imageData: {
          width: 8,
          height: 32,
          data: new Uint8ClampedArray(8 * 32 * 4).fill(255),
        },
      })
    }
  }
  return psd
}

async function pngPixels(blob: Blob) {
  const image = await createImageBitmap(blob)
  try {
    const canvas = new OffscreenCanvas(image.width, image.height)
    const ctx = canvas.getContext('2d')!
    ctx.drawImage(image, 0, 0)
    const pixels = ctx.getImageData(0, 0, image.width, image.height).data
    const hash = await crypto.subtle.digest('SHA-256', pixels)
    return {
      width: image.width,
      height: image.height,
      hash: Iterator.from(new Uint8Array(hash)).toArray(),
    }
  } finally {
    image.close()
  }
}

// No live app, credentials, network service or renderer is involved.
async function run(kind: string, cancel = false) {
  const psd = fixture(kind)
  const bytes = writePsd(psd, { generateThumbnail: false })
  const master = document.createElement('canvas')
  master.width = 192
  master.height = 256
  const ctx = master.getContext('2d')!
  ctx.fillStyle = '#efcdbc'
  ctx.fillRect(0, 0, master.width, master.height)
  const masterBlob = await new Promise<Blob>((resolve) =>
    master.toBlob((b) => resolve(b!)),
  )
  const url = URL.createObjectURL(masterBlob)
  const stages: string[] = []
  const controller = new AbortController()
  try {
    const imported = await prepareRigPsdImport(
      new File([bytes], 'fixture.psd'),
      url,
      (stage) => {
        stages.push(stage)
        if (cancel && stage === 'packing') controller.abort()
      },
      'fixture-generation',
      controller.signal,
    )
    const reference = document.createElement('canvas')
    reference.width = reference.height = 256
    const referenceCtx = reference.getContext('2d')!
    referenceCtx.drawImage(master, 32, 0)
    const expected = await prepareAnime25DRigPsd(
      decodeRigPsd(bytes),
      url,
      anime25DImportCopy(),
      undefined,
      'fixture-generation',
      psd.width === psd.height
        ? referenceCtx.getImageData(0, 0, 256, 256)
        : undefined,
    )
    return {
      stages,
      sourceEqual:
        JSON.stringify(imported.source) === JSON.stringify(expected.source),
      atlas: await pngPixels(imported.atlas),
      expectedAtlas: await pngPixels(expected.atlas),
      reference: await pngPixels(imported.analysisReference),
      expectedReference: await pngPixels(expected.analysisReference),
      roles: imported.source.anime25dPlayback!.layers.map((l) => l.role),
      partCount: imported.partCount,
    }
  } catch (error) {
    return {
      stages,
      error: (error as Error).message,
      name: (error as Error).name,
    }
  } finally {
    URL.revokeObjectURL(url)
  }
}

async function localizedFailure() {
  localStorage.setItem('locale', 'zh-CN')
  await loadLocale('zh-CN')
  return {
    result: await run('missing-face'),
    expected: currentCopy().merope.anime25dMissingFace,
  }
}

async function relativeSourceFailure() {
  try {
    const bytes = writePsd(fixture('ordinary'), { generateThumbnail: false })
    await prepareRigPsdImport(
      new File([bytes], 'fixture.psd'),
      'assets/master.png',
    )
    return { error: null }
  } catch (error) {
    return {
      error: (error as Error).message,
      expected: currentCopy().merope.psdPreviewFailed,
    }
  }
}

Object.assign(window, {
  rigImportTest: {
    run,
    localizedFailure,
    relativeSourceFailure,
    eyeRuntime,
    eyePixels,
    bodyReplay,
    directorReplay,
    touchPicking,
    touchSurface,
  },
})

async function touchSurface(region = 'face') {
  const psd = fixture('ordinary')
  psd.height = 300
  const prepared = await prepareRigPsdImport(new File([writePsd(psd)], 'touch-live.psd'), '/unused.png')
  const canvas = document.createElement('canvas')
  canvas.id = 'touch-character'
  canvas.style.touchAction = 'none'
  document.body.append(canvas)
  const player = new Anime25DPlayer(canvas, prepared.source.anime25dPlayback!)
  const url = URL.createObjectURL(prepared.atlas)
  await player.loadAtlas(url)
  player.resize(256, 300, 1)
  // This fixture isolates touch. Speech/music overlap has its own full replay;
  // the driver's default talk=true otherwise adds unrequested talking motion.
  player.setTarget({ idle: false, blink: false, rand: false, talk: false })
  player.tick(1 / 60)
  const runtime = new MotionRuntime(new RigMotionCoordinator())
  const events: string[] = []
  const unbind = bindCharacterTouch(canvas, (x, y) => player.hitTestTouch(x, y), (touch, now) => {
    events.push(`${touch.phase}:${touch.gesture}`)
    runtime.touch.update('test-surface', touch, now)
  }, () => runtime.touch.release('test-surface'))
  let raf = 0
  let revision = -1
  const tick = () => {
    const now = performance.now()
    const frame = runtime.frame(now)
    player.setMotionPolicy(frame.snapshot.owners)
    if (frame.behaviorRevision !== revision) {
      revision = frame.behaviorRevision
      if (frame.behaviorPlan) player.setBehaviorMotionUnits(realizeAnime25DBehaviorPlan(frame.behaviorPlan, now).units, now)
      else player.setBehaviorMotionUnits([], now)
    }
    player.tick(1 / 60)
    raf = requestAnimationFrame(tick)
  }
  raf = requestAnimationFrame(tick)
  const bounds = canvas.getBoundingClientRect()
  let point: { x: number; y: number } | null = null
  for (let y = bounds.top + 20; y < bounds.bottom - 20 && !point; y += 10) {
    for (let x = bounds.left + 20; x < bounds.right - 20; x += 10) {
      if (player.hitTestTouch(x, y)?.region === region) { point = { x, y }; break }
    }
  }
  Object.assign(window, { touchSurfaceState: {
    events,
    current: () => ({ active: runtime.touch.current() !== null, form: runtime.touch.current()?.behaviors[0].form.id, pose: player.getCurrent() }),
    dispose: () => {
      unbind(); runtime.touch.release(); cancelAnimationFrame(raf)
      player.dispose(); canvas.remove(); URL.revokeObjectURL(url)
    },
  } })
  return point
}

async function touchPicking(kind: string) {
  const psd = fixture(kind)
  psd.height = 300
  const prepared = await prepareRigPsdImport(
    new File([writePsd(psd)], 'touch.psd'), '/unused-master.png',
  )
  const canvas = document.createElement('canvas')
  document.body.append(canvas)
  const player = new Anime25DPlayer(canvas, prepared.source.anime25dPlayback!)
  const url = URL.createObjectURL(prepared.atlas)
  const gl = canvas.getContext('webgl2')!
  let hits = 0
  let falseHits = 0
  let misses = 0
  const regions = new Set<string>()
  try {
    await player.loadAtlas(url)
    player.resize(256, 300, 1)
    for (const direction of [-0.7, 0, 0.7]) {
      player.setTarget({ angleX: direction, angleY: direction * 0.4, idle: false, blink: false })
      for (let i = 0; i < 45; i++) player.tick(1 / 60)
      const rect = canvas.getBoundingClientRect()
      const pixels = new Uint8Array(canvas.width * canvas.height * 4)
      gl.readPixels(0, 0, canvas.width, canvas.height, gl.RGBA, gl.UNSIGNED_BYTE, pixels)
      for (let y = 5; y < canvas.height; y += 9) {
        for (let x = 5; x < canvas.width; x += 9) {
          const hit = player.hitTestTouch(
            rect.left + (x + 0.5) / canvas.width * rect.width,
            rect.top + (y + 0.5) / canvas.height * rect.height,
          )
          const alpha = pixels[((canvas.height - 1 - y) * canvas.width + x) * 4 + 3]
          if (hit) {
            hits++
            if (hit.region) regions.add(hit.region)
            if (alpha < 10) falseHits++
          } else if (alpha > 240) { misses++
}
        }
      }
    }
    return { hits, falseHits, misses, regions: Iterator.from(regions).toArray(), glError: gl.getError() }
  } finally {
    player.dispose()
    canvas.remove()
    URL.revokeObjectURL(url)
  }
}

async function directorReplay(parallel = false, race = false, touchCase = '', fps = 60, standingExpression?: 'tense' | 'withdrawn') {
  const psd = fixture('ordinary')
  psd.height = 300
  const prepared = await prepareRigPsdImport(
    new File([writePsd(psd)], 'director.psd'),
    '/unused-master.png',
  )
  const canvas = document.createElement('canvas')
  const random = Math.random
  let seed = 713
  if (touchCase) { Math.random = () => {
    seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0
    return seed / 4294967296
  }
}
  const player = new Anime25DPlayer(canvas, prepared.source.anime25dPlayback!)
  const url = URL.createObjectURL(prepared.atlas)
  let now = 1000
  let musicTick: ((time: number) => void) | null = null
  const coordinator = new RigMotionCoordinator()
  const audio = { paused: false, currentTime: 0 }
  const music = new MusicMotionSource(
    coordinator,
    {
      now: () => now,
      raf: (callback) => {
        musicTick = callback
        return 1
      },
      caf: () => {
        musicTick = null
      },
    },
    {
      getCurrentAudio: () => audio,
      getMotionAudioFeatures: () => ({
        energy: 0.6,
        bass: 0.4,
        pulse: 0.5,
        presence: 0.3,
      }),
      connectAudioToAnalyser: () => true,
    },
    { isPageVisible: () => true, onVisibility: () => () => {} },
  )
  const runtime = new MotionRuntime(coordinator, music)
  const tracker = new TouchGestureTracker()
  let touchDeferred: PromiseWithResolvers<unknown> | undefined
  let touchRequests = 0
  let touchApplied = 0
  let touchWriteMutation = false
  const appraisal = new TouchAppraisal({
    now: () => now,
    request: () => {
      touchRequests++
      touchDeferred = Promise.withResolvers<unknown>()
      return touchDeferred.promise
    },
    apply: (revision, reaction) => {
      const before = JSON.stringify(player.getCurrent())
      touchApplied++
      runtime.touch.refine('replay', revision, reaction, now)
      touchWriteMutation ||= before !== JSON.stringify(player.getCurrent())
    },
  })
  const applyState = createMotionApplyState()
  let release: (() => void) | null = null
  let planId: string | null = null
  let accepted = 0
  let rejected = 0
  let planWrites = 0
  const previousGeneration = liveMotionGeneration()
  const raceChecks: Record<string, boolean> = {}
  const deliveredText: string[] = []
  const eventBase = (generation: number) => ({
    source: 'reply',
    generation,
    messageId: `race-${generation}`,
    utteranceId: `race-${generation}`,
  })
  const speak = (generation: number, phase: string, text = '') =>
    dispatchMeropeSpeech({ ...eventBase(generation), phase, text })
  const direct = (
    generation: number,
    intent: string,
    motionIntentId = `intent-${generation}`,
  ) =>
    dispatchMeropePerformance({
      ...eventBase(generation),
      text: '',
      motionIntentId,
      performance: {
        phase: 'delivery',
        moodRevision: 1,
        motionStyle: 'even',
        plan: {
          cues: [
            {
              intent,
              atMs: 0,
              intensity: 1,
              tempo: 1,
              fadeInMs: 80,
              fadeOutMs: 300,
              interrupt: 'replace',
            },
          ],
        },
      },
    })
  const clockDescriptor = Object.getOwnPropertyDescriptor(performance, 'now')
  // Mirrors the character's thin imperative bridge. The source, scheduler,
  // frame writer, realization and player are production implementations.
  const port: Parameters<typeof applyMotionFrame>[0] = {
    setMotionPolicy: (v) => player.setMotionPolicy(v),
    setBearing: (v) => player.setBearing(standingExpression
      ? { expression: standingExpression, posture: 'neutral', motionEnergy: 1, attention: 1 } : v),
    setMood: () => {
      throw new Error('Mood is outside this replay')
    },
    setSpeechActive: (v) => player.setSpeechActive(v),
    setAutoSpeech: (v) => {
      if (!v) player.clearSpeechText()
      player.setTarget({
        talk: v,
        mouthOpen: 0,
        mouthWide: 0,
        mouthRound: 0,
        mouthNarrow: 0,
        mouthSeal: 0,
      })
    },
    setSpeechEnergy: (v) => player.setTarget(speechEnergyDriverPatch(v)),
    setSpeechArticulation: (v) =>
      player.setTarget(speechArticulationDriverPatch(v)),
    setSpeechProsody: (v) => player.setSpeechProsody(v),
    enqueueSpeechText: (text, locale) => {
      deliveredText.push(text)
      player.enqueueSpeechText(text, locale)
    },
    setSinging: (v) => player.setSinging(v),
    setSingingTrack: (v) => player.setSingingTrack(v),
    setMusicSignal: (v) => player.setMusicSignal(v),
    playBehaviorPlan: (plan) => {
      const result = realizeAnime25DBehaviorPlan(plan, now)
      player.setBehaviorMotionUnits(result.units, now)
      planId = plan.id
      planWrites++
      return result.reports
    },
    stopBehaviorPlan: (id) => {
      if (id && id !== planId) return
      player.clearBehaviorMotionUnits()
      planId = null
    },
  }
  const apply = () => {
    const frame = runtime.frame(now)
    applyMotionFrame(port, frame, applyState, (report) => {
      if (report.result === 'accepted') accepted++
      else rejected++
      runtime.reportBehaviorRealizer(
        report.planId,
        report.behaviorId,
        report.result,
        now,
        report.reason,
      )
    })
    return frame
  }
  const publish = (
    intent: 'respond' | 'maniac',
    phase: 'reaction' | 'delivery',
  ) =>
    runtime.performance.handleForTest(
      {
        phase,
        moodRevision: 1,
        motionStyle: 'even',
        plan: {
          cues: [
            {
              intent,
              atMs: 0,
              intensity: 1,
              tempo: 1,
              fadeInMs: 80,
              fadeOutMs: 300,
              interrupt: 'replace',
            },
          ],
        },
      },
      { text: '', source: 'reply', runId: 'replay', messageId: 'replay' },
    )
  try {
    await player.loadAtlas(url)
    player.resize(128, 150, 1)
    player.setTarget({
      idle: false,
      rand: false,
      blink: false,
      talk: false,
      phys: false,
    })
    Object.defineProperty(performance, 'now', {
      configurable: true,
      value: () => now,
    })
    if (parallel) {
      release = runtime.retain()
      music.setTrack({ trackId: 'replay-music', duration: 60 })
      music.setPlayback(true, false)
      if (!race) {
        runtime.speech.handleForTest({
          phase: 'start',
          messageId: 'spoken',
          utteranceId: 'spoken',
          source: 'reply',
        })
        runtime.speech.handleForTest({
          phase: 'chunk',
          messageId: 'spoken',
          utteranceId: 'spoken',
          source: 'reply',
          text: '我们来听听这首歌，接着慢慢聊聊今天发生的事。',
        })
      }
    }
    for (let i = 0; i < 60; i++) player.tick(1 / 60)
    if (race) {
      setLiveMotionGeneration(41)
      speak(41, 'start')
      speak(41, 'chunk', '第一句。')
      direct(41, 'respond')
    } else if (!touchCase) {
      publish('respond', 'reaction')
    }
    const frames = []
    let replacementMutation = false
    let duplicateWrites = 0
    for (let step = 0; step < 5 * fps; step++) {
      const i = step * 60 / fps
      now += 1000 / fps
      if (touchCase && touchCase !== 'control') {
        const before = JSON.stringify(player.getCurrent())
        const sample = { pointerId: 1, x: i < 78 ? (i - 6) * 0.002 : -0.2,
          y: 0, atMs: now, region: i < 78 ? 'hair' as const : 'face' as const }
        const observation = i === 6 || i === 112 ? tracker.begin(sample)
          : i === 100 || i === 140 ? tracker.end(sample) : tracker.update(sample)
        if (observation) {
          const touch = { ...observation, position: { x: i < 78 ? 0.8 : -0.8, y: -0.3 } }
          runtime.touch.notePresented('replay', player.getPresentedTouch(), now)
          runtime.touch.update('replay', touch, now)
          appraisal.observe(touch, runtime.touch.version())
        }
        if (i === (touchCase === 'late' ? 116 : touchCase === 'changed' ? 90 : 60)) {
          if (touchCase === 'fail') touchDeferred?.reject(new Error('injected provider failure'))
          else touchDeferred?.resolve({ reaction: touchCase === 'accept' ? 'accept' : 'withdraw' })
        }
        // Flush the actual request controller's promise chain without waiting
        // on real time or opening a provider connection.
        for (let flush = 0; flush < 5; flush++) await Promise.resolve()
        touchWriteMutation ||= before !== JSON.stringify(player.getCurrent())
      }
      if (parallel) {
        audio.currentTime = (now - 1000) / 1000
        musicTick?.(now)
        if (i === 120 && !race) runtime.speech.stop()
        if (i === 240) {
          audio.paused = true
          music.setPlayback(false, false)
        }
      }
      if (race) {
        if (i === 6) {
          const before = runtime.performance.current().motionIntentId
          speak(41, 'chunk', '后半句继续。')
          raceChecks.chunkKeepsDirector =
            before === runtime.performance.current().motionIntentId
        }
        if (i === 12) {
          speak(41, 'cancel')
          setLiveMotionGeneration(42)
          speak(42, 'start')
          speak(42, 'chunk', '新回复正在说话。')
          direct(42, 'maniac')
        }
        if (i === 18) {
          const before = JSON.stringify(runtime.performance.current())
          direct(41, 'cry', 'late-old')
          speak(41, 'chunk', '过期内容不应播放。')
          speak(41, 'cancel')
          raceChecks.staleKeptDirector =
            before === JSON.stringify(runtime.performance.current())
          raceChecks.staleKeptSpeech =
            runtime.frame(now).snapshot.owners.mouth === 'speech'
        }
        if (i === 24) {
          const before = runtime.performance.current().motionIntentId
          direct(42, 'maniac', 'reconnect-new-id')
          raceChecks.reconnectDeduplicated =
            before === runtime.performance.current().motionIntentId
        }
        if (i === 60) {
          speak(42, 'cancel')
          direct(42, 'cry', 'late-cancelled')
          speak(42, 'chunk', '取消内容不应播放。')
          raceChecks.cancelledPlanAbsent =
            runtime.performance.current().behaviorPlan === null
        }
      }
      if (i === 12 && !race && !touchCase) {
        const before = JSON.stringify(player.getCurrent())
        publish('maniac', 'delivery')
        apply()
        replacementMutation = before !== JSON.stringify(player.getCurrent())
      }
      const frame = apply()
      const writes = planWrites
      applyMotionFrame(port, frame, applyState)
      duplicateWrites += planWrites - writes
      player.tick(1 / fps)
      const pose = player.getCurrent()
      const gl = canvas.getContext('webgl2')!
      const pixels = new Uint8Array(canvas.width * canvas.height * 4)
      gl.readPixels(
        0,
        0,
        canvas.width,
        canvas.height,
        gl.RGBA,
        gl.UNSIGNED_BYTE,
        pixels,
      )
      frames.push({
        at: i / 60,
        touchForm: runtime.touch.current()?.behaviors[0].form.id ?? null,
        presentedTouch: player.getPresentedTouch()?.reaction ?? null,
        touchApplied,
        body: pose.body,
        eyeX: pose.eyeX,
        angleX: pose.angleX,
        angleY: pose.angleY,
        angleZ: pose.angleZ,
        maniac: pose.maniac,
        mouthOpen: pose.mouthOpen,
        mouthForm: pose.mouthForm,
        browAngSym: pose.browAngSym,
        eyeOpenL: pose.eyeOpenL,
        owners: frame.snapshot.owners,
        active: frame.behaviors.map((b) => `${b.form.id}:${b.phase}`),
        musicBehaviors: frame.behaviors.filter((b) => b.source === 'music')
          .length,
        pixelSum: pixels.reduce((sum, v) => sum + v, 0),
      })
    }
    return {
      frames,
      accepted,
      rejected,
      replacementMutation,
      duplicateWrites,
      raceChecks,
      deliveredText,
      touchRequests,
      touchApplied,
      touchWriteMutation,
      glError: canvas.getContext('webgl2')!.getError(),
    }
  } finally {
    Math.random = random
    appraisal.dispose()
    runtime.touch.release()
    release?.()
    setLiveMotionGeneration(previousGeneration)
    if (clockDescriptor)
      Object.defineProperty(performance, 'now', clockDescriptor)
    else Reflect.deleteProperty(performance, 'now')
    runtime.speech.stop()
    player.dispose()
    URL.revokeObjectURL(url)
  }
}

async function bodyReplay(kind: string, fps: number) {
  const psd = fixture(kind)
  psd.height = 300
  for (const layer of psd.children!) {
    const color =
      layer.name === 'neckwear_1'
        ? [0, 255, 0]
        : layer.name === 'topwear'
          ? [0, 0, 255]
          : [180, 80, 80]
    const data = layer.imageData!.data
    for (let i = 0; i < data.length; i += 4) data.set(color, i)
  }
  const prepared = await prepareRigPsdImport(
    new File([writePsd(psd, { generateThumbnail: false })], 'body-replay.psd'),
    '/unused-master.png',
  )
  const canvas = document.createElement('canvas')
  const playback = prepared.source.anime25dPlayback!
  const player = new Anime25DPlayer(canvas, playback)
  const url = URL.createObjectURL(prepared.atlas)
  const state = player as unknown as {
    layers: Anime25DGpuLayer[]
    collarClip: CollarClipMesh | null
    deform: () => void
    uploadGeometry: () => void
    draw: () => void
  }
  const gl = canvas.getContext('webgl2')!
  const geometry = () => [
    ...state.layers.flatMap((l) => [...l.deformed, ...l.layerTransform]),
    ...(state.collarClip?.deformed ?? []),
  ]
  // Matrix translation alone is not displacement: rotation about a pivot has
  // large cancelling translation terms. Measure transformed vertices instead.
  const positions = () =>
    state.layers.flatMap((layer) => {
      const m = layer.layerTransform
      const output: number[] = []
      for (let i = 0; i < layer.deformed.length; i += 2) {
        const x = layer.deformed[i]
        const y = layer.deformed[i + 1]
        output.push(m[0] * x + m[3] * y + m[6], m[1] * x + m[4] * y + m[7])
      }
      return output
    })
  const pixels = () => {
    const data = new Uint8Array(canvas.width * canvas.height * 4)
    gl.readPixels(
      0,
      0,
      canvas.width,
      canvas.height,
      gl.RGBA,
      gl.UNSIGNED_BYTE,
      data,
    )
    return data
  }
  let idempotenceError = 0
  let pixelMismatch = 0
  let targetMutation = 0
  let maxStep = 0
  let excursion = 0
  let invalid = 0
  let minAccessoryPixels = Infinity
  let minClothingPixels = Infinity
  let checkedFrames = 0
  let rootEdges = 0
  let minRootStretch = Infinity
  let maxRootStretch = 0
  try {
    await player.loadAtlas(url)
    player.resize(128, 150, 1)
    player.setTarget({
      idle: false,
      rand: false,
      blink: false,
      talk: false,
      phys: true,
    })
    for (let i = 0; i < fps; i++) player.tick(1 / fps)
    const warmupError = gl.getError()
    const initial = positions()
    let previous = initial
    // Reverse before settling, stop at the current pose, then reverse again.
    // The driver path is deliberate here; this does not claim director coverage.
    const targets = [
      { angleX: 0.7, angleY: -0.4, angleZ: 0.25 },
      { angleX: -0.7, angleY: 0.4, angleZ: -0.25 },
      null,
      { angleX: 0.45, angleY: -0.3, angleZ: 0.15 },
      { angleX: 0, angleY: 0, angleZ: 0 },
    ]
    for (const target of targets) {
      const before = geometry()
      const current = player.getCurrent()
      player.setTarget(
        target ?? {
          angleX: current.angleX,
          angleY: current.angleY,
          angleZ: current.angleZ,
        },
      )
      const after = geometry()
      after.forEach((v, i) => {
        targetMutation = Math.max(targetMutation, Math.abs(v - before[i]))
      })
      for (let frame = 0; frame < fps / 2; frame++) {
        player.tick(1 / fps)
        const actual = geometry()
        const points = positions()
        points.forEach((v, i) => {
          if (!Number.isFinite(v)) invalid++
          maxStep = Math.max(maxStep, Math.abs(v - previous[i]))
          excursion = Math.max(excursion, Math.abs(v - initial[i]))
        })
        previous = points
        for (const layer of state.layers) {
          if (!layer.springs?.length || !layer.alongStrand) continue
          for (
            let vertex = 0;
            vertex < layer.alongStrand.length - 1;
            vertex++
          ) {
            if (
              (vertex + 1) % (layer.cols + 1) === 0 ||
              layer.alongStrand[vertex] > 0.1 ||
              layer.alongStrand[vertex + 1] > 0.1
            ) {
              continue
            }
            const i = vertex * 2
            const restLength = Math.hypot(
              layer.rest[i + 2] - layer.rest[i],
              layer.rest[i + 3] - layer.rest[i + 1],
            )
            if (restLength < 0.001) continue
            const stretch =
              Math.hypot(
                layer.deformed[i + 2] - layer.deformed[i],
                layer.deformed[i + 3] - layer.deformed[i + 1],
              ) / restLength
            minRootStretch = Math.min(minRootStretch, stretch)
            maxRootStretch = Math.max(maxRootStretch, stretch)
            rootEdges++
          }
        }
        const image = pixels()
        let green = 0
        let blue = 0
        for (let i = 0; i < image.length; i += 4) {
          if (image[i] < 8 && image[i + 1] > 64 && image[i + 2] < 8) green++
          if (image[i] < 8 && image[i + 1] < 8 && image[i + 2] > 64) blue++
        }
        minAccessoryPixels = Math.min(minAccessoryPixels, green)
        minClothingPixels = Math.min(minClothingPixels, blue)
        // Re-evaluate from the same pose, without advancing springs or clocks.
        // Force cacheable layers too: caches must not hide accumulating transforms.
        const cacheable = state.layers.map((l) => l.deformationPlan.cacheable)
        try {
          state.layers.forEach((l) => {
            l.deformationPlan.cacheable = false
          })
          state.deform()
          state.uploadGeometry()
          state.draw()
          geometry().forEach((v, i) => {
            idempotenceError = Math.max(
              idempotenceError,
              Math.abs(v - actual[i]),
            )
          })
          pixels().forEach((v, i) => {
            if (v !== image[i]) pixelMismatch++
          })
        } finally {
          state.layers.forEach((l, i) => {
            l.deformationPlan.cacheable = cacheable[i]
          })
        }
        checkedFrames++
      }
    }
    return {
      roles: state.layers.map((l) => l.source.role),
      collarClip: Boolean(state.collarClip),
      hairLayers: state.layers.filter((l) => l.springs?.length).length,
      idempotenceError,
      pixelMismatch,
      targetMutation,
      maxStep,
      excursion,
      invalid,
      minAccessoryPixels,
      minClothingPixels,
      checkedFrames,
      warmupError,
      rootEdges,
      minRootStretch,
      maxRootStretch,
      glError: gl.getError(),
    }
  } finally {
    player.dispose()
    URL.revokeObjectURL(url)
  }
}

async function eyeRuntime() {
  const psd = fixture('alternate-eyes')
  psd.height = 300
  const prepared = await prepareRigPsdImport(
    new File([writePsd(psd, { generateThumbnail: false })], 'eyes.psd'),
    '/unused-master.png',
  )
  const canvas = document.createElement('canvas')
  canvas.width = 256
  canvas.height = 300
  const player = new Anime25DPlayer(canvas, prepared.source.anime25dPlayback!)
  const url = URL.createObjectURL(prepared.atlas)
  // Test-only inspection of actual compiled frame state; no production debug API.
  const state = player as unknown as {
    layers: Array<{
      source: { role: string; side: string }
      frameOpacity: number
    }>
    irisRebound: { x: number; y: number }
  }
  const opacity = (role: string, side: string) =>
    state.layers.find((l) => l.source.role === role && l.source.side === side)
      ?.frameOpacity ?? 0
  const advance = (frames = 60) => {
    for (let i = 0; i < frames; i++) player.tick(1 / 60)
  }
  try {
    await player.loadAtlas(url)
    player.setTarget({
      idle: false,
      rand: false,
      talk: false,
      blink: false,
      phys: false,
    })
    advance()
    player.blinkNow()
    let ordinaryBlink = 0
    let alternateBlink = 0
    for (let i = 0; i < 60; i++) {
      player.tick(1 / 60)
      ordinaryBlink = Math.max(ordinaryBlink, opacity('eye-close', 'L'))
      alternateBlink = Math.max(alternateBlink, opacity('eye-close2', 'L'))
    }
    player.setTarget({ eyeOpenL: 0 })
    advance()
    const wink = {
      left: opacity('eye-close2', 'L'),
      ordinaryLeft: opacity('eye-close', 'L'),
      right: opacity('eye-close', 'R'),
    }
    player.setTarget({ eyeOpenR: 0 })
    advance()
    const both = {
      left: opacity('eye-close2', 'L'),
      right: opacity('eye-close', 'R'),
    }
    player.setTarget({ eyeCry: 1 })
    advance()
    const cry = opacity('eye-close2', 'L')
    player.setTarget({ eyeOpenL: 1, eyeOpenR: 1, eyeCry: 0, blink: true })
    advance()
    player.blinkNow()
    let rebound = 0
    for (let i = 0; i < 90; i++) {
      player.tick(1 / 60)
      if (player.getCurrent().eyeOpenL > 0.6)
        rebound = Math.max(rebound, Math.abs(state.irisRebound.x - 1))
    }
    return {
      ordinaryBlink,
      alternateBlink,
      wink,
      both,
      cry,
      rebound,
      glError: canvas.getContext('webgl2')!.getError(),
    }
  } finally {
    player.dispose()
    URL.revokeObjectURL(url)
  }
}

// Diagnostic colors make visibility measurable without a machine-specific PNG
// baseline. The real importer, deformation, shaders and stencil all still run.
async function eyePixels(fps: number) {
  const psd = fixture('alternate-eyes')
  psd.height = 300
  for (const layer of psd.children!) {
    const color =
      layer.name === 'irides'
        ? [255, 0, 0]
        : layer.name === 'eye_close2'
          ? [0, 255, 0]
          : layer.name === 'eyelash_c'
            ? [0, 0, 255]
            : [255, 255, 255]
    const data = layer.imageData!.data
    for (let i = 0; i < data.length; i += 4) data.set(color, i)
  }
  const prepared = await prepareRigPsdImport(
    new File([writePsd(psd, { generateThumbnail: false })], 'pixel-eyes.psd'),
    '/unused-master.png',
  )
  const playback = prepared.source.anime25dPlayback!
  // Isolate eyes so hair/face cannot conceal a failed draw or leaking iris.
  playback.layers = playback.layers.filter((l) =>
    ['eyewhite', 'irides', 'eye-close', 'eye-close2'].includes(l.role),
  )
  const canvas = document.createElement('canvas')
  const player = new Anime25DPlayer(canvas, playback)
  const url = URL.createObjectURL(prepared.atlas)
  interface Layer {
    source: { role: string; side: string }
    frameOpacity: number
    deformed: Float32Array
  }
  const state = player as unknown as { layers: Layer[]; draw: () => void }
  const gl = canvas.getContext('webgl2')!
  const read = () => {
    const pixels = new Uint8Array(canvas.width * canvas.height * 4)
    gl.readPixels(
      0,
      0,
      canvas.width,
      canvas.height,
      gl.RGBA,
      gl.UNSIGNED_BYTE,
      pixels,
    )
    return pixels
  }
  const colors = (pixels: Uint8Array) => {
    const counts = [0, 0, 0]
    for (let i = 0; i < pixels.length; i += 4) {
      for (let channel = 0; channel < 3; channel++) {
        if (
          pixels[i + channel] > 32 &&
          pixels[i + ((channel + 1) % 3)] < 8 &&
          pixels[i + ((channel + 2) % 3)] < 8
        ) {
          counts[channel]++
        }
      }
    }
    return counts
  }
  let outsideMask = 0
  let invalidVertices = 0
  let samples = 0
  let phase = ''
  let maxCoverageError = 0
  let reversals = 0
  const blinkColors = [0, 0, 0]
  const endpoints: Record<string, number[]> = {}
  const sample = () => {
    const layers = state.layers
    const opacity = layers.map((l) => l.frameOpacity)
    if (['open', 'wink', 'closed', 'turn', 'reverse'].includes(phase)) {
      for (const side of ['L', 'R']) {
        const coverage = layers.reduce(
          (sum, layer) =>
            sum +
            (layer.source.side === side && layer.source.role !== 'irides'
              ? layer.frameOpacity
              : 0),
          0,
        )
        maxCoverageError = Math.max(maxCoverageError, Math.abs(coverage - 1))
      }
    }
    for (const layer of layers) {
      for (const value of layer.deformed) {
        if (!Number.isFinite(value)) invalidVertices++
      }
    }
    try {
      // Independently render each eye's white as a pixel oracle, then its iris
      // with that same white hidden. Hidden whites must still clip the iris.
      for (const side of ['L', 'R']) {
        state.layers = layers.filter(
          (l) => l.source.side === side && l.source.role === 'eyewhite',
        )
        for (const layer of state.layers) layer.frameOpacity = 1
        state.draw()
        const mask = read()
        state.layers = layers.filter(
          (l) =>
            l.source.side === side &&
            ['eyewhite', 'irides'].includes(l.source.role),
        )
        for (const layer of state.layers) {
          layer.frameOpacity =
            layer.source.role === 'eyewhite'
              ? 0
              : opacity[layers.indexOf(layer)]
        }
        state.draw()
        const iris = read()
        for (let i = 3; i < iris.length; i += 4) {
          if (iris[i] > 8 && mask[i] === 0) outsideMask++
        }
      }
    } finally {
      state.layers = layers
      layers.forEach((l, i) => {
        l.frameOpacity = opacity[i]
      })
      state.draw()
    }
    samples++
  }
  const advance = (
    name: string,
    target: Parameters<Anime25DPlayer['setTarget']>[0],
  ) => {
    phase = name
    player.setTarget(target)
    let previousOpen = player.getCurrent().eyeOpenL
    const direction =
      target.eyeOpenL === undefined
        ? 0
        : Math.sign(target.eyeOpenL - previousOpen)
    for (let frame = 0; frame < fps; frame++) {
      player.tick(1 / fps)
      const open = player.getCurrent().eyeOpenL
      if (direction && (open - previousOpen) * direction < -1e-6) reversals++
      previousOpen = open
      sample()
      if (name === 'blink') {
        const counts = colors(read())
        counts.forEach((count, i) => {
          blinkColors[i] = Math.max(blinkColors[i], count)
        })
      }
    }
    endpoints[name] = colors(read())
  }
  try {
    await player.loadAtlas(url)
    player.resize(128, 150, 1)
    player.setTarget({
      idle: false,
      rand: false,
      talk: false,
      blink: false,
      phys: false,
    })
    advance('open', { eyeOpenL: 1, eyeOpenR: 1 })
    player.blinkNow()
    advance('blink', {})
    advance('wink', { eyeOpenL: 0 })
    advance('closed', { eyeOpenR: 0 })
    advance('special', { eyeCry: 1 })
    advance('reopen', { eyeCry: 0, eyeOpenL: 1, eyeOpenR: 1 })
    advance('turn', { angleX: 0.6, angleY: -0.3, eyeX: 1 })
    advance('reverse', { angleX: -0.6, angleY: 0.3, eyeX: -1 })
    return {
      endpoints,
      outsideMask,
      invalidVertices,
      maxCoverageError,
      reversals,
      blinkColors,
      samples,
      glError: gl.getError(),
    }
  } finally {
    player.dispose()
    URL.revokeObjectURL(url)
  }
}
