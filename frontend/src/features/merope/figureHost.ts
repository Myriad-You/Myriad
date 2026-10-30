/**
 * What the page hosting the figure lends it. The figure knows no locale, API
 * origin or turn telemetry of its own: until the page installs a host it
 * speaks English, resolves assets against the page and keeps what it did with
 * a turn to itself.
 */
export interface FigureCopy {
  anime25dPlaybackFailed: string
  anime25dWebglFailed: string
  psdNeedAsset: string
  psdTooLarge: string
  anime25dMissingFace: string
  anime25dMissingLayer: string
  anime25dContractMissing: string
  anime25dPartCount: string
  anime25dBoneLimit: string
  anime25dLayerTooWide: string
  anime25dAtlasOverflow: string
  rigAtlasFailed: string
  canvasUnsupported: string
  psdPreviewFailed: string
  psdLayerCountInvalid: string
  psdSpecInvalid: string
}

export interface FigureFrameSample {
  dropped: boolean
  cpuMs?: number
}

/** Reasons the figure drops a motion the page sent it. */
export type FigureTraceDrop = 'stale_generation' | 'foreign_speech_frame'

/** Where the figure reports what it did with a turn, for the page's own trace. */
export interface FigureTrace {
  mark: (span: string, extra?: Record<string, string | number | boolean>) => void
  drop: (reason: FigureTraceDrop) => void
  leaseExpiry: (count: number) => void
  frame: (sample: FigureFrameSample) => void
}

export interface FigureHost {
  copy: () => FigureCopy
  /** Where the API lives; empty is the page's own origin. */
  apiUrl: () => string
  trace: FigureTrace
}

const ENGLISH_COPY: FigureCopy = {
  anime25dPlaybackFailed: '2.5D motion could not play.',
  anime25dWebglFailed: 'This browser does not support WebGL2, so 2.5D motion cannot play.',
  psdNeedAsset: 'The current master portrait is missing an asset id.',
  psdTooLarge: 'The PSD must be smaller than 32 MB.',
  anime25dMissingFace: 'The Anime2.5DRig PSD is missing the required face layer.',
  anime25dMissingLayer: 'The Anime2.5DRig PSD is missing the {role} layer.',
  anime25dContractMissing: 'The 2.5D visual contract is missing: {missing}',
  anime25dPartCount: 'Anime2.5DRig part count must be 1–{max}.',
  anime25dBoneLimit: 'Anime2.5DRig bone count exceeds {max}.',
  anime25dLayerTooWide: 'Layer {id} is wider than {max}.',
  anime25dAtlasOverflow: 'A layer does not fit a {max} canvas.',
  rigAtlasFailed: 'Failed to generate the rig atlas PNG.',
  canvasUnsupported: 'This browser does not support 2D Canvas.',
  psdPreviewFailed: 'Could not compose a PSD preview.',
  psdLayerCountInvalid: 'The PSD must have 1–64 visible pixel layers.',
  psdSpecInvalid: 'The PSD must be a 256–4096px 8-bit RGB document.',
}

const DEFAULT_HOST: FigureHost = {
  copy: () => ENGLISH_COPY,
  apiUrl: () => '',
  trace: {
    mark: () => {},
    drop: () => {},
    leaseExpiry: () => {},
    frame: () => {},
  },
}

let current = DEFAULT_HOST

export function installFigureHost(host: Partial<FigureHost>): void {
  current = { ...DEFAULT_HOST, ...host }
}

export function figureHost(): FigureHost {
  return current
}
