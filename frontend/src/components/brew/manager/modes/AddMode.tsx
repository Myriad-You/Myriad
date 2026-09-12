import type { AddSubmitInput } from './useAddSourceForm'
import {
  LuAlertCircle as AlertCircle,
  LuCheck as Check,
  LuChevronDown as ChevronDown,
  LuDownload as Download,
  LuExternalLink as ExternalLink,
  LuFileText as FileText,
  LuFolderOpen as FolderOpen,
  LuLink as Link,
  NotionIcon,
  LuRss as Rss,
  RSSHubIcon,
  LuSparkles as Sparkles,
  LuUpload as Upload,
  LuX as X,
} from '@lib/icons'
import { useI18n } from '../../../../contexts/I18nContext'
import { Spinner } from '../../../Spinner'
import { BrewBarWrap, BrewMark } from '../../ui/Bar'
import {
  Sheet,
  SheetBody,
  SheetChoice,
  SheetChoices,
  SheetCount,
  SheetDrop,
  SheetField,
  SheetFoot,
  SheetGhost,
  SheetGhostLabel,
  SheetGrow,
  SheetInput,
  SheetMark,
  SheetMenu,
  SheetMenuBody,
  SheetMenuItem,
  SheetNotice,
  SheetPair,
  SheetRow,
  SheetStack,
  SheetSubmit,
  SheetSwitch,
  SheetTab,
  SheetTrigger,
} from '../../ui/Sheet'
import {
  addHintKey,
  addSubmitLabelKey,
  addUrlLabelKey,
  addUrlPlaceholder,
} from './addSource'
import { useAddSourceForm } from './useAddSourceForm'

export interface AddModeProps {
  allCategories: string[]
  sourcesCount: number
  onSubmit?: (
    data: AddSubmitInput,
  ) => Promise<{ success: boolean; error?: string; title?: string }>
  onDiscover?: (url: string) => Promise<{
    url: string
    autocompleted: boolean
    title: string
    feed_type: string
  } | null>
  onImportOpml?: (
    content: string,
    signal?: AbortSignal,
  ) => Promise<{ imported: number; skipped: number }>
  onExportOpml?: () => void
  RSSHubConfigComponent?: React.ComponentType<{
    onConfigChange: (config: unknown, fullUrl: string) => void
    disabled?: boolean
  }>
}

export function AddMode({
  allCategories,
  sourcesCount,
  onSubmit,
  onDiscover,
  onImportOpml,
  onExportOpml,
  RSSHubConfigComponent,
}: AddModeProps) {
  const { t: i18n, format } = useI18n()
  const t = i18n.brew
  const form = useAddSourceForm({
    onSubmit,
    onDiscover,
    onImportOpml,
    onExportOpml,
  })
  const hint = t[addHintKey(form.fieldKind)]
  const urlLabel = t[addUrlLabelKey(form.fieldKind)]
  const submitLabel = t[addSubmitLabelKey(form.sourceType)]

  return (
    <Sheet>
      <SheetBody>
        {form.tab === 'single' ? (
          <SheetStack onSubmit={form.handleSubmit}>
            <SheetField label={t.sourceTypeLabel} hint={hint}>
              <SheetChoices>
                <SheetChoice
                  on={form.sourceType === 'link'}
                  disabled={form.loading}
                  onClick={() => form.pickKind('link')}
                >
                  <ExternalLink />
                  <span>{t.pureLink}</span>
                </SheetChoice>
                <SheetChoice
                  on={form.fieldKind === 'rss'}
                  disabled={form.loading}
                  onClick={() => form.pickKind('rss')}
                >
                  <Rss />
                  <span>RSS</span>
                </SheetChoice>
                <SheetChoice
                  on={form.sourceType === 'rsshub'}
                  disabled={form.loading}
                  onClick={() => form.pickKind('rsshub')}
                >
                  <RSSHubIcon />
                  <span>RSSHub</span>
                </SheetChoice>
                <SheetChoice
                  on={form.feedType === 'notion'}
                  disabled={form.loading}
                  onClick={() => form.pickKind('notion')}
                >
                  <NotionIcon />
                  <span>Notion</span>
                </SheetChoice>
              </SheetChoices>
            </SheetField>

            {form.sourceType !== 'link' && form.sourceType !== 'rsshub' ? (
              <SheetSwitch
                icon={<Sparkles />}
                title="Brewlia AI"
                description={t.brewliaShortDesc}
                on={form.sourceType === 'brewlia'}
                onToggle={() =>
                  form.setSourceType(
                    form.sourceType === 'brewlia' ? 'rss' : 'brewlia',
                  )
                }
                disabled={form.loading}
                toggleTitle={
                  form.sourceType === 'brewlia' ? t.disableAI : t.enableAI
                }
              />
            ) : null}

            {form.sourceType === 'rsshub' && RSSHubConfigComponent ? (
              <>
                <RSSHubConfigComponent
                  onConfigChange={form.setRsshub}
                  disabled={form.loading}
                />
                <SheetSwitch
                  icon={<Sparkles />}
                  title="Brewlia AI"
                  description={t.brewliaFeatures}
                  on={form.enableBrewliaForRsshub}
                  onToggle={() =>
                    form.setEnableBrewliaForRsshub(!form.enableBrewliaForRsshub)
                  }
                  disabled={form.loading}
                  toggleTitle={
                    form.enableBrewliaForRsshub ? t.disableAI : t.enableAI
                  }
                />
              </>
            ) : null}

            {form.sourceType !== 'rsshub' ? (
              <SheetField label={urlLabel} required>
                <SheetRow>
                  <SheetGrow>
                    <BrewMark>
                      <Link />
                    </BrewMark>
                    <SheetInput
                      type="url"
                      withMark
                      value={form.url}
                      onChange={(event) => {
                        form.setUrl(event.target.value)
                        form.setDiscovered(null)
                      }}
                      placeholder={addUrlPlaceholder(form.fieldKind)}
                      disabled={form.loading}
                    />
                  </SheetGrow>
                  {form.fieldKind === 'rss' ? (
                    <SheetGhost
                      fit
                      onClick={form.handleDiscover}
                      disabled={form.discovering || !form.url.trim()}
                    >
                      {form.discovering ? (
                        <Spinner size="xs" color="current" />
                      ) : (
                        t.discover
                      )}
                    </SheetGhost>
                  ) : null}
                </SheetRow>
              </SheetField>
            ) : null}

            {form.feedType === 'notion' ? (
              <SheetField label="Notion Integration Token" required>
                <SheetInput
                  type="password"
                  value={form.notionToken}
                  onChange={(event) => form.setNotionToken(event.target.value)}
                  placeholder="secret_xxx..."
                  disabled={form.loading}
                />
              </SheetField>
            ) : null}

            {form.discovered && form.fieldKind === 'rss' ? (
              <SheetNotice tone="ok">
                <Check />
                <span>{form.discovered.title}</span>
                <SheetCount>
                  {form.discovered.feed_type.toUpperCase()}
                </SheetCount>
                {form.sourceType === 'brewlia' ? (
                  <SheetCount>AI</SheetCount>
                ) : null}
              </SheetNotice>
            ) : null}

            <SheetPair>
              <SheetField
                label={t.nameLabel}
                required={form.sourceType === 'link'}
              >
                <SheetInput
                  type="text"
                  value={form.name}
                  onChange={(event) => form.setName(event.target.value)}
                  placeholder={
                    form.sourceType === 'link' ? t.enterName : t.autoFetch
                  }
                  disabled={form.loading}
                />
              </SheetField>
              <SheetField label={t.category}>
                <BrewBarWrap>
                  <SheetTrigger
                    onClick={() => form.setCategoryOpen(!form.categoryOpen)}
                    disabled={form.loading}
                  >
                    <span>{form.category || t.selectCategory}</span>
                    <ChevronDown
                      className={`brew-bar__chev${form.categoryOpen ? ' is-open' : ''}`}
                    />
                  </SheetTrigger>
                  {form.categoryOpen ? (
                    <SheetMenu>
                      <div style={{ padding: '0.35rem' }}>
                        <SheetInput
                          type="text"
                          value={form.category}
                          onChange={(event) =>
                            form.setCategory(event.target.value)
                          }
                          placeholder={t.inputNewCategory}
                          onClick={(event) => event.stopPropagation()}
                        />
                      </div>
                      <SheetMenuBody>
                        <SheetMenuItem
                          on={!form.category}
                          onClick={() => form.pickCategory('')}
                        >
                          {t.noCategory}
                          {!form.category ? <Check /> : null}
                        </SheetMenuItem>
                        {allCategories.map((cat) => (
                          <SheetMenuItem
                            key={cat}
                            on={form.category === cat}
                            onClick={() => form.pickCategory(cat)}
                          >
                            {cat}
                            {form.category === cat ? <Check /> : null}
                          </SheetMenuItem>
                        ))}
                      </SheetMenuBody>
                    </SheetMenu>
                  ) : null}
                </BrewBarWrap>
              </SheetField>
            </SheetPair>

            <SheetField label={t.siteIcon}>
              <SheetRow>
                <SheetMark>
                  {form.displayIcon ? (
                    <img src={form.displayIcon} alt="" />
                  ) : (
                    <Rss />
                  )}
                </SheetMark>
                <SheetGhostLabel fit>
                  <Upload />
                  {t.upload}
                  <input
                    ref={form.iconInputRef}
                    type="file"
                    accept="image/*"
                    onChange={form.handleIconUpload}
                    className="brew-bar__file"
                    disabled={form.loading}
                  />
                </SheetGhostLabel>
                {form.customIcon ? (
                  <SheetGhost
                    fit
                    onClick={form.clearIcon}
                    title={t.deleteIcon}
                  >
                    <X />
                  </SheetGhost>
                ) : null}
              </SheetRow>
            </SheetField>

            {form.error ? (
              <SheetNotice tone="bad">
                <AlertCircle />
                {form.error}
              </SheetNotice>
            ) : null}
            {form.success ? (
              <SheetNotice tone="ok">
                <Check />
                {form.success}
              </SheetNotice>
            ) : null}

            <SheetSubmit disabled={form.loading || !form.canSubmit}>
              {form.loading ? <Spinner size="xs" color="current" /> : null}
              {submitLabel}
            </SheetSubmit>
          </SheetStack>
        ) : (
          <SheetStack>
            <SheetDrop
              role="button"
              tabIndex={0}
              on={form.dragOver}
              onDragOver={(event) => {
                event.preventDefault()
                form.setDragOver(true)
              }}
              onDragLeave={() => form.setDragOver(false)}
              onDrop={form.handleDrop}
              onClick={() => form.fileInputRef.current?.click()}
              onKeyDown={(event) => {
                if (event.key === 'Enter' || event.key === ' ') {
                  event.preventDefault()
                  form.fileInputRef.current?.click()
                }
              }}
            >
              <input
                ref={form.fileInputRef}
                type="file"
                accept=".opml,.xml"
                onChange={form.handleFileSelect}
                className="brew-bar__file"
                title={t.selectOpmlFile}
              />
              <FolderOpen />
              <p>{t.dropOpmlHere}</p>
              <small>{t.supportedFormats}</small>
            </SheetDrop>

            {form.opmlContent ? (
              <SheetSubmit
                type="button"
                onClick={form.handleImport}
                disabled={form.opmlLoading}
              >
                {form.opmlLoading ? (
                  <Spinner size="xs" color="current" />
                ) : (
                  <Upload />
                )}
                {t.startImport}
              </SheetSubmit>
            ) : null}

            <SheetGhost
              onClick={form.handleExport}
              disabled={form.exporting || sourcesCount === 0}
            >
              {form.exporting ? (
                <Spinner size="xs" color="current" />
              ) : (
                <Download />
              )}
              {format(t.exportOpml, { count: sourcesCount })}
            </SheetGhost>

            {form.opmlResult ? (
              <SheetNotice tone="ok">
                <Check />
                {format(t.importResult, {
                  imported: form.opmlResult.imported,
                  skipped:
                    form.opmlResult.skipped > 0
                      ? format(t.skippedCount, {
                          count: form.opmlResult.skipped,
                        })
                      : '',
                })}
              </SheetNotice>
            ) : null}

            {form.error ? (
              <SheetNotice tone="bad">
                <AlertCircle />
                {form.error}
              </SheetNotice>
            ) : null}
          </SheetStack>
        )}
      </SheetBody>

      <SheetFoot>
        <SheetTab
          on={form.tab === 'single'}
          onClick={() => form.setTab('single')}
        >
          <Link />
          {t.singleAdd}
        </SheetTab>
        <SheetTab on={form.tab === 'opml'} onClick={() => form.setTab('opml')}>
          <FileText />
          OPML
        </SheetTab>
      </SheetFoot>
    </Sheet>
  )
}
