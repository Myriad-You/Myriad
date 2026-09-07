/** Catalog list body: loading / error / featured / installed / category lists. */

import type { ReactNode } from 'react'
import type {
  CategorySortOrder,
  InstalledSortOrder,
  InstalledTappInfo,
  StoreSelection,
  UnifiedAppItem,
} from './types'
import { FaExclamationTriangle, FaFilter } from '@lib/icons'
import { useI18n } from '../../../contexts/I18nContext'
import { isStoreCatalogPending } from '../../utils/storeCatalogState'
import { getAppIconStyle } from './storeAppMeta'
import { StoreCatalogSkeleton } from './StoreCatalogSkeleton'
import { FeaturedTappPreview } from './StorePreviews'
import { DISCOVER_ALL_PREVIEW_LIMIT } from './types'

export interface StoreCatalogViewProps {
  loading: boolean
  error: string | null
  remoteEmpty: boolean
  filteredApps: UnifiedAppItem[]
  featuredApps: UnifiedAppItem[]
  /** Discover “最新” (max 2) */
  latestApps: UnifiedAppItem[]
  selectedCategory: StoreSelection
  isDiscoverView: boolean
  sectionTitle: string
  installedSortOrder: InstalledSortOrder
  categorySortOrder: CategorySortOrder
  setInstalledSortOrder: (v: InstalledSortOrder) => void
  setCategorySortOrder: (v: CategorySortOrder) => void
  sortedAvailableUpdates: UnifiedAppItem[]
  installedCurrentApps: UnifiedAppItem[]
  installedTapps: Map<string, InstalledTappInfo>
  onRetry: () => void
  onOpenDetail: (app: UnifiedAppItem) => void
  /** Discover “全部” → full catalog secondary page */
  onSeeAllApps?: () => void
  renderAppCard: (
    app: UnifiedAppItem,
    index: number,
    date?: string,
  ) => ReactNode
}

export function StoreCatalogView({
  loading,
  error,
  remoteEmpty,
  filteredApps,
  featuredApps,
  latestApps,
  selectedCategory,
  isDiscoverView,
  sectionTitle,
  installedSortOrder,
  categorySortOrder,
  setInstalledSortOrder,
  setCategorySortOrder,
  sortedAvailableUpdates,
  installedCurrentApps,
  installedTapps,
  onRetry,
  onOpenDetail,
  onSeeAllApps,
  renderAppCard,
}: StoreCatalogViewProps) {
  const { t } = useI18n()
  const discoverPreviewApps =
    isDiscoverView && !selectedCategory
      ? filteredApps.slice(0, DISCOVER_ALL_PREVIEW_LIMIT)
      : filteredApps
  // Always offer “查看全部” on discover, even when total ≤ preview limit.
  const showSeeAll =
    isDiscoverView && !selectedCategory && Boolean(onSeeAllApps)

  if (isStoreCatalogPending(loading, remoteEmpty, selectedCategory)) {
    return (
      <StoreCatalogSkeleton variant={isDiscoverView ? 'discover' : 'list'} />
    )
  }

  if (error && remoteEmpty && filteredApps.length === 0) {
    return (
      <div className="as-store__state">
        <FaExclamationTriangle className="h-10 w-10 text-amber-500 opacity-80" />
        <p>{error}</p>
        <button type="button" className="as-store__retry" onClick={onRetry}>
          {t.tapp.retry}
        </button>
      </div>
    )
  }

  if (filteredApps.length === 0) {
    return (
      <div className="as-store__state">
        <FaFilter className="h-9 w-9 opacity-40" />
        <p>{t.tapp.noMatchingApps}</p>
        {error && (
          <p className="as-store__state-hint" role="status">
            {error}
          </p>
        )}
      </div>
    )
  }

  return (
    <>
      {error && (
        <div className="as-store__error-banner" role="status">
          <FaExclamationTriangle className="h-4 w-4 shrink-0" />
          <span>{error}</span>
          <button
            type="button"
            className="as-store__retry as-store__retry--inline"
            onClick={onRetry}
          >
            {t.tapp.retry}
          </button>
        </div>
      )}
      {featuredApps.length > 0 && (
        <section className="as-store__section as-store__section--featured">
          <div className="as-store__section-head">
            <h3 className="as-store__section-title">{t.tapp.storeFeatured}</h3>
          </div>
          <div className="as-store__featured">
            {featuredApps.map((app, index) => {
              const style = getAppIconStyle(app)
              return (
                <article
                  key={`feat-${app.id}`}
                  className={`as-store__feature-card ${style.className}`}
                  data-feature-index={index}
                  style={{
                    ...style.style,
                    ['--as-enter-i' as string]: index,
                  }}
                >
                  <FeaturedTappPreview app={app} />
                  <span className="as-store__feature-eyebrow">
                    {t.tapp.storeFeaturedEyebrow}
                  </span>
                  <span className="as-store__feature-name">{app.name}</span>
                  {app.description && (
                    <span className="as-store__feature-sub">
                      {app.description}
                    </span>
                  )}
                  <button
                    type="button"
                    className="as-store__feature-hit"
                    onClick={() => onOpenDetail(app)}
                    aria-label={`${app.name}: ${t.tapp.viewDetails}`}
                  />
                </article>
              )
            })}
          </div>
        </section>
      )}

      {isDiscoverView && latestApps.length > 0 && (
        <section className="as-store__section as-store__section--latest">
          <div className="as-store__section-head">
            <h3 className="as-store__section-title">{t.tapp.storeLatest}</h3>
          </div>
          <div className="as-store__list">
            {latestApps.map((app, index) =>
              renderAppCard(app, index, app.updatedAt),
            )}
          </div>
        </section>
      )}

      {selectedCategory === '__installed__' ? (
        <div className="as-store__installed-sections">
          {sortedAvailableUpdates.length > 0 && (
            <section className="as-store__installed-section">
              <div className="as-store__section-head">
                <h3 className="as-store__section-title">
                  {t.tapp.storeUpdates}
                </h3>
              </div>
              <div className="as-store__list">
                {sortedAvailableUpdates.map((app, index) =>
                  renderAppCard(app, index, app.updatedAt),
                )}
              </div>
            </section>
          )}

          <section className="as-store__installed-section">
            <div className="as-store__section-head">
              <div
                className="as-store__sort-control"
                role="group"
                aria-label={t.tapp.storeSortOrder}
              >
                <button
                  type="button"
                  className="as-store__sort-option"
                  data-active={
                    installedSortOrder === 'category' ? 'true' : 'false'
                  }
                  aria-pressed={installedSortOrder === 'category'}
                  onClick={() => setInstalledSortOrder('category')}
                >
                  {t.tapp.storeSortByCategory}
                </button>
                <button
                  type="button"
                  className="as-store__sort-option"
                  data-active={installedSortOrder === 'date' ? 'true' : 'false'}
                  aria-pressed={installedSortOrder === 'date'}
                  onClick={() => setInstalledSortOrder('date')}
                >
                  {t.tapp.storeSortByDate}
                </button>
              </div>
            </div>
            <div key={installedSortOrder} className="as-store__list">
              {installedCurrentApps.map((app, index) =>
                renderAppCard(
                  app,
                  index,
                  installedTapps.get(app.id)?.installedAt,
                ),
              )}
            </div>
          </section>
        </div>
      ) : (
        <section className="as-store__section">
          <div className="as-store__section-head">
            {selectedCategory ? (
              <div
                className="as-store__sort-control"
                role="group"
                aria-label={t.tapp.storeSortOrder}
              >
                <button
                  type="button"
                  className="as-store__sort-option"
                  data-active={
                    categorySortOrder === 'name' ? 'true' : 'false'
                  }
                  aria-pressed={categorySortOrder === 'name'}
                  onClick={() => setCategorySortOrder('name')}
                >
                  {t.tapp.storeSortByName}
                </button>
                <button
                  type="button"
                  className="as-store__sort-option"
                  data-active={
                    categorySortOrder === 'date' ? 'true' : 'false'
                  }
                  aria-pressed={categorySortOrder === 'date'}
                  onClick={() => setCategorySortOrder('date')}
                >
                  {t.tapp.storeSortByDate}
                </button>
                <button
                  type="button"
                  className="as-store__sort-option"
                  data-active={
                    categorySortOrder === 'downloads' ? 'true' : 'false'
                  }
                  aria-pressed={categorySortOrder === 'downloads'}
                  onClick={() => setCategorySortOrder('downloads')}
                >
                  {t.tapp.storeSortByDownloads}
                </button>
              </div>
            ) : (
              <h3 className="as-store__section-title">
                {isDiscoverView ? t.tapp.allApps : sectionTitle}
              </h3>
            )}
            {showSeeAll && (
              <button
                type="button"
                className="as-store__see-all"
                onClick={onSeeAllApps}
              >
                {t.tapp.seeAllApps}
              </button>
            )}
          </div>
          <div
            key={selectedCategory ? categorySortOrder : 'preview'}
            className="as-store__list"
          >
            {discoverPreviewApps.map((app, index) =>
              renderAppCard(app, index),
            )}
          </div>
        </section>
      )}
    </>
  )
}
