import React from 'react'
import { useI18n } from '../../contexts/I18nContext'
import {
  LuExternalLink,
  LuGitFork,
  LuScale,
  LuTag,
  LuUsers,
} from '../../lib/icons'
import { getBuildInfo } from '../../utils/buildInfo'
import { SettingGroup, SettingSection, useSettingGuide } from '../settings'
import { UpdaterInlinePanel } from './UpdaterConfigSection'

interface AboutConfigSectionProps {
  title: string
  icon: React.ReactNode
  description: string
  sectionId?: string
}

const ORG_NAME = 'Myriad-You'
const REPO_NAME = 'Myriad-You/Myriad'
const ORG_URL = 'https://github.com/Myriad-You'
const REPO_URL = 'https://github.com/Myriad-You/Myriad'

export const AboutConfigSection: React.FC<AboutConfigSectionProps> = ({
  title,
  icon,
  description,
  sectionId,
}) => {
  const { t } = useI18n()
  const { catalog: g, bindGuide } = useSettingGuide()
  const buildInfo = getBuildInfo()

  const devInfo: Array<{
    label: string
    value: string
    icon: React.ReactNode
    href?: string
  }> = [
    {
      label: t.config.aboutVersion,
      value: buildInfo.commitSha
        ? `${buildInfo.version} · ${buildInfo.commitSha.slice(0, 7)}`
        : buildInfo.version,
      icon: <LuTag />,
      href: buildInfo.commitUrl ?? undefined,
    },
    { label: t.config.aboutLicense, value: 'GPL-3.0', icon: <LuScale /> },
    {
      label: t.config.aboutOrganization,
      value: ORG_NAME,
      icon: <LuUsers />,
      href: ORG_URL,
    },
    {
      label: t.config.aboutRepository,
      value: REPO_NAME,
      icon: <LuGitFork />,
      href: REPO_URL,
    },
  ]

  return (
    <SettingSection
      showResetPage={false}
      helpToggle={true}
      title={title}
      icon={icon}
      description={description}
      {...bindGuide('about.section', g.about.section)}
      sectionId={sectionId}
    >
      <SettingGroup>
        <div className="about">
          <div className="about-hero">
            <img
              src="/logo.webp"
              alt={t.config.aboutLogoAlt}
              className="about-logo"
            />
            <h3 className="about-name">Myriad</h3>
            <p className="about-tagline">{t.config.aboutTagline}</p>
          </div>

          <div className="about-bento">
            {devInfo.map((card) => {
              const inner = (
                <>
                  <span className="about-card-label">
                    <span className="about-card-icon" aria-hidden="true">
                      {card.icon}
                    </span>
                    {card.label}
                  </span>
                  <span className="about-card-value">{card.value}</span>
                </>
              )
              return card.href ? (
                <a
                  key={card.label}
                  className="about-card is-link"
                  href={card.href}
                  target="_blank"
                  rel="noreferrer noopener"
                >
                  {inner}
                  <span className="about-card-arrow" aria-hidden="true">
                    <LuExternalLink />
                  </span>
                </a>
              ) : (
                <div key={card.label} className="about-card">
                  {inner}
                </div>
              )
            })}
          </div>
        </div>
      </SettingGroup>

      {/* Updater 管理（仅 admin 可见；非 admin 调 /api/admin/updater/* 会 403，UI 自然提示） */}
      <UpdaterInlinePanel heading={t.config.updaterTitle} />
    </SettingSection>
  )
}

export default AboutConfigSection
