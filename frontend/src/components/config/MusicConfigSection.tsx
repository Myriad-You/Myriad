/**
 * 音乐播放器配置区块
 * 使用通用设置组件重构
 */

import { FaHeadphones, FaTrash, SiNeteasecloudmusic } from '@lib/icons'
import React, { useCallback } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { clearPlaylistCache } from '../../utils/musicPlayer'
import {
  ButtonItem,
  InfoCard,
  InputItem,
  ProviderItem,
  SettingGroup,
  SettingSection,
  SwitchItem,
} from '../settings'

interface ConfigField {
  key: string
  value: string
}

interface MusicConfigSectionProps {
  /** UI 配置字段数组 */
  configFields: ConfigField[]
  /** 更新配置字段值 */
  updateValue: (key: string, value: string) => void
  /** 消息回调 */
  onMessage?: (message: string) => void
  title: string
  icon: React.ReactNode
  description: string
}

export const MusicConfigSection: React.FC<MusicConfigSectionProps> = ({
  configFields,
  updateValue,
  onMessage,
  title,
  icon,
  description,
}) => {
  const { t } = useI18n()

  // 辅助函数：获取配置字段值
  const getFieldValue = useCallback((key: string) => {
    return configFields.find(f => f.key === key)?.value || ''
  }, [configFields])

  const handleClearCache = useCallback(() => {
    clearPlaylistCache()
    onMessage?.(t.config.musicCacheCleared)
  }, [onMessage, t])

  const musicEnabled = getFieldValue('music_enabled') === 'true'
  const musicSource = getFieldValue('music_source')
  const playlistId = getFieldValue('music_playlist_id')

  return (
    <SettingSection
      title={title}
      icon={icon}
      description={description}
    >
      {/* 使用说明 */}
      <InfoCard
        title={t.config.musicUsageTitle}
        content={t.config.musicUsageInfo}
      />

      {/* 开关和平台选择 */}
      <SwitchItem
        itemKey="music_enabled"
        label={t.config.enableMusicPlayer}
        description={t.config.musicPlayerDesc}
        value={musicEnabled}
        onChange={v => updateValue('music_enabled', v.toString())}
        layout="horizontal"
      />

      <ProviderItem
        itemKey="music_source"
        label={t.config.musicPlatform}
        value={musicSource}
        onChange={v => updateValue('music_source', v)}
        options={[
          {
            value: 'netease',
            label: t.config.neteaseMusic,
            icon: <SiNeteasecloudmusic />,
          },
          {
            value: 'qq',
            label: t.config.qqMusic,
            icon: <FaHeadphones />,
          },
        ]}
        layout="horizontal"
      />

      {/* 歌单 ID */}
      <InputItem
        itemKey="music_playlist_id"
        label={t.config.playlistId}
        required
        value={playlistId}
        onChange={v => updateValue('music_playlist_id', v)}
        placeholder={
          musicSource === 'netease'
            ? t.config.neteasePlaylistExample
            : t.config.qqPlaylistExample
        }
        hint={
          musicSource === 'netease'
            ? t.config.neteasePlaylistHint
            : t.config.qqPlaylistHint
        }
        layout="vertical"
      />

      {/* 缓存管理 */}
      <SettingGroup title={t.config.cacheManagement}>
        <ButtonItem
          itemKey="clear_cache"
          description={t.config.clearMusicCacheDesc}
          buttonText={t.config.clearMusicCacheBtn}
          buttonIcon={<FaTrash />}
          variant="secondary"
          onClick={handleClearCache}
        />
      </SettingGroup>
    </SettingSection>
  )
}

export default MusicConfigSection
