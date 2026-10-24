import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

const dir = dirname(fileURLToPath(import.meta.url))

describe('note editor abort', () => {
  it('draft load and preview pass AbortSignal', () => {
    const editor = [
      'notes/NoteEditor.tsx',
      'notes/NoteEditorView.tsx',
      'notes/useNoteEditorSession.ts',
      'notes/useNoteEditorSidecar.ts',
      'notes/useNoteEditorPreview.ts',
      'notes/useNoteEditorFormat.ts',
      'notes/useNoteEditorAuthors.ts',
      'notes/useNoteEditorOpen.ts',
      'notes/useNoteVisual.ts',
      'notes/useNotePublish.ts',
      'notes/useNoteCollab.ts',
    ]
      .map((file) => readFileSync(join(dir, file), 'utf8'))
      .join('\n')
    const api = readFileSync(join(dir, '../../services/phantasiApi.ts'), 'utf8')
    assert.match(api, /export async function getNoteDoc\(\s*id: number,\s*signal\?: AbortSignal/)
    assert.match(editor, /openNoteCloudDoc\(/)
    assert.match(editor, /previewNote\([^,]+, controller\.signal\)/)
    assert.match(editor, /prepareNoteReaderHtml\(/)
    assert.match(editor, /decorateNoteReadSurface\(/)
    assert.match(editor, /notePreviewFailed/)
    assert.doesNotMatch(editor, /setHtml\(markdownToVisualHtml/)
    assert.doesNotMatch(editor, /setHtml\(withLinkDefinitions/)
    assert.doesNotMatch(editor, /withLinkDefinitions/)
    assert.match(editor, /previewMdRef\.current = source/)
    assert.match(editor, /contentMdRef\.current === source/)
    assert.match(editor, /previewMdRef\.current === source && htmlRef\.current/)
    assert.match(editor, /previewMdRef\.current !== contentMd/)
    assert.match(editor, /!html && !previewing && !contentMd\.trim\(\)/)
    assert.match(editor, /controller\.abort\(\)/)
    assert.match(editor, /pane !== 'preview'/)
    assert.match(editor, /textareaSupportsFieldSizing\(\)/)
    assert.match(editor, /growTextarea\(/)
  })

  it('发布载荷带主题、封面和发布时间', () => {
    const editor = [
      'notes/NoteEditor.tsx',
      'notes/NoteEditorView.tsx',
      'notes/useNoteEditorSession.ts',
      'notes/useNoteEditorSidecar.ts',
      'notes/useNoteEditorPreview.ts',
      'notes/useNoteEditorFormat.ts',
      'notes/useNoteEditorAuthors.ts',
      'notes/useNoteEditorOpen.ts',
      'notes/useNoteVisual.ts',
      'notes/useNotePublish.ts',
      'notes/useNoteCollab.ts',
    ]
      .map((file) => readFileSync(join(dir, file), 'utf8'))
      .join('\n')
    assert.match(editor, /toNoteWritePayload\(/)
    assert.match(editor, /noteFieldError\(/)
    assert.match(editor, /publishNoteDoc\(/)
    assert.match(editor, /scheduleNoteDoc\(/)
    assert.match(editor, /noteDocWsUrl\(/)
    assert.match(editor, /contentEditable/)
    assert.match(editor, /markdownToVisualHtml\(/)
    assert.match(editor, /previewNote\([^,]+, controller\.signal\)/)
  })

  it('外壳是编辑器：薄顶栏、浮动条、底栏、发布抽屉；不进口 phantasiApi', () => {
    const chrome = readFileSync(join(dir, 'notes/NoteEditorChrome.tsx'), 'utf8')
    const controls = readFileSync(join(dir, 'notes/NoteControls.tsx'), 'utf8')
    const editor = [
      'notes/NoteEditor.tsx',
      'notes/NoteEditorView.tsx',
      'notes/useNoteEditorSession.ts',
      'notes/useNoteEditorSidecar.ts',
      'notes/useNoteEditorPreview.ts',
      'notes/useNoteEditorFormat.ts',
      'notes/useNoteEditorAuthors.ts',
    ]
      .map((file) => readFileSync(join(dir, file), 'utf8'))
      .join('\n')
    // 编辑器用自己的控件，不借设置页的。
    assert.doesNotMatch(chrome, /from ['"][^'"]*settings\//)
    assert.doesNotMatch(controls, /from ['"][^'"]*settings\//)
    for (const control of [
      'NoteButton',
      'NoteSwitch',
      'NoteDateInput',
      'NoteField',
      'NoteSection',
      'NoteChip',
    ]) {
      assert.match(controls, new RegExp(`export function ${control}\\b`))
      assert.match(chrome, new RegExp(`<${control}\\b`))
    }
    assert.match(chrome, /notePublishSettings/)
    assert.match(chrome, /noteAuthors/)
    assert.match(chrome, /noteAuthorOwner/)
    assert.match(chrome, /authorLine/)
    for (const part of ['NoteTopBar', 'NoteBubble', 'NoteGutter', 'NoteFootBar', 'NoteSettingsDrawer', 'NoteByline']) {
      assert.match(chrome, new RegExp(`export function ${part}\\(`))
      assert.match(editor, new RegExp(`<${part}\\b`))
    }
    assert.doesNotMatch(chrome, /InputItem/)
    assert.doesNotMatch(chrome, /from ['"].*phantasiApi['"]/)
    assert.match(editor, /phantasi-note__title/)
    assert.match(editor, /useNoteSelection\(/)
    assert.match(editor, /visualWidgets\.portals/)
    assert.match(editor, /showNoteNotice\(/)
    assert.match(editor, /useNoteWidgetCatalog\(true\)/)
    assert.match(editor, /preloadNoteWidgets\(/)
    assert.match(editor, /from '.\/noteWidgetCatalog'/)
    assert.match(editor, /visualEditing\.current = false/)
    assert.match(editor, /commitVisualMd\(/)
    assert.match(editor, /commitVisualMd\(toggleVisualHeading/)
    assert.match(editor, /commitVisualMd\(insertImage/)
    assert.match(editor, /commitVisualMd\(serializeVisual/)
    assert.match(editor, /visualMarkdownStamp\.get\(el\) === contentMd/)
    assert.match(editor, /closest<HTMLElement>\('\.note-widget'\)/)
    assert.match(editor, /!widget && caret && block\.contains\(caret\.node\)/)
    assert.match(editor, /getArticleProseClass\(/)
    assert.match(editor, /phantasi-note__article/)
    assert.match(editor, /readerSurfaceStyle/)
    assert.match(editor, /style=\{readerSurfaceStyle\}/)
    assert.match(editor, /useReaderSettings\(/)
    assert.doesNotMatch(chrome, /phantasi-note__alert/)
    assert.doesNotMatch(editor, /error=\{error\}/)
  })
})
