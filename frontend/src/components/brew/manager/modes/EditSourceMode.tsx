import type { BrewSource } from '../../../../types/brew'
import type { SubscriptionMode } from './editSource'
import {
  LuAlertCircle as AlertCircle,
  LuCheck as Check,
  LuChevronDown as ChevronDown,
  LuEyeOff as EyeOff,
  LuPlus as Plus,
  LuRss as Rss,
  LuSparkles as Sparkles,
  LuTag as Tag,
  LuUpload as Upload,
  LuX as X,
} from '@lib/icons'

import { useEffect, useMemo, useRef, useState } from 'react'
import { useI18n } from '../../../../contexts/I18nContext'
import { userFacingError } from '../../../../utils/userFacingError'
import { Spinner } from '../../../Spinner'
import {
  getIconUrl,
  isFriendLinkCategory,
  isMineCategory,
  PRESET_CATEGORY_DB_VALUES,
} from '../../constants'
import { RequestTurn, unlessAborted } from '../../logic/requestTurn'
import { BrewBarWrap } from '../../ui/Bar'
import {
  Sheet,
  SheetBody,
  SheetChoice,
  SheetChoices,
  SheetField,
  SheetGhost,
  SheetGhostLabel,
  SheetHint,
  SheetInput,
  SheetMark,
  SheetMenu,
  SheetMenuBody,
  SheetMenuItem,
  SheetNotice,
  SheetPair,
  SheetPill,
  SheetPills,
  SheetRow,
  SheetStack,
  SheetSubmit,
  SheetSwatch,
  SheetSwitch,
  SheetTrigger,
} from '../../ui/Sheet'
import {
  canAddCategory,
  categoriesOf,
  EDIT_INTERVALS,
  resolveEditSourcePayload,

  subscriptionModeOf,
} from './editSource'

export interface EditSourceModeProps {
  source: BrewSource
  categories: string[]
  onSave: (
    id: number,
    data: ReturnType<typeof resolveEditSourcePayload>,
  ) => Promise<void>
  onGenerateStyleTags?: (
    sourceId: number,
    signal?: AbortSignal,
  ) => Promise<{ success: boolean; tags?: string[] }>
}

function categoryLabel(
  cat: string,
  labels: { friendLinks: string; me: string },
): string {
  if (isFriendLinkCategory(cat)) return labels.friendLinks
  if (isMineCategory(cat)) return labels.me
  return cat
}

export function EditSourceMode({
  source,
  categories,
  onSave,
  onGenerateStyleTags,
}: EditSourceModeProps) {
  const { t } = useI18n()
  const brew = t.brew
  const iconInputRef = useRef<HTMLInputElement>(null)
  const generateTurns = useRef(new RequestTurn())
  useEffect(() => () => generateTurns.current.cancel(), [])
  const [name, setName] = useState(source.name)
  const [selectedCategories, setSelectedCategories] = useState(
    categoriesOf(source),
  )
  const [newCategory, setNewCategory] = useState('')
  const [showCategoryMenu, setShowCategoryMenu] = useState(false)
  const [updateInterval, setUpdateInterval] = useState(source.update_interval)
  const [showIntervalMenu, setShowIntervalMenu] = useState(false)
  const [customIcon, setCustomIcon] = useState<string | null>(null)
  const [iconPreview, setIconPreview] = useState<string | null>(source.icon)
  const [themeColor, setThemeColor] = useState(source.theme_color || '')
  const [subscriptionMode, setSubscriptionMode] = useState<SubscriptionMode>(
    subscriptionModeOf(source),
  )
  const [styleTags, setStyleTags] = useState(source.ai_style_tags || [])
  const [newTag, setNewTag] = useState('')
  const [generatingTags, setGeneratingTags] = useState(false)
  const [adminOnly, setAdminOnly] = useState(source.admin_only)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const isLink = source.source_type === 'link'
  const isNote = source.source_type === 'note'
  const isRssHub = source.feed_type === 'rsshub'
  const isNotion = source.feed_type === 'notion'
  const showMode = !isLink && !isNote
  const showInterval = showMode && subscriptionMode !== 'disabled'
  const showAiTags = subscriptionMode === 'brewlia'
  const showCustomTags = isLink
  const allCategories = useMemo(
    () =>
      Iterator.from(
        new Set(PRESET_CATEGORY_DB_VALUES).union(new Set(categories)),
      ).toArray(),
    [categories],
  )
  const intervalLabels: Record<number, string> = {
    15: brew.interval15min,
    30: brew.interval30min,
    60: brew.interval1hour,
    120: brew.interval2hour,
    360: brew.interval6hour,
    720: brew.interval12hour,
    1440: brew.intervalDaily,
  }
  const mark = getIconUrl(iconPreview)
  const presetLabels = { friendLinks: brew.friendLinks, me: brew.me }

  const handleIcon = (file: File | undefined) => {
    if (!file) return
    if (!file.type.startsWith('image/')) {
      setError(brew.errorSelectImage)
      return
    }
    if (file.size > 500 * 1024) {
      setError(brew.errorImageSize)
      return
    }
    const reader = new FileReader()
    reader.onload = (event) => {
      const next = event.target?.result as string
      setCustomIcon(next)
      setIconPreview(next)
      setThemeColor('')
      setError(null)
    }
    reader.onerror = () => setError(brew.errorImageRead)
    reader.readAsDataURL(file)
  }

  const addTag = () => {
    const tag = newTag.trim()
    if (!tag || styleTags.includes(tag) || styleTags.length >= 3) return
    setStyleTags((prev) => [...prev, tag])
    setNewTag('')
  }

  const handleSave = async () => {
    setSaving(true)
    setError(null)
    try {
      await onSave(
        source.id,
        resolveEditSourcePayload({
          source,
          name,
          selectedCategories,
          newCategory,
          updateInterval,
          subscriptionMode,
          customIcon,
          themeColor,
          styleTags,
          adminOnly,
        }),
      )
    } catch (err) {
      setError(userFacingError(err, brew.errorSaveFailed))
    } finally {
      setSaving(false)
    }
  }

  return (
    <Sheet edit>
      <SheetBody>
        <SheetStack
          onSubmit={(event) => {
            event.preventDefault()
            void handleSave()
          }}
        >
          <SheetField label={brew.siteIcon}>
            <SheetRow>
              <SheetMark>{mark ? <img src={mark} alt="" /> : <Rss />}</SheetMark>
              <SheetGhostLabel fit>
                <Upload />
                {iconPreview ? brew.replace : brew.upload}
                <input
                  ref={iconInputRef}
                  type="file"
                  accept="image/*"
                  className="brew-bar__file"
                  aria-label={brew.uploadIcon}
                  onChange={(event) => handleIcon(event.target.files?.[0])}
                />
              </SheetGhostLabel>
              {iconPreview ? (
                <SheetGhost
                  fit
                  title={brew.clearIcon}
                  onClick={() => {
                    setCustomIcon('')
                    setIconPreview(null)
                    setThemeColor('')
                    if (iconInputRef.current) iconInputRef.current.value = ''
                  }}
                >
                  <X />
                  {brew.delete}
                </SheetGhost>
              ) : null}
            </SheetRow>
          </SheetField>

          <SheetPair>
            <SheetField label={brew.nameLabel} htmlFor="brew-edit-name">
              <SheetInput
                id="brew-edit-name"
                value={name}
                onChange={(event) => setName(event.target.value)}
                placeholder={brew.sourceName}
              />
            </SheetField>
            <SheetField label={brew.themeColor}>
              <SheetRow>
                <SheetSwatch>
                  <input
                    type="color"
                    value={themeColor || '#f97316'}
                    onChange={(event) => setThemeColor(event.target.value)}
                    title={brew.themeColor}
                  />
                </SheetSwatch>
                <SheetInput
                  value={themeColor}
                  onChange={(event) => setThemeColor(event.target.value)}
                  placeholder="#f97316"
                />
              </SheetRow>
            </SheetField>
          </SheetPair>

          <SheetField label={brew.category}>
            {selectedCategories.length > 0 ? (
              <SheetPills>
                {selectedCategories.map((cat) => (
                  <SheetPill key={cat}>
                    {categoryLabel(cat, presetLabels)}
                    <button
                      type="button"
                      title={brew.removeCategory}
                      onClick={() =>
                        setSelectedCategories((prev) =>
                          prev.filter((item) => item !== cat),
                        )
                      }
                    >
                      <X />
                    </button>
                  </SheetPill>
                ))}
              </SheetPills>
            ) : null}
            <BrewBarWrap>
              <SheetTrigger
                disabled={!canAddCategory(selectedCategories)}
                onClick={() => setShowCategoryMenu((open) => !open)}
              >
                <span>
                  {selectedCategories.length >= 2
                    ? brew.maxCategories
                    : selectedCategories.length === 1 &&
                        !canAddCategory(selectedCategories)
                      ? brew.needFriendLinkFirst
                      : brew.addCategory}
                </span>
                <ChevronDown
                  className={`brew-bar__chev${showCategoryMenu ? ' is-open' : ''}`}
                />
              </SheetTrigger>
              {showCategoryMenu && canAddCategory(selectedCategories) ? (
                <SheetMenu>
                  <div style={{ padding: '0.35rem' }}>
                    <SheetInput
                      value={newCategory}
                      placeholder={brew.enterCategoryHint}
                      onChange={(event) => setNewCategory(event.target.value)}
                      onKeyDown={(event) => {
                        if (event.key !== 'Enter' || !newCategory.trim()) return
                        event.preventDefault()
                        const next = newCategory.trim()
                        if (selectedCategories.includes(next)) return
                        setSelectedCategories((prev) => [...prev, next])
                        setNewCategory('')
                        setShowCategoryMenu(false)
                      }}
                    />
                  </div>
                  <SheetMenuBody>
                    {selectedCategories.length > 0 ? (
                      <SheetMenuItem
                        onClick={() => {
                          setSelectedCategories([])
                          setShowCategoryMenu(false)
                        }}
                      >
                        {brew.clearAllCategories}
                      </SheetMenuItem>
                    ) : null}
                    {allCategories
                      .filter((cat) => !selectedCategories.includes(cat))
                      .map((cat) => (
                        <SheetMenuItem
                          key={cat}
                          onClick={() => {
                            setSelectedCategories((prev) => [...prev, cat])
                            setShowCategoryMenu(false)
                          }}
                        >
                          {categoryLabel(cat, presetLabels)}
                        </SheetMenuItem>
                      ))}
                  </SheetMenuBody>
                </SheetMenu>
              ) : null}
            </BrewBarWrap>
          </SheetField>

          {showMode ? (
            <SheetField label={brew.subscriptionMode}>
              <SheetChoices mode>
                <SheetChoice
                  on={subscriptionMode === 'disabled'}
                  onClick={() => setSubscriptionMode('disabled')}
                >
                  <X />
                  <span>{brew.stop}</span>
                  <small>{brew.pauseFetch}</small>
                </SheetChoice>
                <SheetChoice
                  on={subscriptionMode === 'normal'}
                  onClick={() => setSubscriptionMode('normal')}
                >
                  <Rss />
                  <span>{brew.subscribe}</span>
                  <small>
                    {isRssHub
                      ? 'RSSHub'
                      : isNotion
                        ? 'Notion'
                        : brew.standardMode}
                  </small>
                </SheetChoice>
                <SheetChoice
                  on={subscriptionMode === 'brewlia'}
                  onClick={() => setSubscriptionMode('brewlia')}
                >
                  <Sparkles />
                  <span>Brewlia AI</span>
                  <small>{brew.brewliaFeatures}</small>
                </SheetChoice>
              </SheetChoices>
            </SheetField>
          ) : null}

          {showInterval ? (
            <SheetField label={brew.updateInterval}>
              <BrewBarWrap>
                <SheetTrigger
                  onClick={() => setShowIntervalMenu((open) => !open)}
                >
                  <span>
                    {intervalLabels[updateInterval] || brew.interval1hour}
                  </span>
                  <ChevronDown
                    className={`brew-bar__chev${showIntervalMenu ? ' is-open' : ''}`}
                  />
                </SheetTrigger>
                {showIntervalMenu ? (
                  <SheetMenu>
                    <SheetMenuBody>
                      {EDIT_INTERVALS.map((value) => (
                        <SheetMenuItem
                          key={value}
                          on={updateInterval === value}
                          onClick={() => {
                            setUpdateInterval(value)
                            setShowIntervalMenu(false)
                          }}
                        >
                          {intervalLabels[value]}
                          {updateInterval === value ? <Check /> : null}
                        </SheetMenuItem>
                      ))}
                    </SheetMenuBody>
                  </SheetMenu>
                ) : null}
              </BrewBarWrap>
            </SheetField>
          ) : null}

          {showAiTags ? (
            <SheetField label={brew.aiStyleTags} hint={brew.styleTagsDesc}>
              <SheetRow>
                <SheetGhost
                  fit
                  disabled={generatingTags || !onGenerateStyleTags}
                  onClick={async () => {
                    if (!onGenerateStyleTags) return
                    const signal = generateTurns.current.begin()
                    setGeneratingTags(true)
                    setError(null)
                    try {
                      const result = await onGenerateStyleTags(source.id, signal)
                      unlessAborted(signal, () => {
                        if (result.success && result.tags) setStyleTags(result.tags)
                      })
                    } catch (err) {
                      unlessAborted(signal, () => {
                        setError(
                          userFacingError(err, brew.errorGenerateStyleTags),
                        )
                      })
                    } finally {
                      unlessAborted(signal, () => setGeneratingTags(false))
                    }
                  }}
                >
                  {generatingTags ? (
                    <Spinner size="xs" color="current" />
                  ) : (
                    <Sparkles />
                  )}
                  {styleTags.length > 0 ? brew.regenerateTags : brew.generateTags}
                </SheetGhost>
              </SheetRow>
              {styleTags.length > 0 ? (
                <SheetPills>
                  {styleTags.map((tag) => (
                    <SheetPill key={tag}>
                      {tag}
                      <button
                        type="button"
                        title={brew.deleteTag}
                        onClick={() =>
                          setStyleTags((prev) =>
                            prev.filter((item) => item !== tag),
                          )
                        }
                      >
                        <X />
                      </button>
                    </SheetPill>
                  ))}
                </SheetPills>
              ) : (
                <SheetHint>{brew.noTagsHint}</SheetHint>
              )}
            </SheetField>
          ) : null}

          {showCustomTags ? (
            <SheetField label={brew.customTag} hint={brew.customTagDesc}>
              <SheetRow>
                <SheetInput
                  value={newTag}
                  maxLength={10}
                  placeholder={brew.tagInputPlaceholder}
                  disabled={styleTags.length >= 3}
                  onChange={(event) => setNewTag(event.target.value)}
                  onKeyDown={(event) => {
                    if (event.key === 'Enter') {
                      event.preventDefault()
                      addTag()
                    }
                  }}
                />
                <SheetGhost
                  fit
                  disabled={!newTag.trim() || styleTags.length >= 3}
                  onClick={addTag}
                >
                  <Plus />
                  {brew.addTag}
                </SheetGhost>
              </SheetRow>
              {styleTags.length > 0 ? (
                <SheetPills>
                  {styleTags.map((tag) => (
                    <SheetPill key={tag}>
                      <Tag />
                      {tag}
                      <button
                        type="button"
                        title={brew.deleteTag}
                        onClick={() =>
                          setStyleTags((prev) =>
                            prev.filter((item) => item !== tag),
                          )
                        }
                      >
                        <X />
                      </button>
                    </SheetPill>
                  ))}
                </SheetPills>
              ) : (
                <SheetHint>{brew.noCustomTagHint}</SheetHint>
              )}
            </SheetField>
          ) : null}

          <SheetSwitch
            icon={<EyeOff />}
            title={brew.adminOnlyVisible}
            description={brew.adminOnlyVisibleHint}
            on={adminOnly}
            onToggle={() => setAdminOnly((next) => !next)}
          />

          {error ? (
            <SheetNotice tone="bad">
              <AlertCircle />
              {error}
            </SheetNotice>
          ) : null}

          <SheetSubmit disabled={saving}>
            {saving ? <Spinner size="xs" color="current" /> : <Check />}
            {saving ? brew.saving : brew.saveChanges}
          </SheetSubmit>
        </SheetStack>
      </SheetBody>
    </Sheet>
  )
}
