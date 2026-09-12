import type {
  CompareResult,
  makeUpdaterApi, ReleaseListItem,
} from '../../../services/updaterApi'
import type { ChannelOption, U } from './helpers'
import React, { useEffect, useRef, useState } from 'react'
import { useI18n } from '../../../contexts/I18nContext'
import { InputItem, SettingsButton, SettingTitleTag } from '../../settings'
import { Spinner } from '../../Spinner'
import {

  format,
  isReleaseTag,

} from './helpers'

interface PickerItem {
  key: string
  tag: string
  label: string
  message: string
  date: string | null
  kind: 'commit' | 'release'
  title?: string
}

export function TargetPicker({
  api,
  option,
  disabled,
  installing,
  u,
  onInstall,
}: {
  api: ReturnType<typeof makeUpdaterApi>
  option: ChannelOption
  disabled: boolean
  installing: boolean
  u: U
  onInstall: (
    target: string,
    opts: { isDowngrade: boolean; needsRisk: boolean },
  ) => void
}) {
  const { locale } = useI18n()
  const [items, setItems] = useState<PickerItem[]>([])
  const [selected, setSelected] = useState('')
  const [input, setInput] = useState('')
  const [compare, setCompare] = useState<CompareResult | null>(null)
  const [listLoading, setListLoading] = useState(true)
  const [targetSource, setTargetSource] = useState<'github' | 'dockerhub'>(
    'github',
  )
  const compareTimerRef = useRef<number | null>(null)

  const isCommit = option.mode === 'commit'
  const target = (isCommit && input.trim()) || selected

  useEffect(() => {
    let cancelled = false
    setListLoading(true)
    setSelected('')
    setInput('')
    setItems([])
    setTargetSource('github')

    const load = async () => {
      if (!isCommit) {
        const response = await api.releases({
          channel: option.channel,
          limit: 25,
        })
        if (cancelled) return
        setItems(
          (response.items ?? []).map((r: ReleaseListItem) => ({
            key: r.tag_name,
            tag: r.tag_name,
            label: r.tag_name,
            message: `${r.name || r.tag_name}${r.prerelease ? u.updaterPrereleaseSuffix : ''}`,
            date: null,
            kind: 'release' as const,
          })),
        )
        return
      }

      try {
        const builds = await api.builds({ limit: 25 })
        const buildItems = builds.items ?? []
        if (buildItems.length > 0) {
          if (cancelled) return
          setTargetSource('dockerhub')
          setItems(
            buildItems.map((build) => {
              const kind: 'commit' | 'release' =
                build.kind === 'release' || isReleaseTag(build.tag)
                  ? 'release'
                  : 'commit'
              return {
                key: build.tag,
                tag: build.tag,
                label: kind === 'release' ? build.tag : build.short_sha,
                message:
                  kind === 'release' ? build.tag : u.updaterDockerHubBuild,
                date: build.pushed_at,
                kind,
                title: build.tag,
              }
            }),
          )
          return
        }
      } catch {
      }

      const next: PickerItem[] = []
      try {
        const rel = await api.releases({ channel: 'preview', limit: 15 })
        for (const r of rel.items ?? []) {
          next.push({
            key: `rel-${r.tag_name}`,
            tag: r.tag_name,
            label: r.tag_name,
            message: `${r.name || r.tag_name}${r.prerelease ? u.updaterPrereleaseSuffix : ''}`,
            date: null,
            kind: 'release',
          })
        }
      } catch {
      }
      try {
        const response = await api.commits({
          branch: option.channel,
          limit: 25,
        })
        for (const c of response.items ?? []) {
          next.push({
            key: c.sha,
            tag: c.tag,
            label: c.short_sha,
            message: c.message,
            date: c.committed_at,
            kind: 'commit',
            title: c.sha,
          })
        }
      } catch {
      }
      if (!cancelled) {
        setTargetSource('github')
        setItems(
          next.toSorted((a, b) => {
            const da = a.date ? Date.parse(a.date) : Number.NaN
            const db = b.date ? Date.parse(b.date) : Number.NaN
            const aOk = !Number.isNaN(da)
            const bOk = !Number.isNaN(db)
            if (aOk && bOk) return db - da
            if (a.kind === 'release' && b.kind !== 'release') return -1
            if (b.kind === 'release' && a.kind !== 'release') return 1
            if (aOk && !bOk) return -1
            if (!aOk && bOk) return 1
            return 0
          }),
        )
      }
    }

    load()
      .catch(() => {
        if (!cancelled) setItems([])
      })
      .finally(() => {
        if (!cancelled) setListLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [api, option, isCommit, u.updaterDockerHubBuild])

  useEffect(() => {
    if (compareTimerRef.current) {
      window.clearTimeout(compareTimerRef.current)
      compareTimerRef.current = null
    }
    if (!target || target.length < 3) {
      setCompare(null)
      return
    }
    compareTimerRef.current = window.setTimeout(() => {
      api
        .compare(target)
        .then(setCompare)
        .catch(() => setCompare(null))
    }, 450)
    return () => {
      if (compareTimerRef.current) window.clearTimeout(compareTimerRef.current)
    }
  }, [api, target])

  const compareTone = compare?.is_downgrade
    ? 'downgrade'
    : compare?.is_upgrade
      ? 'upgrade'
      : 'neutral'
  let compareText: string | null = null
  if (compare) {
    if (compare.is_upgrade) {
      compareText = format(u.updaterFreshnessAhead, {
        n: String(compare.ahead_by),
      })
    } else if (compare.is_downgrade) {
      compareText = format(u.updaterFreshnessBehind, {
        n: String(compare.behind_by),
      })
    } else if (compare.relation === 'identical') {
      compareText = u.updaterFreshnessIdentical
    } else if (compare.relation === 'diverged') {
      compareText = format(u.updaterFreshnessDiverged, {
        ahead: String(compare.ahead_by),
        behind: String(compare.behind_by),
      })
    } else {
      compareText = u.updaterFreshnessUnknown
    }
  }

  return (
    <div className="updater-target">
      <div className="updater-commit-list">
        <div className="updater-commit-list-head">
          {isCommit
            ? targetSource === 'dockerhub'
              ? u.updaterTargetDockerHubHead
              : u.updaterTargetCommitHead
            : u.updaterTargetReleaseHead}
          {' · '}
          <code>{option.channel}</code>
          {isCommit && targetSource === 'dockerhub' && (
            <SettingTitleTag
              variant="muted"
              detail={u.updaterDockerHubFallback}
            >
              {u.updaterDockerHubSourceTag}
            </SettingTitleTag>
          )}
        </div>
        {listLoading ? (
          <div className="updater-empty flex justify-center py-6" role="status">
            <Spinner size="sm" color="primary" />
          </div>
        ) : items.length === 0 ? (
          <p className="updater-empty">{u.updaterTargetEmpty}</p>
        ) : (
          <ul>
            {items.map((item) => (
              <li key={item.key}>
                <button
                  type="button"
                  className={
                    selected === item.tag
                      ? 'updater-commit-item selected'
                      : 'updater-commit-item'
                  }
                  disabled={disabled}
                  title={item.title}
                  onClick={() => {
                    setSelected(item.tag)
                    setInput('')
                  }}
                >
                  <code>{item.label}</code>
                  <span className="updater-commit-msg">
                    {item.kind === 'release' && isCommit ? 'release · ' : ''}
                    {item.message}
                  </span>
                  {item.date && (
                    <span className="updater-commit-date">
                      {new Date(item.date).toLocaleString(locale)}
                    </span>
                  )}
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>

      {isCommit && (
        <InputItem
          itemKey="updater-commit-target"
          label={u.updaterCommitTarget}
          value={input}
          disabled={disabled}
          placeholder={u.updaterCommitPlaceholder}
          inputType="text"
          autoComplete="off"
          layout="vertical"
          size="sm"
          className="updater-commit-input-item"
          onChange={(value) => {
            setInput(value)
            if (value.trim()) setSelected('')
          }}
        />
      )}

      {compareText && target && (
        <div className={`updater-compare-preview ${compareTone}`}>
          {compareText}
        </div>
      )}

      <div className="updater-target-actions">
        <SettingsButton
          variant="secondary"
          disabled={disabled || installing || !target}
          loading={installing}
          onClick={() => {
            if (!target) return
            const isDowngrade = compare?.is_downgrade === true
            const isUpgrade = compare?.is_upgrade === true
            // clear upgrades skip the risk dialog
            const needsRisk =
              !compare ||
              compare.relation === 'diverged' ||
              (compare.relation === 'unknown' && !isUpgrade)
            onInstall(target, { isDowngrade, needsRisk })
          }}
        >
          {format(u.updaterInstallTarget, { version: target || '…' })}
        </SettingsButton>
      </div>
    </div>
  )
}
