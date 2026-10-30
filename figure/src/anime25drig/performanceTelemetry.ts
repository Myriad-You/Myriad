export interface Anime25DFrameWork {
  deformedLayers: number
  deformedVertices: number
  uploadedBytes: number
  uploadSubmitMs: number
  shaderOnlyLayers: number
  skippedVertices: number
  savedUploadBytes: number
  drawnLayers: number
  drawCalls: number
}

export interface Anime25DPerformanceSample extends Anime25DFrameWork {
  frameCpuMs: number
  driverMs: number
  springsMs: number
  deformMs: number
  drawSubmitMs: number
}

export interface Anime25DPerformanceSnapshot extends Anime25DPerformanceSample {
  samples: number
}

const SAMPLE_INTERVAL_FRAMES = 30
const OBSERVATION_WINDOW_FRAMES = 120
const SMOOTHING = 0.25

const EMPTY_SAMPLE: Anime25DPerformanceSample = {
  frameCpuMs: 0,
  driverMs: 0,
  springsMs: 0,
  deformMs: 0,
  uploadSubmitMs: 0,
  shaderOnlyLayers: 0,
  skippedVertices: 0,
  savedUploadBytes: 0,
  drawSubmitMs: 0,
  deformedLayers: 0,
  deformedVertices: 0,
  uploadedBytes: 0,
  drawnLayers: 0,
  drawCalls: 0,
}
const SAMPLE_KEYS = Object.keys(EMPTY_SAMPLE) as Array<
  keyof Anime25DPerformanceSample
>

export class Anime25DPerformanceTelemetry {
  private frame = 0
  private observationFrames = 0
  private samples = 0
  private readonly average: Anime25DPerformanceSample = { ...EMPTY_SAMPLE }

  shouldSample(): boolean {
    if (this.observationFrames <= 0) return false
    this.observationFrames -= 1
    const sample = this.frame % SAMPLE_INTERVAL_FRAMES === 0
    this.frame += 1
    return sample
  }

  record(sample: Anime25DPerformanceSample): void {
    const alpha = this.samples === 0 ? 1 : SMOOTHING
    for (const key of SAMPLE_KEYS) {
      const value = finiteNonNegative(sample[key])
      this.average[key] += (value - this.average[key]) * alpha
    }
    this.samples += 1
  }

  snapshot(): Anime25DPerformanceSnapshot {
    return { ...this.average, samples: this.samples }
  }

  observe(): Anime25DPerformanceSnapshot {
    this.observationFrames = OBSERVATION_WINDOW_FRAMES
    return this.snapshot()
  }
}

export function createAnime25DFrameWork(): Anime25DFrameWork {
  return {
    deformedLayers: 0,
    deformedVertices: 0,
    uploadedBytes: 0,
    uploadSubmitMs: 0,
    shaderOnlyLayers: 0,
    skippedVertices: 0,
    savedUploadBytes: 0,
    drawnLayers: 0,
    drawCalls: 0,
  }
}

function finiteNonNegative(value: number): number {
  return Number.isFinite(value) ? Math.max(0, value) : 0
}
