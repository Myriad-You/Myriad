import {
  FaCheck,
  FaDatabase,
  FaExclamationTriangle,
  FaUser,
  LuArrowRight,
  LuClipboardList,
  LuDatabase,
  LuInfo,
  LuServer,
  LuShieldCheck,
  LuSparkles,
  LuWrench,
} from '@lib/icons'
import React, { useCallback, useEffect, useRef, useState } from 'react'
import { API_URL } from '../config'
import { useI18n } from '../contexts/I18nContext'
import { Spinner } from './Spinner'
import './SetupWizard.css'

interface SetupStatus {
  is_setup_required: boolean
  has_database: boolean
  has_admin_user: boolean
  missing_configs: string[]
}

type SetupNotice = {
  tone: 'info' | 'success' | 'error'
  message: string
} | null

async function getResponseError(response: Response, fallback: string) {
  try {
    const body = await response.json()
    return body.message || body.error || fallback
  } catch {
    return fallback
  }
}

const SetupWizard: React.FC = () => {
  const { t } = useI18n()
  const [status, setStatus] = useState<SetupStatus | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [notice, setNotice] = useState<SetupNotice>(null)
  const [hasEnteredSetup, setHasEnteredSetup] = useState(() => {
    return sessionStorage.getItem('myriad-setup-started') === 'true'
  })
  const pollTimerRef = useRef<number | null>(null)

  // 数据库配置
  const [dbConfig, setDbConfig] = useState({
    host: 'localhost',
    port: '5432',
    database: 'myriad',
    username: 'postgres',
    password: '',
  })
  const [savingDb, setSavingDb] = useState(false)
  const [migratingDb, setMigratingDb] = useState(false)
  const [dbConfigured, setDbConfigured] = useState(false)

  // 管理员账户
  const [adminForm, setAdminForm] = useState({
    username: '',
    password: '',
    confirmPassword: '',
  })
  const [creatingAdmin, setCreatingAdmin] = useState(false)
  const [adminCreated, setAdminCreated] = useState(false)

  const checkSetupStatus = useCallback(async () => {
    try {
      setLoading(true)
      setError('')

      // 先检查健康状态,看是否处于配置模式
      const healthResponse = await fetch(`${API_URL}/health`)
      if (!healthResponse.ok) {
        throw new Error('Failed to connect to backend')
      }
      const healthData = await healthResponse.json()

      // 如果处于配置模式(数据库未连接),显示数据库配置界面
      if (
        healthData.mode === 'configuration' ||
        !healthData.database_connected
      ) {
        setStatus({
          is_setup_required: true,
          has_database: false,
          has_admin_user: false,
          missing_configs: ['Database not configured'],
        })
        setDbConfigured(false)
        setAdminCreated(false)
        setLoading(false)
        return
      }

      // 如果数据库已连接,检查详细的设置状态
      const response = await fetch(`${API_URL}/api/setup/status`)
      if (!response.ok) {
        // 如果是 503，说明某些功能还未就绪，但不是连接问题
        if (response.status === 503) {
          // 使用健康检查数据创建基本状态
          setStatus({
            is_setup_required: true,
            has_database: healthData.database_connected,
            has_admin_user: false,
            missing_configs: ['Checking configuration...'],
          })
          // 数据库已连接，应该显示下一步
          setDbConfigured(true)
          setAdminCreated(false)
          setLoading(false)
          return
        }
        throw new Error('Failed to check setup status')
      }
      const data = await response.json()
      setStatus(data)
      // 数据库连接成功就算配置完成，即使表还没初始化
      // 因为用户接下来就要初始化数据库
      setDbConfigured(healthData.database_connected)
      setAdminCreated(data.has_admin_user)
      if (!data.is_setup_required) {
        sessionStorage.removeItem('myriad-setup-started')
      }
    } catch (_err) {
      setError(t.setup.connectionFailedDesc)
    } finally {
      setLoading(false)
    }
  }, [t.setup.connectionFailedDesc])

  useEffect(() => {
    void checkSetupStatus()

    return () => {
      if (pollTimerRef.current !== null) {
        window.clearTimeout(pollTimerRef.current)
      }
    }
  }, [checkSetupStatus])

  const enterSetup = () => {
    sessionStorage.setItem('myriad-setup-started', 'true')
    setHasEnteredSetup(true)
  }

  const handleSaveDbConfig = async () => {
    const port = Number(dbConfig.port)
    if (
      !dbConfig.host.trim() ||
      !dbConfig.database.trim() ||
      !dbConfig.username.trim() ||
      !dbConfig.password
    ) {
      setNotice({ tone: 'error', message: t.setup.dbFieldsRequired })
      return
    }
    if (!Number.isInteger(port) || port < 1 || port > 65535) {
      setNotice({ tone: 'error', message: t.setup.invalidPort })
      return
    }

    setNotice(null)
    setSavingDb(true)

    try {
      // 使用新的数据库配置 API
      const response = await fetch(`${API_URL}/api/setup/database-config`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          host: dbConfig.host,
          port,
          username: dbConfig.username,
          password: dbConfig.password,
          database: dbConfig.database,
        }),
      })

      if (!response.ok) {
        throw new Error(
          await getResponseError(response, t.setup.saveConfigFailed),
        )
      }

      const result = await response.json()

      if (result.restart_triggered || result.reload_triggered) {
        setNotice({
          tone: 'info',
          message: `${t.setup.dbConfigSaved} ${t.setup.dbReconnecting} ${t.setup.waitingForConnection}`,
        })

        // 后端会由受管环境重启；轮询直到新进程以完整路由表启动
        pollDatabaseConnection()
      } else {
        setNotice({
          tone: 'info',
          message: `${t.setup.dbConfigSaved} ${t.setup.restartRequired}`,
        })
        setSavingDb(false)
      }
    } catch (err: any) {
      setNotice({
        tone: 'error',
        message: `${t.setup.saveConfigFailed}: ${err.message}`,
      })
      setSavingDb(false)
    }
  }

  // 轮询检查数据库连接状态
  const pollDatabaseConnection = async () => {
    let attempts = 0
    const maxAttempts = 30 // 最多尝试30次（60秒）
    const pollInterval = 2000 // 每2秒检查一次

    const checkConnection = async () => {
      attempts++

      try {
        const healthResponse = await fetch(`${API_URL}/health`)
        if (healthResponse.ok) {
          const healthData = await healthResponse.json()

          // 检查是否已经连接到数据库（不再是配置模式）
          if (
            healthData.database_connected &&
            healthData.mode !== 'configuration'
          ) {
            setNotice({
              tone: 'success',
              message: `${t.setup.dbConnectionSuccess} ${t.setup.systemSwitchedToNormal}`,
            })
            setSavingDb(false)
            setDbConfigured(true)
            void checkSetupStatus()
            return
          }
        }
      } catch (_err) {
        // 轮询检查失败，继续尝试
      }

      // 如果还没成功且未超过最大尝试次数，继续轮询
      if (attempts < maxAttempts) {
        pollTimerRef.current = window.setTimeout(checkConnection, pollInterval)
      } else {
        setNotice({
          tone: 'error',
          message: `${t.setup.dbConnectionTimeout}: ${t.setup.dbConnectionTimeoutDesc}`,
        })
        setSavingDb(false)
        void checkSetupStatus()
      }
    }

    // 等待3秒后开始第一次检查（给后端一些处理时间）
    pollTimerRef.current = window.setTimeout(checkConnection, 3000)
  }

  const handleMigrateDatabase = async () => {
    setNotice(null)
    setMigratingDb(true)

    try {
      const response = await fetch(`${API_URL}/api/setup/init-database`, {
        method: 'POST',
      })

      if (!response.ok) {
        throw new Error(
          await getResponseError(response, t.setup.dbMigrationFailed),
        )
      }

      const result = await response.json()

      // 显示详细的验证信息
      let message = result.message
      if (result.verification) {
        const v = result.verification
        message += `\n\n${t.setup.verificationResult}:`
        message += `\n• ${t.setup.totalTables}: ${v.total_tables}`
        message += `\n• ${t.setup.usersTable}: ${v.users_table ? t.setup.yes : t.setup.no}`
        message += `\n• ${t.setup.platformsTable}: ${v.platforms_table ? t.setup.yes : t.setup.no}`
        message += `\n• ${t.setup.configurationsTable}: ${v.configurations_table ? t.setup.yes : t.setup.no}`
      }

      setNotice({ tone: 'success', message })

      // 重新检查状态以更新 UI
      await checkSetupStatus()
    } catch (err: any) {
      setNotice({
        tone: 'error',
        message: `${t.setup.dbMigrationFailed}: ${err.message}`,
      })
    } finally {
      setMigratingDb(false)
    }
  }

  const handleCreateAdmin = async () => {
    setNotice(null)
    if (adminForm.username.length < 3 || adminForm.username.length > 20) {
      setNotice({ tone: 'error', message: t.setup.usernameLengthError })
      return
    }

    const usernameRegex = /^\w+$/
    if (!usernameRegex.test(adminForm.username)) {
      setNotice({ tone: 'error', message: t.setup.usernameFormatError })
      return
    }

    if (
      adminForm.password.length < 8 ||
      !/\p{L}/u.test(adminForm.password) ||
      !/\p{N}/u.test(adminForm.password)
    ) {
      setNotice({ tone: 'error', message: t.setup.passwordComplexityError })
      return
    }

    if (adminForm.password !== adminForm.confirmPassword) {
      setNotice({ tone: 'error', message: t.setup.passwordMismatch })
      return
    }

    setCreatingAdmin(true)

    try {
      const response = await fetch(`${API_URL}/api/setup/create-admin`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          username: adminForm.username,
          password: adminForm.password,
        }),
      })

      if (!response.ok) {
        throw new Error(
          await getResponseError(response, t.setup.createAdminFailed),
        )
      }

      setNotice({ tone: 'success', message: t.setup.adminCreated })
      setAdminCreated(true)
      await checkSetupStatus()
    } catch (err: any) {
      setNotice({
        tone: 'error',
        message: `${t.setup.createFailed}: ${err.message}`,
      })
    } finally {
      setCreatingAdmin(false)
    }
  }

  if (loading) {
    return (
      <div className="setup-state-card glass" role="status">
        <img src="/logo.webp" alt="Myriad" className="setup-state-logo" />
        <Spinner size="md" color="primary" />
      </div>
    )
  }

  if (error) {
    return (
      <div className="setup-flow flex w-full items-center justify-center">
        <div className="max-w-md w-full glass rounded-2xl shadow-xl p-6 sm:p-8 text-center">
          <div className="w-14 h-14 sm:w-16 sm:h-16 bg-red-100 dark:bg-red-900/30 rounded-full flex items-center justify-center mx-auto mb-4">
            <FaExclamationTriangle className="text-2xl sm:text-3xl text-red-600 dark:text-red-400" />
          </div>
          <h2 className="text-xl sm:text-2xl font-bold text-gray-900 dark:text-gray-100 mb-2">
            {t.setup.connectionFailed}
          </h2>
          <p className="text-sm sm:text-base text-gray-600 dark:text-gray-400 mb-6">
            {error}
          </p>
          <button
            onClick={() => void checkSetupStatus()}
            className="px-6 py-3 text-white rounded-lg transition-colors setup-retry-button min-h-11 text-sm sm:text-base"
          >
            {t.setup.retry}
          </button>
        </div>
      </div>
    )
  }

  if (!status) return null

  if (status.is_setup_required && !hasEnteredSetup) {
    return (
      <section
        className="setup-welcome glass"
        aria-labelledby="setup-welcome-title"
      >
        <div className="setup-welcome-glow" aria-hidden="true" />
        <img src="/logo.webp" alt="Myriad" className="setup-welcome-logo" />
        <div className="setup-welcome-copy">
          <p className="setup-eyebrow">{t.setup.welcomeEyebrow}</p>
          <h1 id="setup-welcome-title">{t.setup.welcomeTitle}</h1>
          <p className="setup-welcome-description">{t.setup.welcomeDesc}</p>
        </div>
        <div className="setup-welcome-features">
          <div>
            <LuServer aria-hidden="true" />
            <span>{t.setup.welcomeDatabase}</span>
          </div>
          <div>
            <LuShieldCheck aria-hidden="true" />
            <span>{t.setup.welcomeAdmin}</span>
          </div>
          <div>
            <LuSparkles aria-hidden="true" />
            <span>{t.setup.welcomeReady}</span>
          </div>
        </div>
        <button
          type="button"
          className="setup-primary-button"
          onClick={enterSetup}
        >
          <span>{t.setup.getStarted}</span>
          <LuArrowRight aria-hidden="true" />
        </button>
        <p className="setup-welcome-footnote">{t.setup.welcomeFootnote}</p>
      </section>
    )
  }

  // 如果设置完成，显示完成页面
  if (!status.is_setup_required) {
    return (
      <div className="setup-flow w-full py-8 md:py-12">
        <div className="max-w-4xl mx-auto">
          <div className="glass rounded-2xl shadow-xl p-6 sm:p-8 text-center">
            <div className="w-16 h-16 sm:w-20 sm:h-20 bg-green-100 dark:bg-green-900/30 rounded-full flex items-center justify-center mx-auto mb-4 sm:mb-6">
              <FaCheck className="text-3xl sm:text-4xl text-green-600 dark:text-green-400" />
            </div>
            <h2 className="text-2xl sm:text-3xl font-bold text-gray-900 dark:text-gray-100 mb-3 sm:mb-4">
              {t.setup.complete}
            </h2>
            <p className="text-sm sm:text-base text-gray-600 dark:text-gray-400 mb-6 sm:mb-8">
              {t.setup.completeDesc}
            </p>
            <a
              href="/login"
              className="inline-flex px-6 sm:px-8 py-3 text-sm sm:text-base text-white rounded-lg transition-colors setup-complete-link min-h-11 items-center justify-center"
            >
              {t.setup.goToLogin}
            </a>
          </div>
        </div>
      </div>
    )
  }

  return (
    <div className="setup-flow w-full py-8 md:py-12">
      <div className="max-w-6xl mx-auto">
        {/* 进度标签栏 */}
        <div className="flex justify-center mb-4 md:mb-6">
          <div className="glass rounded-xl p-1.5 inline-flex gap-1 sm:gap-1.5 w-full sm:w-auto">
            <div className="setup-step-completed flex flex-1 items-center justify-center gap-1.5 rounded-lg px-3 py-2.5 text-xs font-semibold sm:flex-none sm:gap-2 sm:px-6 sm:text-sm">
              <FaCheck className="text-green-600 dark:text-green-400" />
              <span className="hidden sm:inline">{t.setup.welcomeStep}</span>
              <span className="sm:hidden">{t.setup.welcomeStepShort}</span>
            </div>
            <div
              className={`flex-1 sm:flex-none px-3 sm:px-6 py-2.5 rounded-lg font-semibold text-xs sm:text-sm transition-all duration-300 flex items-center justify-center gap-1.5 sm:gap-2 ${
                !dbConfigured
                  ? 'bg-white dark:bg-neutral-900 shadow-sm setup-step-active'
                  : 'setup-step-completed'
              }`}
            >
              {dbConfigured ? (
                <FaCheck className="text-green-600 dark:text-green-400" />
              ) : (
                <FaDatabase />
              )}
              <span className="hidden sm:inline">{t.setup.databaseConfig}</span>
              <span className="sm:hidden">{t.setup.database}</span>
            </div>
            <div
              className={`flex-1 sm:flex-none px-3 sm:px-6 py-2.5 rounded-lg font-semibold text-xs sm:text-sm transition-all duration-300 flex items-center justify-center gap-1.5 sm:gap-2 ${
                dbConfigured && !adminCreated
                  ? 'bg-white dark:bg-neutral-900 shadow-sm setup-step-active'
                  : dbConfigured && adminCreated
                    ? 'setup-step-completed'
                    : 'setup-step-disabled'
              }`}
            >
              {adminCreated ? (
                <FaCheck className="text-green-600 dark:text-green-400" />
              ) : (
                <FaUser />
              )}
              <span className="hidden sm:inline">{t.setup.adminAccount}</span>
              <span className="sm:hidden">{t.auth.username}</span>
            </div>
          </div>
        </div>

        {notice && (
          <div
            className={`setup-notice setup-notice-${notice.tone}`}
            role={notice.tone === 'error' ? 'alert' : 'status'}
            aria-live="polite"
          >
            {notice.tone === 'success' ? (
              <FaCheck aria-hidden="true" />
            ) : notice.tone === 'error' ? (
              <FaExclamationTriangle aria-hidden="true" />
            ) : (
              <LuInfo aria-hidden="true" />
            )}
            <span>{notice.message}</span>
          </div>
        )}

        <div className="space-y-6">
          {/* 数据库配置卡片 */}
          {!dbConfigured && (
            <form
              className="glass rounded-xl p-4 md:p-5"
              onSubmit={(event) => {
                event.preventDefault()
                void handleSaveDbConfig()
              }}
            >
              <div className="flex flex-col md:flex-row items-start justify-between gap-4 md:gap-6">
                <div className="flex-1 w-full md:w-auto">
                  <div className="flex items-center gap-3 mb-3 md:mb-4">
                    <div className="w-10 h-10 sm:w-12 sm:h-12 rounded-xl flex items-center justify-center text-xl sm:text-2xl setup-db-icon-wrapper">
                      <FaDatabase />
                    </div>
                    <div>
                      <h2 className="text-base sm:text-lg font-bold text-gray-800 dark:text-gray-100">
                        {t.setup.databaseConfig}
                      </h2>
                      <p className="text-xs text-gray-500 dark:text-gray-400 mt-0.5">
                        {t.setup.databaseConfigDesc}
                      </p>
                    </div>
                  </div>

                  <div className="space-y-3 md:space-y-4">
                    {/* 配置模式提示 */}
                    {!dbConfigured && (
                      <div className="bg-amber-50 dark:bg-amber-900/20 border border-amber-200 dark:border-amber-800/50 rounded-lg p-3">
                        <div className="flex items-start gap-2">
                          <FaExclamationTriangle className="text-amber-600 dark:text-amber-500 mt-0.5 shrink-0" />
                          <div>
                            <p className="text-sm font-semibold text-amber-800 dark:text-amber-200 mb-1">
                              <LuWrench size={14} className="inline mr-1" />
                              {t.setup.configurationMode}
                            </p>
                            <p className="text-xs text-amber-700 dark:text-amber-300">
                              {t.setup.configurationModeDesc}
                            </p>
                          </div>
                        </div>
                      </div>
                    )}

                    {/* 配置表单 */}
                    <div className="setup-form-surface bg-white/50 rounded-lg p-4 border border-gray-200/50">
                      <h3 className="font-semibold text-gray-800 mb-3 text-sm">
                        {t.setup.connectionInfo}
                      </h3>
                      <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
                        <div>
                          <label className="block text-xs font-medium text-gray-700 mb-1">
                            {t.setup.host}
                          </label>
                          <input
                            type="number"
                            value={dbConfig.host}
                            onChange={(e) =>
                              setDbConfig({ ...dbConfig, host: e.target.value })
                            }
                            className="w-full px-3 py-2 text-sm border border-gray-300 rounded-lg focus:ring-2 focus:ring-indigo-500 focus:border-transparent"
                            placeholder="localhost"
                            autoComplete="off"
                          />
                        </div>
                        <div>
                          <label className="block text-xs font-medium text-gray-700 mb-1">
                            {t.setup.port}
                          </label>
                          <input
                            type="text"
                            value={dbConfig.port}
                            onChange={(e) =>
                              setDbConfig({ ...dbConfig, port: e.target.value })
                            }
                            className="w-full px-3 py-2 text-sm border border-gray-300 rounded-lg focus:ring-2 focus:ring-indigo-500 focus:border-transparent"
                            placeholder="5432"
                            min={1}
                            max={65535}
                            autoComplete="off"
                          />
                        </div>
                        <div>
                          <label className="block text-xs font-medium text-gray-700 mb-1">
                            {t.setup.database}
                          </label>
                          <input
                            type="text"
                            value={dbConfig.database}
                            onChange={(e) =>
                              setDbConfig({
                                ...dbConfig,
                                database: e.target.value,
                              })
                            }
                            className="w-full px-3 py-2 text-sm border border-gray-300 rounded-lg focus:ring-2 focus:ring-indigo-500 focus:border-transparent"
                            placeholder="myriad"
                            autoComplete="off"
                          />
                        </div>
                        <div>
                          <label className="block text-xs font-medium text-gray-700 mb-1">
                            {t.setup.username}
                          </label>
                          <input
                            type="text"
                            value={dbConfig.username}
                            onChange={(e) =>
                              setDbConfig({
                                ...dbConfig,
                                username: e.target.value,
                              })
                            }
                            className="w-full px-3 py-2 text-sm border border-gray-300 rounded-lg focus:ring-2 focus:ring-indigo-500 focus:border-transparent"
                            placeholder="postgres"
                            autoComplete="off"
                          />
                        </div>
                        <div className="sm:col-span-2">
                          <label className="block text-xs font-medium text-gray-700 mb-1">
                            {t.auth.password}
                          </label>
                          <input
                            type="password"
                            value={dbConfig.password}
                            onChange={(e) =>
                              setDbConfig({
                                ...dbConfig,
                                password: e.target.value,
                              })
                            }
                            className="w-full px-3 py-2 text-sm border border-gray-300 rounded-lg focus:ring-2 focus:ring-indigo-500 focus:border-transparent"
                            placeholder={t.auth.enterPassword}
                            autoComplete="off"
                          />
                        </div>
                      </div>
                    </div>

                    {/* 操作按钮 */}
                    <div className="flex gap-3">
                      <button
                        type="submit"
                        disabled={savingDb}
                        className="w-full py-3 text-white rounded-lg transition-colors disabled:opacity-50 disabled:cursor-not-allowed text-sm font-bold flex items-center justify-center gap-2 shadow-lg setup-save-button"
                      >
                        {savingDb ? (
                          <Spinner size="xs" color="white" />
                        ) : (
                          <LuDatabase size={16} />
                        )}
                        <span>
                          {savingDb ? t.setup.saving : t.setup.saveAndConnect}
                        </span>
                      </button>
                    </div>

                    {/* 说明文字 */}
                    <div className="text-xs text-gray-500 text-center">
                      <LuInfo size={12} className="inline mr-1" />
                      {t.setup.saveHint}
                    </div>
                  </div>
                </div>
              </div>
            </form>
          )}

          {/* 管理员账户卡片 */}
          {dbConfigured && !adminCreated && (
            <form
              className="glass rounded-xl p-5"
              onSubmit={(event) => {
                event.preventDefault()
                void handleCreateAdmin()
              }}
            >
              <div className="flex items-start justify-between gap-6">
                <div className="flex-1">
                  <div className="flex items-center gap-3 mb-4">
                    <div className="w-12 h-12 rounded-xl flex items-center justify-center text-2xl setup-admin-icon-wrapper">
                      <FaUser />
                    </div>
                    <div>
                      <h2 className="text-lg font-bold text-gray-800 dark:text-gray-100">
                        {t.setup.adminAccount}
                      </h2>
                      <p className="text-xs text-gray-500 dark:text-gray-400 mt-0.5">
                        {t.setup.adminAccountDesc}
                      </p>
                    </div>
                  </div>

                  <div className="space-y-4">
                    {/* 数据库表初始化提示 */}
                    {status && !status.has_database && (
                      <div className="bg-blue-50 dark:bg-blue-950/30 border border-blue-200 dark:border-blue-800/50 rounded-lg p-3">
                        <div className="flex items-start gap-2">
                          <FaDatabase className="text-blue-600 dark:text-blue-400 mt-0.5 shrink-0" />
                          <div className="flex-1">
                            <p className="text-sm font-semibold text-blue-800 dark:text-blue-200 mb-2">
                              <LuClipboardList
                                size={14}
                                className="inline mr-1"
                              />
                              {t.setup.initDatabase}
                            </p>
                            <p className="text-xs text-blue-700 dark:text-blue-300 mb-3">
                              {t.setup.initDatabaseDesc}
                            </p>
                            <button
                              type="button"
                              onClick={handleMigrateDatabase}
                              disabled={migratingDb}
                              className="w-full py-2 text-white rounded-lg transition-colors disabled:opacity-50 disabled:cursor-not-allowed text-sm font-semibold flex items-center justify-center gap-2 setup-migrate-button"
                            >
                              {migratingDb ? (
                                <Spinner size="xs" color="white" />
                              ) : (
                                <FaDatabase />
                              )}
                              <span>
                                {migratingDb
                                  ? t.setup.initializing
                                  : t.setup.initDatabase}
                              </span>
                            </button>
                          </div>
                        </div>
                      </div>
                    )}

                    {/* 管理员表单 - 仅在数据库已初始化后显示 */}
                    {status && status.has_database && (
                      <>
                        {/* 说明 */}
                        <div className="bg-blue-50 dark:bg-blue-950/30 border border-blue-200 dark:border-blue-800/50 rounded-lg p-3">
                          <p className="text-sm text-blue-800 dark:text-blue-200 mb-2">
                            {t.setup.adminAccountFullDesc}
                          </p>
                          <ul className="text-xs text-blue-700 dark:text-blue-300 space-y-1 list-disc list-inside">
                            <li>{t.setup.adminUsernameHint}</li>
                            <li>{t.setup.adminPasswordHint}</li>
                          </ul>
                        </div>

                        {/* 表单 */}
                        <div className="setup-form-surface bg-white/50 rounded-lg p-4 border border-gray-200/50">
                          <div className="space-y-3">
                            <div>
                              <label className="block text-xs font-medium text-gray-700 mb-1">
                                {t.auth.username} *
                              </label>
                              <input
                                type="text"
                                value={adminForm.username}
                                onChange={(e) =>
                                  setAdminForm({
                                    ...adminForm,
                                    username: e.target.value,
                                  })
                                }
                                className="w-full px-3 py-2 text-sm border border-gray-300 rounded-lg focus:ring-2 focus:ring-indigo-500 focus:border-transparent"
                                placeholder="admin"
                                pattern="^[a-zA-Z0-9_]{3,20}$"
                                autoComplete="username"
                              />
                            </div>
                            <div>
                              <label className="block text-xs font-medium text-gray-700 mb-1">
                                {t.auth.password} *
                              </label>
                              <input
                                type="password"
                                value={adminForm.password}
                                onChange={(e) =>
                                  setAdminForm({
                                    ...adminForm,
                                    password: e.target.value,
                                  })
                                }
                                className="w-full px-3 py-2 text-sm border border-gray-300 rounded-lg focus:ring-2 focus:ring-indigo-500 focus:border-transparent"
                                placeholder={t.setup.atLeast8Chars}
                                minLength={8}
                                autoComplete="new-password"
                              />
                            </div>
                            <div>
                              <label className="block text-xs font-medium text-gray-700 mb-1">
                                {t.auth.confirmPassword} *
                              </label>
                              <input
                                type="password"
                                value={adminForm.confirmPassword}
                                onChange={(e) =>
                                  setAdminForm({
                                    ...adminForm,
                                    confirmPassword: e.target.value,
                                  })
                                }
                                className="w-full px-3 py-2 text-sm border border-gray-300 rounded-lg focus:ring-2 focus:ring-indigo-500 focus:border-transparent"
                                placeholder={t.setup.enterPasswordAgain}
                                minLength={8}
                                autoComplete="new-password"
                              />
                            </div>
                          </div>
                        </div>

                        {/* 创建按钮 */}
                        <button
                          type="submit"
                          disabled={creatingAdmin}
                          className="w-full py-2.5 text-white rounded-lg transition-colors disabled:opacity-50 disabled:cursor-not-allowed text-sm font-semibold flex items-center justify-center gap-2 setup-create-admin-button"
                        >
                          {creatingAdmin ? (
                            <Spinner size="xs" color="white" />
                          ) : (
                            <FaUser />
                          )}
                          <span>
                            {creatingAdmin
                              ? t.setup.creating
                              : t.setup.createAdmin}
                          </span>
                        </button>
                      </>
                    )}
                  </div>
                </div>
              </div>
            </form>
          )}
        </div>
      </div>
    </div>
  )
}

export default SetupWizard
