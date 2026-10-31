import type { PhantasiNoteDoc } from '../../../../types/phantasi'
import type { ImportProgress, NoteTransferKind, TransferNote } from './types'
import { formatCurrent } from '../../../../i18n/localeCopy'
import { ApiError } from '../../../../services/api'
import * as phantasiApi from '../../../../services/phantasiApi'
import { userFacingError } from '../../../../utils/userFacingError'
import { isOldPathTakenError } from '../../notes/noteOldPaths'
import { isSlugTakenError } from '../../notes/noteSlug'
import { showPhantasiError } from '../../phantasiNotice'
import { addressFields } from './addresses'
import { parseHaloJson, serializeHaloJson } from './halo'
import { parseMarkdownFile, serializeMarkdownFile } from './markdown'
import { fileSlug } from './text'
import { parseTypechoXml, serializeTypechoXml } from './typecho'
import { parseWordpressXml, serializeWordpressXml } from './wordpress'

export interface ContentIoCopy {
  exportSuccess: string
  errorExportFailed: string
  importStepReading: string
  importStepParsing: string
  importStepImporting: string
  errorInvalidFormat: string
  importSuccess: string
  errorImportFailed: string
  untitled: string
  siteTitle: string
}

type IoResult = { ok: true; message: string } | { ok: false; error: string }

interface ZipEntry {
  name: string
  dir: boolean
  async: (kind: 'string') => Promise<string>
}

interface ZipReader {
  file: (path: string | RegExp) => ZipEntry | ZipEntry[] | null
}

async function loadJSZip() {
  const module = await import('jszip')
  return module.default
}

function downloadBlob(blob: Blob, filename: string): void {
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = filename
  document.body.appendChild(a)
  a.click()
  document.body.removeChild(a)
  URL.revokeObjectURL(url)
}

function fill(
  template: string,
  params: Record<string, string | number>,
): string {
  return formatCurrent(template, params)
}

function dayStamp(): string {
  return new Date().toISOString().slice(0, 10)
}

function fromDocs(docs: readonly PhantasiNoteDoc[]): TransferNote[] {
  return docs.map((doc) => ({
    title: doc.title,
    content_md: doc.content_md,
    topic: doc.topic,
    published_at: doc.published_at,
    status: doc.status === 'published' ? 'published' : 'draft',
    ...(doc.status === 'published'
      ? addressFields(doc.slug?.trim() || null, doc.old_paths ?? [])
      : {}),
  }))
}

/** 旧地址被别的文章占着，或这里不收（400 里同样带 old address）。 */
function isOldPathRejected(error: unknown): boolean {
  if (isOldPathTakenError(error)) return true
  return error instanceof ApiError && error.status === 400 &&
    error.message.toLowerCase().includes('old address')
}

/**
 * 发布导入的一篇。slug 被占用就退回自动分配；旧地址被别的文章占着就不带旧地址，
 * 文章照样发出去，旧地址的问题另外提示。
 */
export async function publishImported(
  publish: typeof phantasiApi.publishNoteDoc,
  docId: number,
  revision: number,
  body: { title: string; content_md: string; topic: string | null; published_at: number | null },
  note: TransferNote,
  onAddressDropped: (error: unknown) => void,
): Promise<void> {
  let slug = note.slug
  let oldPaths = note.old_paths
  for (;;) {
    try {
      await publish(docId, {
        ...body,
        ...(slug ? { slug } : {}),
        ...(oldPaths?.length ? { old_paths: oldPaths } : {}),
        revision,
      })
      return
    } catch (err) {
      if (slug && isSlugTakenError(err)) {
        slug = undefined
        continue
      }
      if (oldPaths?.length && isOldPathRejected(err)) {
        oldPaths = undefined
        onAddressDropped(err)
        continue
      }
      throw err
    }
  }
}

function zipEntries(
  zip: ZipReader,
  pattern: RegExp,
): Array<{
  name: string
  dir: boolean
  async: (kind: 'string') => Promise<string>
}> {
  const found = zip.file(pattern)
  if (!found) return []
  return Array.isArray(found) ? found : [found]
}

function keepZipPath(name: string): boolean {
  return !/(^|\/)(\.|__)/.test(name)
}

export async function parseTransferFile(
  kind: NoteTransferKind,
  file: File,
): Promise<TransferNote[]> {
  const name = file.name.toLowerCase()
  if (kind === 'wordpress') return parseWordpressXml(await file.text())
  if (kind === 'typecho') return parseTypechoXml(await file.text())
  if (kind === 'halo') {
    if (name.endsWith('.zip')) {
      const JSZip = await loadJSZip()
      const zip = (await JSZip.loadAsync(file)) as ZipReader
      const jsonFile = zipEntries(zip, /\.json$/i).find(
        (entry) => !entry.dir && keepZipPath(entry.name),
      )
      if (jsonFile) return parseHaloJson(await jsonFile.async('string'))
      return parseMarkdownZip(zip)
    }
    return parseHaloJson(await file.text())
  }
  if (name.endsWith('.md')) {
    return [parseMarkdownFile(file.name, await file.text())]
  }
  const JSZip = await loadJSZip()
  return parseMarkdownZip((await JSZip.loadAsync(file)) as ZipReader)
}

async function parseMarkdownZip(zip: ZipReader): Promise<TransferNote[]> {
  const notes: TransferNote[] = []
  for (const entry of zipEntries(zip, /\.md$/i)) {
    if (entry.dir || !keepZipPath(entry.name)) continue
    const base = entry.name.split('/').pop() || entry.name
    notes.push(parseMarkdownFile(base, await entry.async('string')))
  }
  return notes
}

export async function exportTransferFile(
  kind: NoteTransferKind,
  docs: readonly PhantasiNoteDoc[],
  copy: ContentIoCopy,
  signal?: AbortSignal,
): Promise<IoResult> {
  try {
    const notes = fromDocs(docs)
    if (signal?.aborted) return { ok: false, error: copy.errorExportFailed }
    if (kind === 'wordpress') {
      downloadBlob(
        new Blob([serializeWordpressXml(notes, copy.siteTitle)], {
          type: 'text/xml',
        }),
        `phantasi-wordpress-${dayStamp()}.xml`,
      )
    } else if (kind === 'halo') {
      downloadBlob(
        new Blob([serializeHaloJson(notes)], { type: 'application/json' }),
        `phantasi-halo-${dayStamp()}.json`,
      )
    } else if (kind === 'typecho') {
      downloadBlob(
        new Blob([serializeTypechoXml(notes)], { type: 'text/xml' }),
        `phantasi-typecho-${dayStamp()}.xml`,
      )
    } else {
      const JSZip = await loadJSZip()
      const zip = new JSZip()
      const used = new Set<string>()
      notes.forEach((note, index) => {
        let slug = fileSlug(note.title, index)
        if (used.has(slug)) slug = `${slug}-${index + 1}`
        used.add(slug)
        zip.file(`${slug}.md`, serializeMarkdownFile(note))
      })
      if (signal?.aborted) return { ok: false, error: copy.errorExportFailed }
      downloadBlob(
        await zip.generateAsync({ type: 'blob' }),
        `phantasi-markdown-${dayStamp()}.zip`,
      )
    }
    return {
      ok: true,
      message: fill(copy.exportSuccess, { count: notes.length }),
    }
  } catch {
    return { ok: false, error: copy.errorExportFailed }
  }
}

export async function importTransferFile(
  kind: NoteTransferKind,
  file: File,
  copy: ContentIoCopy,
  onProgress: (progress: ImportProgress | null) => void,
  signal?: AbortSignal,
): Promise<IoResult> {
  onProgress({ step: copy.importStepReading, current: 0, total: 0 })
  try {
    onProgress({ step: copy.importStepParsing, current: 0, total: 0 })
    const notes = await parseTransferFile(kind, file)
    if (notes.length === 0) throw new Error(copy.errorInvalidFormat)
    let imported = 0
    let skipped = 0
    for (let i = 0; i < notes.length; i++) {
      if (signal?.aborted) break
      const note = notes[i]!
      const title = note.title.trim() || copy.untitled
      onProgress({
        step: fill(copy.importStepImporting, { name: title }),
        current: i + 1,
        total: notes.length,
      })
      try {
        const doc = await phantasiApi.createNoteDoc({
          title,
          content_md: note.content_md,
          topic: note.topic ?? null,
          published_at: note.published_at,
        })
        if (note.status === 'published') {
          try {
            await publishImported(
              phantasiApi.publishNoteDoc,
              doc.id,
              doc.revision,
              {
                title,
                content_md: note.content_md,
                topic: note.topic ?? null,
                published_at: note.published_at,
              },
              note,
              (err) => void showPhantasiError(err, copy.errorImportFailed),
            )
          } catch (err) {
            void showPhantasiError(err, copy.errorImportFailed)
          }
        }
        imported += 1
      } catch (err) {
        skipped += 1
        void showPhantasiError(err, copy.errorImportFailed)
      }
    }
    onProgress(null)
    return {
      ok: true,
      message: fill(copy.importSuccess, { imported, skipped }),
    }
  } catch (err) {
    onProgress(null)
    return {
      ok: false,
      error: userFacingError(err, copy.errorImportFailed),
    }
  }
}
