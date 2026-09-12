/** 先做可安装性预检，以免下载后台会拒的包。 */

import type { TappPlaygroundProject } from '../services/TappPlaygroundService'
import {
  PlaygroundPackageValidationError,
  validatePlaygroundPackage,
} from './validatePlaygroundPackage'

function sanitizeFilename(id: string): string {
  const cleaned = id
    .trim()
    .replaceAll(/[^\w.-]+/g, '_')
    .replaceAll(/_+/g, '_')
    .replaceAll(/^[_.]+|[_.]+$/g, '')
  return cleaned || 'tapp'
}

function triggerDownload(blob: Blob, filename: string): void {
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = filename
  document.body.appendChild(a)
  a.click()
  document.body.removeChild(a)
  URL.revokeObjectURL(url)
}

export async function exportPlaygroundProjectAsTapp(
  project: TappPlaygroundProject,
): Promise<string> {
  const validation = validatePlaygroundPackage(project)
  if (!validation.ok) {
    throw new PlaygroundPackageValidationError(validation.errors)
  }

  const JSZip = (await import('jszip')).default
  const zip = new JSZip()
  const { manifest, files } = validation.package

  for (const [path, content] of Object.entries(files)) {
    zip.file(path, content)
  }

  const blob = await zip.generateAsync({
    type: 'blob',
    compression: 'DEFLATE',
    compressionOptions: { level: 6 },
  })
  const filename = `${sanitizeFilename(manifest.id)}.tapp`
  triggerDownload(blob, filename)
  return filename
}
