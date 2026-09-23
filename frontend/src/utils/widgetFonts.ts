import { API_URL } from '../config'
import { currentCopy } from '../i18n/localeCopy'
import { apiService } from '../services/api'

export const WIDGET_FONT_MAX_BYTES = 2 * 1024 * 1024
export const WIDGET_FONT_URL_RE =
  /^\/api\/home\/widget-fonts\/[a-f0-9]{64}\.(woff2|woff|ttf|otf)$/

const FONT_URL_PREFIX = '/api/home/widget-fonts/'

export function sanitizeWidgetFontUrl(raw: unknown): string {
  if (typeof raw !== 'string') return ''
  const url = raw.trim()
  return WIDGET_FONT_URL_RE.test(url) ? url : ''
}

export function widgetFontFamilyName(url: string): string {
  return `gp${url.slice(FONT_URL_PREFIX.length, FONT_URL_PREFIX.length + 16)}`
}

export function widgetFontFaceUrl(url: string): string {
  return `${API_URL}${url}`
}

function readFileAsDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = () => {
      const result = typeof reader.result === 'string' ? reader.result : ''
      if (!result.startsWith('data:')) {
        reject(new Error(currentCopy().gamePresenceWidget.customFontFailed))
        return
      }
      resolve(result)
    }
    reader.onerror = () => {
      reject(new Error(currentCopy().gamePresenceWidget.customFontFailed))
    }
    reader.readAsDataURL(file)
  })
}

export async function uploadWidgetFont(file: File): Promise<string> {
  const failed = currentCopy().gamePresenceWidget.customFontFailed
  if (file.size <= 0 || file.size > WIDGET_FONT_MAX_BYTES) {
    throw new Error(failed)
  }
  const font = await readFileAsDataUrl(file)
  const data = await apiService.post('/home/widget-fonts', { font })
  const url = sanitizeWidgetFontUrl(
    data && typeof data === 'object' && Object.hasOwn(data, 'url')
      ? (data as { url: unknown }).url
      : '',
  )
  if (!url) {
    throw new Error(failed)
  }
  return url
}
