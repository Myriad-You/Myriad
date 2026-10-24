import type {
  CSSProperties,
} from 'react'
import type { NoteEditorTool } from './NoteEditorChrome'
import type { useNoteEditorSession } from './useNoteEditorSession'
import {
  LuBold as Bold,
  LuBookOpen as BookOpen,
  LuCheckSquare as CheckSquare,
  LuCode as Code,
  LuColumns2 as Columns2,
  LuFileText as FileText,
  LuImage as Image,
  LuImagePlus as ImagePlus,
  LuItalic as Italic,
  LuLink as Link,
  LuList as List,
  LuListOrdered as ListOrdered,
  LuMinus as Minus,
  LuPuzzle as Puzzle,
  LuQuote as Quote,
  LuSigma as Sigma,
  LuSparkles as Sparkles,
  LuSquareCode as SquareCode,
  LuStrikethrough as Strikethrough,
  FaUndo as Undo,
} from '@lib/icons'
import { motionShim as motion } from '@lib/motionShim'
import { useEffect, useState } from 'react'
import { createPortal } from 'react-dom'
import { getPhantasiTransition, phantasiAnimationPresets } from '../../../hooks/animation/pages/phantasi'
import { Spinner } from '../../Spinner'
import { widgetDisplayLabel } from '../../widgetLibraryModel'
import { WidgetInstanceSettings } from '../../widgets/shared/WidgetInstanceSettings'
import { NoteAiPanel } from './NoteAiPanel'
import { NoteCompanionSection } from './NoteCompanionSection'
import { NoteButton } from './NoteControls'
import {
  hardBreak,
  indentLines,
} from './noteDraft'
import {
  NoteBlockBar,
  NoteBubble,
  NoteByline,
  NoteFootBar,
  NoteGutter,
  NoteSettingsDrawer,
  NoteTopBar,
  peerHue,
  splitNoteTools,
} from './NoteEditorChrome'
import { NoteEditorSettings } from './NoteEditorSettings'
import { matchEnterRule, matchSpaceRule } from './noteInputRules'
import { decodeWidgetConfigAttr } from './noteLayout'
import { caretOffsetStyle } from './noteMerge'
import {
  insertPastedMarkdown,
  pastedClipboardToMarkdown,
  pastedClipboardToVisualHtml,
} from './notePaste'
import {
  lineIsBlank,
  visualEmptyLineRect,
} from './noteSelection'
import {
  applyInlineMarkdownAtCaret,
  applyVisualInputRule,
  beginVisualMathEdit,
  columnsAddColumn,
  columnsRemove,
  columnsRemoveColumn,
  createVisualMarkdownSerializer,
  insertColumnsMarkdown,
  insertColumnsVisual,
  insertFootnoteMarkdown,
  insertTableMarkdown,
  looksLikeMarkdown,
  pasteMarkdownIntoVisual,
  pasteVisualHtml,
  removeImage,
  removeNoteWidget,
  setImageAlt,
  setImageLink,
  setImageSrc,
  setNoteWidgetConfig,
  setNoteWidgetSize,
  tableAddColumn,
  tableAddRow,
  tableRemove,
  tableRemoveColumn,
  tableRemoveRow,
  tableSetAlign,
  tableStep,
  textBeforeCaret,
  toggleVisualTask,
  visualBlockAt,
  visualClosest,
} from './noteVisual'
import { hydrateVisualMath } from './renderMath'
import 'katex/dist/katex.min.css'
import '../ui/phantasi.css'
import './NoteEditor.css'

export function NoteEditorView({
  ai,
  openAi,
  compositionStart,
  compositionEnd,
  t,
  animation,
  motionEnabled,
  previewSurfaceClass,
  readerSurfaceStyle,
  cloudId,
  categoryNames,
  title,
  setTitle,
  contentMd,
  setContentMd,
  topic,
  setTopic,
  cover,
  setCover,
  publishedAt,
  setPublishedAt,
  slugInput,
  slugInvalid,
  onSlugInput,
  articleHref,
  companionItemId,
  loading,
  saving,
  uploading,
  lastError,
  html,
  previewing,
  pane,
  setPane,
  settingsOpen,
  preference,
  restoreHistory,
  setSettingsOpen,
  insertMenu,
  setInsertMenu,
  linkOpen,
  linkInitial,
  setBlockFocus,
  codeLang,
  columnAlign,
  setColumnAlign,
  selectedImage,
  selectedWidget,
  setSelectedWidget,
  widgetSettingsOpen,
  setWidgetSettingsOpen,
  docStatus,
  authorLine,
  authorChips,
  addableAuthors,
  authorBusy,
  scheduledAt,
  setScheduledAt,
  cloudHint,
  textareaRef,
  visualRef,
  bindVisual,
  titleInputRef,
  scrollRef,
  bindPreview,
  selectedImageRef,
  selectedWidgetRef,
  widgetSettingsOpenRef,
  blockBarRef,
  widgetCatalog,
  imageReplaceRef,
  fileRef,
  coverFileRef,
  visualEditing,
  coverPreview,
  selectedWidgetEntry,
  selectedWidgetSizes,
  selectedWidgetSettings,
  canConfigureWidget,
  bodyChars,
  peers,
  collabConnection,
  markEditing,
  selectionAnchor,
  activeMarks,
  caretLine,
  block,
  visualWidgets,
  previewWidgets,
  focusBody,
  slashTrigger,
  jumpFromPreview,
  applyEdit,
  runTool,
  wrap,
  prefix,
  insertText,
  handleSave,
  handleSchedule,
  handleUnschedule,
  handleDelete,
  requestClose,
  placeImage,
  handleUpload,
  heading,
  finishMath,
  displayMath,
  bold,
  italic,
  strike,
  inlineCode,
  inlineMath,
  openLink,
  rememberVisualRange,
  applyLink,
  pasteAsLink,
  selectImage,
  selectWidget,
  imageOp,
  footnoteJump,
  closeOverlays,
  tableOp,
  columnsOp,
  widgetOp,
  changeCodeLang,
  syncVisualFromMarkdown,
  commitVisualMd,
  runVisual,
  openBlockBelow,
  placeWidget,
  handleAddAuthor,
  handleRemoveAuthor,
  handleCreateTopic,
  bulletList,
  orderedList,
  taskList,
  paneRef,
  canDelete,
  framed = false,
}: ReturnType<typeof useNoteEditorSession> & { framed?: boolean }) {
  const [serializeVisual] = useState(createVisualMarkdownSerializer)
  useEffect(() => () => serializeVisual.dispose(), [serializeVisual])
  const tools = [
    {
      key: 'bold',
      icon: <Bold className="h-4 w-4" />,
      label: t.phantasi.noteToolBold,
      run: bold,
    },
    {
      key: 'italic',
      icon: <Italic className="h-4 w-4" />,
      label: t.phantasi.noteToolItalic,
      run: italic,
    },
    {
      key: 'strike',
      icon: <Strikethrough className="h-4 w-4" />,
      label: t.phantasi.noteToolStrike,
      run: strike,
    },
    {
      key: 'inline-code',
      icon: <Code className="h-4 w-4" />,
      label: t.phantasi.noteToolInlineCode,
      run: inlineCode,
    },
    {
      key: 'inline-math',
      icon: <Sigma className="h-4 w-4" />,
      label: t.phantasi.noteToolInlineMath,
      run: inlineMath,
    },
    {
      key: 'link',
      icon: <Link className="h-4 w-4" />,
      label: t.phantasi.noteToolLink,
      run: openLink,
    },
    {
      key: 'h2',
      icon: <span aria-hidden="true">H2</span>,
      label: t.phantasi.noteToolH2,
      run: () => heading(2),
    },
    {
      key: 'h3',
      icon: <span aria-hidden="true">H3</span>,
      label: t.phantasi.noteToolH3,
      run: () => heading(3),
    },
    {
      key: 'quote',
      icon: <Quote className="h-4 w-4" />,
      label: t.phantasi.noteToolQuote,
      run: () =>
        runTool(() => prefix('> '), { command: 'formatBlock', value: 'blockquote' }),
    },
    {
      key: 'code',
      icon: <SquareCode className="h-4 w-4" />,
      label: t.phantasi.noteToolCode,
      run: () =>
        runTool(() => wrap('\n```\n', '\n```\n', ''), {
          command: 'formatBlock',
          value: 'pre',
        }),
    },
    {
      key: 'list',
      icon: <List className="h-4 w-4" />,
      label: t.phantasi.noteToolList,
      run: bulletList,
    },
    {
      key: 'ol',
      icon: <ListOrdered className="h-4 w-4" />,
      label: t.phantasi.noteToolOl,
      run: orderedList,
    },
    {
      key: 'task',
      icon: <CheckSquare className="h-4 w-4" />,
      label: t.phantasi.noteToolTask,
      run: taskList,
    },
    {
      key: 'divider',
      icon: <Minus className="h-4 w-4" />,
      label: t.phantasi.noteToolDivider,
      run: () =>
        runTool(() => wrap('\n\n---\n\n', '', ''), {
          command: 'insertHorizontalRule',
        }),
    },
    {
      key: 'table',
      icon: <FileText className="h-4 w-4" />,
      label: t.phantasi.noteToolTable,
      run: () =>
        runTool(() => wrap('\n', `\n${insertTableMarkdown()}\n`, ''), {
          command: 'insertHTML',
          value: '<table><tr><th></th><th></th></tr><tr><td></td><td></td></tr></table>',
        }),
    },
    {
      key: 'footnote',
      icon: <BookOpen className="h-4 w-4" />,
      label: t.phantasi.noteToolFootnote,
      run: () => {
        const note = insertFootnoteMarkdown(
          (contentMd.match(/\[\^\d+\]/g)?.length ?? 0) + 1,
        )
        if (pane === 'visual') {
          const next = `${contentMd.trimEnd()}\n\n${note.mark} ${note.definition}`
          syncVisualFromMarkdown(next)
          return
        }
        insertText(note.mark, '', '')
        setContentMd((current) => `${current.trimEnd()}\n\n${note.definition}`)
      },
    },
  ]

  const { marks, inserts } = splitNoteTools(tools)

  const onOwnLine = (tool: NoteEditorTool): NoteEditorTool => ({
    ...tool,
    run: () => {
      openBlockBelow()
      tool.run()
    },
    runWith: tool.runWith
      ? (value) => {
          openBlockBelow()
          tool.runWith?.(value)
        }
      : undefined,
  })

  const insertItems: NoteEditorTool[] = [
    onOwnLine({
      key: 'image',
      icon: <Image className="h-4 w-4" />,
      label: t.phantasi.noteToolImage,
      run: () => fileRef.current?.click(),
    }),
    onOwnLine({
      key: 'image-url',
      icon: <ImagePlus className="h-4 w-4" />,
      label: t.phantasi.noteImageByUrl,
      prompt: t.phantasi.noteImageUrlPlaceholder,
      run: () => {},
      runWith: (url) => placeImage(url, ''),
    }),
    ...[1, 2, 3].map((level) => ({
      key: `h${level}-block`,
      icon: <span aria-hidden="true">H{level}</span>,
      label: t.phantasi[`noteToolH${level}` as 'noteToolH1' | 'noteToolH2' | 'noteToolH3'],
      run: () => heading(level),
    })),
    ...inserts.map((tool) =>
      ['code', 'table', 'divider', 'footnote'].includes(tool.key) ? onOwnLine(tool) : tool,
    ),
    onOwnLine({
      key: 'display-math',
      icon: <Sigma className="h-4 w-4" />,
      label: t.phantasi.noteToolDisplayMath,
      run: displayMath,
    }),
    onOwnLine({
      key: 'columns',
      icon: <Columns2 className="h-4 w-4" />,
      label: t.phantasi.noteToolColumns,
      run: () => {
        const root = visualRef.current
        if (paneRef.current === 'visual' && root) {
          commitVisualMd(insertColumnsVisual(root))
          return
        }
        insertText(insertColumnsMarkdown(), '', '')
      },
    }),
    {
      key: 'widget',
      icon: <Puzzle className="h-4 w-4" />,
      label: t.phantasi.noteToolWidget,
      page: 'widgets',
      run: rememberVisualRange,
    },
  ]

  return createPortal(
    <motion.div
      data-phantasi-shortcuts="suspended"
      onInputCapture={markEditing}
      className="phantasi-skin phantasi-note"
      initial={motionEnabled ? phantasiAnimationPresets.readerEnter.initial : false}
      animate={phantasiAnimationPresets.readerEnter.animate}
      exit={phantasiAnimationPresets.readerEnter.exit}
      transition={getPhantasiTransition(animation)}
    >
      <div className="phantasi-note__frame">
        <NoteTopBar
          docStatus={docStatus}
          lastError={lastError}
          scheduledAt={scheduledAt}
          cloudHint={cloudHint}
          peers={peers}
          collabConnection={collabConnection}
          saving={saving}
          loading={loading}
          onPublish={() => {
            void handleSave()
          }}
          articleHref={articleHref}
          settingsOpen={settingsOpen}
          onToggleSettings={() => setSettingsOpen((open) => !open)}
          onClose={framed ? undefined : requestClose}
        />

        <div className="phantasi-note__scroll" ref={scrollRef}>
          <NoteAiPanel ai={ai} previewClass={previewSurfaceClass} previewStyle={readerSurfaceStyle} />
          {loading ? (
            <div className="phantasi-note__center">
              <Spinner size="lg" />
            </div>
          ) : (
            <article className="phantasi-note__paper">
              <textarea
                ref={titleInputRef}
                onCompositionStart={compositionStart}
                onCompositionEnd={() => compositionEnd()}
                className="phantasi-note__title"
                rows={1}
                value={title}
                onChange={(e) => setTitle(e.target.value.replaceAll('\n', ''))}
                onKeyDown={(e) => {
                  if (e.key !== 'Enter') return
                  e.preventDefault()
                  focusBody()
                }}
                placeholder={t.phantasi.noteTitlePlaceholder}
                aria-label={t.phantasi.noteTitlePlaceholder}
                spellCheck={false}
              />
              <NoteByline
                authorLine={authorLine}
                topic={topic}
                publishedAt={publishedAt}
                scheduledAt={scheduledAt}
                docStatus={docStatus}
                onOpenSettings={() => setSettingsOpen(true)}
              />

              <div
                className={`phantasi-note__write phantasi-note__source${pane === 'write' ? '' : ' is-hidden'}`}
              >
                {peers.map((peer) =>
                  peer.cursor == null ? null : (
                    <span
                      key={peer.peerId}
                      className="phantasi-note__caret"
                      style={
                        {
                          ...caretOffsetStyle(contentMd, peer.cursor),
                          '--peer-hue': peerHue(peer.peerId),
                        } as CSSProperties
                      }
                      title={peer.name ?? undefined}
                    />
                  ),
                )}
                <textarea
                  ref={textareaRef}
                  onCompositionStart={compositionStart}
                  onCompositionEnd={() => compositionEnd()}
                  value={contentMd}
                  onChange={(e) => {
                    setContentMd(e.target.value)
                  }}
                  onKeyDown={(e) => {
                    // 用指针点开的插入菜单，一继续敲字就收掉。
                    if (insertMenu === 'pointer') setInsertMenu(null)
                    if (e.key === 'Tab') {
                      e.preventDefault()
                      const outdent = e.shiftKey
                      applyEdit((v, s, end) => indentLines(v, s, end, outdent))
                      return
                    }
                    if (e.key === 'Enter' && e.shiftKey) {
                      e.preventDefault()
                      applyEdit(hardBreak)
                      return
                    }
                    slashTrigger(e, () => {
                      const el = e.currentTarget
                      return (
                        el.selectionStart === el.selectionEnd &&
                        lineIsBlank(el.value, el.selectionStart)
                      )
                    })
                  }}
                  onPaste={(e) => {
                    const file = [...e.clipboardData.files].find((item) =>
                      item.type.startsWith('image/'),
                    )
                    if (file) {
                      e.preventDefault()
                      void handleUpload(file, 'body')
                      return
                    }
                    const html = e.clipboardData.getData('text/html')
                    const text = e.clipboardData.getData('text/plain')
                    if (pasteAsLink(text)) {
                      e.preventDefault()
                      return
                    }
                    const converted = pastedClipboardToMarkdown(html, text)
                    if (converted) {
                      e.preventDefault()
                      applyEdit((value, start, end) =>
                        insertPastedMarkdown(value, start, end, converted),
                      )
                    }
                  }}
                  onDrop={(e) => {
                    const file = [...e.dataTransfer.files].find((item) =>
                      item.type.startsWith('image/'),
                    )
                    if (!file) return
                    e.preventDefault()
                    void handleUpload(file, 'body')
                  }}
                  placeholder={t.phantasi.noteBodyPlaceholder}
                  aria-label={t.phantasi.noteBodyPlaceholder}
                  spellCheck={false}
                />
              </div>
              <div
                className={`phantasi-note__write${pane === 'visual' ? '' : ' is-hidden'}`}
              >
                <div
                  ref={bindVisual}
                  onCompositionStart={compositionStart}
                  onCompositionEnd={() => compositionEnd()}
                  className="phantasi-note-preview phantasi-note__visual"
                  style={readerSurfaceStyle}
                  contentEditable
                  suppressContentEditableWarning
                  role="textbox"
                  aria-label={t.phantasi.noteTabVisual}
                  data-placeholder={t.phantasi.noteBodyPlaceholder}
                  onFocus={() => {
                    visualEditing.current = true
                  }}
                  onBlur={(e) => {
                    const next = e.relatedTarget as HTMLElement | null
                    if (
                      next?.closest('.phantasi-note__blockbar') ||
                      next?.closest('.widget-settings-tip') ||
                      widgetSettingsOpenRef.current
                    ) {
                      visualEditing.current = true
                      return
                    }
                    visualEditing.current = false
                    selectImage(null)
                    selectWidget(null)
                  }}
                  onPaste={(e) => {
                    const file = [...e.clipboardData.files].find((item) =>
                      item.type.startsWith('image/'),
                    )
                    if (file) {
                      e.preventDefault()
                      void handleUpload(file, 'body')
                      return
                    }
                    const text = e.clipboardData.getData('text/plain')
                    if (pasteAsLink(text)) {
                      e.preventDefault()
                      return
                    }
                    const html = e.clipboardData.getData('text/html')
                    const visual = pastedClipboardToVisualHtml(html, text)
                    if (visual) {
                      e.preventDefault()
                      commitVisualMd(pasteVisualHtml(e.currentTarget, visual))
                      void hydrateVisualMath(e.currentTarget)
                      return
                    }
                    // 贴的是 Markdown（没带 HTML）：按 Markdown 解，图片就是图片。
                    if (!html && looksLikeMarkdown(text)) {
                      e.preventDefault()
                      commitVisualMd(pasteMarkdownIntoVisual(e.currentTarget, text))
                    }
                  }}
                  onKeyDown={(e) => {
                    const root = e.currentTarget
                    if (selectedImageRef.current) selectImage(null)
                    if (selectedWidgetRef.current) selectWidget(null)
                    if (insertMenu === 'pointer') setInsertMenu(null)
                    if (e.key === 'Tab') {
                      e.preventDefault()
                      const cell =
                        visualClosest(root, 'td') ?? visualClosest(root, 'th')
                      if (cell) {
                        const changed = tableStep(root, cell, e.shiftKey)
                        if (changed != null) {
                          commitVisualMd(changed)
                        }
                        return
                      }
                      if (visualClosest(root, 'li')) {
                        runVisual(e.shiftKey ? 'outdent' : 'indent')
                      }
                      return
                    }
                    if (e.nativeEvent.isComposing) return
                    if (e.key === ' ' || e.key === 'Enter') {
                      const blockEl = visualBlockAt(root)
                      if (blockEl === root && root.childElementCount > 0) return
                      const text = (blockEl.textContent ?? '').replaceAll('\u00A0', ' ')
                      const rule =
                        e.key === ' '
                          ? textBeforeCaret(root, blockEl).replaceAll('\u00A0', ' ') === text
                            ? matchSpaceRule(text)
                            : null
                          : matchEnterRule(text)
                      if (rule) {
                        e.preventDefault()
                        commitVisualMd(applyVisualInputRule(root, blockEl, rule))
                        if (rule.kind === 'display-math') {
                          const last = [...root.querySelectorAll<HTMLElement>('.note-math-display')].at(-1)
                          if (last) beginVisualMathEdit(root, last, (md) => finishMath(root, md))
                        } else {
                          void hydrateVisualMath(root)
                        }
                        return
                      }
                    }
                    slashTrigger(e, () => visualEmptyLineRect(root) != null)
                  }}
                  onClick={(e) => {
                    const root = e.currentTarget
                    const target = e.target as HTMLElement
                    const widget = target.closest<HTMLElement>('.note-widget')
                    if (widget && root.contains(widget)) {
                      selectWidget(widget)
                      selectImage(null)
                      return
                    }
                    const math = target.closest<HTMLElement>('.note-math')
                    if (math && root.contains(math)) {
                      e.preventDefault()
                      beginVisualMathEdit(root, math, (md) => finishMath(root, md))
                      return
                    }
                    selectWidget(null)
                    selectImage(target instanceof HTMLImageElement ? target : null)
                    if (target.closest('a')) e.preventDefault()
                    if (footnoteJump(root, target)) {
                      e.preventDefault()
                      return
                    }
                    const item = target.closest<HTMLElement>('li[data-task]')
                    if (!item || target !== item || !root.contains(item)) return
                    // 勾选框画在 li 内容区左边的 ::before 上，点在内容左侧就是点它。
                    if (e.clientX < item.getBoundingClientRect().left) {
                      e.preventDefault()
                      commitVisualMd(toggleVisualTask(root, item))
                    }
                  }}
                  onInput={(e) => {
                    const root = e.currentTarget as HTMLDivElement
                    // 敲完 `)` / `` ` `` / `*` / `~` / `$`：光标前刚好凑成 Markdown 记号就地渲染。
                    const typed = (e.nativeEvent as InputEvent).data ?? ''
                    if (/[)`*~$]/.test(typed)) {
                      const converted = applyInlineMarkdownAtCaret(root)
                      if (converted != null) {
                        commitVisualMd(converted)
                        void hydrateVisualMath(root)
                        return
                      }
                    }
                    commitVisualMd(serializeVisual(root))
                  }}
                />
              </div>
              <div
                className={`phantasi-note__read${pane === 'preview' ? '' : ' is-hidden'}`}
                onClick={jumpFromPreview}
              >
                {previewing && !html ? (
                  <div className="phantasi-note__center">
                    <Spinner size="md" />
                  </div>
                ) : null}
                {!html && !previewing && !contentMd.trim() ? (
                  <p className="phantasi-note__empty">{t.phantasi.notePreviewEmpty}</p>
                ) : null}
                <div
                  className={previewSurfaceClass}
                  style={readerSurfaceStyle}
                  hidden={!html}
                >
                  <div ref={bindPreview} />
                </div>
              </div>
            </article>
          )}
          <NoteBubble
            anchor={selectionAnchor}
            tools={marks}
            active={activeMarks}
            containerRef={scrollRef}
            linkOpen={linkOpen}
            linkInitial={linkInitial}
            onLinkOpenChange={(open) => {
              if (open) openLink()
              else closeOverlays()
            }}
            onLink={applyLink}
          />
          <NoteBlockBar
            block={
              selectedWidget
                ? { kind: 'widget', anchor: selectedWidget.anchor }
                : selectedImage
                  ? { kind: 'img', anchor: selectedImage.anchor }
                  : selectionAnchor
                    ? null
                    : block
            }
            codeLang={codeLang}
            onCodeLangChange={changeCodeLang}
            columnAlign={columnAlign}
            onTableAlign={(align) => {
              setColumnAlign(align)
              tableOp((root, table) => tableSetAlign(root, table, align))
            }}
            onTableAddRow={() => tableOp(tableAddRow)}
            onTableAddColumn={() => tableOp(tableAddColumn)}
            onTableRemoveRow={() => tableOp(tableRemoveRow)}
            onTableRemoveColumn={() => tableOp(tableRemoveColumn)}
            onTableRemove={() => tableOp(tableRemove)}
            imageAlt={selectedImage?.alt ?? ''}
            imageSrc={selectedImage?.src ?? ''}
            imageHref={selectedImage?.href ?? ''}
            onImageApply={(values) => {
              imageOp((root, img) => {
                setImageSrc(root, img, values.src)
                setImageLink(root, img, values.href)
                return setImageAlt(root, img, values.alt)
              })
            }}
            onImageReplace={() => imageReplaceRef.current?.click()}
            onImageRemove={() => imageOp(removeImage, false)}
            onColumnsAdd={() => columnsOp(columnsAddColumn)}
            onColumnsRemoveCol={() => columnsOp(columnsRemoveColumn)}
            onColumnsRemove={() => columnsOp(columnsRemove)}
            widgetSize={selectedWidget?.size ?? ''}
            widgetSizes={selectedWidgetSizes}
            onWidgetSize={(size) => {
              widgetOp((root, widget) => setNoteWidgetSize(root, widget, size))
              setSelectedWidget((current) =>
                current ? { ...current, size } : current,
              )
            }}
            canConfigure={canConfigureWidget}
            widgetConfigOpen={widgetSettingsOpen}
            onWidgetConfig={() => {
              if (selectedWidgetSettings.length) {
                setWidgetSettingsOpen((open) => !open)
                return
              }
              setWidgetSettingsOpen(true)
              requestAnimationFrame(() => {
                selectedWidgetRef.current
                  ?.querySelector<HTMLButtonElement>('.widget-longpress-hint')
                  ?.click()
              })
            }}
            onWidgetRemove={() => widgetOp(removeNoteWidget, false)}
            onFocusChange={setBlockFocus}
            barRef={blockBarRef}
          />
          {selectedWidgetSettings.length > 0 ? (
            <WidgetInstanceSettings
              open={widgetSettingsOpen}
              title={
                selectedWidgetEntry
                  ? widgetDisplayLabel(
                      selectedWidgetEntry,
                      t.widgets as unknown as Record<string, unknown>,
                    )
                  : t.phantasi.noteWidgetConfig
              }
              settings={selectedWidgetSettings}
              value={
                decodeWidgetConfigAttr(selectedWidgetRef.current?.dataset.config) ??
                {}
              }
              anchor={
                selectedWidgetRef.current?.getBoundingClientRect() ?? null
              }
              ignoreRef={blockBarRef}
              onSave={(next) => {
                widgetOp((root, widget) => setNoteWidgetConfig(root, widget, next))
                setWidgetSettingsOpen(false)
                visualRef.current?.focus()
              }}
              onClose={() => {
                setWidgetSettingsOpen(false)
                visualRef.current?.focus()
              }}
            />
          ) : null}
          <NoteGutter
            caretLine={caretLine}
            menu={insertMenu}
            onMenuChange={setInsertMenu}
            items={insertItems}
            busy={uploading}
            widgets={widgetCatalog}
            onPickWidget={placeWidget}
          />
        </div>

        <NoteFootBar
          aiActions={<>
            {ai.canUndo ? <NoteButton variant="quiet" icon={<Undo />} aria-label={t.phantasi.noteAiUndo} title={t.phantasi.noteAiUndo} onClick={ai.undo}>{t.phantasi.noteAiUndo}</NoteButton> : null}
            <NoteButton
              variant="quiet"
              disabled={loading || saving || !contentMd.trim()}
              icon={<Sparkles />}
              aria-label={t.phantasi.noteAiTitle}
              title={t.phantasi.noteAiTitle}
              active={ai.isOpen}
              aria-expanded={ai.isOpen}
              onMouseDown={event => event.preventDefault()}
              onClick={ai.isOpen ? ai.close : openAi}
            >
              {t.phantasi.noteAiTitle}
            </NoteButton>
          </>}
          chars={bodyChars}
          pane={pane}
          onPaneChange={setPane}
        />

        <NoteSettingsDrawer
          editorSettings={settingsOpen ? <NoteEditorSettings
            cloudId={cloudId}
            defaultView={preference.defaultView}
            preferenceBusy={preference.busy}
            onDefaultView={preference.changeDefaultView}
            onRestore={restoreHistory}
            busy={saving || loading}
                                         /> : null}
          companion={companionItemId != null ? <NoteCompanionSection itemId={companionItemId} /> : null}
          open={settingsOpen}
          onClose={() => setSettingsOpen(false)}
          title={title}
          docStatus={docStatus}
          topic={topic}
          topicChoices={categoryNames}
          onTopicChange={setTopic}
          onCreateTopic={handleCreateTopic}
          publishedAt={publishedAt}
          onPublishedAtChange={setPublishedAt}
          slugInput={slugInput}
          slugInvalid={slugInvalid}
          onSlugInput={onSlugInput}
          articleHref={articleHref}
          scheduledAt={scheduledAt}
          onScheduledAtChange={setScheduledAt}
          canSchedule={cloudId != null}
          busy={saving || loading}
          onSchedule={() => {
            void handleSchedule()
          }}
          onUnschedule={() => {
            void handleUnschedule()
          }}
          cover={cover}
          coverPreview={coverPreview}
          uploading={uploading}
          onPickCover={() => coverFileRef.current?.click()}
          onClearCover={() => setCover(null)}
          canDelete={canDelete}
          onDelete={() => {
            void handleDelete()
          }}
          authors={authorChips}
          addableAuthors={addableAuthors}
          authorBusy={authorBusy}
          onAddAuthor={(userId) => {
            void handleAddAuthor(userId)
          }}
          onRemoveAuthor={(userId) => {
            void handleRemoveAuthor(userId)
          }}
        />
      </div>

      <input
        ref={fileRef}
        type="file"
        accept="image/*"
        className="phantasi-bar__file"
        onChange={(e) => {
          const file = e.target.files?.[0]
          e.target.value = ''
          if (file) void handleUpload(file, 'body')
        }}
      />
      <input
        ref={coverFileRef}
        type="file"
        accept="image/*"
        className="phantasi-bar__file"
        onChange={(e) => {
          const file = e.target.files?.[0]
          e.target.value = ''
          if (file) void handleUpload(file, 'cover')
        }}
      />
      <input
        ref={imageReplaceRef}
        type="file"
        accept="image/*"
        className="phantasi-bar__file"
        onChange={(e) => {
          const file = e.target.files?.[0]
          e.target.value = ''
          if (file) void handleUpload(file, 'replace')
        }}
      />
      {visualWidgets.portals}
      {previewWidgets.portals}
    </motion.div>,
    document.body,
  )
}
