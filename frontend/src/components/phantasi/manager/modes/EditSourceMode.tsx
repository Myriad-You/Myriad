import type { ReactNode } from 'react'
import type { PhantasiSource, RSSHubConfig } from '../../../../types/phantasi'
import type { AddFieldKind } from './addSource'
import type { EditFieldKind, SubscriptionMode } from './editSource'
import { LuRss as Rss, LuSearch as Search, LuSparkles as Sparkles } from '@lib/icons'
import { useEffect, useMemo, useRef, useState } from 'react'
import { useI18n } from '../../../../contexts/I18nContext'
import { userFacingError } from '../../../../utils/userFacingError'
import { CompactSettingGroup } from '../../../settings/CompactSettingGroup'
import { SegmentedControl } from '../../../settings/items/ChoiceControls'
import { InputItem } from '../../../settings/items/InputItem'
import { SettingsButton } from '../../../settings/items/SettingsButton'
import { SwitchItem } from '../../../settings/items/SwitchItem'
import { SettingTitleTag } from '../../../settings/SettingTitleTag'
import { Spinner } from '../../../Spinner'
import {
  getIconUrl,
  isFriendLinkCategory,
  isMineCategory,
} from '../../constants'
import { listPickerCategories } from '../../logic/categories'
import { RequestTurn, unlessAborted } from '../../logic/requestTurn'
import { draftRssShareUrl } from '../../logic/shareRss'
import RSSHubConfigComponent from '../RSSHubConfig'
import { shareRssAddress } from '../shareRss'
import {
  addHintKey,
  addUrlLabelKey,
  addUrlPlaceholder,
} from './addSource'
import {
  canSubmitEdit,
  EDIT_INTERVALS,
  editFieldKind,
  pickEditKind,
  resolveEditSourcePayload,
} from './editSource'
import { FormBlock } from './FormBlock'
import { SourceCategoryField } from './SourceCategoryField'
import { SourceKindControl } from './SourceKindControl'
import './AddMode.css'

interface EditSourceModeProps {
  source: PhantasiSource
  categories: string[]
  onSave: (
    id: number,
    data: ReturnType<typeof resolveEditSourcePayload>,
  ) => Promise<void>
  onGenerateStyleTags?: (
    sourceId: number,
    signal?: AbortSignal,
  ) => Promise<{ success: boolean; tags?: string[] }>
  onDiscover?: (
    url: string,
    signal?: AbortSignal,
  ) => Promise<{
    url: string
    autocompleted: boolean
    title: string
    feed_type: string
  } | null>
  notesRssEnabled?: boolean
}

function categoryLabel(
  cat: string,
  labels: { friendLinks: string; me: string },
): string {
  if (isFriendLinkCategory(cat)) return labels.friendLinks
  if (isMineCategory(cat)) return labels.me
  return cat
}

function TagEditor({
  itemKey,
  label,
  description,
  draft,
  onDraft,
  placeholder,
  tags,
  empty,
  deleteLabel,
  disabled,
  accessory,
  onAdd,
  onRemove,
}: {
  itemKey: string
  label: string
  description: string
  draft: string
  onDraft: (value: string) => void
  placeholder: string
  tags: readonly string[]
  empty: string
  deleteLabel: string
  disabled: boolean
  accessory: ReactNode
  onAdd: () => void
  onRemove: (tag: string) => void
}) {
  return (
    <>
      <div
        onKeyDown={(event) => {
          if (event.key !== 'Enter') return
          event.preventDefault()
          onAdd()
        }}
      >
        <InputItem
          itemKey={itemKey}
          size="sm"
          label={label}
          description={description}
          value={draft}
          onChange={onDraft}
          placeholder={placeholder}
          disabled={disabled}
          labelAccessory={accessory}
        />
      </div>
      <div className="phantasi-add-form__tags">
        {tags.length > 0 ? (
          tags.map((tag) => (
            <SettingTitleTag
              key={tag}
              onDismiss={() => onRemove(tag)}
              dismissAriaLabel={deleteLabel}
            >
              {tag}
            </SettingTitleTag>
          ))
        ) : (
          <SettingTitleTag variant="muted">{empty}</SettingTitleTag>
        )}
      </div>
    </>
  )
}

export function EditSourceMode({
  source,
  categories,
  onSave,
  onGenerateStyleTags,
  onDiscover,
  notesRssEnabled = false,
}: EditSourceModeProps) {
  const { t } = useI18n()
  const phantasi = t.phantasi
  const generateTurns = useRef(new RequestTurn())
  const discoverTurns = useRef(new RequestTurn())
  useEffect(
    () => () => {
      generateTurns.current.cancel()
      discoverTurns.current.cancel()
    },
    [],
  )
  const originalKind = editFieldKind(source)
  const [fieldKind, setFieldKind] = useState<EditFieldKind>(originalKind)
  const [name, setName] = useState(source.name)
  const [category, setCategory] = useState(source.category?.trim() ?? '')
  const [categoryOpen, setCategoryOpen] = useState(false)
  const [url, setUrl] = useState(source.url)
  const [notionToken, setNotionToken] = useState('')
  const [rsshubFullUrl, setRsshubFullUrl] = useState('')
  const [rsshubRoute, setRsshubRoute] = useState(source.rsshub_route ?? '')
  const [discovering, setDiscovering] = useState(false)
  const [updateInterval, setUpdateInterval] = useState(source.update_interval)
  const [icon, setIcon] = useState(
    () => getIconUrl(source.icon) ?? source.icon ?? '',
  )
  const [iconDirty, setIconDirty] = useState(false)
  const [themeColor, setThemeColor] = useState(source.theme_color || '')
  const [paused, setPaused] = useState(!source.enabled)
  const [phantasiaiOn, setPhantasiaiOn] = useState(source.source_type === 'phantasiai')
  const [styleTags, setStyleTags] = useState(source.ai_style_tags || [])
  const [newTag, setNewTag] = useState('')
  const [generatingTags, setGeneratingTags] = useState(false)
  const [adminOnly, setAdminOnly] = useState(source.admin_only)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const isLink = fieldKind === 'link'
  const isNote = fieldKind === 'note'
  const shareUrl = draftRssShareUrl(
    fieldKind,
    typeof window === 'undefined' ? '' : window.location.origin,
  )
  const shareRss = () => {
    if (!shareUrl) return
    void shareRssAddress(
      shareUrl,
      name.trim() || source.name,
      phantasi.rssCopied,
      t.errors.clipboardFailed,
    )
  }
  const showMode = !isLink && !isNote
  const subscriptionMode: SubscriptionMode = paused
    ? 'disabled'
    : phantasiaiOn
      ? 'phantasiai'
      : 'normal'
  const showInterval = showMode && !paused
  const showAiTags = showMode && !paused && phantasiaiOn
  const showCustomTags = isLink
  const allCategories = useMemo(
    () => listPickerCategories(categories),
    [categories],
  )
  const intervalLabels: Record<number, string> = {
    15: phantasi.interval15min,
    30: phantasi.interval30min,
    60: phantasi.interval1hour,
    120: phantasi.interval2hour,
    360: phantasi.interval6hour,
    720: phantasi.interval12hour,
    1440: phantasi.intervalDaily,
  }
  const presetLabels = { friendLinks: phantasi.friendLinks, me: phantasi.me }

  const changeIcon = (value: string) => {
    setIcon(value)
    setIconDirty(true)
    if (value) setThemeColor('')
  }

  const addTag = () => {
    const tag = newTag.trim()
    if (!tag || styleTags.includes(tag) || styleTags.length >= 3) return
    setStyleTags((prev) => [...prev, tag])
    setNewTag('')
  }

  const pickKind = (next: AddFieldKind) => {
    const picked = pickEditKind(next)
    setFieldKind(next)
    if (picked.clearUrl) {
      setUrl('')
      setRsshubFullUrl('')
    } else if (!url.trim()) {
      setUrl(source.url)
    }
    if (next === 'link') setPhantasiaiOn(false)
    if (next !== 'notion') setNotionToken('')
  }

  const handleDiscover = async () => {
    if (!onDiscover || !url.trim()) return
    const signal = discoverTurns.current.begin()
    setDiscovering(true)
    setError(null)
    try {
      const result = await onDiscover(url, signal)
      unlessAborted(signal, () => {
        if (result?.url) {
          setUrl(result.url)
          if (result.title && !name.trim()) setName(result.title)
        }
      })
    } catch (err) {
      unlessAborted(signal, () => {
        setError(userFacingError(err, phantasi.errorDiscoverFailed))
      })
    } finally {
      unlessAborted(signal, () => setDiscovering(false))
    }
  }

  const canSubmit = canSubmitEdit({
    fieldKind,
    originalKind,
    url: fieldKind === 'rsshub' ? rsshubFullUrl || url : url,
    name,
    rsshubFullUrl,
    notionToken,
  })

  const handleSave = async () => {
    if (!canSubmit) return
    setSaving(true)
    setError(null)
    try {
      await onSave(
        source.id,
        resolveEditSourcePayload({
          source,
          fieldKind,
          name,
          category,
          url: fieldKind === 'rsshub' ? rsshubFullUrl || url : url,
          updateInterval,
          subscriptionMode,
          customIcon: iconDirty ? icon : null,
          themeColor,
          styleTags,
          adminOnly,
          notionToken,
          rsshubRoute,
        }),
      )
    } catch (err) {
      setError(userFacingError(err, phantasi.errorSaveFailed))
    } finally {
      setSaving(false)
    }
  }

  return (
    <div
      className={`phantasi-add-form${categoryOpen ? ' is-category-open' : ''}`}
    >
      <form
        className="phantasi-add-form__stack"
        onSubmit={(event) => {
          event.preventDefault()
          void handleSave()
        }}
      >
        <FormBlock
          title={fieldKind === 'note' ? phantasi.boardNotes : phantasi.sourceTypeLabel}
          hint={fieldKind === 'note' ? undefined : phantasi[addHintKey(fieldKind)]}
        >
          {fieldKind === 'note' ? (
            notesRssEnabled && shareUrl ? (
              <div className="phantasi-add-form__tags">
                <SettingTitleTag
                  icon={<Rss />}
                  disabled={saving}
                  title={phantasi.shareRss}
                  onClick={shareRss}
                >
                  {phantasi.shareRss}
                </SettingTitleTag>
              </div>
            ) : null
          ) : (
            <SourceKindControl
              value={fieldKind}
              onChange={pickKind}
              disabled={saving}
              hideLabel
            />
          )}
          {showMode ? (
            <div className="phantasi-add-form__toggles">
              <SwitchItem
                itemKey="phantasi-edit-phantasiai"
                size="sm"
                label={phantasi.phantasiaiLabel}
                description={
                  fieldKind === 'rsshub'
                    ? phantasi.phantasiaiFeatures
                    : phantasi.phantasiaiShortDesc
                }
                value={phantasiaiOn}
                onChange={setPhantasiaiOn}
                disabled={saving}
              />
              <SwitchItem
                itemKey="phantasi-edit-paused"
                size="sm"
                label={phantasi.pauseFetch}
                value={paused}
                onChange={setPaused}
                disabled={saving}
              />
            </div>
          ) : null}
        </FormBlock>

        {fieldKind === 'rsshub' ? (
          <FormBlock>
            <RSSHubConfigComponent
              initialConfig={
                source.rsshub_route
                  ? { instanceUrl: '', routePath: source.rsshub_route }
                  : undefined
              }
              onConfigChange={(config: RSSHubConfig, fullUrl: string) => {
                setRsshubFullUrl(fullUrl)
                setRsshubRoute(config.routePath)
              }}
              disabled={saving}
            />
          </FormBlock>
        ) : null}

        {fieldKind !== 'note' && fieldKind !== 'rsshub' ? (
          <FormBlock>
            <InputItem
              itemKey="phantasi-edit-url"
              size="sm"
              label={phantasi[addUrlLabelKey(fieldKind)]}
              required
              inputType="url"
              value={url}
              onChange={setUrl}
              placeholder={addUrlPlaceholder(fieldKind)}
              disabled={saving}
              labelAccessory={
                fieldKind === 'rss' ? (
                  <SettingTitleTag
                    icon={discovering ? <Spinner size="xs" /> : <Search />}
                    disabled={discovering || !url.trim() || !onDiscover}
                    onClick={() => {
                      void handleDiscover()
                    }}
                  >
                    {phantasi.discover}
                  </SettingTitleTag>
                ) : null
              }
            />
            {fieldKind === 'notion' ? (
              <InputItem
                itemKey="phantasi-edit-notion-token"
                size="sm"
                label="Notion Integration Token"
                required={originalKind !== 'notion'}
                inputType="password"
                value={notionToken}
                onChange={setNotionToken}
                placeholder="secret_xxx..."
                disabled={saving}
              />
            ) : null}
          </FormBlock>
        ) : null}

        <FormBlock>
          <CompactSettingGroup>
            <InputItem
              itemKey="phantasi-edit-name"
              size="sm"
              label={phantasi.nameLabel}
              required={isLink}
              value={name}
              onChange={setName}
              placeholder={isLink ? phantasi.enterName : phantasi.sourceName}
              disabled={saving}
            />
            {!isNote ? (
              <SourceCategoryField
                categories={allCategories}
                value={category}
                open={categoryOpen}
                onOpenChange={setCategoryOpen}
                onChange={setCategory}
                disabled={saving}
                labelFor={(name) => categoryLabel(name, presetLabels)}
              />
            ) : null}
          </CompactSettingGroup>
          <InputItem
            itemKey="phantasi-edit-color"
            size="sm"
            label={phantasi.themeColor}
            value={themeColor}
            onChange={setThemeColor}
            placeholder="#f97316"
            disabled={saving}
            labelAccessory={
              <span className="phantasi-add-form__swatch-well">
                <input
                  type="color"
                  className="phantasi-add-form__swatch"
                  value={themeColor || '#f97316'}
                  onChange={(event) => setThemeColor(event.target.value)}
                  disabled={saving}
                  title={phantasi.themeColor}
                />
              </span>
            }
          />
          <InputItem
            itemKey="phantasi-edit-icon"
            size="sm"
            variant="imageUpload"
            label={phantasi.siteIcon}
            value={icon}
            onChange={changeIcon}
            uploadLabel={phantasi.upload}
            clearImageLabel={phantasi.deleteIcon}
            disabled={saving}
          />
        </FormBlock>

        {showInterval || showAiTags || showCustomTags ? (
          <FormBlock title={showInterval ? phantasi.updateInterval : undefined}>
            {showInterval ? (
              <SegmentedControl
                size="sm"
                columns={4}
                className="phantasi-add-form__intervals"
                ariaLabel={phantasi.updateInterval}
                value={String(updateInterval)}
                onChange={(next) => setUpdateInterval(Number(next))}
                disabled={saving}
                options={(EDIT_INTERVALS.includes(
                  updateInterval as (typeof EDIT_INTERVALS)[number],
                )
                  ? EDIT_INTERVALS
                  : [...EDIT_INTERVALS, updateInterval].toSorted((a, b) => a - b)
                ).map((value) => ({
                  value: String(value),
                  label: intervalLabels[value] ?? `${value}`,
                }))}
              />
            ) : null}

            {showAiTags ? (
              <TagEditor
                itemKey="phantasi-edit-ai-tag"
                label={phantasi.aiStyleTags}
                description={phantasi.styleTagsDesc}
                draft={newTag}
                onDraft={setNewTag}
                placeholder={phantasi.tagInputPlaceholder}
                tags={styleTags}
                empty={phantasi.noTagsHint}
                deleteLabel={phantasi.deleteTag}
                disabled={saving || generatingTags || styleTags.length >= 3}
                onAdd={addTag}
                onRemove={(tag) =>
              setStyleTags((prev) => prev.filter((item) => item !== tag))
            }
                accessory={
              <SettingTitleTag
                icon={generatingTags ? <Spinner size="xs" /> : <Sparkles />}
                disabled={generatingTags || !onGenerateStyleTags || saving}
                onClick={() => {
                  if (!onGenerateStyleTags) return
                  const signal = generateTurns.current.begin()
                  setGeneratingTags(true)
                  setError(null)
                  void onGenerateStyleTags(source.id, signal)
                    .then((result) => {
                      unlessAborted(signal, () => {
                        if (result.success && result.tags) {
                          setStyleTags(result.tags)
                        }
                      })
                    })
                    .catch((err: unknown) => {
                      unlessAborted(signal, () => {
                        setError(
                          userFacingError(err, phantasi.errorGenerateStyleTags),
                        )
                      })
                    })
                    .finally(() => {
                      unlessAborted(signal, () => setGeneratingTags(false))
                    })
                }}
              >
                {styleTags.length > 0 ? phantasi.regenerateTags : phantasi.generateTags}
              </SettingTitleTag>
            }
              />
        ) : null}

        {showCustomTags ? (
          <TagEditor
            itemKey="phantasi-edit-custom-tag"
            label={phantasi.customTag}
            description={phantasi.customTagDesc}
            draft={newTag}
            onDraft={setNewTag}
            placeholder={phantasi.tagInputPlaceholder}
            tags={styleTags}
            empty={phantasi.noCustomTagHint}
            deleteLabel={phantasi.deleteTag}
            disabled={saving || styleTags.length >= 3}
            onAdd={addTag}
            onRemove={(tag) =>
              setStyleTags((prev) => prev.filter((item) => item !== tag))
            }
            accessory={
              <SettingTitleTag
                disabled={!newTag.trim() || styleTags.length >= 3 || saving}
                onClick={addTag}
              >
                {phantasi.addTag}
              </SettingTitleTag>
            }
          />
        ) : null}
          </FormBlock>
        ) : null}

        <div className="phantasi-add-form__toggles">
          <SwitchItem
            itemKey="phantasi-edit-admin"
            size="sm"
            label={phantasi.adminOnlyVisible}
            description={phantasi.adminOnlyVisibleHint}
            value={adminOnly}
            onChange={setAdminOnly}
            disabled={saving}
          />
        </div>

        {error ? (
          <SettingTitleTag variant="danger">{error}</SettingTitleTag>
        ) : null}

        <SettingsButton
          type="submit"
          variant="primary"
          size="sm"
          block
          loading={saving}
          disabled={saving || !canSubmit}
        >
          {phantasi.saveChanges}
        </SettingsButton>
      </form>
    </div>
  )
}
