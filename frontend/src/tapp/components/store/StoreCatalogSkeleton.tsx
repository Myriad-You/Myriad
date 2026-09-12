import { useI18n } from '../../../contexts/I18nContext'
import {
  CATALOG_SKELETON_DISCOVER_ALL_ROWS,
  CATALOG_SKELETON_LIST_ROWS,
  DISCOVER_LATEST_LIMIT,
} from './types'

function SkeletonRow({ index }: { index: number }) {
  return (
    <div
      className="as-store__skel-row"
      data-skel-i={index}
      aria-hidden
    >
      <span className="as-store__skel-bone as-store__skel-icon" />
      <div className="as-store__skel-body">
        <span className="as-store__skel-bone as-store__skel-name" />
        <span className="as-store__skel-bone as-store__skel-sub" />
      </div>
      <span className="as-store__skel-bone as-store__skel-cta" />
    </div>
  )
}

function SkeletonHeading() {
  return <span className="as-store__skel-bone as-store__skel-heading" />
}

function SkeletonList({ count, prefix }: { count: number; prefix: string }) {
  return (
    <div className="as-store__list">
      {Array.from({ length: count }, (_, index) => (
        <SkeletonRow key={`${prefix}-${index}`} index={index} />
      ))}
    </div>
  )
}

export function StoreCatalogSkeleton({
  variant,
}: {
  variant: 'discover' | 'list'
}) {
  const { t } = useI18n()

  return (
    <div
      className="as-store__skeleton"
      data-variant={variant}
      role="status"
      aria-busy="true"
      aria-label={t.tapp.loadingRemoteApps}
    >
      {variant === 'discover' ? (
        <>
          <section className="as-store__section as-store__section--featured">
            <div className="as-store__section-head">
              <SkeletonHeading />
            </div>
            <div className="as-store__featured">
              <div className="as-store__skel-feature" data-feature-index={0} />
              <div className="as-store__skel-feature" data-feature-index={1} />
            </div>
          </section>
          <section className="as-store__section as-store__section--latest">
            <div className="as-store__section-head">
              <SkeletonHeading />
            </div>
            <SkeletonList count={DISCOVER_LATEST_LIMIT} prefix="latest" />
          </section>
          <section className="as-store__section">
            <div className="as-store__section-head">
              <SkeletonHeading />
            </div>
            <SkeletonList
              count={CATALOG_SKELETON_DISCOVER_ALL_ROWS}
              prefix="all"
            />
          </section>
        </>
      ) : (
        <section className="as-store__section">
          <SkeletonList count={CATALOG_SKELETON_LIST_ROWS} prefix="list" />
        </section>
      )}
    </div>
  )
}
