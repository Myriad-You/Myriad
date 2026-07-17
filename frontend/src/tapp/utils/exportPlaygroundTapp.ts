/**
 * Client-side .tapp export from Playground project state.
 * Zips the shared package file map from `buildPlaygroundPackageFiles` so the
 * archive matches direct-install staging (reinstall via install-file).
 */

import type { TappPlaygroundProject } from '../services/TappPlaygroundService'
import { buildPlaygroundPackageFiles } from './playgroundPackageFiles'

function sanitizeFilename(id: string): string {
  const cleaned = id
    .trim()
    .replace(/[^\w.-]+/g, '_')
    .replace(/_+/g, '_')
    .replace(/^[_.]+|[_.]+$/g, '')
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

/**
 * Build a .tapp ZIP from the current playground project and download it.
 */
export async function exportPlaygroundProjectAsTapp(
  project: TappPlaygroundProject,
): Promise<string> {
  const JSZip = (await import('jszip')).default
  const zip = new JSZip()
  const { manifest, files } = buildPlaygroundPackageFiles(
    project.manifest,
    project.code,
  )

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
