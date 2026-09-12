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

export interface ManagedListStatMetric {
  key: string
  label: string
  value: number | string
  tone?: ManagedListTone
  kind?: 'metric'
}

export interface ManagedListStatSwitch {
  key: string
  label: string
  kind: 'switch'
  checked: boolean
  onChange: (checked: boolean) => void
  disabled?: boolean
  loading?: boolean
  description?: string
  icon?: ReactNode
  title?: string
  guide?: ReactNode
  guidePath?: string
}

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
  className?: string
}

export type ManagedListStat =
  | ManagedListStatMetric
  | ManagedListStatSwitch
  | ManagedListStatChoice

export interface ManagedListAction {
  key: string
  label: string
  description?: string
  icon?: ReactNode
  onClick: () => void
  disabled?: boolean
  loading?: boolean
  variant?: ManagedListButtonVariant
  confirm?: string
  ariaLabel?: string
  title?: string
}

export interface ManagedListFilterOption {
  key: string
  label: string
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
  badges?: Array<{ label: string; tone?: ManagedListTone }>
  leading?: ReactNode
  trailing?: ReactNode
  actions?: ManagedListAction[]
  expandContent?: ReactNode
  expanded?: boolean
  onToggleExpand?: () => void
  renderHit?: (parts: { leading: ReactNode; main: ReactNode }) => ReactNode
  busy?: boolean
  className?: string
}

export interface ManagedListProps {
  stats?: ManagedListStat[]
  toolbar?: ManagedListAction[]
  search?: ManagedListSearch
  filters?: ManagedListFilters
  filterGroups?: ManagedListFilters[]
  queryToggleLabel?: ReactNode
  queryToggleDescription?: ReactNode
  queryToggleIcon?: ReactNode
  queryCollapseLabel?: ReactNode
  queryCollapseDescription?: ReactNode
  queryCollapseIcon?: ReactNode
  /** ignored when `queryOpen` is set */
  queryDefaultOpen?: boolean
  queryOpen?: boolean
  onQueryOpenChange?: (open: boolean) => void
  queryNeutralFilter?: string
  queryChrome?: 'panel' | 'plain'
  queryCollapsible?: boolean
  form?: ReactNode
  formPlacement?: 'before' | 'after'
  formTitle?: ReactNode
  formDescription?: ReactNode
  formIcon?: ReactNode
  formOpenTitle?: ReactNode
  formCollapseLabel?: ReactNode
  formCollapseDescription?: ReactNode
  formCollapseIcon?: ReactNode
  /** ignored when `formOpen` is set */
  formDefaultOpen?: boolean
  formOpen?: boolean
  onFormOpenChange?: (open: boolean) => void
  items: ManagedListItem[]
  emptyText: string
  loading?: boolean
  working?: boolean
  maxHeight?: string | number | null
  maxVisibleItems?: number | null
  truncateFooter?: (shown: number, total: number) => ReactNode
  className?: string
  footer?: ReactNode
}

function toneClass(tone: ManagedListTone | undefined, prefix: string): string {
  return `${prefix} ${prefix}--${tone ?? 'default'}`
}

const ListActionButton = React.memo(({
  action,
  size = 'md',
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
  const { t, format } = useI18n()
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

  const pageSize = useMemo(() => {
    if (maxVisibleItems === undefined) return constrain ? 80 : null
    return maxVisibleItems
  }, [maxVisibleItems, constrain])

  const totalCount = items.length
  const [visiblePages, setVisiblePages] = useState(1)

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
        : format(t.config.managedListShowing, {
            shown: visibleItems.length,
            total: totalCount,
          })
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
                  {/* hit: leading+main; side actions stopPropagation */}
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
      {(stats && stats.length > 0) || hasChromeBar ? (
        <div className="managed-list-top">
          <div
            className="managed-list-stats"
            role="group"
            aria-label={t.config.managedListStatsAria}
          >
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

            {(stats?.some((s) => s.kind === 'switch' || s.kind === 'choice') ||
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
                        id={`cfg-g-${s.guidePath.replaceAll('.', '-')}`}
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
