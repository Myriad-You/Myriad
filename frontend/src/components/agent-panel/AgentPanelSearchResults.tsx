import type {
  SiteSearchLibraryItem,
  SiteSearchPost,
  SiteSearchResults,
} from '../../services/siteSearchApi'
import React from 'react'
import { Link } from 'react-router-dom'
import { useI18n } from '../../contexts/I18nContext'
import { siteMediaUrl } from '../../utils/siteMediaUrl'

/** Where a result goes. Library entries have no page of their own: the library. */
export const LIBRARY_PATH = '/library'

/** The first result, which Enter opens when the field cannot be sent. */
export function firstSiteResult(results: SiteSearchResults): string | null {
  return results.posts[0]?.href ?? (results.library.length ? LIBRARY_PATH : null)
}

function PostRow({ post }: { post: SiteSearchPost }) {
  return <li>
    {/* The blog is served apart from the app, so this is a full page load. */}
    <a className="agent-panel-search-row" href={post.href}>
      <span className="agent-panel-search-title">{post.title}</span>
      {post.excerpt ? (
        <span className="agent-panel-search-excerpt">{post.excerpt}</span>
      ) : null}
    </a>
  </li>
}

function LibraryRow({ item }: { item: SiteSearchLibraryItem }) {
  return <li>
    <Link className="agent-panel-search-row" data-kind="library" to={LIBRARY_PATH}>
      {item.cover ? (
        <img
          className="agent-panel-search-cover"
          src={siteMediaUrl(item.cover)}
          alt=""
          loading="lazy"
          referrerPolicy="no-referrer"
        />
      ) : null}
      <span className="agent-panel-search-title">{item.title}</span>
      <span className="agent-panel-search-excerpt">{item.platform}</span>
    </Link>
  </li>
}

/** What the site has for the text in the field, above it. */
export const AgentPanelSearchResults: React.FC<{ results: SiteSearchResults }> = ({
  results,
}) => {
  const { t } = useI18n()
  const copy = t.agentPanel.search
  return (
    <nav className="agent-panel-search glass" aria-label={copy.label}>
      {results.posts.length ? (
        <section>
          <h3 className="agent-panel-search-heading">{copy.posts}</h3>
          <ul>
            {results.posts.map((post) => (
              <PostRow key={post.href} post={post} />
            ))}
          </ul>
        </section>
      ) : null}
      {results.library.length ? (
        <section>
          <h3 className="agent-panel-search-heading">{copy.library}</h3>
          <ul>
            {results.library.map((item) => (
              <LibraryRow key={`${item.platform}:${item.item_type}:${item.title}`} item={item} />
            ))}
          </ul>
        </section>
      ) : null}
    </nav>
  )
}
