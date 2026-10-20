import type { BlogSchemeSwitch, BlogSchemeTarget, BlogTheme } from '../../types/blog'
import { LuCheck, LuMoon, LuPencil } from '@lib/icons'
import { useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { blogSchemeMarkup, blogSchemeSwitchValid } from '../../services/blogState'
import { SettingsButton } from '../settings'

type Choice = 'html-class' | 'body-class' | 'html-attribute' | 'body-attribute' | 'system' | 'none'

const CHOICES: Choice[] = ['html-class', 'body-class', 'html-attribute', 'body-attribute', 'system', 'none']

interface Draft {
  choice: Choice
  name: string
  dark: string
  light: string
}

function draftOf(value: BlogSchemeSwitch): Draft {
  if (value.kind === 'system' || value.kind === 'none') {
    return { choice: value.kind, name: 'data-theme', dark: 'dark', light: '' }
  }
  return {
    choice: `${value.target}-${value.kind}` as Choice,
    name: value.kind === 'attribute' ? value.name : 'data-theme',
    dark: value.dark,
    light: value.light ?? '',
  }
}

function switchOf(draft: Draft): BlogSchemeSwitch {
  if (draft.choice === 'system' || draft.choice === 'none') return { kind: draft.choice }
  const [target, kind] = draft.choice.split('-') as [BlogSchemeTarget, 'class' | 'attribute']
  const light = draft.light.trim() || null
  const dark = draft.dark.trim()
  return kind === 'class'
    ? { kind, target, dark, light }
    : { kind, target, name: draft.name.trim(), dark, light }
}

/**
 * 主题行里的「深浅色」：安装时认出来的办法，站长确认一下或改掉。博客页不跑主题的脚本，
 * 主站切深浅色时由后端照这里替主题切。
 */
export function BlogThemeScheme({
  theme,
  disabled,
  onSave,
}: {
  theme: BlogTheme
  disabled: boolean
  /** 存好了返回真；失败时外面已经报过错，编辑框留着。 */
  onSave: (value: BlogSchemeSwitch) => Promise<boolean>
}) {
  const { t, format } = useI18n()
  const copy = t.studio
  const scheme = theme.scheme
  const [editing, setEditing] = useState(false)
  const [draft, setDraft] = useState<Draft>(() => draftOf(scheme?.switch ?? { kind: 'none' }))
  const [saving, setSaving] = useState(false)
  if (!scheme || theme.builtin) return null

  const save = async (value: BlogSchemeSwitch) => {
    setSaving(true)
    try {
      if (await onSave(value)) setEditing(false)
    } finally {
      setSaving(false)
    }
  }

  const choiceLabel: Record<Choice, string> = {
    'html-class': copy.themeSchemeHtmlClass,
    'body-class': copy.themeSchemeBodyClass,
    'html-attribute': copy.themeSchemeHtmlAttribute,
    'body-attribute': copy.themeSchemeBodyAttribute,
    system: copy.themeSchemeSystem,
    none: copy.themeSchemeNone,
  }
  const current = scheme.switch
  const darkMarkup = blogSchemeMarkup(current, true)
  const lightMarkup = blogSchemeMarkup(current, false)
  const next = switchOf(draft)
  const valid = blogSchemeSwitchValid(next)
  const needsName = draft.choice.endsWith('attribute')
  const needsTokens = needsName || draft.choice.endsWith('class')

  return (
    <div className={`blog-scheme${scheme.confirmed ? '' : ' is-pending'}`}>
      <div className="blog-scheme__line">
        <LuMoon className="blog-scheme__icon" aria-hidden />
        <span className="blog-scheme__label">{copy.themeScheme}</span>
        <span className="blog-scheme__value">
          {darkMarkup ? (
            <>
              <code>{darkMarkup}</code>
              <span className="blog-scheme__sep">/</span>
              <code>{lightMarkup}</code>
            </>
          ) : (
            choiceLabel[current.kind === 'system' ? 'system' : 'none']
          )}
        </span>
        {scheme.confirmed ? null : <span className="blog-tag blog-tag--pending">{copy.themeSchemePending}</span>}
        {editing ? null : (
          <span className="blog-scheme__actions">
            {scheme.confirmed ? null : (
              <SettingsButton
                size="sm"
                variant="secondary"
                icon={<LuCheck />}
                loading={saving}
                disabled={disabled || saving}
                onClick={() => { void save(current) }}
              >
                {copy.themeSchemeConfirm}
              </SettingsButton>
            )}
            <SettingsButton
              size="sm"
              variant="ghost"
              icon={<LuPencil />}
              disabled={disabled || saving}
              onClick={() => {
                setDraft(draftOf(current))
                setEditing(true)
              }}
            >
              {copy.themeSchemeEdit}
            </SettingsButton>
          </span>
        )}
      </div>
      {!editing && !scheme.confirmed ? (
        <p className="blog-scheme__hint">
          {copy.themeSchemeHint}
          {scheme.evidence.length > 0
            ? ` ${format(copy.themeSchemeEvidence, { files: scheme.evidence.join('、') })}`
            : ''}
        </p>
      ) : null}
      {editing ? (
        <div className="blog-scheme__editor">
          <label className="blog-scheme__field">
            <span>{copy.themeSchemeHow}</span>
            <select
              value={draft.choice}
              onChange={(event) => setDraft({ ...draft, choice: event.target.value as Choice })}
            >
              {CHOICES.map((choice) => (
                <option key={choice} value={choice}>{choiceLabel[choice]}</option>
              ))}
            </select>
          </label>
          {needsName ? (
            <label className="blog-scheme__field">
              <span>{copy.themeSchemeAttribute}</span>
              <input
                value={draft.name}
                spellCheck={false}
                onChange={(event) => setDraft({ ...draft, name: event.target.value })}
              />
            </label>
          ) : null}
          {needsTokens ? (
            <>
              <label className="blog-scheme__field">
                <span>{copy.themeSchemeDark}</span>
                <input
                  value={draft.dark}
                  spellCheck={false}
                  onChange={(event) => setDraft({ ...draft, dark: event.target.value })}
                />
              </label>
              <label className="blog-scheme__field">
                <span>{copy.themeSchemeLight}</span>
                <input
                  value={draft.light}
                  spellCheck={false}
                  placeholder={copy.themeSchemeLightEmpty}
                  onChange={(event) => setDraft({ ...draft, light: event.target.value })}
                />
              </label>
            </>
          ) : null}
          {valid ? null : <p className="blog-scheme__error">{copy.themeSchemeInvalid}</p>}
          <div className="blog-scheme__buttons">
            <SettingsButton size="sm" variant="ghost" disabled={saving} onClick={() => setEditing(false)}>
              {copy.themeSchemeCancel}
            </SettingsButton>
            <SettingsButton
              size="sm"
              variant="primary"
              loading={saving}
              disabled={disabled || saving || !valid}
              onClick={() => { void save(next) }}
            >
              {copy.save}
            </SettingsButton>
          </div>
        </div>
      ) : null}
    </div>
  )
}
