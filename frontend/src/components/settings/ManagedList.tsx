/**
 * Generic list-management panel for settings pages.
 *
 * Layout: optional stats → chrome toolbar (query first, then domain
 * actions, then add-form) → optional expanded query / form panels →
 * scrollable rows with badge + actions.
 * Search/filter is a first-class ManagedList feature (not caller layout).
 * Callers own data; this only renders structure and wires clicks. Prefer
 * optimistic row updates at the call site so delete/cancel never need a full
 * page refresh.
 */

import type { ReactNode } from 'react'
import type { SettingsButtonVariant } from './items/SettingsButton'
import React, { useCallback, useEffect, useMemo, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { Spinner } from '../Spinner'
import { CheckboxCard } from './items/CheckboxCard'
import { SegmentedControl } from './items/ChoiceControls'
import { FieldSelect } from './items/FieldSelect'
import { InputItem } from './items/InputItem'
import { SettingsButton } from './items/SettingsButton'
import { SettingTitleGuideEntry } from './SettingTitleGuideEntry'
import './ManagedList.css'

/**
 * List chrome press chip — same shell as register switch.
 * Optional leading icon + tone (primary / danger bulk actions).
 */
function ChromeCard({
  label,
  checked = false,
  onPress,
  description,
  icon,
  disabled,
  loading,
  title,
  className,
  tone,
  'aria-expanded': ariaExpanded,
  'aria-label': ariaLabel,
}: {
  label: ReactNode
  checked?: boolean
  onPress: () => void
  description?: ReactNode
  icon?: ReactNode
  disabled?: boolean
  loading?: boolean
  title?: string
  className?: string
  tone?: 'default' | 'primary' | 'danger'
  'aria-expanded'?: boolean
  'aria-label'?: string
}) {
  return (
    <CheckboxCard
      variant="action"
      size="sm"
      tone={tone}
      label={label}
      description={description}
      icon={icon}
      checked={checked}
      onChange={() => onPress()}
      disabled={disabled}
      loading={loading}
      title={title}
      className={className}
      aria-expanded={ariaExpanded}
      aria-label={ariaLabel}
    />
  )
}

export type ManagedListTone =
  | 'default'
  | 'active'
  | 'success'
  | 'warn'
  | 'danger'
  | 'muted'

export type ManagedListButtonVariant = SettingsButtonVariant

/** Numeric (or text) metric chip */
export interface ManagedListStatMetric {
  key: string
  label: string
  value: number | string
  tone?: ManagedListTone
  kind?: 'metric'
}

/**
 * Toggle chip after metrics — CheckboxCard size="sm"
 * (list mini-button density + 权限下放 card language).
 */
export interface ManagedListStatSwitch {
  key: string
  label: string
  kind: 'switch'
  checked: boolean
  onChange: (checked: boolean) => void
  disabled?: boolean
  loading?: boolean
  /** Second line under the title (same as CheckboxGroup cards) */
  description?: string
  /** Optional leading icon (CheckboxCard) */
  icon?: ReactNode
  /** Native tooltip (optional; defaults not set when description is shown) */
  title?: string
  /**
   * Structured option guide (modal only).
   * 「显示说明」开启时标题旁出现入口；不内联、不进 ⓘ tooltip。
   */
  guide?: ReactNode
  /** 指南路径，供配置搜索跳转 */
  guidePath?: string
}

/**
 * Choice chip — same chrome as switch stats, with an inline select
 * (e.g. users “personal Tapp cleanup” next to allow-register).
 */
export interface ManagedListStatChoice {
  key: string
  kind: 'choice'
  label: string
  description?: string
  icon?: ReactNode
  value: string
  options: { value: string; label: string }[]
  onChange: (value: string) => void
  disabled?: boolean
  loading?: boolean
  title?: string
  guide?: ReactNode
  guidePath?: string
  /** Extra class on the card shell */
  className?: string
}

export type ManagedListStat =
  | ManagedListStatMetric
  | ManagedListStatSwitch
  | ManagedListStatChoice

export interface ManagedListAction {
  key: string
  label: string
  /** Second line under label (CheckboxCard desc; same as register switch). */
  description?: string
  /** Optional leading icon on chrome CheckboxCard / SettingsButton */
  icon?: ReactNode
  onClick: () => void
  disabled?: boolean
  loading?: boolean
  variant?: ManagedListButtonVariant
  /** Optional window.confirm message before onClick. */
  confirm?: string
  /** Accessible name when label is short. */
  ariaLabel?: string
  /** Native title / tooltip (e.g. why a button is disabled). */
  title?: string
}

export interface ManagedListFilterOption {
  key: string
  label: string
  /** Optional count badge on the chip. */
  count?: number
}

export interface ManagedListSearch {
  value: string
  onChange: (value: string) => void
  placeholder?: string
  ariaLabel?: string
}

export interface ManagedListFilters {
  options: ManagedListFilterOption[]
  value: string
  onChange: (key: string) => void
  ariaLabel?: string
}

export interface ManagedListItem {
  id: string | number
  title: ReactNode
  subtitle?: ReactNode
  meta?: ReactNode
  badge?: { label: string; tone?: ManagedListTone }
  /** Extra badges after the primary one (role + status, etc.). */
  badges?: Array<{ label: string; tone?: ManagedListTone }>
  /** Optional leading slot (icon / avatar). */
  leading?: ReactNode
  /**
   * Optional control between main text and action buttons
   * (e.g. FieldSelect for trust level).
   */
  trailing?: ReactNode
  actions?: ManagedListAction[]
  /**
   * Expandable detail panel under the row (caller-owned content).
   * When set, the main area becomes a toggle control.
   */
  expandContent?: ReactNode
  /** Controlled expand state. */
  expanded?: boolean
  onToggleExpand?: () => void
  /**
   * Replace the default expand hit with a custom control (e.g. a guide
   * float trigger). Receives the same leading + main nodes. When set,
   * `expandContent` is not shown inline.
   */
  renderHit?: (parts: { leading: ReactNode; main: ReactNode }) => ReactNode
  /** Dim row + block pointer while this row’s action runs. */
  busy?: boolean
  className?: string
}

export interface ManagedListProps {
  /** Compact counters above the list (pending / dead / …). */
  stats?: ManagedListStat[]
  /** Bulk / refresh actions. */
  toolbar?: ManagedListAction[]
  /** Domain search field above the body. */
  search?: ManagedListSearch
  /**
   * Single filter chip group (shorthand). Prefer `filterGroups` when
   * multiple independent SegmentedControls are needed (e.g. role + online).
   */
  filters?: ManagedListFilters
  /** Multiple filter chip groups rendered stacked in the query panel. */
  filterGroups?: ManagedListFilters[]
  /**
   * Search + filter (generic ManagedList query chrome).
   * When `search` / `filters` / `filterGroups` are set, a **leading**
   * toolbar button expands the query panel (collapsed by default).
   * Order is fixed: query → toolbar actions → add form.
   */
  /** Label on the expand control when the query bar is collapsed. */
  queryToggleLabel?: ReactNode
  /** Second line under query toggle (same card layout as register switch). */
  queryToggleDescription?: ReactNode
  /** Optional icon on the query toggle chip. */
  queryToggleIcon?: ReactNode
  /** Collapse control when the query bar is open. Default “Done”. */
  queryCollapseLabel?: ReactNode
  /** Second line under query collapse control. */
  queryCollapseDescription?: ReactNode
  /** Optional icon on the query collapse chip. */
  queryCollapseIcon?: ReactNode
  /**
   * Uncontrolled initial open state for search/filters. Default `false`.
   * Ignored when `queryOpen` is provided.
   */
  queryDefaultOpen?: boolean
  /** Controlled open state for the search/filter bar. */
  queryOpen?: boolean
  onQueryOpenChange?: (open: boolean) => void
  /**
   * Filter key treated as “no filter” for the active indicator
   * (default `'all'`). Applied to every filter group.
   */
  queryNeutralFilter?: string
  /**
   * Query panel chrome:
   * - `panel` (default): titled card with collapse control
   * - `plain`: no title row; search + filters only
   */
  queryChrome?: 'panel' | 'plain'
  /**
   * When false, query stays always open: no toolbar toggle, no collapse.
   * Default `true`.
   */
  queryCollapsible?: boolean
  /**
   * Optional create / add form panel. Callers own fields + submit;
   * ManagedList only provides chrome and placement.
   * Default: above the list body (`formPlacement="before"`).
   * Collapsed by default — expand via the toggle button.
   */
  form?: ReactNode
  /**
   * Where to put `form` relative to the list body.
   * - `before` (default): add-then-see-list
   * - `after`: list first, form below
   */
  formPlacement?: 'before' | 'after'
  /**
   * Label on the expand control when the form is collapsed.
   * Also used as the form panel heading when open (unless
   * `formOpenTitle` is set).
   */
  formTitle?: ReactNode
  /** Second line under form expand control. */
  formDescription?: ReactNode
  /** Optional icon on the form expand chip. */
  formIcon?: ReactNode
  /** Heading inside the open form panel; defaults to `formTitle`. */
  formOpenTitle?: ReactNode
  /** Label for the collapse control; default “Cancel”. */
  formCollapseLabel?: ReactNode
  /** Second line under form collapse control. */
  formCollapseDescription?: ReactNode
  /** Optional icon on the form collapse chip. */
  formCollapseIcon?: ReactNode
  /**
   * Uncontrolled initial open state. Default `false` (hidden).
   * Ignored when `formOpen` is provided.
   */
  formDefaultOpen?: boolean
  /** Controlled open state for the add form. */
  formOpen?: boolean
  onFormOpenChange?: (open: boolean) => void
  items: ManagedListItem[]
  emptyText: string
  /** Initial list fetch / full refresh spinner over the body. */
  loading?: boolean
  /** Soft working state (bulk action) without blanking the list. */
  working?: boolean
  /**
   * Body max height. Pass `null` / `'none'` to grow with content
   * (short settings lists). Default scrolls at 18rem.
   */
  maxHeight?: string | number | null
  /**
   * Soft page size when the body is height-constrained.
   * Avoids painting hundreds of empty-looking shells (delivery queue etc.).
   * Truncation offers “Show more” to grow the window by this size each click.
   * - `undefined`: auto — `80` when scrolling, unlimited when not
   * - `null`: never cap
   * - number: explicit page size
   */
  maxVisibleItems?: number | null
  /**
   * Footer when the list is truncated.
   * Receives (shown, total). Default: i18n `config.managedListShowing`.
   */
  truncateFooter?: (shown: number, total: number) => ReactNode
  className?: string
  /** Optional footer (e.g. “showing N of M”). */
  footer?: ReactNode
}

function toneClass(tone: ManagedListTone | undefined, prefix: string): string {
  return `${prefix} ${prefix}--${tone ?? 'default'}`
}

const ListActionButton = React.memo(({
  action,
  size = 'md',
  /**
   * Toolbar chrome: always CheckboxCard (incl. primary/danger bulk).
   * Row actions: SettingsButton (confirm / danger still available).
   */
  chrome = false,
}: {
  action: ManagedListAction
  size?: 'sm' | 'md'
  chrome?: boolean
}) => {
  const handle = useCallback(() => {
    if (action.confirm && !window.confirm(action.confirm)) return
    action.onClick()
  }, [action])

  // Top chip strip — unified CheckboxCard for all toolbar actions.
  if (chrome && size === 'sm') {
    const tone =
      action.variant === 'danger'
        ? 'danger'
        : action.variant === 'primary'
          ? 'primary'
          : 'default'
    return (
      <ChromeCard
        label={action.label}
        description={action.description}
        icon={action.icon}
        tone={tone}
        checked={false}
        onPress={handle}
        disabled={action.disabled}
        loading={action.loading}
        title={action.title ?? action.description}
        aria-label={action.ariaLabel ?? action.label}
      />
    )
  }

  return (
    <SettingsButton
      variant={action.variant ?? 'secondary'}
      size={size}
      icon={action.icon}
      disabled={action.disabled}
      loading={action.loading}
      confirm={action.confirm}
      aria-label={action.ariaLabel ?? action.label}
      title={action.title}
      onClick={handle}
    >
      {action.label}
    </SettingsButton>
  )
})

export const ManagedList = React.memo(({
  stats,
  toolbar,
  search,
  filters,
  filterGroups,
  queryToggleLabel: queryToggleLabelProp,
  queryToggleDescription,
  queryToggleIcon,
  queryCollapseLabel: queryCollapseLabelProp,
  queryCollapseDescription,
  queryCollapseIcon,
  queryDefaultOpen = false,
  queryOpen: queryOpenProp,
  onQueryOpenChange,
  queryNeutralFilter = 'all',
  queryChrome = 'panel',
  queryCollapsible = true,
  form,
  formPlacement = 'before',
  formTitle,
  formDescription,
  formIcon,
  formOpenTitle,
  formCollapseLabel: formCollapseLabelProp,
  formCollapseDescription,
  formCollapseIcon,
  formDefaultOpen = false,
  formOpen: formOpenProp,
  onFormOpenChange,
  items,
  emptyText,
  loading = false,
  working = false,
  maxHeight = '18rem',
  maxVisibleItems,
  truncateFooter,
  className = '',
  footer,
}: ManagedListProps) => {
  const { t } = useI18n()
  const queryToggleLabel =
    queryToggleLabelProp ?? t.config.managedListSearchFilter
  const queryCollapseLabel =
    queryCollapseLabelProp ?? t.config.managedListQueryDone
  const formCollapseLabel =
    formCollapseLabelProp ?? t.config.managedListFormCancel
  const constrain =
    maxHeight != null && maxHeight !== 'none' && maxHeight !== ''
  const heightStyle = constrain
    ? typeof maxHeight === 'number'
      ? `${maxHeight}px`
      : String(maxHeight)
    : undefined

  /**
   * Page size for scroll bodies; uncapped lists stay unlimited.
   * “Show more” multiplies this window without remounting the list.
   */
  const pageSize = useMemo(() => {
    if (maxVisibleItems === undefined) return constrain ? 80 : null
    return maxVisibleItems
  }, [maxVisibleItems, constrain])

  const totalCount = items.length
  /** How many pages of `pageSize` are currently visible (1-based growth). */
  const [visiblePages, setVisiblePages] = useState(1)

  // Reset only when list size / page size changes — not on every new array ref.
  useEffect(() => {
    setVisiblePages(1)
  }, [totalCount, pageSize])

  const softCap =
    pageSize == null ? null : Math.min(totalCount, pageSize * visiblePages)

  const visibleItems = useMemo(() => {
    if (softCap == null || totalCount <= softCap) return items
    return items.slice(0, softCap)
  }, [items, softCap, totalCount])

  const isTruncated = softCap != null && softCap < totalCount
  const canShowMore = isTruncated

  const truncateNote =
    pageSize != null && totalCount > pageSize
      ? truncateFooter
        ? truncateFooter(visibleItems.length, totalCount)
        : t.config.managedListShowing
            .replace('{shown}', String(visibleItems.length))
            .replace('{total}', String(totalCount))
      : null

  const handleShowMore = useCallback(() => {
    setVisiblePages((p) => p + 1)
  }, [])

  const resolvedFooter =
    footer != null || truncateNote != null || canShowMore ? (
      <>
        {footer != null ? <div>{footer}</div> : null}
        {truncateNote != null || canShowMore ? (
          <div className="managed-list-footer-truncate">
            {truncateNote != null ? (
              <span className="managed-list-footer-truncate-text">
                {truncateNote}
              </span>
            ) : null}
            {canShowMore ? (
              <SettingsButton
                variant="ghost"
                size="sm"
                className="managed-list-show-more"
                onClick={handleShowMore}
              >
                {t.config.managedListShowMore}
              </SettingsButton>
            ) : null}
          </div>
        ) : null}
      </>
    ) : null

  const resolvedFilterGroups = useMemo(() => {
    if (filterGroups && filterGroups.length > 0) return filterGroups
    if (filters && filters.options.length > 0) return [filters]
    return [] as ManagedListFilters[]
  }, [filterGroups, filters])

  const hasFilterBar =
    !!search || resolvedFilterGroups.some((g) => g.options.length > 0)

  const queryActive =
    (!!search && search.value.trim().length > 0) ||
    resolvedFilterGroups.some(
      (g) => g.options.length > 0 && g.value !== queryNeutralFilter,
    )

  const queryControlled = queryOpenProp !== undefined
  const [queryOpenInternal, setQueryOpenInternal] = useState(
    queryDefaultOpen || !queryCollapsible,
  )
  const queryOpen = !queryCollapsible
    ? true
    : queryControlled
      ? !!queryOpenProp
      : queryOpenInternal

  const setQueryOpen = useCallback(
    (open: boolean) => {
      if (!queryCollapsible) return
      if (!queryControlled) setQueryOpenInternal(open)
      onQueryOpenChange?.(open)
    },
    [queryCollapsible, queryControlled, onQueryOpenChange],
  )

  const formControlled = formOpenProp !== undefined
  const [formOpenInternal, setFormOpenInternal] = useState(formDefaultOpen)
  const formOpen = formControlled ? !!formOpenProp : formOpenInternal

  const setFormOpen = useCallback(
    (open: boolean) => {
      if (!formControlled) setFormOpenInternal(open)
      onFormOpenChange?.(open)
    },
    [formControlled, onFormOpenChange],
  )

  const expandLabel =
    formTitle != null && formTitle !== ''
      ? formTitle
      : t.config.managedListFormAdd
  const openHeading =
    formOpenTitle != null && formOpenTitle !== ''
      ? formOpenTitle
      : formTitle != null && formTitle !== ''
        ? formTitle
        : null

  // Keep chips in a fixed slot: open → same chip becomes cancel (no jump).
  const showQueryChip = queryCollapsible && hasFilterBar
  const showFormChip = form != null
  const hasToolbarActions = !!(toolbar && toolbar.length > 0)
  const hasChromeBar =
    hasToolbarActions || showQueryChip || showFormChip

  const formPanel =
    form != null && formOpen ? (
      <div
        className="managed-list-form"
        role="group"
        aria-label={
          typeof expandLabel === 'string'
            ? expandLabel
            : t.config.managedListFormAdd
        }
      >
        {/* Collapse stays on the top chip strip — no second cancel here */}
        {openHeading != null ? (
          <div className="managed-list-form-header is-title-only">
            <div className="managed-list-form-title">{openHeading}</div>
          </div>
        ) : null}
        <div className="managed-list-form-body settings-stack">{form}</div>
      </div>
    ) : null

  const queryPanel =
    hasFilterBar && queryOpen ? (
      <div
        className={[
          'managed-list-filter-bar',
          queryChrome === 'plain' ? 'is-plain' : '',
        ]
          .filter(Boolean)
          .join(' ')}
        role="search"
        aria-label={
          typeof queryToggleLabel === 'string'
            ? queryToggleLabel
            : t.config.managedListSearchFilter
        }
      >
        {/* Collapse stays on the top chip strip — title-only when panel chrome */}
        {queryChrome === 'panel' && queryCollapsible && (
          <div className="managed-list-filter-bar-header is-title-only">
            <div className="managed-list-form-title">
              {queryToggleLabel}
              {queryActive ? (
                <span className="managed-list-query-active-badge">·</span>
              ) : null}
            </div>
          </div>
        )}
        {search && (
          <div className="managed-list-search">
            <InputItem
              itemKey="managed-list-search"
              label={
                search.ariaLabel ??
                search.placeholder ??
                t.common.search
              }
              value={search.value}
              onChange={search.onChange}
              placeholder={search.placeholder}
              inputType="search"
              size="md"
              layout="vertical"
              autoComplete="off"
              className="managed-list-search-item"
            />
          </div>
        )}
        {resolvedFilterGroups.length > 0 && (
          <div className="managed-list-filter-groups">
            {resolvedFilterGroups.map((group, gi) =>
              group.options.length > 0 ? (
                <SegmentedControl
                  key={group.ariaLabel ?? `filter-group-${gi}`}
                  size="sm"
                  className="managed-list-filters"
                  ariaLabel={
                    group.ariaLabel ?? t.config.managedListFilterAria
                  }
                  value={group.value}
                  options={group.options.map((opt) => ({
                    value: opt.key,
                    label: opt.label,
                    count: opt.count,
                  }))}
                  onChange={group.onChange}
                />
              ) : null,
            )}
          </div>
        )}
      </div>
    ) : null

  const listBody = (
    <div
      className={`managed-list-body${constrain ? ' is-scroll' : ''}`}
      style={heightStyle ? { maxHeight: heightStyle } : undefined}
      role="list"
    >
      {loading && totalCount === 0 ? (
        <div className="managed-list-empty" role="status">
          <Spinner size="sm" color="primary" />
        </div>
      ) : totalCount === 0 ? (
        <div className="managed-list-empty" role="status">
          {emptyText}
        </div>
      ) : (
        visibleItems.map((item) => {
          const hasActions = !!(item.actions && item.actions.length > 0)
          const hasTrailing = item.trailing != null
          const hasCustomHit = item.renderHit != null
          const canExpand = item.expandContent != null || hasCustomHit
          const isExpanded = !!item.expanded && !hasCustomHit
          const badges = [
            ...(item.badge ? [item.badge] : []),
            ...(item.badges ?? []),
          ]
          const mainInner = (
            <>
              <div className="managed-list-row-title-line">
                {badges.map((b, bi) => (
                  <span
                    key={`${b.label}-${bi}`}
                    className={toneClass(b.tone, 'managed-list-badge')}
                  >
                    {b.label}
                  </span>
                ))}
                <div className="managed-list-row-title">{item.title}</div>
              </div>
              {item.subtitle != null && item.subtitle !== '' && (
                <div className="managed-list-row-subtitle">
                  {item.subtitle}
                </div>
              )}
              {item.meta != null && item.meta !== '' && (
                <div className="managed-list-row-meta">{item.meta}</div>
              )}
            </>
          )
          const leading = item.leading != null && (
            <div className="managed-list-row-leading">{item.leading}</div>
          )
          const side =
            hasTrailing || hasActions ? (
              <div
                className="managed-list-row-side"
                onClick={(e) => e.stopPropagation()}
                onKeyDown={(e) => e.stopPropagation()}
              >
                {hasTrailing && (
                  <div className="managed-list-row-trailing">
                    {item.trailing}
                  </div>
                )}
                {hasActions && (
                  <div className="managed-list-row-actions">
                    {item.actions!.map((a) => (
                      <ListActionButton
                        key={a.key}
                        action={{
                          ...a,
                          disabled: a.disabled || item.busy,
                          loading: a.loading,
                        }}
                        size="sm"
                      />
                    ))}
                  </div>
                )}
              </div>
            ) : null

          return (
            <div
              key={item.id}
              role="listitem"
              className={`managed-list-row${isExpanded ? ' is-expanded' : ''}${item.busy ? ' is-busy' : ''}${canExpand ? ' is-expandable' : ''}${item.className ? ` ${item.className}` : ''}`}
              aria-busy={item.busy || undefined}
            >
              {hasCustomHit ? (
                <div className="managed-list-row-head">
                  {item.renderHit!({
                    leading,
                    main: (
                      <div className="managed-list-row-main">{mainInner}</div>
                    ),
                  })}
                  {side}
                </div>
              ) : canExpand ? (
                <div className="managed-list-row-head">
                  {/*
                    Full hit target: leading + main. Side actions stay outside
                    and stopPropagation so they don't toggle expand.
                  */}
                  <button
                    type="button"
                    className="managed-list-row-hit"
                    onClick={item.onToggleExpand}
                    aria-expanded={isExpanded}
                  >
                    {leading}
                    <div className="managed-list-row-main">{mainInner}</div>
                  </button>
                  {side}
                </div>
              ) : (
                <div className="managed-list-row-head">
                  {leading}
                  <div className="managed-list-row-main">{mainInner}</div>
                  {side}
                </div>
              )}
              {canExpand && isExpanded && (
                <div className="managed-list-row-detail">
                  {item.expandContent}
                </div>
              )}
            </div>
          )
        })
      )}
    </div>
  )

  return (
    <div
      className={`managed-list${working ? ' is-working' : ''}${className ? ` ${className}` : ''}`}
    >
      {/*
        Unified top chip strip (one region, equal height):
        [metrics…] | [switch…] [query] [toolbar] [form…]
        All interactive chips sit after numeric data in the same wrap area.
      */}
      {(stats && stats.length > 0) || hasChromeBar ? (
        <div className="managed-list-top">
          <div
            className="managed-list-stats"
            role="group"
            aria-label={t.config.managedListStatsAria}
          >
            {/* 1. Numeric metrics */}
            {stats
              ?.filter(
                (s): s is ManagedListStatMetric =>
                  s.kind !== 'switch' && s.kind !== 'choice',
              )
              .map((s) => (
                <div
                  key={s.key}
                  className={toneClass(s.tone, 'managed-list-stat')}
                >
                  <span className="managed-list-stat-value">{s.value}</span>
                  <span className="managed-list-stat-label">{s.label}</span>
                </div>
              ))}

            {/* 2. Interactive chips after data (same card region) */}
            {((stats &&
              stats.some((s) => s.kind === 'switch' || s.kind === 'choice')) ||
              hasChromeBar) && (
              <div
                className="managed-list-chip-actions"
                role="toolbar"
                aria-label={t.config.managedListActionsAria}
              >
                {stats
                  ?.filter(
                    (s): s is ManagedListStatSwitch | ManagedListStatChoice =>
                      s.kind === 'switch' || s.kind === 'choice',
                  )
                  .map((s) => {
                    const labelNode =
                      s.guide != null && s.guide !== false && s.guide !== '' ? (
                        <>
                          {s.label}
                          <SettingTitleGuideEntry
                            title={String(s.label)}
                            guide={s.guide}
                          />
                        </>
                      ) : (
                        s.label
                      )

                    const card =
                      s.kind === 'switch' ? (
                        <CheckboxCard
                          size="sm"
                          label={labelNode}
                          description={s.description}
                          icon={s.icon}
                          checked={s.checked}
                          onChange={s.onChange}
                          disabled={s.disabled}
                          loading={s.loading}
                          title={s.title}
                        />
                      ) : (
                        <div
                          className={[
                            'checkbox-group-card',
                            'checkbox-group-card--sm',
                            'has-icon',
                            'no-indicator',
                            'managed-list-choice-card',
                            s.disabled || s.loading ? 'is-disabled' : '',
                            s.className ?? '',
                          ]
                            .filter(Boolean)
                            .join(' ')}
                        >
                          {/* Single header row: icon | title+desc | select (like switch chip) */}
                          <span className="checkbox-group-card-header">
                            {s.icon ? (
                              <span
                                className="checkbox-group-card-icon"
                                aria-hidden
                              >
                                {s.icon}
                              </span>
                            ) : null}
                            <span className="checkbox-group-card-text">
                              <span className="checkbox-group-card-label">
                                {labelNode}
                              </span>
                              {s.description ? (
                                <span
                                  className="checkbox-group-card-desc"
                                  title={
                                    typeof s.description === 'string'
                                      ? s.description
                                      : s.title
                                  }
                                >
                                  {s.description}
                                </span>
                              ) : null}
                            </span>
                            <div
                              className="managed-list-choice-select"
                              onClick={(e) => e.stopPropagation()}
                              onKeyDown={(e) => e.stopPropagation()}
                            >
                              <FieldSelect
                                size="sm"
                                value={s.value}
                                disabled={s.disabled || s.loading}
                                aria-label={
                                  typeof s.label === 'string'
                                    ? s.label
                                    : undefined
                                }
                                options={s.options.map((o) => ({
                                  value: o.value,
                                  label: o.label,
                                }))}
                                onChange={s.onChange}
                              />
                            </div>
                          </span>
                        </div>
                      )

                    if (!s.guidePath) {
                      return <React.Fragment key={s.key}>{card}</React.Fragment>
                    }
                    return (
                      <span
                        key={s.key}
                        id={`cfg-g-${s.guidePath.replace(/\./g, '-')}`}
                        data-guide-path={s.guidePath}
                        className="has-guide-anchor managed-list-guide-anchor"
                      >
                        {card}
                      </span>
                    )
                  })}
                {showQueryChip && (
                  <ChromeCard
                    label={
                      queryOpen ? queryCollapseLabel : queryToggleLabel
                    }
                    description={
                      queryOpen
                        ? queryCollapseDescription
                        : queryToggleDescription
                    }
                    icon={
                      queryOpen
                        ? (queryCollapseIcon ?? queryToggleIcon)
                        : queryToggleIcon
                    }
                    checked={queryOpen || queryActive}
                    onPress={() => setQueryOpen(!queryOpen)}
                    className="managed-list-query-btn"
                    aria-expanded={queryOpen}
                    title={
                      queryOpen
                        ? typeof queryCollapseDescription === 'string'
                          ? queryCollapseDescription
                          : undefined
                        : typeof queryToggleDescription === 'string'
                          ? queryToggleDescription
                          : queryActive &&
                              typeof queryToggleLabel === 'string'
                            ? `${queryToggleLabel} · ${t.config.managedListQueryActive}`
                            : undefined
                    }
                  />
                )}
                {hasToolbarActions &&
                  toolbar!.map((a) => (
                    <ListActionButton
                      key={a.key}
                      action={a}
                      size="sm"
                      chrome
                    />
                  ))}
                {showFormChip && (
                  <ChromeCard
                    label={formOpen ? formCollapseLabel : expandLabel}
                    description={
                      formOpen ? formCollapseDescription : formDescription
                    }
                    icon={
                      formOpen ? (formCollapseIcon ?? formIcon) : formIcon
                    }
                    checked={formOpen}
                    onPress={() => setFormOpen(!formOpen)}
                    className="managed-list-form-btn"
                    aria-expanded={formOpen}
                  />
                )}
              </div>
            )}
          </div>
        </div>
      ) : null}

      {formPlacement === 'before' && (
        <>
          {queryPanel}
          {formPanel}
        </>
      )}
      {listBody}
      {formPlacement === 'after' && (
        <>
          {queryPanel}
          {formPanel}
        </>
      )}

      {resolvedFooter != null && (
        <div className="managed-list-footer">{resolvedFooter}</div>
      )}
    </div>
  )
})

export default ManagedList
