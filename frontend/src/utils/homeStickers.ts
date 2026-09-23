import { currentCopy } from '../i18n/localeCopy'
import { apiService } from '../services/api'

export interface GenerateHomeStickerInput {
  prompt: string
  width: number
  height: number
  referenceImages?: string[]
  aspect?: string
  slotCols?: number
  slotRows?: number
}

export interface GeneratedHomeSticker {
  imageUrl: string
  width: number
  height: number
}

export const HOME_STICKER_MAX_REFERENCES = 4
const MAX_PROMPT_CHARS = 2_000
/** Keep in sync with MEROPE_PROXY_TIMEOUT_MS. */
const STICKER_REQUEST_TIMEOUT_MS = 15 * 60 * 1000

export function normalizeHomeStickerPrompt(raw: string): string {
  return raw.trim()
}

export function parseGenerateHomeStickerResponse(
  data: unknown,
): GeneratedHomeSticker {
  if (!data || typeof data !== 'object') {
    throw new Error(currentCopy().home.stickerFailed)
  }
  const record = data as Record<string, unknown>
  const imageUrl = record.imageUrl
  if (typeof imageUrl !== 'string' || !imageUrl.trim()) {
    throw new Error(currentCopy().home.stickerFailed)
  }
  return {
    imageUrl: imageUrl.trim(),
    width: typeof record.width === 'number' ? record.width : 0,
    height: typeof record.height === 'number' ? record.height : 0,
  }
}

export async function generateHomeSticker(
  input: GenerateHomeStickerInput,
): Promise<GeneratedHomeSticker> {
  const prompt = normalizeHomeStickerPrompt(input.prompt)
  if (!prompt || prompt.length > MAX_PROMPT_CHARS) {
    throw new Error(currentCopy().home.stickerFailed)
  }
  const referenceImages = (input.referenceImages ?? []).slice(
    0,
    HOME_STICKER_MAX_REFERENCES,
  )
  const data = await apiService.post(
    '/home/stickers/generate',
    {
      prompt,
      width: input.width,
      height: input.height,
      referenceImages,
      aspect: input.aspect,
      slotCols: input.slotCols,
      slotRows: input.slotRows,
    },
    { timeout: STICKER_REQUEST_TIMEOUT_MS },
  )
  return parseGenerateHomeStickerResponse(data)
}

export async function uploadHomeSticker(input: {
  image: string
}): Promise<GeneratedHomeSticker> {
  const image = input.image.trim()
  if (!image.startsWith('data:image/')) {
    throw new Error(currentCopy().home.stickerUploadFailed)
  }
  const data = await apiService.post(
    '/home/stickers/upload',
    { image },
    { timeout: STICKER_REQUEST_TIMEOUT_MS },
  )
  return parseGenerateHomeStickerResponse(data)
}
