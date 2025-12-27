import React, { useState, useEffect, useMemo, useCallback } from 'react';
import { useNavigate } from 'react-router-dom';
import { motionShim as motion } from '@lib/motionShim';
import PlatformIcon from './PlatformIcon';
import Toast from './Toast';
import { ButtonSpinner } from './Spinner';
import {
  FaSearch,
  FaTimes,
  FaStar,
  FaGlobe,
  FaDatabase,
  FaCog,
  FaMusic,
  FaLock,
  FaUsers,
  FaWrench,
  FaLink,
  FaCheck,
  FaExclamationTriangle,
  LuSparkles
} from '@lib/icons';
import { SiNeteasecloudmusic } from '@lib/icons';
import {
  fetchConfig,
  updateConfig,
  fetchPermissionsConfig,
  updatePermissionsConfig,
  reloadSystemConfig,
  testPlatformConfig,
  checkSpeechStatus
} from '../lib/api';
import { useDebounce } from '../hooks/useDebounce';
import { getCSRFToken } from '../utils/csrf';
import { clearPlaylistCache } from '../utils/musicPlayer';
import { useI18n } from '../contexts/I18nContext';
import './ConfigForm.css';

// 导入迁移后的配置区块组件
import {
  MusicConfigSection,
  NetworkConfigSection,
  OAuthConfigSection,
  UiConfigSection,
  PermissionsConfigSection,
  AiConfigSection,
  AdvancedConfigSection,
} from './config';

interface ConfigField {
  key: string;
  label: string;
  field_type: string;
  value: string;
  placeholder: string;
  required: boolean;
}

interface PlatformConfig {
  name: string;
  enabled: boolean;
  has_token: boolean;
  config_fields: ConfigField[];
  description: string;
  icon: string;
}

interface AiConfig {
  provider: string;
  model: string;
  api_key: string;
  enabled: boolean;
  // AI 图片生成配置
  image_provider: string;
  config_fields: ConfigField[];
}

interface ReportConfig {
  topic_style: string;
  config_fields: ConfigField[];
}

interface UiConfig {
  wallpaper_url: string;
  wallpaper_blur: number;
  theme: string;
  primary_color: string;
  secondary_color: string;
  config_fields: ConfigField[];
}

interface Config {
  platforms: PlatformConfig[];
  ai_config: AiConfig;
  report_config: ReportConfig;
  ui_config: UiConfig;
}

interface QuickAccessItem {
  id: string;
  label: string;
  icon: React.ReactNode;
  section: string;
  subsection?: string;
}

// 优化：提取为独立的 memo 组件避免不必要的重渲染
interface QuickAccessCardProps {
  item: QuickAccessItem;
  isActive: boolean;
  isFavorite: boolean;
  onCardClick: (section: string) => void;
  onToggleFavorite: (id: string) => void;
}

const QuickAccessCard = React.memo<QuickAccessCardProps>(({
  item,
  isActive,
  isFavorite,
  onCardClick,
  onToggleFavorite
}) => {
  const handleCardClick = React.useCallback(() => {
    onCardClick(item.section);
  }, [onCardClick, item.section]);

  const handleFavoriteClick = React.useCallback((e: React.MouseEvent) => {
    e.stopPropagation();
    onToggleFavorite(item.id);
  }, [onToggleFavorite, item.id]);

  return (
    <div
      onClick={handleCardClick}
      className={`quick-access-card ${isActive ? 'active' : ''}`}
    >
      <span className="card-icon">{item.icon}</span>
      <span className="card-label">{item.label}</span>
      <button
        onClick={handleFavoriteClick}
        className={`favorite-btn ${isFavorite ? 'active' : ''}`}
        aria-label={isFavorite ? 'Remove from favorites' : 'Add to favorites'}
      >
        <FaStar />
      </button>
    </div>
  );
});

QuickAccessCard.displayName = 'QuickAccessCard';

const ModernConfigForm: React.FC = () => {
  const navigate = useNavigate();
  const { t } = useI18n();
  const [config, setConfig] = useState<Config | null>(null);
  const [initialConfig, setInitialConfig] = useState<Config | null>(null);
  const [loading, setLoading] = useState(true);
  const [testing, setTesting] = useState<string | null>(null);
  const [message, setMessage] = useState('');
  const [activeSection, setActiveSection] = useState<string>('platforms');
  const [platformModalOpen, setPlatformModalOpen] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState('');
  const [favorites, setFavorites] = useState<string[]>(() => {
    if (typeof window === 'undefined') return ['platforms', 'ai'];
    const saved = localStorage.getItem('config_favorites');
    return saved ? JSON.parse(saved) : ['platforms', 'ai'];
  });

  // Tapp 权限下放配置状态（9个 elevated 权限 × 2 角色 + AI 限额配置）
  const [permissionConfig, setPermissionConfig] = useState({
    // 普通用户 elevated 权限 (platform:write 和 platform:register 已升为 privileged)
    user_perm_ai_generate: false,
    user_perm_ai_analyze: false,
    user_perm_ai_chat: false,
    user_perm_report_write: false,
    user_perm_network_fetch: false,
    user_perm_media_control: false,
    user_perm_component_theme: false,
    user_perm_shortcut_register: false,
    user_perm_event_publish: false,
    // 游客 elevated 权限
    guest_perm_ai_generate: false,
    guest_perm_ai_analyze: false,
    guest_perm_ai_chat: false,
    guest_perm_report_write: false,
    guest_perm_network_fetch: false,
    guest_perm_media_control: false,
    guest_perm_component_theme: false,
    guest_perm_shortcut_register: false,
    guest_perm_event_publish: false,
    // AI 使用限额配置
    user_ai_daily_calls: 50,
    user_ai_daily_tokens: 20000,
    user_ai_cooldown_seconds: 5,
    guest_ai_daily_calls: 10,
    guest_ai_daily_tokens: 5000,
    guest_ai_cooldown_seconds: 10,
  });
  const [permissionLoading, setPermissionLoading] = useState(false);

  // 更新权限配置
  const updatePermissionConfig = useCallback(async (key: string, value: boolean | number) => {
    const prevValue = permissionConfig[key as keyof typeof permissionConfig];
    setPermissionConfig(prev => ({ ...prev, [key]: value }));

    // 自动保存权限配置
    try {
      // 强制刷新 CSRF Token 确保有效
      await getCSRFToken(true);
      const response = await updatePermissionsConfig({ [key]: value });

      if (response.success) {
        setMessage(t.config.permissionsSaved);
        setTimeout(() => setMessage(''), 2000);
      } else {
        throw new Error(response.message || 'Failed');
      }
    } catch (error) {
      console.error('Failed to save permission:', error);
      setMessage(t.config.permissionsSaveFailed);
      // 回滚
      setPermissionConfig(prev => ({ ...prev, [key]: prevValue }));
    }
  }, [t, permissionConfig]);

  // 加载权限配置
  const loadPermissionConfig = useCallback(async () => {
    try {
      setPermissionLoading(true);
      const response = await fetchPermissionsConfig();

      if (response.success && response.config) {
        const { guest, user, user_ai_quota, guest_ai_quota } = response.config;
        setPermissionConfig({
          // 普通用户权限 (9个 elevated)
          user_perm_ai_generate: user.ai_generate,
          user_perm_ai_analyze: user.ai_analyze,
          user_perm_ai_chat: user.ai_chat,
          user_perm_report_write: user.report_write,
          user_perm_network_fetch: user.network_fetch,
          user_perm_media_control: user.media_control,
          user_perm_component_theme: user.component_theme,
          user_perm_shortcut_register: user.shortcut_register,
          user_perm_event_publish: user.event_publish,
          // 游客权限 (9个 elevated)
          guest_perm_ai_generate: guest.ai_generate,
          guest_perm_ai_analyze: guest.ai_analyze,
          guest_perm_ai_chat: guest.ai_chat,
          guest_perm_report_write: guest.report_write,
          guest_perm_network_fetch: guest.network_fetch,
          guest_perm_media_control: guest.media_control,
          guest_perm_component_theme: guest.component_theme,
          guest_perm_shortcut_register: guest.shortcut_register,
          guest_perm_event_publish: guest.event_publish,
          // AI 使用限额配置
          user_ai_daily_calls: user_ai_quota?.daily_calls ?? 50,
          user_ai_daily_tokens: user_ai_quota?.daily_tokens ?? 20000,
          user_ai_cooldown_seconds: user_ai_quota?.cooldown_seconds ?? 5,
          guest_ai_daily_calls: guest_ai_quota?.daily_calls ?? 10,
          guest_ai_daily_tokens: guest_ai_quota?.daily_tokens ?? 5000,
          guest_ai_cooldown_seconds: guest_ai_quota?.cooldown_seconds ?? 10,
        });
      }
    } catch (error) {
      console.error('Failed to load permissions:', error);
    } finally {
      setPermissionLoading(false);
    }
  }, []);


  const isPlatformConfigured = useCallback((platform: PlatformConfig) => {
    if (!platform.config_fields || platform.config_fields.length === 0) return true;

    return platform.config_fields.every(field => {
      if (!field.required) return true;
      return field.value && String(field.value).trim().length > 0;
    });
  }, []);

  // 获取翻译后的字段标签（覆盖后端返回的标签）
  const getFieldLabel = useCallback((fieldKey: string, originalLabel: string): string => {
    const fieldLabels: Record<string, string> = {
      'wallpaper_url': t.config.fieldWallpaperUrl,
      'wallpaper_blur': t.config.fieldWallpaperBlur,
      'wallpaper_parallax': t.config.fieldWallpaperParallax,
      'pet_enabled': t.config.fieldPetEnabled,
      'pet_image_url': t.config.fieldPetImageUrl,
      'site_title': t.config.fieldSiteTitle,
      'site_description': t.config.fieldSiteDescription,
      'site_favicon': t.config.fieldSiteFavicon,
      'music_enabled': t.config.fieldMusicEnabled,
      'music_source': t.config.fieldMusicSource,
      'music_playlist_id': t.config.fieldMusicPlaylistId,
    };
    return fieldLabels[fieldKey] || originalLabel;
  }, [t]);

  // 获取翻译后的占位符
  const getFieldPlaceholder = useCallback((fieldKey: string, originalPlaceholder: string): string => {
    const placeholders: Record<string, string> = {
      'wallpaper_url': t.config.placeholderWallpaperUrl,
      'site_title': t.config.placeholderSiteTitle,
      'site_description': t.config.placeholderSiteDescription,
      'site_favicon': t.config.placeholderSiteFavicon,
      'pet_image_url': t.config.placeholderPetImageUrl,
    };
    return placeholders[fieldKey] || originalPlaceholder;
  }, [t]);

  // 使用防抖优化搜索性能 - 避免频繁搜索
  const debouncedSearchQuery = useDebounce(searchQuery, 300);

  // 快速访问项（使用 useMemo 避免每次渲染重新创建数组）
  const quickAccessItems: QuickAccessItem[] = useMemo(() => [
    { id: 'platforms', label: t.config.platforms, icon: <FaGlobe />, section: 'platforms' },
    { id: 'data', label: t.config.data, icon: <FaDatabase />, section: 'data' },
    { id: 'ai', label: t.config.ai, icon: <LuSparkles />, section: 'ai' },
    { id: 'ui', label: t.config.basic, icon: <FaCog />, section: 'ui' },
    { id: 'music', label: t.config.music, icon: <FaMusic />, section: 'music' },
    { id: 'oauth', label: t.config.oauth, icon: <FaLock />, section: 'oauth' },
    { id: 'network', label: t.config.network, icon: <FaLink />, section: 'network' },
    { id: 'permissions', label: t.config.permissions, icon: <FaUsers />, section: 'permissions' },
    { id: 'advanced', label: t.config.advanced, icon: <FaWrench />, section: 'advanced' },
  ], [t]);

  // 搜索功能
  const searchableContent = useMemo(() => {
    if (!config) return [];

    const items: Array<{ type: string; section: string; title: string; description: string; keywords: string[] }> = [];

    // 平台配置
    config.platforms.forEach(platform => {
      items.push({
        type: 'platform',
        section: 'platforms',
        title: platform.name,
        description: platform.description,
        keywords: [platform.name.toLowerCase(), '平台', '数据源', 'token', 'api']
      });
    });

    // AI配置
    items.push({
      type: 'section',
      section: 'ai',
      title: t.config.ai,
      description: t.config.aiDesc,
      keywords: ['ai', 'gemini', 'openai', 'api', '模型', '智能', '图片', '生成', 'image']
    });

    // UI配置
    items.push({
      type: 'section',
      section: 'ui',
      title: t.config.basic,
      description: t.config.basicDesc,
      keywords: ['basic', '基础', '站点', '主题', '背景', '样式', 'theme', 'url']
    });

    // OAuth配置
    items.push({
      type: 'section',
      section: 'oauth',
      title: t.config.oauth,
      description: t.config.oauthDesc,
      keywords: ['oauth', 'github', '登录', 'auth', '认证']
    });

    // 音乐播放器
    items.push({
      type: 'section',
      section: 'music',
      title: t.config.music,
      description: t.config.musicDesc,
      keywords: ['音乐', 'music', '歌单', '播放器', '网易云', 'qq音乐']
    });

    // 网络代理
    items.push({
      type: 'section',
      section: 'network',
      title: t.config.network || '网络代理',
      description: t.config.networkDesc || '配置网络代理以访问外部服务',
      keywords: ['proxy', '代理', '网络', 'gemini', 'github', 'api', '镜像', 'mirror', 'socks']
    });

    // 高级配置
    items.push({
      type: 'section',
      section: 'advanced',
      title: t.config.advanced,
      description: t.config.advancedDesc,
      keywords: ['advanced', '高级', 'danger', 'reset', '重置', '危险']
    });

    return items;
  }, [config, t]);

  // 使用防抖后的搜索查询优化性能
  const filteredContent = useMemo(() => {
    if (!debouncedSearchQuery.trim()) return searchableContent;

    const query = debouncedSearchQuery.toLowerCase();
    return searchableContent.filter(item =>
      item.title.toLowerCase().includes(query) ||
      item.description.toLowerCase().includes(query) ||
      item.keywords.some(k => k.includes(query))
    );
  }, [debouncedSearchQuery, searchableContent]);

  // 切换收藏
  const toggleFavorite = React.useCallback((section: string) => {
    setFavorites(prev => {
      const updated = prev.includes(section)
        ? prev.filter(s => s !== section)
        : [...prev, section];
      if (typeof window !== 'undefined') {
        localStorage.setItem('config_favorites', JSON.stringify(updated));
      }
      return updated;
    });
  }, []);

  // 处理节切换
  const handleSectionChange = React.useCallback((section: string) => {
    // 如果是数据管理，直接跳转到专门页面
    if (section === 'data') {
      navigate('/data-management');
      return;
    }
    setActiveSection(section);
    setSearchQuery('');
  }, [navigate]);

  const handleSave = React.useCallback(async () => {
    if (!config) {
      window.dispatchEvent(
        new CustomEvent('config-save-result', {
          detail: { success: false, message: t.config.configEmpty },
        })
      );
      return;
    }

    setMessage(t.config.savingConfig);

    try {
      // 获取 CSRF Token
      await getCSRFToken(true);

      const result = await updateConfig(config);

      setMessage(`✓ ${t.config.configSaved} ${t.config.refreshing}`);
      setInitialConfig(JSON.parse(JSON.stringify(config)));
      notifyDirtyState(false);
      window.dispatchEvent(
        new CustomEvent('config-save-result', {
          detail: { success: true, message: result.message || t.config.configSaved },
        })
      );

      try {
        // reload-config 也需要 CSRF Token
        await getCSRFToken(true);
        await reloadSystemConfig();

        setMessage(`✓ ${t.config.savedSuccess}`);

        // 等待后端完成配置保存和环境变量重新加载，然后刷新页面
        setTimeout(() => {
          window.location.reload();
        }, 2000);
      } catch (restartError) {
        setMessage(`✓ ${t.config.savedSuccess}`);
        // 即使刷新配置失败，仍然刷新页面以应用数据库中的新配置
        setTimeout(() => {
          window.location.reload();
        }, 2000);
      }
    } catch (error) {
      const errorMsg = `✗ ${t.config.configSaveFailed}: ` + (error instanceof Error ? error.message : t.errors.networkError);
      setMessage(errorMsg);
      window.dispatchEvent(
        new CustomEvent('config-save-result', {
          detail: { success: false, message: errorMsg },
        })
      );
    }
  }, [config]);

  const handleReset = React.useCallback(async () => {
    setMessage(t.config.resettingConfig);

    try {
      const data = await fetchConfig();

      const clearedData = {
        ...data,
        platforms: data.platforms.map((platform: any) => ({
          ...platform,
          enabled: false,
          has_token: false,
          config_fields: platform.config_fields.map((field: any) => ({
            ...field,
            value: '',
          })),
        })),
        ai_config: {
          ...data.ai_config,
          enabled: false,
          api_key: '',
          config_fields: data.ai_config.config_fields.map((field: any) => {
            let defaultValue = '';
            if (field.key === 'model') defaultValue = 'gemini-pro';
            else if (field.key === 'ai_image_provider') defaultValue = 'pollinations';
            else if (field.key === 'ai_image_model') defaultValue = 'flux-anime';
            else if (field.key === 'ai_image_width') defaultValue = '512';
            else if (field.key === 'ai_image_height') defaultValue = '768';
            return { ...field, value: defaultValue };
          }),
        },
        ui_config: {
          ...data.ui_config,
          config_fields: data.ui_config.config_fields.map((field: any) => {
            let defaultValue = '';
            if (field.key === 'wallpaper_url') defaultValue = 'https://images.unsplash.com/photo-1579546929518-9e396f3cc809';
            else if (field.key === 'wallpaper_blur') defaultValue = '3';
            return { ...field, value: defaultValue };
          }),
        },
      };

      setConfig(clearedData);
      notifyDirtyState(false);

      await new Promise(resolve => setTimeout(resolve, 200));

      setMessage(t.config.savingDefault);

      // 获取 CSRF Token
      await getCSRFToken(true);

      const saveResult = await updateConfig(clearedData);

      setMessage(`✓ ${t.config.configReset}`);
      setTimeout(() => setMessage(''), 5000);

      window.dispatchEvent(
        new CustomEvent('config-reset-result', {
          detail: { success: true, message: saveResult.message || t.config.configReset },
        })
      );
    } catch (error) {
      const errorMsg = `${t.config.resetFailed}` + (error instanceof Error ? error.message : t.errors.unknown);
      setMessage(errorMsg);
      window.dispatchEvent(
        new CustomEvent('config-reset-result', {
          detail: { success: false, message: errorMsg },
        })
      );
    }
  }, []);

  const notifyDirtyState = React.useCallback((dirty: boolean) => {
    window.dispatchEvent(
      new CustomEvent('config-dirty-state', {
        detail: { dirty },
      })
    );
  }, []);

  const loadConfig = React.useCallback(async () => {
    setLoading(true);
    try {
      const data = await fetchConfig();
      setConfig(data);
      setInitialConfig(JSON.parse(JSON.stringify(data)));
      notifyDirtyState(false);

      const event = new CustomEvent('config-loaded', { detail: data });
      window.dispatchEvent(event);
    } catch (error) {
      setMessage(t.config.loadConfigFailed);
    } finally {
      setLoading(false);
    }
  }, [notifyDirtyState]);

  useEffect(() => {
    loadConfig();
    loadPermissionConfig();
  }, [loadConfig, loadPermissionConfig]);

  useEffect(() => {
    const handleSaveEvent = () => handleSave();
    const handleResetEvent = () => handleReset();

    window.addEventListener('request-config-save', handleSaveEvent);
    window.addEventListener('config-reset', handleResetEvent);

    return () => {
      window.removeEventListener('request-config-save', handleSaveEvent);
      window.removeEventListener('config-reset', handleResetEvent);
    };
  }, [handleSave, handleReset]);

  const handleTest = React.useCallback(async (platformName: string) => {
    const platform = config?.platforms.find(p => p.name === platformName);
    if (!platform) return;

    setTesting(platformName);
    setMessage('');

    try {
      const configObj: any = {};
      platform.config_fields.forEach(field => {
        configObj[field.key] = field.value;
      });

      // 获取 CSRF Token
      await getCSRFToken(true);

      const result = await testPlatformConfig(platformName, configObj);

      setMessage(result.message);
      setTimeout(() => setMessage(''), 5000);
    } catch (error) {
      setMessage(`✗ ${t.config.testFailed}`);
    } finally {
      setTesting(null);
    }
  }, [config]);

  // 测试语音服务可用性（返回 Promise 供组件使用）
  const handleSpeechTest = React.useCallback(async (): Promise<{ success: boolean; message: string }> => {
    if (!config) {
      return { success: false, message: 'Config not loaded' };
    }

    try {
      const result = await checkSpeechStatus();

      return {
        success: result.available === true,
        message: result.available ? t.config.speechTestSuccess : (result.error || t.config.speechTestFailed)
      };
    } catch (error) {
      return {
        success: false,
        message: t.config.speechTestFailed
      };
    }
  }, [config, t]);

  const updateConfigField = React.useCallback((
    section: 'ai' | 'ui',
    fieldKey: string,
    value: string,
    providerFieldKey?: string
  ) => {
    if (!config) return;

    const sectionKey = `${section}_config` as 'ai_config' | 'ui_config';
    const sectionConfig = config[sectionKey];
    const newFields = [...sectionConfig.config_fields];
    const field = newFields.find(f => f.key === fieldKey);

    if (field) {
      // 🔒 安全措施：如果新值包含掩码字符，说明用户在掩码上直接输入，需要清除掩码
      const isMasked = (val: string) => val.includes('••') || val.includes('**') || val === '********';
      if (isMasked(value) && value !== '••••••••' && value !== '********') {
        // 移除所有掩码字符，只保留用户新输入的内容
        field.value = value.replace(/[•*]+/g, '');
      } else {
        field.value = value;
      }

      if (providerFieldKey && fieldKey === providerFieldKey) {
        setConfig({
          ...config,
          [sectionKey]: {
            ...sectionConfig,
            provider: value,
            config_fields: newFields
          }
        });
      } else {
        setConfig({
          ...config,
          [sectionKey]: { ...sectionConfig, config_fields: newFields }
        });
      }
      notifyDirtyState(true);
    }
  }, [config, notifyDirtyState]);

  const updateFieldValue = React.useCallback((platformIndex: number, fieldKey: string, value: string) => {
    if (!config) return;

    const newPlatforms = [...config.platforms];
    const field = newPlatforms[platformIndex].config_fields.find(f => f.key === fieldKey);
    if (field) {
      // 🔒 安全措施：如果新值包含掩码字符，说明用户在掩码上直接输入，需要清除掩码
      // 检测是否在掩码基础上输入（例如 "a••••••••"）
      const isMasked = (val: string) => val.includes('••') || val.includes('**') || val === '********';
      if (isMasked(value) && value !== '••••••••' && value !== '********') {
        // 移除所有掩码字符，只保留用户新输入的内容
        field.value = value.replace(/[•*]+/g, '');
      } else {
        field.value = value;
      }
      setConfig({ ...config, platforms: newPlatforms });
      notifyDirtyState(true);
    }
  }, [config, notifyDirtyState]);

  const updateAiFieldValue = React.useCallback((fieldKey: string, value: string) => {
    updateConfigField('ai', fieldKey, value, 'provider');
  }, [updateConfigField]);

  const updateUiFieldValue = React.useCallback((fieldKey: string, value: string) => {
    updateConfigField('ui', fieldKey, value);
  }, [updateConfigField]);

  const togglePlatform = React.useCallback((platformIndex: number) => {
    if (!config) return;

    const newPlatforms = [...config.platforms];
    newPlatforms[platformIndex].enabled = !newPlatforms[platformIndex].enabled;
    setConfig({ ...config, platforms: newPlatforms });
    notifyDirtyState(true);
  }, [config]);

  const isConfigDirty = useMemo(() => {
    if (!config || !initialConfig) return false;
    return JSON.stringify(config) !== JSON.stringify(initialConfig);
  }, [config, initialConfig]);

  const getSectionProps = (sectionId: string) => {
    const item = quickAccessItems.find(i => i.id === sectionId);
    if (!item) {
      // 理论上不会发生，因为 activeSection 总是有效的
      return { title: '', icon: null, description: '' };
    }
    return {
      title: item.label,
      icon: item.icon,
      description: searchableContent.find(c => c.section === sectionId)?.description || ''
    };
  };

  const renderActiveSection = () => {
    if (!config) return null;

    const props = getSectionProps(activeSection);

    switch (activeSection) {
      case 'platforms':
        return (
          <div className="config-section">
            <div className="section-header">
              <div className="section-header-left">
                <span className="section-icon icon-platforms">{props.icon}</span>
                <div>
                  <h2 className="section-title">{props.title}</h2>
                  <p className="section-description">{props.description}</p>
                </div>
              </div>
            </div>

            <div className="platforms-grid">
              {config.platforms.map((platform, index) => (
                <motion.div
                  key={platform.name}
                  className="platform-card"
                  initial={{ opacity: 0, y: 20 }}
                  animate={{ opacity: 1, y: 0 }}
                  transition={{ delay: 0.2 + index * 0.05, duration: 0.3 }}
                  onClick={() => setPlatformModalOpen(platform.name)}
                  style={{ cursor: 'pointer' }}
                >
                  <div className="platform-header">
                    <div className="platform-info">
                      <div className="platform-icon-wrapper">
                        <PlatformIcon platform={platform.name} className="platform-icon" />
                      </div>
                      <div className="platform-details">
                        <div className="platform-title-row">
                          <h3 className="platform-name">{platform.name}</h3>
                          <span className={`status-badge ${isPlatformConfigured(platform) ? 'configured' : 'unconfigured'}`}>
                            {isPlatformConfigured(platform) ? `✓ ${t.config.configured}` : `⚠ ${t.config.notConfigured}`}
                          </span>
                        </div>
                        <p className="platform-desc">{platform.description}</p>
                      </div>
                    </div>
                    <div className="platform-actions">
                      <label className={`toggle-switch ${isPlatformConfigured(platform) ? '' : 'disabled'}`} onClick={(e) => e.stopPropagation()}>
                        <input
                          type="checkbox"
                          checked={platform.enabled}
                          onChange={() => togglePlatform(index)}
                          aria-label={`Enable ${platform.name}`}
                          disabled={!isPlatformConfigured(platform)}
                        />
                        <span className="toggle-slider"></span>
                      </label>
                    </div>
                  </div>
                </motion.div>
              ))}
            </div>
          </div>
        );
      case 'ai':
        return (
          <AiConfigSection
            configFields={config.ai_config.config_fields}
            updateValue={updateAiFieldValue}
            onSpeechTest={handleSpeechTest}
            {...props}
          />
        );
      case 'ui':
        return (
          <UiConfigSection
            configFields={config.ui_config.config_fields}
            updateValue={updateUiFieldValue}
            getFieldLabel={getFieldLabel}
            getFieldPlaceholder={getFieldPlaceholder}
            {...props}
          />
        );
      case 'oauth':
        return (
          <OAuthConfigSection
            configFields={config.ui_config.config_fields}
            updateValue={updateUiFieldValue}
            {...props}
          />
        );
      case 'music':
        return (
          <MusicConfigSection
            configFields={config.ui_config.config_fields}
            updateValue={updateUiFieldValue}
            onMessage={(msg) => {
              setMessage(msg);
              setTimeout(() => setMessage(''), 3000);
            }}
            {...props}
          />
        );
      case 'network':
        return (
          <NetworkConfigSection
            configFields={config.ui_config.config_fields}
            updateValue={updateUiFieldValue}
            {...props}
          />
        );
      case 'permissions':
        return (
          <PermissionsConfigSection
            permissionConfig={permissionConfig}
            updatePermissionConfig={updatePermissionConfig}
            loading={permissionLoading}
            {...props}
          />
        );
      case 'advanced':
        return (
          <AdvancedConfigSection
            onReset={handleReset}
            {...props}
          />
        );
      default:
        return null;
    }
  };

  if (loading) {
    return null;
  }

  if (!config) {
    return (
      <div className="modern-config-error">
        <FaExclamationTriangle className="error-icon" />
        <p>{t.config.loadConfigFailed}</p>
      </div>
    );
  }

  return (
    <motion.div
      className="modern-config-container"
      initial={{ opacity: 0, y: 20 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: -20 }}
      transition={{ duration: 0.4, ease: [0.34, 1.56, 0.64, 1] }}
    >
      {/* 消息提示 */}
      {message && <Toast message={message} />}

      {/* 配置导航卡片 */}
      <motion.div
        className="config-nav-card"
        initial={{ opacity: 0, y: 10 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ delay: 0.1, duration: 0.3 }}
      >
        <div className="config-nav-header">
          <div className="nav-header-left">
            <span className="nav-icon"><FaWrench /></span>
            <div>
              <h3 className="nav-title">{t.config.title}</h3>
              <p className="nav-subtitle">{t.config.selectProject}</p>
            </div>
          </div>
          <div className="nav-header-actions">
            {/* 操作按钮已移至底部悬浮栏 */}
          </div>
        </div>

        {/* 搜索栏 */}
        <div className="config-search-bar">
          <div className="search-input-wrapper">
            <FaSearch className="search-icon" />
            <input
              type="text"
              placeholder={t.config.searchConfig}
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              className="search-input"
            />
            {searchQuery && (
              <button
                onClick={() => setSearchQuery('')}
                className="search-clear"
                aria-label="Clear search"
              >
                <FaTimes />
              </button>
            )}
          </div>
        </div>

        {/* 搜索结果 */}
        {searchQuery ? (
          <div className="search-results">
            <h4 className="search-results-title">
              {t.config.searchResults} ({filteredContent.length})
            </h4>
            <div className="search-results-list">
              {filteredContent.length > 0 ? (
                filteredContent.map((item, index) => (
                  <button
                    key={index}
                    onClick={() => {
                      handleSectionChange(item.section);
                    }}
                    className="search-result-item"
                  >
                    <div className="search-result-content">
                      <h4>{item.title}</h4>
                      <p>{item.description}</p>
                    </div>
                    <span className="search-result-arrow">→</span>
                  </button>
                ))
              ) : (
                <div className="search-no-results">
                  <p>{t.config.noMatchingConfig}</p>
                </div>
              )}
            </div>
          </div>
        ) : (
          <div className="config-nav-content">
            {/* 收藏夹 */}
            {favorites.length > 0 && (
              <div className="nav-section">
                <div className="nav-section-header">
                  <FaStar className="nav-section-icon" />
                  <span className="nav-section-title">{t.config.favorites}</span>
                </div>
                <div className="quick-access-grid">
                  {favorites.map(fav => {
                    const item = quickAccessItems.find(i => i.id === fav);
                    return item ? (
                      <QuickAccessCard
                        key={item.id}
                        item={item}
                        isActive={activeSection === item.section}
                        isFavorite={true}
                        onCardClick={handleSectionChange}
                        onToggleFavorite={toggleFavorite}
                      />
                    ) : null;
                  })}
                </div>
              </div>
            )}

            {/* 所有配置 */}
            <div className="nav-section">
              <div className="nav-section-header">
                <span className="nav-section-title">{t.config.allConfig}</span>
              </div>
              <div className="quick-access-grid">
                {quickAccessItems.map(item => (
                  <QuickAccessCard
                    key={item.id}
                    item={item}
                    isActive={activeSection === item.section}
                    isFavorite={favorites.includes(item.id)}
                    onCardClick={handleSectionChange}
                    onToggleFavorite={toggleFavorite}
                  />
                ))}
              </div>
            </div>
          </div>
        )}
      </motion.div>

      {/* 配置内容区域 */}
      {!searchQuery && (
        <div className="config-content">
          {renderActiveSection()}
        </div>
      )}

      {/* 平台配置弹窗 */}
      {platformModalOpen && config && (() => {
        const platformIndex = config.platforms.findIndex(p => p.name === platformModalOpen);
        if (platformIndex === -1) return null;
        const platform = config.platforms[platformIndex];

        return (
          <div className="modal-overlay" onClick={() => setPlatformModalOpen(null)}>
            <div className="modal-content" onClick={(e) => e.stopPropagation()}>
              <div className="modal-header">
                <div className="modal-title-section">
                  <div className="platform-icon-wrapper">
                    <PlatformIcon platform={platform.name} className="platform-icon" />
                  </div>
                  <div>
                    <h3 className="modal-title">{platform.name}</h3>
                    <p className="modal-subtitle">{platform.description}</p>
                  </div>
                </div>
                <button
                  onClick={() => setPlatformModalOpen(null)}
                  className="modal-close-button"
                  aria-label={t.config.closeLabel}
                >
                  <FaTimes />
                </button>
              </div>

              <div className="modal-body">
                {platform.config_fields.map((field) => (
                  <div key={field.key} className="config-field">
                    <label htmlFor={`modal-platform-${platformIndex}-${field.key}`} className="field-label">
                      {field.label}
                      {field.required && <span className="required">*</span>}
                    </label>
                    <input
                      id={`modal-platform-${platformIndex}-${field.key}`}
                      type={field.field_type}
                      value={field.value}
                      onChange={(e) => updateFieldValue(platformIndex, field.key, e.target.value)}
                      onFocus={(e) => {
                        // 🔒 如果是掩码值，自动选中全部内容，用户输入会直接替换
                        const isMasked = e.target.value === '••••••••' || e.target.value === '********';
                        if (isMasked) {
                          e.target.select();
                        }
                      }}
                      placeholder={field.placeholder}
                      className="field-input"
                    />
                  </div>
                ))}
              </div>
              <div className="modal-footer">
                <button
                  onClick={() => setPlatformModalOpen(null)}
                  className="btn-base btn-primary"
                >
                  {t.common.confirm}
                </button>
              </div>
            </div>
          </div>
        );
      })()}

      {isConfigDirty && (
        <div className="floating-save-container">
          <button
            onClick={handleSave}
            className="btn-base btn-primary floating-save-btn"
            aria-label={t.config.saveConfigLabel}
          >
            <svg fill="none" stroke="currentColor" viewBox="0 0 24 24" width="20" height="20">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M5 13l4 4L19 7" />
            </svg>
            <span>{t.config.saveConfig}</span>
          </button>
        </div>
      )}
    </motion.div>
  );
};

export default ModernConfigForm;
