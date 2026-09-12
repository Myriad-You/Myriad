export interface Anime25DDriver {
  angleX: number
  angleY: number
  angleZ: number
  eyeOpenL: number
  eyeOpenR: number
  eyeDizzy: number
  eyeSqueeze: number
  eyeCry: number
  anger: number
  speechless: number
  maniac: number
  silly: number
  lovestruck: number
  eyeX: number
  eyeY: number
  brow: number
  mouthOpen: number
  mouthWide: number
  mouthRound: number
  mouthNarrow: number
  mouthSeal: number
  mouthForm: number
  mouthCY: number
  body: number
  bodyYaw: number
  physAmp: number
  soft: number
  browAngL: number
  browAngR: number
  browAngSym: number
  bangL: number
  bangC: number
  bangR: number
  armY: number
  armPos: number
  bust: number
  bustY: number
  irisScale: number
  mouthEase: number
  eyeEase: number
  fhAmp: number
  fhSoft: number
  eyeCY: number
  eyeCAng: number
  mouthCAng: number
  eyeScaleL: number
  eyeScaleR: number
  mouthScale: number
  idle: boolean
  blink: boolean
  rand: boolean
  thinking: boolean
  singing: boolean
  talk: boolean
  mouse: boolean
  phys: boolean
}

export const DEFAULT_FRONT_HAIR_SWAY = 1
export const DEFAULT_REAR_HAIR_SWAY = 0.5

export const IDENTITY_DRIVER: Anime25DDriver = {
  angleX: 0,
  angleY: 0,
  angleZ: 0,
  eyeOpenL: 1,
  eyeOpenR: 1,
  eyeDizzy: 0,
  eyeSqueeze: 0,
  eyeCry: 0,
  anger: 0,
  speechless: 0,
  maniac: 0,
  silly: 0,
  lovestruck: 0,
  eyeX: 0,
  eyeY: 0,
  brow: 0,
  mouthOpen: 0,
  mouthWide: 0,
  mouthRound: 0,
  mouthNarrow: 0,
  mouthSeal: 0,
  mouthForm: 0,
  mouthCY: 0,
  body: 0,
  bodyYaw: 1,
  physAmp: DEFAULT_REAR_HAIR_SWAY,
  soft: 2,
  browAngL: 0,
  browAngR: 0,
  browAngSym: 0,
  bangL: 0,
  bangC: 0,
  bangR: 0,
  armY: 0,
  armPos: 0,
  bust: 2.5,
  bustY: 1,
  irisScale: 1,
  mouthEase: 0.45,
  eyeEase: 0.3,
  fhAmp: DEFAULT_FRONT_HAIR_SWAY,
  fhSoft: 0.4,
  eyeCY: 0,
  eyeCAng: 0,
  mouthCAng: 0,
  eyeScaleL: 1,
  eyeScaleR: 1,
  mouthScale: 1,
  idle: true,
  blink: true,
  rand: true,
  thinking: false,
  singing: false,
  talk: true,
  mouse: false,
  phys: true,
}

export const WORKBENCH_DRIVER: Anime25DDriver = {
  ...IDENTITY_DRIVER,
  idle: false,
  rand: false,
  talk: false,
  blink: true,
  mouse: false,
  phys: true,
}

const DRIVER_LIMITS: Partial<
  Record<keyof Anime25DDriver, readonly [number, number]>
> = {
  angleX: [-1, 1],
  angleY: [-1, 1],
  angleZ: [-1, 1],
  eyeOpenL: [0, 1],
  eyeOpenR: [0, 1],
  eyeDizzy: [0, 1],
  eyeSqueeze: [0, 1],
  eyeCry: [0, 1],
  anger: [0, 1],
  speechless: [0, 1],
  maniac: [0, 1],
  silly: [0, 1],
  lovestruck: [0, 1],
  eyeX: [-1, 1],
  eyeY: [-1, 1],
  brow: [-1, 1],
  mouthOpen: [0, 1],
  mouthWide: [0, 1],
  mouthRound: [0, 1],
  mouthNarrow: [0, 1],
  mouthSeal: [0, 1],
  mouthForm: [-1, 1],
  mouthCY: [-1, 1],
  body: [-1, 1],
  bodyYaw: [0, 1],
  physAmp: [0, 3],
  soft: [0, 3],
  browAngL: [-1, 1],
  browAngR: [-1, 1],
  browAngSym: [-1, 1],
  bangL: [-1, 1],
  bangC: [-1, 1],
  bangR: [-1, 1],
  armY: [-1, 1],
  armPos: [-1, 1],
  bust: [0, 4],
  bustY: [-3, 3],
  irisScale: [0.5, 1.3],
  mouthEase: [0, 1],
  eyeEase: [0, 1],
  fhAmp: [0, 3],
  fhSoft: [0, 2],
  eyeCY: [-1, 1],
  eyeCAng: [-1, 1],
  mouthCAng: [-1, 1],
  eyeScaleL: [0.5, 1.5],
  eyeScaleR: [0.5, 1.5],
  mouthScale: [0.5, 1.5],
}

export function sanitizeDriverPatch(
  partial: Partial<Anime25DDriver>,
): Partial<Anime25DDriver> {
  const sanitized: Partial<Anime25DDriver> = {}
  const output = sanitized as Record<string, unknown>
  for (const [rawKey, rawValue] of Object.entries(partial)) {
    const key = rawKey as keyof Anime25DDriver
    const identityValue = IDENTITY_DRIVER[key]
    if (typeof identityValue === 'boolean') {
      if (typeof rawValue === 'boolean') output[rawKey] = rawValue
      continue
    }
    if (typeof rawValue !== 'number' || !Number.isFinite(rawValue)) continue
    const limits = DRIVER_LIMITS[key]
    output[rawKey] = limits
      ? Math.max(limits[0], Math.min(limits[1], rawValue))
      : rawValue
  }
  return sanitized
}
