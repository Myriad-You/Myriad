import type { ReactNode } from 'react'
import type { PhantasiNoteDoc } from '../../../types/phantasi'
import type { NoteTransferKind, WorkbenchPane } from '../logic/board'
import {
  HaloIcon,
  LuDownload,
  LuFolderOpen,
  LuUpload,
  SiMarkdown,
  SiWordpress,
  TypechoIcon,
} from '@lib/icons'
import { useRef, useState } from 'react'
import { useI18n } from '../../../contexts/I18nContext'
import {
  SettingGroupGrid,
  SettingsButton,
} from '../../settings'
import { SettingItemWrapper } from '../../settings/items/SettingItemWrapper'
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

function TransferActions({
  accept,
  busy,
  canExport,
  progress,
  dropLabel,
  formatsLabel,
  startLabel,
  exportLabel,
  mark,
  onExport,
  onImport,
}: {
  accept: string
  busy: boolean
  canExport: boolean
  progress: string | null
  dropLabel: string
  formatsLabel: string
  startLabel: string
  exportLabel: string
  mark?: ReactNode
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
    <div className="phantasi-workbench__transfer">
      <button
        type="button"
        className={`phantasi-workbench__drop${dragOver ? ' is-on' : ''}`}
        disabled={busy}
        onDragOver={(event) => {
          event.preventDefault()
          setDragOver(true)
        }}
        onDragLeave={() => setDragOver(false)}
        onDrop={(event) => {
          event.preventDefault()
          setDragOver(false)
          takeFile(event.dataTransfer.files[0])
        }}
        onClick={() => inputRef.current?.click()}
      >
        <input
          ref={inputRef}
          type="file"
          accept={accept}
          className="phantasi-workbench__drop-file"
          disabled={busy}
          onChange={(event) => {
            const file = event.target.files?.[0]
            event.target.value = ''
            takeFile(file)
          }}
        />
        {mark ?? <LuFolderOpen />}
        <p>{dropLabel}</p>
        <small>{picked ? picked.name : formatsLabel}</small>
      </button>
      <div className="phantasi-workbench__transfer-actions">
        {picked ? (
          <SettingsButton
            variant="primary"
            size="sm"
            block
            icon={<LuUpload />}
            disabled={busy}
            loading={busy}
            onClick={() => {
              const file = picked
              setPicked(null)
              onImport(file)
            }}
          >
            {startLabel}
          </SettingsButton>
        ) : null}
        <SettingsButton
          size="sm"
          block
          icon={<LuDownload />}
          disabled={busy || !canExport}
          loading={busy}
          onClick={onExport}
        >
          {exportLabel}
        </SettingsButton>
      </div>
      {progress ? (
        <p className="setting-description phantasi-workbench__transfer-progress">
          {progress}
        </p>
      ) : null}
    </div>
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

  if (pane === 'notesIo') {
    return (
      <WorkbenchPage
        title={phantasi.workbenchNavTransfer}
        back={back}
      >
        <SettingGroupGrid
          columns={2}
          variant="plain"
          align="start"
          minColumnWidth="16rem"
          className="phantasi-workbench__io-grid"
        >
          {NOTE_TRANSFER_FORMATS.map((item) => (
            <SettingItemWrapper
              key={item.kind}
              itemKey={`transfer-${item.kind}`}
              className="phantasi-workbench__io-item"
              size="sm"
              label={phantasi[item.title]}
              detail={phantasi[item.hint]}
              icon={item.icon}
            >
              <TransferActions
                mark={item.icon}
                accept={item.accept}
                busy={notesBusy}
                canExport={docs.length > 0}
                progress={notesKind === item.kind ? notesProgress : null}
                dropLabel={format(phantasi.workbenchDropNamed, {
                  name: phantasi[item.title],
                })}
                formatsLabel={format(phantasi.workbenchDropFormats, {
                  formats: item.formats,
                })}
                startLabel={phantasi.startImport}
                exportLabel={phantasi.workbenchExportNotes}
                onExport={() => onExportNotes(item.kind)}
                onImport={(file) => onImportNotes(item.kind, file)}
              />
            </SettingItemWrapper>
          ))}
        </SettingGroupGrid>
      </WorkbenchPage>
    )
  }

  return null
}
