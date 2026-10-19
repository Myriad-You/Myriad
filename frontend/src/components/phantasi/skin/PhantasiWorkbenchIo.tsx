import type { ReactNode } from 'react'
import type { PhantasiNoteDoc } from '../../../types/phantasi'
import type { NoteTransferKind, WorkbenchPane } from '../logic/board'
import {
  HaloIcon,
  LuDownload,
  LuFolderOpen,
  LuLoader2,
  LuUpload,
  SiMarkdown,
  SiWordpress,
  TypechoIcon,
} from '@lib/icons'
import { useRef, useState } from 'react'
import { useI18n } from '../../../contexts/I18nContext'
import { WorkbenchPage } from './PhantasiWorkbenchChrome'

const NOTE_TRANSFER_FORMATS: Array<{
  kind: NoteTransferKind
  title:
    | 'workbenchWordpress'
    | 'workbenchHalo'
    | 'workbenchTypecho'
    | 'workbenchMarkdown'
  hint:
    | 'workbenchWordpressHint'
    | 'workbenchHaloHint'
    | 'workbenchTypechoHint'
    | 'workbenchMarkdownHint'
  accept: string
  formats: string
  icon: ReactNode
}> = [
  {
    kind: 'wordpress',
    title: 'workbenchWordpress',
    hint: 'workbenchWordpressHint',
    accept: '.xml,.wxr,text/xml',
    formats: '.xml / .wxr',
    icon: <SiWordpress />,
  },
  {
    kind: 'halo',
    title: 'workbenchHalo',
    hint: 'workbenchHaloHint',
    accept: '.json,.zip,application/json,application/zip',
    formats: '.json / .zip',
    icon: <HaloIcon />,
  },
  {
    kind: 'typecho',
    title: 'workbenchTypecho',
    hint: 'workbenchTypechoHint',
    accept: '.xml,text/xml',
    formats: '.xml',
    icon: <TypechoIcon />,
  },
  {
    kind: 'markdown',
    title: 'workbenchMarkdown',
    hint: 'workbenchMarkdownHint',
    accept: '.zip,.md,text/markdown,application/zip',
    formats: '.zip / .md',
    icon: <SiMarkdown />,
  },
]

/** 一种格式一行：整行能拖文件进来，右边是导入和导出。选了文件才出现「开始导入」。 */
function TransferRow({
  mark,
  name,
  hint,
  formats,
  accept,
  busy,
  canExport,
  progress,
  labels,
  onExport,
  onImport,
}: {
  mark: ReactNode
  name: string
  hint: string
  formats: string
  accept: string
  busy: boolean
  canExport: boolean
  progress: string | null
  labels: { pick: string; start: string; cancel: string; export: string; drop: string }
  onExport: () => void
  onImport: (file: File) => void
}) {
  const inputRef = useRef<HTMLInputElement>(null)
  const [dragOver, setDragOver] = useState(false)
  const [picked, setPicked] = useState<File | null>(null)

  const takeFile = (file: File | undefined) => {
    if (!file || busy) return
    setPicked(file)
  }

  return (
    <li
      className={`studio-transfer__row${dragOver ? ' is-on' : ''}${picked ? ' is-picked' : ''}`}
      aria-label={labels.drop}
      onDragOver={(event) => {
        event.preventDefault()
        if (!busy) setDragOver(true)
      }}
      onDragLeave={() => setDragOver(false)}
      onDrop={(event) => {
        event.preventDefault()
        setDragOver(false)
        takeFile(event.dataTransfer.files[0])
      }}
    >
      <span className="studio-transfer__mark" aria-hidden>{mark}</span>
      <span className="studio-transfer__text">
        <span className="studio-transfer__name">{name}</span>
        <span className="studio-transfer__hint">
          {picked ? picked.name : `${hint} ${formats}`}
        </span>
        {progress ? <span className="studio-transfer__progress">{progress}</span> : null}
      </span>
      <input
        ref={inputRef}
        type="file"
        accept={accept}
        hidden
        disabled={busy}
        onChange={(event) => {
          const file = event.target.files?.[0]
          event.target.value = ''
          takeFile(file)
        }}
      />
      <span className="studio-transfer__actions">
        {picked ? (
          <>
            <button type="button" className="tool-pane__action" disabled={busy} onClick={() => setPicked(null)}>
              {labels.cancel}
            </button>
            <button
              type="button"
              className="tool-pane__action is-primary"
              disabled={busy}
              onClick={() => {
                const file = picked
                setPicked(null)
                onImport(file)
              }}
            >
              <span className="tool-pane__action-icon" aria-hidden><LuUpload /></span>
              <span>{labels.start}</span>
            </button>
          </>
        ) : (
          <>
            <button type="button" className="tool-pane__action" disabled={busy} onClick={() => inputRef.current?.click()}>
              <span className="tool-pane__action-icon" aria-hidden><LuFolderOpen /></span>
              <span>{labels.pick}</span>
            </button>
            <button type="button" className="tool-pane__action" disabled={busy || !canExport} onClick={onExport}>
              <span className="tool-pane__action-icon" aria-hidden>
                {busy ? <LuLoader2 className="tool-pane__spin" /> : <LuDownload />}
              </span>
              <span>{labels.export}</span>
            </button>
          </>
        )}
      </span>
    </li>
  )
}

export function WorkbenchIoPane({
  pane,
  back,
  docs,
  notesBusy,
  notesKind,
  notesProgress,
  onExportNotes,
  onImportNotes,
}: {
  pane: WorkbenchPane
  back: ReactNode
  docs: PhantasiNoteDoc[]
  notesBusy: boolean
  notesKind: NoteTransferKind | null
  notesProgress: string | null
  onExportNotes: (kind: NoteTransferKind) => void
  onImportNotes: (kind: NoteTransferKind, file: File) => void
}) {
  const { t, format } = useI18n()
  const phantasi = t.phantasi

  if (pane !== 'notesIo') return null

  return (
    <WorkbenchPage title={phantasi.workbenchNavTransfer} back={back}>
      <ul className="studio-transfer">
        {NOTE_TRANSFER_FORMATS.map((item) => (
          <TransferRow
            key={item.kind}
            mark={item.icon}
            name={phantasi[item.title]}
            hint={phantasi[item.hint]}
            formats={item.formats}
            accept={item.accept}
            busy={notesBusy}
            canExport={docs.length > 0}
            progress={notesKind === item.kind ? notesProgress : null}
            labels={{
              pick: phantasi.workbenchImportPick,
              start: phantasi.startImport,
              cancel: phantasi.cancel,
              export: phantasi.workbenchExportNotes,
              drop: format(phantasi.workbenchDropNamed, { name: phantasi[item.title] }),
            }}
            onExport={() => onExportNotes(item.kind)}
            onImport={(file) => onImportNotes(item.kind, file)}
          />
        ))}
      </ul>
    </WorkbenchPage>
  )
}
