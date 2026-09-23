import {
  LuAlertTriangle,
  LuArrowRight,
  LuCheck,
  LuDatabase,
  LuGlobe,
  LuRotateCw,
  LuServer,
  LuShieldCheck,
  LuSparkles,
} from '@lib/icons'
import React, {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from 'react'
import { API_URL } from '../config'
import { useConfigI18n as useI18n } from '../contexts/I18nContext'
import { ApiError } from '../services/api'
import { updateConfig } from '../services/configApi'
import { parseAuthMeResponse } from '../utils/authMe'
import { consumeSetupSecretFromLocation } from '../utils/setupSecretFromUrl'
import { userFacingError } from '../utils/userFacingError'
import { InputItem, SegmentedControl, SwitchItem } from './settings'
import { SettingItemWrapper } from './settings/items/SettingItemWrapper'
import {
  ActionBar,
  Aurora,
  BackButton,
  BrandMark,
  BrandTag,
  Field,
  Note,
  PrimaryButton,
  StepBody,
  StepHero,
  StepTopBar,
  TextInput,
  useTopBarDense,
} from './setup/SetupChrome'
import { Spinner } from './Spinner'
import './SetupWizard.css'

interface SetupStatus {
  is_setup_required: boolean
  has_database: boolean
  has_admin_user: boolean
  setup_secret_required?: boolean
  missing_configs: string[]
}

type SetupNotice = {
  tone: 'info' | 'success' | 'error'
  message: string
} | null

type Stage =
  | 'blank'
  | 'loading'
  | 'error'
  | 'claimed'
  | 'welcome'
  | 'database'
  | 'migrate'
  | 'admin'
  | 'site'
  | 'done'

const TOTAL_STEPS = 4

// 问候名写入 session，完成页/刷新仍能叫出刚建的管理员。
const SETUP_ADMIN_NAME_KEY = 'myriad-setup-admin-name'

// loading/error 用负秩走淡入；同一步内细分仍算前进。
const STAGE_RANK: Record<Stage, number> = {
  blank: -2,
  loading: -1,
  error: -1,
  claimed: -1,
  welcome: 0,
  database: 1,
  migrate: 2,
  admin: 3,
  site: 4,
  done: 5,
}

type EnterDir = 'forward' | 'back' | 'fade'

function enterDirBetween(from: Stage, to: Stage): EnterDir {
  const a = STAGE_RANK[from]
  const b = STAGE_RANK[to]
  if (a < 0 || b < 0) return 'fade'
  if (b > a) return 'forward'
  if (b < a) return 'back'
  return 'fade'
}

async function getResponseError(response: Response, fallback: string) {
  try {
    const body = await response.json()
    const raw = [body.message, body.error].find(
      (value) => typeof value === 'string' && value.trim(),
    )
    return userFacingError(
      new ApiError(
        typeof raw === 'string' ? raw : fallback,
        response.status,
        typeof body.code === 'string' ? body.code : undefined,
        undefined,
        typeof body.hint === 'string' ? body.hint : undefined,
      ),
      fallback,
    )
  } catch {
    return userFacingError(new ApiError('', response.status), fallback)
  }
}

const SetupWizard: React.FC = () => {
  const { t, format } = useI18n()
  const [status, setStatus] = useState<SetupStatus | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [setupClaimedClosed, setSetupClaimedClosed] = useState(false)
  const [notice, setNotice] = useState<SetupNotice>(null)
  const [hasEnteredSetup, setHasEnteredSetup] = useState(() => {
    return sessionStorage.getItem('myriad-setup-started') === 'true'
  })
  const pollTimerRef = useRef<number | null>(null)
  const cardRef = useRef<HTMLDivElement | null>(null)
  const paneRef = useRef<HTMLElement | null>(null)
  const bindPane = useCallback((node: HTMLElement | null) => {
    paneRef.current = node
  }, [])

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

  const [adminForm, setAdminForm] = useState({
    username: '',
    password: '',
    confirmPassword: '',
    setupSecret: '',
  })
  const [creatingAdmin, setCreatingAdmin] = useState(false)
  const [adminCreated, setAdminCreated] = useState(false)

  const [siteForm, setSiteForm] = useState({
    title: '',
    description: '',
    favicon: '',
    visibility: 'ai_full',
    analytics: true,
    memorySaver: false,
  })
  const [savingSite, setSavingSite] = useState(false)
  // 建完管理员后用本地标记压住第 4 步，避免状态机直接冲到完成。
  const [atSiteStep, setAtSiteStep] = useState(false)
  const [signedIn, setSignedIn] = useState(false)
  // 问候名不能只靠 adminForm：刷新或直开完成态时表单是空的。
  const [doneUserName, setDoneUserName] = useState(
    () => sessionStorage.getItem(SETUP_ADMIN_NAME_KEY) || '',
  )

  const rememberAdminName = useCallback((name: string) => {
    const trimmed = name.trim()
    if (!trimmed) return
    sessionStorage.setItem(SETUP_ADMIN_NAME_KEY, trimmed)
    setDoneUserName(trimmed)
  }, [])

  const checkSetupStatus = useCallback(async () => {
    try {
      setLoading(true)
      setError('')
      setSetupClaimedClosed(false)

      const healthResponse = await fetch(`${API_URL}/health`)
      if (!healthResponse.ok) {
        throw new Error(
          format(t.errors.backendUnreachable, {
            status: healthResponse.status,
          }),
        )
      }
      const healthData = await healthResponse.json()

      const configResponse = await fetch(`${API_URL}/api/setup/config`)
      if (!configResponse.ok) {
        throw new Error(
          format(t.errors.setupConfigFailed, {
            status: configResponse.status,
          }),
        )
      }
      const setupConfig = await configResponse.json()
      const setupSecretRequired = Boolean(setupConfig.setup_secret_required)
      const windowOpen = setupConfig.setup_window_open === true
      const refuseClosedWindow = () => {
        setSetupClaimedClosed(true)
        setStatus(null)
        setLoading(false)
      }

      if (
        healthData.mode === 'configuration' ||
        !healthData.database_connected
      ) {
        if (!windowOpen) {
          refuseClosedWindow()
          return
        }
        setStatus({
          is_setup_required: true,
          has_database: false,
          has_admin_user: false,
          setup_secret_required: setupSecretRequired,
          missing_configs: ['Database not configured'],
        })
        setDbConfigured(false)
        setAdminCreated(false)
        setLoading(false)
        return
      }

      const response = await fetch(`${API_URL}/api/setup/status`)
      if (!response.ok) {
        // 503 且未认领才继续向导；认领后窗口已关，不画写步骤。
        if (response.status === 503) {
          if (!windowOpen) {
            refuseClosedWindow()
            return
          }
          setStatus({
            is_setup_required: true,
            has_database: false,
            has_admin_user: false,
            setup_secret_required: setupSecretRequired,
            missing_configs: ['Database tables not initialized'],
          })
          setDbConfigured(Boolean(healthData.database_connected))
          setAdminCreated(false)
          setLoading(false)
          return
        }
        throw new Error(
          format(t.errors.setupCheckFailed, { status: response.status }),
        )
      }
      const data = await response.json()
      if (data.is_setup_required && !windowOpen) {
        refuseClosedWindow()
        return
      }
      setStatus(data)
      // 连通≠已建表：health 说 connected 才标数据库已配置。管理员表单仍闸在 has_database。
      setDbConfigured(Boolean(healthData.database_connected))
      setAdminCreated(data.has_admin_user)
      if (!data.is_setup_required) {
        sessionStorage.removeItem('myriad-setup-started')
      }
    } catch (err) {
      setError(userFacingError(err, t.setup.connectionFailedDesc))
    } finally {
      setLoading(false)
    }
  }, [t, format])

  useEffect(() => {
    void checkSetupStatus()

    return () => {
      if (pollTimerRef.current !== null) {
        window.clearTimeout(pollTimerRef.current)
      }
    }
  }, [checkSetupStatus])

  useEffect(() => {
    const secret = consumeSetupSecretFromLocation(window.location, (url) => {
      window.history.replaceState(window.history.state, '', url)
    })
    if (!secret) return
    setAdminForm((prev) =>
      prev.setupSecret ? prev : { ...prev, setupSecret: secret },
    )
  }, [])

  const enterSetup = () => {
    sessionStorage.setItem('myriad-setup-started', 'true')
    setHasEnteredSetup(true)
  }

  // 返回只回欢迎页，清 atSiteStep，否则第 4 步本地标记会把画面卡住。
  const leaveSetup = () => {
    sessionStorage.removeItem('myriad-setup-started')
    setNotice(null)
    setHasEnteredSetup(false)
    setAtSiteStep(false)
  }

  const setupSecretRequired = Boolean(status?.setup_secret_required)
  const setupWriteHeaders = (extra: Record<string, string> = {}) => {
    const headers: Record<string, string> = { ...extra }
    const secret = adminForm.setupSecret.trim()
    if (secret) {
      headers['X-Setup-Secret'] = secret
    }
    return headers
  }
  const ensureSetupSecret = () => {
    if (setupSecretRequired && !adminForm.setupSecret.trim()) {
      setNotice({ tone: 'error', message: t.setup.setupSecretRequired })
      return false
    }
    return true
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

    if (!ensureSetupSecret()) {
      return
    }

    setNotice(null)
    setSavingDb(true)

    try {
      const headers = setupWriteHeaders({
        'Content-Type': 'application/json',
      })
      const response = await fetch(`${API_URL}/api/setup/database-config`, {
        method: 'POST',
        headers,
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

        pollDatabaseConnection()
      } else {
        setNotice({
          tone: 'info',
          message: `${t.setup.dbConfigSaved} ${t.setup.restartRequired}`,
        })
        setSavingDb(false)
      }
    } catch (err: unknown) {
      setNotice({
        tone: 'error',
        message: userFacingError(err, t.setup.saveConfigFailed),
      })
      setSavingDb(false)
    }
  }

  const pollDatabaseConnection = async () => {
    let attempts = 0
    const maxAttempts = 30
    const pollInterval = 2000

    const checkConnection = async () => {
      attempts++

      try {
        const healthResponse = await fetch(`${API_URL}/health`)
        if (healthResponse.ok) {
          const healthData = await healthResponse.json()

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
      } catch {
      }

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

    pollTimerRef.current = window.setTimeout(checkConnection, 3000)
  }

  const handleMigrateDatabase = async () => {
    setNotice(null)
    if (!ensureSetupSecret()) {
      return
    }
    setMigratingDb(true)

    try {
      const response = await fetch(`${API_URL}/api/setup/init-database`, {
        method: 'POST',
        headers: setupWriteHeaders(),
      })

      if (!response.ok) {
        throw new Error(
          await getResponseError(response, t.setup.dbMigrationFailed),
        )
      }

      const result = await response.json()

      const heading =
        result.kind === 'initialized' ||
        (typeof result.message === 'string' &&
          /initialized|初始化完成/i.test(result.message))
          ? t.setup.dbInitialized
          : t.setup.dbMigrationChecked
      let message = heading
      if (result.verification) {
        const v = result.verification
        message += `\n\n${t.setup.verificationResult}:`
        message += `\n• ${t.setup.totalTables}: ${v.total_tables}`
        message += `\n• ${t.setup.usersTable}: ${v.users_table ? t.setup.yes : t.setup.no}`
        message += `\n• ${t.setup.platformsTable}: ${v.platforms_table ? t.setup.yes : t.setup.no}`
        message += `\n• ${t.setup.configurationsTable}: ${v.configurations_table ? t.setup.yes : t.setup.no}`
      }
      setNotice({ tone: 'success', message })

      await checkSetupStatus()
    } catch (err: unknown) {
      setNotice({
        tone: 'error',
        message: userFacingError(err, t.setup.dbMigrationFailed),
      })
    } finally {
      setMigratingDb(false)
    }
  }

  // /api/auth/login 在 CSRF 豁免名单里，这里不需要 token。
  const signInAsNewAdmin = async () => {
    try {
      const response = await fetch(`${API_URL}/api/auth/login`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        credentials: 'include',
        body: JSON.stringify({
          username: adminForm.username,
          password: adminForm.password,
        }),
      })
      return response.ok
    } catch {
      return false
    }
  }

  const loadSiteInfoDraft = async () => {
    try {
      const response = await fetch(`${API_URL}/api/config`, {
        credentials: 'include',
      })
      if (!response.ok) return
      const config = await response.json()
      const fields = config?.ui_config?.config_fields
      if (!Array.isArray(fields)) return
      const pick = (key: string) =>
        fields.find((field: any) => field?.key === key)?.value || ''
      setSiteForm({
        title: pick('site_title'),
        description: pick('site_description'),
        favicon: pick('site_favicon'),
        visibility: pick('site_visibility_policy') || 'ai_full',
        analytics: pick('analytics_enabled') !== 'false',
        memorySaver: pick('memory_saver_enabled') === 'true',
      })
    } catch {
    }
  }

  const finishSetup = async () => {
    setSavingSite(false)
    setAtSiteStep(false)
    setNotice(null)
    sessionStorage.removeItem('myriad-setup-started')
    await checkSetupStatus()
  }

  const handleSaveSiteInfo = async () => {
    setNotice(null)
    setSavingSite(true)

    try {
      // POST /api/config 收整份 ConfigResponse：先取全量只改这几格，免得抹平别的配置。
      const current = await fetch(`${API_URL}/api/config`, {
        credentials: 'include',
      })
      if (!current.ok) {
        throw new Error(await getResponseError(current, t.setup.siteInfoFailed))
      }
      const config = await current.json()
      const fields = config?.ui_config?.config_fields
      if (!Array.isArray(fields)) {
        // 读回不是 ConfigResponse 就停手，整份回写会抹平别的配置。
        throw new TypeError(t.setup.siteInfoFailed)
      }

      // base_url 不在向导里改（域名走 /admin/site/domain）；整份回写只会写一半。site_noindex 是派生位。
      const patch: Record<string, string> = {
        site_title: siteForm.title.trim(),
        site_description: siteForm.description.trim(),
        site_favicon: siteForm.favicon.trim(),
        site_visibility_policy: siteForm.visibility,
        site_noindex: siteForm.visibility === 'private' ? 'true' : 'false',
        analytics_enabled: siteForm.analytics ? 'true' : 'false',
        memory_saver_enabled: siteForm.memorySaver ? 'true' : 'false',
      }
      config.ui_config.config_fields = fields.map((field: any) =>
        field && typeof field.key === 'string' && Object.hasOwn(patch, field.key)
          ? { ...field, value: patch[field.key] }
          : field,
      )

      await updateConfig(config)

      await finishSetup()
    } catch (err: unknown) {
      setNotice({
        tone: 'error',
        message: userFacingError(err, t.setup.siteInfoFailed),
      })
      setSavingSite(false)
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

    if (!ensureSetupSecret()) {
      return
    }

    setCreatingAdmin(true)

    try {
      const response = await fetch(`${API_URL}/api/setup/create-admin`, {
        method: 'POST',
        headers: setupWriteHeaders({
          'Content-Type': 'application/json',
        }),
        body: JSON.stringify({
          username: adminForm.username,
          password: adminForm.password,
          ...(setupSecretRequired
            ? { setup_secret: adminForm.setupSecret.trim() }
            : {}),
        }),
      })

      if (!response.ok) {
        throw new Error(
          await getResponseError(response, t.setup.createAdminFailed),
        )
      }

      setNotice({ tone: 'success', message: t.setup.adminCreated })
      setAdminCreated(true)
      rememberAdminName(adminForm.username)

      // 第 4 步 POST /api/config 要会话；登录不上也不阻断，送到完成页让他手动登录。
      const loggedIn = await signInAsNewAdmin()
      setSignedIn(loggedIn)
      if (loggedIn) {
        await loadSiteInfoDraft()
        setAtSiteStep(true)
        return
      }
      setNotice({ tone: 'info', message: t.setup.autoLoginFailed })
      await checkSetupStatus()
    } catch (err: unknown) {
      setNotice({
        tone: 'error',
        message: userFacingError(err, t.setup.createFailed),
      })
    } finally {
      setCreatingAdmin(false)
    }
  }

  // atSiteStep 优先：建完号服务端已说不用再配，本地标记压住第 4 步。
  const stage: Stage = atSiteStep
    ? 'site'
    : loading
      ? 'loading'
      : setupClaimedClosed
        ? 'claimed'
        : error
          ? 'error'
          : !status
            ? 'blank'
            : !status.is_setup_required
              ? 'done'
              : !hasEnteredSetup
                ? 'welcome'
                : !dbConfigured
                  ? 'database'
                  : !status.has_database
                    ? 'migrate'
                    : 'admin'

  useTopBarDense(cardRef, paneRef, stage)

  // 换步进场方向在 paint 前算好，避免首帧无 data-dir。动效打在内容层，卡片本身不动。
  const prevStageRef = useRef<Stage>(stage)
  const [enterDir, setEnterDir] = useState<EnterDir>('fade')
  useLayoutEffect(() => {
    const prev = prevStageRef.current
    if (prev !== stage) {
      setEnterDir(enterDirBetween(prev, stage))
      prevStageRef.current = stage
    }
  }, [stage])

  useEffect(() => {
    if (stage !== 'done') return

    const fromForm = adminForm.username.trim()
    if (fromForm) {
      rememberAdminName(fromForm)
      return
    }

    let cancelled = false
    void (async () => {
      try {
        const response = await fetch(`${API_URL}/api/auth/me`, {
          credentials: 'include',
        })
        if (!response.ok || cancelled) return
        const parsed = parseAuthMeResponse(await response.json())
        if (cancelled || !parsed.authenticated) return
        setSignedIn(true)
        if (parsed.user.username) {
          rememberAdminName(parsed.user.username)
        }
      } catch {
      }
    })()

    return () => {
      cancelled = true
    }
  }, [stage, adminForm.username, rememberAdminName])

  if (stage === 'blank') return null

  const greetingName = adminForm.username.trim() || doneUserName.trim()

  const stepIndex =
    stage === 'welcome'
      ? 1
      : stage === 'database' || stage === 'migrate'
        ? 2
        : stage === 'admin'
          ? 3
          : stage === 'site'
            ? 4
            : 0
  const stepName =
    stage === 'welcome'
      ? t.setup.welcomeStepShort
      : stage === 'database' || stage === 'migrate'
        ? t.setup.databaseStepShort
        : stage === 'admin'
          ? t.setup.adminStepShort
          : stage === 'site'
            ? t.setup.siteStepShort
            : ''

  const noticeNode = notice ? (
    <Note tone={notice.tone}>{notice.message}</Note>
  ) : null

  return (
    <section className="setup-ob" aria-label={t.setup.title}>
      <Aurora />

      <div className="setup-ob__card" ref={cardRef}>
        <StepTopBar
          stepName={stepName || undefined}
          current={stepIndex || undefined}
          total={stepIndex ? TOTAL_STEPS : undefined}
          progressText={format(t.setup.stepOf, {
            current: stepIndex,
            total: TOTAL_STEPS,
          })}
          back={
            stage === 'database' ||
            stage === 'migrate' ||
            stage === 'admin' ||
            stage === 'site' ? (
              <BackButton
                label={format(t.setup.backTo, {
                  step: t.setup.welcomeStepShort,
                })}
                destination={t.setup.welcomeStepShort}
                disabled={
                  savingDb || migratingDb || creatingAdmin || savingSite
                }
                onClick={leaveSetup}
              />
            ) : stage === 'welcome' ? (
              <BrandMark label={t.setup.welcomeEyebrow} />
            ) : stage === 'done' ? (
              <BrandMark label={t.setup.doneEyebrow} icon={LuCheck} />
            ) : (
              <BrandTag label="Myriad" />
            )
          }
        />

        <div className="setup-ob__viewport">
          {stage === 'loading' && (
            <div
              className="setup-ob__pane is-centered"
              ref={bindPane}
              data-dir={enterDir}
              key="loading"
            >
              <div className="setup-ob-state" role="status">
                <img src="/logo.webp" alt="" className="setup-ob-state__logo" />
                <Spinner size="md" color="primary" />
                <p>{t.setup.checkingStatus}</p>
              </div>
            </div>
          )}

          {stage === 'claimed' && (
            <div
              className="setup-ob__pane is-centered"
              ref={bindPane}
              data-dir={enterDir}
              key="claimed"
            >
              <div className="setup-ob-state">
                <span className="setup-ob-state__glyph is-bad" aria-hidden>
                  <LuAlertTriangle />
                </span>
                <h1>{t.setup.claimedRepairTitle}</h1>
                <p>{t.setup.claimedRepairDesc}</p>
              </div>
              <ActionBar>
                <PrimaryButton
                  label={t.setup.retry}
                  icon={LuRotateCw}
                  onClick={() => void checkSetupStatus()}
                />
              </ActionBar>
            </div>
          )}

          {stage === 'error' && (
            <div
              className="setup-ob__pane is-centered"
              ref={bindPane}
              data-dir={enterDir}
              key="error"
            >
              <div className="setup-ob-state">
                <span className="setup-ob-state__glyph is-bad" aria-hidden>
                  <LuAlertTriangle />
                </span>
                <h1>{t.setup.connectionFailed}</h1>
                <p>{error}</p>
              </div>
              <ActionBar>
                <PrimaryButton
                  label={t.setup.retry}
                  icon={LuRotateCw}
                  onClick={() => void checkSetupStatus()}
                />
              </ActionBar>
            </div>
          )}

          {stage === 'done' && (
            <div
              className="setup-ob__pane is-welcome"
              ref={bindPane}
              data-dir={enterDir}
              key="done"
            >
              <div className="setup-ob-welcome">
                <img
                  src="/icons/widgets/welcome.webp"
                  alt=""
                  aria-hidden
                  draggable={false}
                  className="setup-ob-welcome__logo setup-ob-welcome__wave"
                />
                <StepHero
                  title={
                    greetingName ? (
                      <>
                        {format(t.setup.doneGreeting, { name: greetingName })}
                        <br />
                        {t.setup.doneReadyTitle}
                      </>
                    ) : (
                      t.setup.doneReadyTitle
                    )
                  }
                  lead={t.setup.completeDesc}
                  titleId="setup-ob-title"
                />
              </div>
              <ActionBar>
                <a
                  href={signedIn ? '/' : '/login'}
                  className="setup-ob-cta"
                  title={signedIn ? t.setup.enterSite : t.setup.goToLogin}
                  aria-label={signedIn ? t.setup.enterSite : t.setup.goToLogin}
                >
                  <span>
                    {signedIn ? t.setup.enterSite : t.setup.goToLogin}
                  </span>
                  <LuArrowRight aria-hidden />
                </a>
              </ActionBar>
            </div>
          )}

          {stage === 'welcome' && (
            <div
              className="setup-ob__pane is-welcome"
              ref={bindPane}
              data-dir={enterDir}
              key="welcome"
            >
              <div className="setup-ob-welcome">
                <img
                  src="/logo.webp"
                  alt="Myriad"
                  className="setup-ob-welcome__logo"
                />
                <StepHero
                  title={t.setup.welcomeTitle}
                  lead={t.setup.welcomeDesc}
                  titleId="setup-ob-title"
                  notes={noticeNode}
                />
                <StepBody>
                  <div className="setup-ob-features">
                    <div>
                      <LuServer aria-hidden />
                      <span>{t.setup.welcomeDatabase}</span>
                    </div>
                    <div>
                      <LuShieldCheck aria-hidden />
                      <span>{t.setup.welcomeAdmin}</span>
                    </div>
                    <div>
                      <LuGlobe aria-hidden />
                      <span>{t.setup.welcomeSite}</span>
                    </div>
                    <div>
                      <LuSparkles aria-hidden />
                      <span>{t.setup.welcomeReady}</span>
                    </div>
                  </div>
                </StepBody>
              </div>
              <ActionBar split>
                <p className="setup-ob-bar__note">{t.setup.welcomeFootnote}</p>
                <PrimaryButton
                  label={t.setup.getStarted}
                  onClick={enterSetup}
                />
              </ActionBar>
            </div>
          )}

          {stage === 'database' && (
            <form
              className="setup-ob__pane"
              ref={bindPane}
              data-dir={enterDir}
              key="database"
              onSubmit={(event) => {
                event.preventDefault()
                void handleSaveDbConfig()
              }}
            >
              <StepHero
                title={t.setup.databaseConfig}
                lead={t.setup.databaseConfigDesc}
                titleId="setup-ob-title"
                notes={
                  <>
                    <Note tone="warn">
                      {`${t.setup.configurationMode}\n${t.setup.configurationModeDesc}`}
                    </Note>
                    {noticeNode}
                  </>
                }
              />
              <StepBody>
                <div className="setup-ob-grid">
                  <Field label={t.setup.host}>
                    <TextInput
                      type="text"
                      value={dbConfig.host}
                      onChange={(e) =>
                        setDbConfig({ ...dbConfig, host: e.target.value })
                      }
                      placeholder="localhost"
                      autoComplete="off"
                    />
                  </Field>
                  <Field label={t.setup.port}>
                    <TextInput
                      type="number"
                      value={dbConfig.port}
                      onChange={(e) =>
                        setDbConfig({ ...dbConfig, port: e.target.value })
                      }
                      placeholder="5432"
                      min={1}
                      max={65535}
                      autoComplete="off"
                    />
                  </Field>
                  <Field label={t.setup.database}>
                    <TextInput
                      type="text"
                      value={dbConfig.database}
                      onChange={(e) =>
                        setDbConfig({ ...dbConfig, database: e.target.value })
                      }
                      placeholder="myriad"
                      autoComplete="off"
                    />
                  </Field>
                  <Field label={t.setup.username}>
                    <TextInput
                      type="text"
                      value={dbConfig.username}
                      onChange={(e) =>
                        setDbConfig({ ...dbConfig, username: e.target.value })
                      }
                      placeholder="postgres"
                      autoComplete="off"
                    />
                  </Field>
                  <Field label={t.auth.password} wide>
                    <TextInput
                      type="password"
                      value={dbConfig.password}
                      onChange={(e) =>
                        setDbConfig({ ...dbConfig, password: e.target.value })
                      }
                      placeholder={t.setup.enterDbPassword}
                      autoComplete="off"
                    />
                  </Field>
                  {setupSecretRequired ? (
                    <Field
                      label={t.setup.setupSecret}
                      hint={t.setup.setupSecretHint}
                      wide
                    >
                      <TextInput
                        type="password"
                        mono
                        value={adminForm.setupSecret}
                        onChange={(e) =>
                          setAdminForm({
                            ...adminForm,
                            setupSecret: e.target.value,
                          })
                        }
                        placeholder={t.setup.setupSecretPlaceholder}
                        autoComplete="off"
                      />
                    </Field>
                  ) : null}
                </div>
                <Note>{t.setup.saveHint}</Note>
              </StepBody>
              <ActionBar>
                <PrimaryButton
                  type="submit"
                  label={savingDb ? t.setup.saving : t.setup.saveAndConnect}
                  icon={LuDatabase}
                  busy={savingDb}
                />
              </ActionBar>
            </form>
          )}

          {stage === 'migrate' && (
            <div
              className="setup-ob__pane"
              ref={bindPane}
              data-dir={enterDir}
              key="migrate"
            >
              <StepHero
                title={t.setup.initDatabase}
                lead={t.setup.initDatabaseDesc}
                titleId="setup-ob-title"
                notes={noticeNode}
              />
              <StepBody>
                <Note tone="success">{t.setup.dbConnectionSuccess}</Note>
                {setupSecretRequired ? (
                  <Field
                    label={t.setup.setupSecret}
                    hint={t.setup.setupSecretHint}
                  >
                    <TextInput
                      type="password"
                      mono
                      value={adminForm.setupSecret}
                      onChange={(e) =>
                        setAdminForm({
                          ...adminForm,
                          setupSecret: e.target.value,
                        })
                      }
                      placeholder={t.setup.setupSecretPlaceholder}
                      autoComplete="off"
                    />
                  </Field>
                ) : null}
              </StepBody>
              <ActionBar>
                <PrimaryButton
                  label={
                    migratingDb ? t.setup.initializing : t.setup.initDatabase
                  }
                  icon={LuDatabase}
                  busy={migratingDb}
                  onClick={() => void handleMigrateDatabase()}
                />
              </ActionBar>
            </div>
          )}

          {stage === 'admin' && (
            <form
              className="setup-ob__pane"
              ref={bindPane}
              data-dir={enterDir}
              key="admin"
              onSubmit={(event) => {
                event.preventDefault()
                void handleCreateAdmin()
              }}
            >
              <StepHero
                title={t.setup.adminAccount}
                lead={t.setup.adminAccountFullDesc}
                titleId="setup-ob-title"
                notes={noticeNode}
              />
              <StepBody>
                <Field label={t.auth.username} hint={t.setup.adminUsernameHint}>
                  <TextInput
                    type="text"
                    value={adminForm.username}
                    onChange={(e) =>
                      setAdminForm({ ...adminForm, username: e.target.value })
                    }
                    placeholder="owner"
                    pattern="^[a-zA-Z0-9_]{3,20}$"
                    autoComplete="username"
                  />
                </Field>
                <Field label={t.auth.password} hint={t.setup.adminPasswordHint}>
                  <TextInput
                    type="password"
                    value={adminForm.password}
                    onChange={(e) =>
                      setAdminForm({ ...adminForm, password: e.target.value })
                    }
                    placeholder={t.setup.atLeast8Chars}
                    minLength={8}
                    autoComplete="new-password"
                  />
                </Field>
                <Field label={t.auth.confirmPassword}>
                  <TextInput
                    type="password"
                    value={adminForm.confirmPassword}
                    onChange={(e) =>
                      setAdminForm({
                        ...adminForm,
                        confirmPassword: e.target.value,
                      })
                    }
                    placeholder={t.setup.enterPasswordAgain}
                    minLength={8}
                    autoComplete="new-password"
                  />
                </Field>
                {status?.setup_secret_required ? (
                  <Field
                    label={t.setup.setupSecret}
                    hint={t.setup.setupSecretHint}
                  >
                    <TextInput
                      type="password"
                      mono
                      value={adminForm.setupSecret}
                      onChange={(e) =>
                        setAdminForm({
                          ...adminForm,
                          setupSecret: e.target.value,
                        })
                      }
                      placeholder={t.setup.setupSecretPlaceholder}
                      autoComplete="off"
                    />
                  </Field>
                ) : null}
              </StepBody>
              <ActionBar>
                <PrimaryButton
                  type="submit"
                  label={creatingAdmin ? t.setup.creating : t.setup.createAdmin}
                  busy={creatingAdmin}
                  disabled={adminCreated}
                />
              </ActionBar>
            </form>
          )}

          {stage === 'site' && (
            <form
              className="setup-ob__pane"
              ref={bindPane}
              data-dir={enterDir}
              key="site"
              onSubmit={(event) => {
                event.preventDefault()
                void handleSaveSiteInfo()
              }}
            >
              <StepHero
                title={t.setup.siteInfoTitle}
                lead={t.setup.siteInfoDesc}
                titleId="setup-ob-title"
                notes={noticeNode}
              />
              <StepBody>
                <InputItem
                  itemKey="site_title"
                  label={t.config.fieldSiteTitle}
                  value={siteForm.title}
                  onChange={(v) => setSiteForm({ ...siteForm, title: v })}
                  placeholder={t.config.placeholderSiteTitle}
                  hint={t.setup.siteTitleHint}
                  layout="vertical"
                />
                <InputItem
                  itemKey="site_description"
                  label={t.config.fieldSiteDescription}
                  value={siteForm.description}
                  onChange={(v) => setSiteForm({ ...siteForm, description: v })}
                  placeholder={t.config.placeholderSiteDescription}
                  multiline
                  rows={2}
                  layout="vertical"
                />
                <InputItem
                  itemKey="site_favicon"
                  label={t.config.fieldSiteFavicon}
                  value={siteForm.favicon}
                  onChange={(v) => setSiteForm({ ...siteForm, favicon: v })}
                  placeholder={t.config.placeholderSiteFavicon}
                  inputType="url"
                  variant="imageUpload"
                  accept="image/png,image/jpeg,image/webp,image/gif,image/svg+xml,image/x-icon,.ico"
                  maxImageBytes={512 * 1024}
                  uploadLabel={t.config.imageUpload}
                  clearImageLabel={t.config.imageUploadClear}
                  localImageLabel={t.config.imageUploadLocal}
                  previewAlt={t.config.fieldSiteFavicon}
                  imageTypeError={t.config.imageUploadTypeError}
                  imageSizeError={t.config.imageUploadSizeError}
                  imageReadError={t.config.imageUploadReadError}
                  hint={t.config.imageUploadHint}
                  layout="vertical"
                />
                <SettingItemWrapper
                  label={t.config.fieldSiteVisibilityPolicy}
                  description={t.config.fieldSiteVisibilityPolicyHint}
                  layout="vertical"
                >
                  <SegmentedControl
                    size="sm"
                    columns={4}
                    value={siteForm.visibility}
                    options={[
                      { value: 'ai_full', label: t.config.visibilityAiFull },
                      {
                        value: 'ai_citation',
                        label: t.config.visibilityAiCitation,
                      },
                      {
                        value: 'search_only',
                        label: t.config.visibilitySearchOnly,
                      },
                      { value: 'private', label: t.config.visibilityPrivate },
                    ]}
                    onChange={(v) =>
                      setSiteForm({ ...siteForm, visibility: v })
                    }
                    ariaLabel={t.config.fieldSiteVisibilityPolicy}
                  />
                </SettingItemWrapper>
                <SwitchItem
                  itemKey="analytics_enabled"
                  label={t.config.analytics.visitorTitle}
                  description={t.config.analytics.visitorDesc}
                  value={siteForm.analytics}
                  disabled={savingSite}
                  onChange={(checked) =>
                    setSiteForm({ ...siteForm, analytics: checked })
                  }
                  layout="horizontal"
                />
                <SwitchItem
                  itemKey="memory_saver_enabled"
                  label={t.config.memorySaver}
                  description={t.config.memorySaverHint}
                  value={siteForm.memorySaver}
                  disabled={savingSite}
                  onChange={(checked) =>
                    setSiteForm({ ...siteForm, memorySaver: checked })
                  }
                  layout="horizontal"
                />
              </StepBody>
              <ActionBar>
                <PrimaryButton
                  type="submit"
                  label={
                    savingSite ? t.setup.savingSiteInfo : t.setup.saveSiteInfo
                  }
                  icon={LuCheck}
                  busy={savingSite}
                />
              </ActionBar>
            </form>
          )}
        </div>
      </div>
    </section>
  )
}

export default SetupWizard
