import { FaGithub, FaStar } from '@lib/icons'
import React, { useEffect, useState } from 'react'
import {
  fetchGithubStarCount,
  formatStarCount,
  githubRepoUrl,
  parseGithubRepoUrl,
  readStarCache,
} from './githubProject'
import './GitHubProjectBadge.css'

export interface GitHubProjectBadgeProps {
  url: string
  name?: string
  className?: string
}

export const GitHubProjectBadge: React.FC<GitHubProjectBadgeProps> = ({
  url,
  name,
  className = '',
}) => {
  const ref = parseGithubRepoUrl(url)
  const owner = ref?.owner ?? null
  const repo = ref?.repo ?? null
  const [stars, setStars] = useState<number | null>(() =>
    owner && repo ? readStarCache(owner, repo) : null,
  )

  useEffect(() => {
    if (!owner || !repo) return undefined
    let cancelled = false
    const cached = readStarCache(owner, repo)
    if (cached != null) {
      setStars(cached)
      return undefined
    }
    void fetchGithubStarCount(owner, repo).then((count) => {
      if (!cancelled) setStars(count)
    })
    return () => {
      cancelled = true
    }
  }, [owner, repo])

  if (!ref || !owner || !repo) return null

  const label = name?.trim() || ref.repo
  const href = githubRepoUrl(ref)
  const starText = stars != null ? formatStarCount(stars) : null
  const classes = ['github-project-badge', className].filter(Boolean).join(' ')

  return (
    <a
      className={classes}
      href={href}
      target="_blank"
      rel="noopener noreferrer"
      title={`${label} · GitHub`}
      aria-label={starText ? `${label}, ${starText} stars` : label}
    >
      <span className="github-project-badge-mark" aria-hidden>
        <FaGithub />
      </span>
      <span className="github-project-badge-name">{label}</span>
      {starText ? (
        <span className="github-project-badge-stars">
          <FaStar aria-hidden />
          <span>{starText}</span>
        </span>
      ) : null}
    </a>
  )
}

GitHubProjectBadge.displayName = 'GitHubProjectBadge'
