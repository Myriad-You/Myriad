import { writePsd } from 'ag-psd'
import { Anime25DPlayer } from '../../../src/features/merope/anime25drig/player'
import { anime25DImportCopy } from '../../../src/features/merope/rig/anime25dImportCopy'
import { prepareAnime25DRigPsd } from '../../../src/features/merope/rig/anime25dImporter'
import { syntheticSeeThroughPsd } from '../../../src/features/merope/rig/anime25dImporter.fixture'
import { decodeRigPsd } from '../../../src/features/merope/rig/psdDecode'
import { prepareRigPsdImport } from '../../../src/features/merope/rig/psdImporter'
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
      hash: Array.from(new Uint8Array(hash)),
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
  rigImportTest: { run, localizedFailure, relativeSourceFailure, eyeRuntime },
})

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
