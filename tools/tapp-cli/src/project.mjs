import {
  access,
  lstat,
  mkdir,
  readdir,
  readFile,
  stat,
  writeFile,
} from 'node:fs/promises'
import {
  basename,
  dirname,
  extname,
  join,
  relative,
  resolve,
  sep,
} from 'node:path'
import ts from 'typescript'
import {
  expectedPackagePaths,
  normalizeManifestPaths,
  PACKAGE_MARKERS,
} from './package-layout.mjs'
import { writeZip, zipSize } from './zip.mjs'

const contract = JSON.parse(
  await readFile(new URL('./generated/contract.json', import.meta.url), 'utf8'),
)
const catalog = contract.permissions
const capabilities = contract.capabilities || {
  profiles: ['page', 'widget', 'headless'],
  headlessDeniedActions: [],
}
const HEADLESS_DENIED_ACTIONS = new Set(capabilities.headlessDeniedActions || [])
const schemaDefinitions = contract.schema.$defs
const schemaFields = (name) => Object.keys(schemaDefinitions[name].properties)
const schemaEnum = (name) => schemaDefinitions[name].enum

const TOP_LEVEL_FIELDS = new Set(Object.keys(contract.schema.properties))
const CATEGORIES = new Set([
  ...schemaEnum('TappCategory'),
  ...contract.rules.tappCategoryAliases,
])
const WIDGET_SIZES = new Set(contract.rules.widgetSizes)
const BACKGROUND_REQUIREMENTS = new Set(contract.rules.backgroundRequirements)
const WIDGET_CATEGORIES = new Set([
  ...schemaEnum('TappWidgetCategory'),
  ...contract.rules.widgetCategoryAliases,
])
const WIDGET_REFRESH_MODES = new Set(schemaEnum('TappWidgetRefreshMode'))
const SETTING_TYPES = new Set(contract.rules.settingTypes)
const AI_OPERATIONS = new Set(schemaEnum('TappAiOperation'))
const AI_MODEL_TIERS = new Set(schemaEnum('TappAiModelTier'))
const AI_CONTEXT_SOURCES = new Set(schemaEnum('TappAiContextSource'))
const AI_OUTPUT_FORMATS = new Set(schemaEnum('TappAiOutputFormat'))
const API_ACCESS_LEVELS = new Set(schemaEnum('TappApiAccess'))
const API_PUBLIC_ACCESS = schemaEnum('TappApiAccess').find((value) => value === 'public')
const API_BUILTINS = new Set(contract.rules.apiBuiltins)
const API_TYPES = new Set(contract.rules.apiTypes)
const CSS_MODES = new Set(contract.rules.cssModes)
const AGENT_INTENTS = new Set(contract.rules.agentIntents)
const SETTING_FIELD_TYPES = contract.rules.settingFieldTypes
const SETTING_DEFAULT_KINDS = contract.rules.settingDefaultKinds
const EVENT_DIRECTIONS = Object.keys(contract.rules.eventPermissionRules)
const DATA_EXCHANGE_DIRECTIONS = contract.rules.dataExchangeDirections
const API_BUILTIN_AI_OPERATIONS = contract.rules.apiBuiltinAiOperations
const API_BUILTIN_PERMISSIONS = contract.rules.apiBuiltinPermissions
const MANIFEST_RESOURCE_FIELDS = contract.rules.manifestResourceFields
const AGENT_SCHEMA_FIELDS = new Set(contract.rules.agentSchemaFields)
const URL_FIELDS = contract.rules.urlFields
const PACKAGE_RESOURCE_EXTENSIONS = contract.rules.packageResourceExtensions
const PACKAGE_JSON_OBJECT_DIRECTORIES = new Set(contract.rules.packageJsonObjectDirectories)
const PACKAGE_RESOURCE_FILE_LIMITS = contract.rules.packageResourceFileLimits
const PACKAGE_RESOURCE_BYTE_LIMITS = contract.rules.packageResourceByteLimits
const ASSET_DIRECTORY = contract.rules.assetDirectory
const PAGE_MODULE_DIRECTORY = contract.rules.pageModuleDirectory
const DEFAULT_API_TYPE = contract.rules.defaultApiType
const HTTP_API_TYPE = contract.rules.httpApiType
const BUILTIN_API_TYPE = contract.rules.builtinApiType
const DEFAULT_HTTP_METHOD = contract.rules.defaultHttpMethod
const WIDGET_REFRESH_EVENT_MODE = contract.rules.widgetRefreshModes.event
const WIDGET_REFRESH_INTERVAL_MODE = contract.rules.widgetRefreshModes.interval

const SAFE_COMPONENT = new RegExp(contract.patterns.safeComponent)
const LOCALE_TAG = new RegExp(contract.patterns.localeTag)
const SEMVER = new RegExp(contract.patterns.semver)
const NAMED_VALUE = new RegExp(contract.patterns.namedValue)
const STORAGE_KEY = new RegExp(contract.patterns.storageKey)
const THEME_COLOR = new RegExp(contract.patterns.themeColor)
const HTTP_METHOD = new RegExp(contract.patterns.httpMethod)
const CODE_EXTENSIONS = new Set(contract.rules.sourceCodeExtensions)
const SKIP_DIRECTORIES = new Set(contract.rules.sourceScanSkipDirectories)
const REQUIRED_MANIFEST_FIELDS = new Set([
  ...contract.schema.required,
  ...contract.rules.requiredManifestFields,
])
const ASSET_LITERAL_METHODS = new Set(contract.rules.assetLiteralMethods)
const MAX_ARCHIVE_FILES = contract.limits.archiveFiles
const MAX_ARCHIVE_BYTES = contract.limits.archiveBytes
const MAX_UNCOMPRESSED_BYTES = contract.limits.archiveUncompressedBytes
const MAX_ASSET_BYTES = contract.limits.assetBytes
const MAX_ASSETS_BYTES = contract.limits.assetsTotalBytes
const MAX_I18N_BYTES = contract.limits.i18nResourceBytes
const MAX_AGENT_SCHEMA_BYTES = contract.limits.agentSchemaBytes

function diagnostic(severity, code, message, file = 'manifest.json', line, column) {
  return { severity, code, message, file, line, column }
}

function isObject(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function validateFields(value, allowed, path, diagnostics) {
  if (!isObject(value)) return false
  for (const field of Object.keys(value)) {
    if (!allowed.includes(field)) {
      diagnostics.push(
        diagnostic('error', 'unknown-manifest-field', `Unknown manifest field: ${path}.${field}`),
      )
    }
  }
  return true
}

function hasDuplicates(values) {
  return new Set(values).size !== values.length
}

function arrayValue(value) {
  return Array.isArray(value) ? value : []
}

function formatBytes(bytes) {
  for (const [unitBytes, unit] of [
    [1024 ** 3, 'GiB'],
    [1024 ** 2, 'MiB'],
    [1024, 'KiB'],
  ]) {
    if (bytes >= unitBytes && bytes % unitBytes === 0) {
      return `${bytes / unitBytes} ${unit}`
    }
  }
  return `${bytes} bytes`
}

function isNamedValue(value) {
  return (
    typeof value === 'string' &&
    value.length > 0 &&
    value.length <= contract.limits.tappIdLength &&
    !value.startsWith('.') &&
    !value.endsWith('.') &&
    !value.includes('..') &&
    NAMED_VALUE.test(value)
  )
}

function isDataExchangeId(value) {
  return typeof value === 'string' && value.length > 0 && value.length <= contract.limits.dataExchangeIdLength && NAMED_VALUE.test(value)
}

function validateHttpUrl(value) {
  if (typeof value !== 'string' || value.length > contract.limits.httpUrlLength) return false
  try {
    return contract.rules.httpUrlSchemes.includes(new URL(value).protocol.replace(/:$/, ''))
  } catch {
    return false
  }
}

function validateStorageKey(value) {
  return (
    typeof value === 'string' &&
    value.length > 0 &&
    value.length <= contract.limits.storageKeyLength &&
    !value.startsWith('.') &&
    !value.endsWith('.') &&
    !value.includes('..') &&
    STORAGE_KEY.test(value)
  )
}

function validateInlineSchema(schema) {
  if (!isObject(schema) || Buffer.byteLength(JSON.stringify(schema)) > contract.limits.dataExchangeSchemaBytes) return false
  if (!contract.rules.inlineSchemaRootKeys.some((field) => field in schema)) return false
  const visit = (value, depth) => {
    if (depth > contract.limits.inlineSchemaDepth) return false
    if (Array.isArray(value)) return value.every((child) => visit(child, depth + 1))
    if (!isObject(value)) return true
    if ('$ref' in value) return false
    return Object.values(value).every((child) => visit(child, depth + 1))
  }
  return visit(schema, 0)
}

function validateSettings(settings, path, diagnostics) {
  if (!Array.isArray(settings)) {
    diagnostics.push(diagnostic('error', 'invalid-settings', `${path} must be an array`))
    return
  }
  if (settings.length > contract.limits.tappSettings) {
    diagnostics.push(diagnostic('error', 'invalid-settings', `${path} accepts at most ${contract.limits.tappSettings} entries`))
  }
  const keys = new Set()
  for (const [index, setting] of settings.entries()) {
    const settingPath = `${path}[${index}]`
    if (
      !validateFields(
        setting,
        schemaFields('TappSettingDef'),
        settingPath,
        diagnostics,
      )
    ) {
      diagnostics.push(diagnostic('error', 'invalid-setting', `${settingPath} must be an object`))
      continue
    }
    if (
      !validateStorageKey(setting.key) ||
      keys.has(setting.key) ||
      typeof setting.label !== 'string' ||
      !setting.label ||
      setting.label.length > contract.limits.settingLabelLength ||
      !SETTING_TYPES.has(setting.type)
    ) {
      diagnostics.push(diagnostic('error', 'invalid-setting', `${settingPath} is invalid or duplicated`))
    }
    keys.add(setting.key)
    const optionType = SETTING_FIELD_TYPES.options
    if (setting.type === optionType) {
      if (!Array.isArray(setting.options) || setting.options.length === 0 || setting.options.length > contract.limits.settingOptions) {
        diagnostics.push(diagnostic('error', 'invalid-setting-options', `${settingPath}.options must contain 1-${contract.limits.settingOptions} entries`))
      }
    } else if (setting.options !== undefined) {
      diagnostics.push(diagnostic('error', 'invalid-setting-options', `${settingPath}.options is only valid for select settings`))
    }
    if (Array.isArray(setting.options)) {
      const values = new Set()
      for (const [optionIndex, option] of setting.options.entries()) {
        const optionPath = `${settingPath}.options[${optionIndex}]`
        validateFields(option, schemaFields('TappSettingOption'), optionPath, diagnostics)
        if (
          !isObject(option) ||
          typeof option.value !== 'string' ||
          !option.value ||
          option.value.length > contract.limits.settingOptionValueLength ||
          values.has(option.value) ||
          typeof option.label !== 'string' ||
          !option.label ||
          option.label.length > contract.limits.settingLabelLength
        ) {
          diagnostics.push(diagnostic('error', 'invalid-setting-option', `${optionPath} is invalid or duplicated`))
        }
        values.add(option?.value)
      }
    }
    for (const [field, expectedType] of Object.entries(SETTING_FIELD_TYPES)) {
      if (setting[field] !== undefined && setting.type !== expectedType) {
        diagnostics.push(diagnostic('error', 'invalid-setting', `${settingPath}.${field} requires type ${expectedType}`))
      }
    }
    if (
      (setting.min !== undefined && typeof setting.min !== 'number') ||
      (setting.max !== undefined && typeof setting.max !== 'number') ||
      (setting.step !== undefined && (typeof setting.step !== 'number' || setting.step <= 0)) ||
      (typeof setting.min === 'number' && typeof setting.max === 'number' && setting.min > setting.max)
    ) {
      diagnostics.push(diagnostic('error', 'invalid-setting-range', `${settingPath} has an invalid numeric range`))
    }
    if (setting.defaultValue !== undefined) {
      const defaultKind = SETTING_DEFAULT_KINDS[setting.type]
      const validDefault =
        (defaultKind === 'boolean' && typeof setting.defaultValue === 'boolean') ||
        (defaultKind === 'string' && typeof setting.defaultValue === 'string') ||
        (defaultKind === 'option' && Array.isArray(setting.options) && setting.options.some((option) => option.value === setting.defaultValue)) ||
        (defaultKind === 'number' &&
          typeof setting.defaultValue === 'number' &&
          Number.isFinite(setting.defaultValue) &&
          (setting.min === undefined || setting.defaultValue >= setting.min) &&
          (setting.max === undefined || setting.defaultValue <= setting.max))
      if (!validDefault) {
        diagnostics.push(diagnostic('error', 'invalid-setting-default', `${settingPath}.defaultValue is invalid`))
      }
    }
  }
}

function portablePath(value) {
  return value.split(sep).join('/')
}

function validateResourcePath(value) {
  if (typeof value !== 'string' || !value || value.length > contract.limits.resourcePathLength || value.includes('\\')) {
    return false
  }
  if (value.startsWith('/') || /^[A-Za-z]:/.test(value)) return false
  const components = value.split('/')
  return components.every(
    (component) =>
      component.length <= contract.limits.tappIdLength &&
      component !== '.' &&
      component !== '..' &&
      SAFE_COMPONENT.test(component),
  )
}

function validateStringField(manifest, field, diagnostics, { required = false } = {}) {
  const value = manifest[field]
  if (value === undefined && !required) return
  if (typeof value !== 'string' || !value.trim()) {
    diagnostics.push(
      diagnostic('error', 'manifest-field-type', `${field} must be a non-empty string`),
    )
  }
}

function inferredSurfaces(manifest) {
  const widgets = arrayValue(manifest.widgets)
  const pageModules = arrayValue(manifest.pageModules)
  const backgroundRequirements = arrayValue(manifest.backgroundRequirements)
  const page =
    manifest.hasPage === true ||
    typeof manifest.pageTemplate === 'string' ||
    pageModules.length > 0
  const widget = widgets.length > 0
  const headless = backgroundRequirements.length > 0
  return { page, widget, headless, headlessOnly: headless && !page && !widget }
}

function validateModeConsistency(manifest, diagnostics, requiredPermissions) {
  const surfaces = inferredSurfaces(manifest)
  const widgets = arrayValue(manifest.widgets)

  if (widgets.length > 0) {
    addRequiredPermission(
      requiredPermissions,
      contract.rules.widgetManifestPermission,
      'manifest declares widgets',
    )
  }

  if (manifest.hasPage === true) {
    const hasPageResource =
      typeof manifest.pageTemplate === 'string' ||
      arrayValue(manifest.pageModules).length > 0
    if (!hasPageResource) {
      diagnostics.push(
        diagnostic(
          'error',
          'missing-page-resource',
          'hasPage requires pageTemplate or pageModules',
        ),
      )
    }
  }

  if (!surfaces.page && !surfaces.widget && !surfaces.headless) {
    diagnostics.push(
      diagnostic(
        'warning',
        'empty-runtime-surface',
        'Manifest declares neither Page, Widget, nor backgroundRequirements; only shared core can run',
      ),
    )
  }

  return surfaces
}

function addRequiredPermission(required, permission, reason, file = 'manifest.json') {
  if (!permission || permission === API_PUBLIC_ACCESS) return
  const entry = required.get(permission) || { permission, reasons: [], locations: [] }
  if (!entry.reasons.includes(reason)) entry.reasons.push(reason)
  if (!entry.locations.includes(file)) entry.locations.push(file)
  required.set(permission, entry)
}

function validateDataExchange(value, diagnostics) {
  if (!validateFields(value, schemaFields('TappDataExchangeManifest'), 'dataExchange', diagnostics)) {
    diagnostics.push(diagnostic('error', 'invalid-data-exchange', 'dataExchange must be an object'))
    return
  }
  for (const direction of Object.keys(DATA_EXCHANGE_DIRECTIONS)) {
    if (value[direction] !== undefined && !Array.isArray(value[direction])) {
      diagnostics.push(diagnostic('error', 'invalid-data-exchange', `dataExchange.${direction} must be an array`))
    } else if ((value[direction]?.length || 0) > contract.limits.dataExchangeDeclarations) {
      diagnostics.push(diagnostic('error', 'invalid-data-exchange', `dataExchange.${direction} accepts at most ${contract.limits.dataExchangeDeclarations} entries`))
    }
  }
  const exportDirection = Object.entries(DATA_EXCHANGE_DIRECTIONS).find(([, kind]) => kind === 'export')?.[0]
  const importDirection = Object.entries(DATA_EXCHANGE_DIRECTIONS).find(([, kind]) => kind === 'import')?.[0]
  const exportIds = new Set()
  for (const [index, entry] of arrayValue(value[exportDirection]).entries()) {
    const path = `dataExchange.${exportDirection}[${index}]`
    if (!validateFields(entry, schemaFields('TappDataExport'), path, diagnostics)) {
      diagnostics.push(diagnostic('error', 'invalid-data-export', `${path} must be an object`))
      continue
    }
    if (!isDataExchangeId(entry.id) || exportIds.has(entry.id)) {
      diagnostics.push(diagnostic('error', 'invalid-data-export', `${path}.id is invalid or duplicated`))
    }
    exportIds.add(entry.id)
    if (!validateInlineSchema(entry.schema)) {
      diagnostics.push(diagnostic('error', 'invalid-data-schema', `${path}.schema is not a supported inline JSON Schema`))
    }
    if (!Number.isInteger(entry.maxBytes) || entry.maxBytes < 1 || entry.maxBytes > contract.limits.dataExchangeResponseBytes) {
      diagnostics.push(diagnostic('error', 'invalid-data-export', `${path}.maxBytes must be between 1 and ${contract.limits.dataExchangeResponseBytes}`))
    }
    if (entry.maxRecords !== undefined && (!Number.isInteger(entry.maxRecords) || entry.maxRecords < 1 || entry.maxRecords > contract.limits.dataExchangeRecords)) {
      diagnostics.push(diagnostic('error', 'invalid-data-export', `${path}.maxRecords must be between 1 and ${contract.limits.dataExchangeRecords}`))
    }
    if (entry.description !== undefined && (typeof entry.description !== 'string' || entry.description.length > contract.limits.dataExchangeDescriptionLength)) {
      diagnostics.push(diagnostic('error', 'invalid-data-export', `${path}.description exceeds ${contract.limits.dataExchangeDescriptionLength} characters`))
    }
  }
  const imports = new Set()
  for (const [index, entry] of arrayValue(value[importDirection]).entries()) {
    const path = `dataExchange.${importDirection}[${index}]`
    if (!validateFields(entry, schemaFields('TappDataImport'), path, diagnostics)) {
      diagnostics.push(diagnostic('error', 'invalid-data-import', `${path} must be an object`))
      continue
    }
    const key = `${entry.tappId}\0${entry.exportId}`
    if (!isNamedValue(entry.tappId) || !isDataExchangeId(entry.exportId) || imports.has(key)) {
      diagnostics.push(diagnostic('error', 'invalid-data-import', `${path} is invalid or duplicated`))
    }
    imports.add(key)
  }
}

function validateAi(value, diagnostics, requiredPermissions) {
  if (!validateFields(value, schemaFields('TappAiManifest'), 'ai', diagnostics)) {
    diagnostics.push(diagnostic('error', 'invalid-ai', 'ai must be an object'))
    return
  }
  if (value.protocolVersion !== contract.rules.protocolVersion) {
    diagnostics.push(diagnostic('error', 'invalid-ai', `ai.protocolVersion must be ${contract.rules.protocolVersion}`))
  }
  if (!Array.isArray(value.operations) || value.operations.length < 1 || value.operations.length > contract.limits.aiOperations || value.operations.some((item) => !AI_OPERATIONS.has(item)) || hasDuplicates(value.operations)) {
    diagnostics.push(diagnostic('error', 'invalid-ai', `ai.operations must contain 1-${contract.limits.aiOperations} unique supported operations`))
  } else {
    for (const operation of value.operations) {
      addRequiredPermission(requiredPermissions, contract.rules.aiOperationPermissions[operation], `manifest.ai uses ${operation}`)
    }
  }
  if (!AI_MODEL_TIERS.has(value.modelTier)) {
    diagnostics.push(diagnostic('error', 'invalid-ai', 'ai.modelTier must be standard or pro'))
  }
  const contextSources = value.contextSources ?? []
  if (!Array.isArray(contextSources) || contextSources.length > contract.limits.aiContextSources || contextSources.some((item) => !AI_CONTEXT_SOURCES.has(item)) || hasDuplicates(contextSources)) {
    diagnostics.push(diagnostic('error', 'invalid-ai', 'ai.contextSources must contain unique supported values'))
  } else {
    for (const [source, permission] of Object.entries(contract.rules.aiContextPermissionRules)) {
      if (contextSources.includes(source)) addRequiredPermission(requiredPermissions, permission, `manifest.ai uses ${source} context`)
    }
  }
  if (!Array.isArray(value.outputFormats) || value.outputFormats.length < 1 || value.outputFormats.length > contract.limits.aiOutputFormats || value.outputFormats.some((item) => !AI_OUTPUT_FORMATS.has(item)) || hasDuplicates(value.outputFormats)) {
    diagnostics.push(diagnostic('error', 'invalid-ai', `ai.outputFormats must contain 1-${contract.limits.aiOutputFormats} unique supported values`))
  } else {
    for (const [operation, format] of Object.entries(contract.rules.aiOperationOutputRules)) {
      if (value.operations?.includes(operation) && !value.outputFormats.includes(format)) {
        diagnostics.push(diagnostic('error', 'invalid-ai', `ai ${operation} operation requires ${format} output format`))
      }
    }
  }
}

function validateEvents(value, manifest, diagnostics, requiredPermissions) {
  if (!validateFields(value, schemaFields('TappEventsManifest'), 'events', diagnostics)) {
    diagnostics.push(diagnostic('error', 'invalid-events', 'events must be an object'))
    return
  }
  for (const direction of EVENT_DIRECTIONS) {
    const topics = value[direction] ?? []
    if (!Array.isArray(topics) || topics.length > contract.limits.eventTopics || hasDuplicates(topics)) {
      diagnostics.push(diagnostic('error', 'invalid-events', `events.${direction} must contain at most ${contract.limits.eventTopics} unique topics`))
      continue
    }
    for (const topic of topics) {
      const prefixes = contract.rules.eventTopicPrefixes[direction].map((prefix) =>
        prefix.replace('{id}', manifest.id),
      )
      const valid = isNamedValue(topic) && prefixes.some((prefix) => topic.startsWith(prefix))
      if (!valid) diagnostics.push(diagnostic('error', 'invalid-event-topic', `Invalid events.${direction} topic: ${String(topic)}`))
    }
    if (topics.length) addRequiredPermission(requiredPermissions, contract.rules.eventPermissionRules[direction], `manifest declares event ${direction} topics`)
  }
}

function validateAgent(value, diagnostics) {
  if (!validateFields(value, schemaFields('TappAgentManifest'), 'agent', diagnostics)) {
    diagnostics.push(diagnostic('error', 'invalid-agent', 'agent must be an object'))
    return
  }
  if (value.protocolVersion !== contract.rules.protocolVersion) {
    diagnostics.push(diagnostic('error', 'invalid-agent', `agent.protocolVersion must be ${contract.rules.protocolVersion}`))
  }
  if (!Array.isArray(value.interactions) || value.interactions.length < 1 || value.interactions.length > contract.limits.agentInteractions) {
    diagnostics.push(diagnostic('error', 'invalid-agent', `agent.interactions must contain 1-${contract.limits.agentInteractions} entries`))
  } else {
    const types = new Set()
    for (const [index, interaction] of value.interactions.entries()) {
      const path = `agent.interactions[${index}]`
      if (!validateFields(interaction, schemaFields('TappAgentInteractionDef'), path, diagnostics)) {
        diagnostics.push(diagnostic('error', 'invalid-agent-interaction', `${path} must be an object`))
        continue
      }
      if (!isNamedValue(interaction.type) || types.has(interaction.type)) {
        diagnostics.push(diagnostic('error', 'invalid-agent-interaction', `${path}.type is invalid or duplicated`))
      }
      types.add(interaction.type)
      for (const field of contract.rules.agentSchemaFields) {
        if (interaction[field] !== undefined && (!validateResourcePath(interaction[field]) || extname(interaction[field]) !== contract.rules.resourceExtensions.agentSchema)) {
          diagnostics.push(diagnostic('error', 'invalid-agent-schema', `${path}.${field} must be a safe .json resource path`))
        }
      }
    }
  }
  const intents = value.intents ?? []
  if (!Array.isArray(intents) || intents.length > contract.limits.agentIntents || intents.some((intent) => !AGENT_INTENTS.has(intent)) || hasDuplicates(intents)) {
    diagnostics.push(diagnostic('error', 'invalid-agent', `agent.intents must contain at most ${contract.limits.agentIntents} unique supported intents`))
  }
}

function validateManifest(manifest, diagnostics, requiredPermissions) {
  for (const field of Object.keys(manifest)) {
    if (!TOP_LEVEL_FIELDS.has(field)) {
      diagnostics.push(
        diagnostic('error', 'unknown-manifest-field', `Unknown manifest field: ${field}`),
      )
    }
  }

  for (const field of REQUIRED_MANIFEST_FIELDS) {
    validateStringField(manifest, field, diagnostics, { required: true })
  }

  if (typeof manifest.id === 'string' && !SAFE_COMPONENT.test(manifest.id)) {
    diagnostics.push(
      diagnostic(
        'error',
        'invalid-tapp-id',
        `id must use 1-${contract.limits.tappIdLength} ASCII letters, numbers, dots, underscores, or hyphens`,
      ),
    )
  }
  if (typeof manifest.id === 'string' && manifest.id.length > contract.limits.tappIdLength) {
    diagnostics.push(diagnostic('error', 'invalid-tapp-id', `id exceeds ${contract.limits.tappIdLength} characters`))
  }
  if (typeof manifest.version === 'string' && !SEMVER.test(manifest.version)) {
    diagnostics.push(
      diagnostic('error', 'invalid-version', 'version must be a semantic version'),
    )
  }
  if (manifest.name !== undefined && (typeof manifest.name !== 'string' || !manifest.name.trim() || manifest.name.length > contract.limits.tappNameLength)) {
    diagnostics.push(diagnostic('error', 'invalid-name', `name must contain 1-${contract.limits.tappNameLength} characters`))
  }
  if (manifest.description !== undefined && (typeof manifest.description !== 'string' || manifest.description.length > contract.limits.tappDescriptionLength)) {
    diagnostics.push(diagnostic('error', 'invalid-description', `description must not exceed ${contract.limits.tappDescriptionLength} characters`))
  }
  if (typeof manifest.category === 'string' && !CATEGORIES.has(manifest.category)) {
    diagnostics.push(
      diagnostic(
        'error',
        'invalid-category',
        `category must be one of: ${[...CATEGORIES].join(', ')}`,
      ),
    )
  }
  for (const [field, extensionKey] of Object.entries(MANIFEST_RESOURCE_FIELDS)) {
    const extension = contract.rules.resourceExtensions[extensionKey]
    if (manifest[field] !== undefined && (!validateResourcePath(manifest[field]) || extname(manifest[field]) !== extension)) {
      diagnostics.push(diagnostic('error', field === 'main' ? 'invalid-main' : 'invalid-resource-path', `${field} must be a safe relative ${extension} path`))
    }
  }
  const normalizedSystemVersion = typeof manifest.minSystemVersion === 'string'
    ? contract.rules.semverPrefixes.reduce(
        (value, prefix) => value.startsWith(prefix) ? value.slice(prefix.length) : value,
        manifest.minSystemVersion,
      )
    : manifest.minSystemVersion
  if (manifest.minSystemVersion !== undefined && (typeof manifest.minSystemVersion !== 'string' || !SEMVER.test(normalizedSystemVersion))) {
    diagnostics.push(diagnostic('error', 'invalid-system-version', 'minSystemVersion must be a semantic version'))
  }
  if (manifest.icon !== undefined && (typeof manifest.icon !== 'string' || manifest.icon.length > contract.limits.tappIconLength)) {
    diagnostics.push(diagnostic('error', 'invalid-icon', `icon must not exceed ${contract.limits.tappIconLength} characters`))
  }
  if (manifest.iconSvg !== undefined && (typeof manifest.iconSvg !== 'string' || Buffer.byteLength(manifest.iconSvg) > contract.limits.tappIconSvgBytes)) {
    diagnostics.push(diagnostic('error', 'invalid-icon', `iconSvg must not exceed ${formatBytes(contract.limits.tappIconSvgBytes)}`))
  }

  if (manifest.permissions !== undefined && !Array.isArray(manifest.permissions)) {
    diagnostics.push(
      diagnostic('error', 'invalid-permissions', 'permissions must be an array'),
    )
  } else if (manifest.permissions) {
    if (manifest.permissions.length > contract.limits.tappPermissions) {
      diagnostics.push(
        diagnostic('error', 'invalid-permissions', `permissions accepts at most ${contract.limits.tappPermissions} entries`),
      )
    }
    const seen = new Set()
    for (const permission of manifest.permissions) {
      if (typeof permission !== 'string' || !(permission in catalog.permissionLevels)) {
        diagnostics.push(
          diagnostic('error', 'unknown-permission', `Unknown permission: ${String(permission)}`),
        )
      } else if (seen.has(permission)) {
        diagnostics.push(
          diagnostic('error', 'duplicate-permission', `Duplicate permission: ${permission}`),
        )
      }
      seen.add(permission)
    }
  }

  if (manifest.author !== undefined) {
    validateFields(manifest.author, schemaFields('TappAuthor'), 'author', diagnostics)
    if (!isObject(manifest.author) || typeof manifest.author.name !== 'string' || !manifest.author.name.trim() || manifest.author.name.length > contract.limits.tappNameLength) {
      diagnostics.push(
        diagnostic('error', 'invalid-author', 'author must contain a string name'),
      )
    }
    if (manifest.author?.email !== undefined && (typeof manifest.author.email !== 'string' || manifest.author.email.length > contract.limits.authorEmailLength || /\s/.test(manifest.author.email) || !manifest.author.email.includes('@'))) {
      diagnostics.push(diagnostic('error', 'invalid-author', 'author.email is invalid'))
    }
    if (manifest.author?.url !== undefined && !validateHttpUrl(manifest.author.url)) {
      diagnostics.push(diagnostic('error', 'invalid-author', 'author.url must be an HTTP(S) URL'))
    }
  }
  if (
    manifest.themeColor !== undefined &&
    (typeof manifest.themeColor !== 'string' || !THEME_COLOR.test(manifest.themeColor))
  ) {
    diagnostics.push(
      diagnostic('error', 'invalid-theme-color', 'themeColor must use #RRGGBB'),
    )
  }
  for (const field of URL_FIELDS) {
    if (manifest[field] === undefined) continue
    try {
      if (!validateHttpUrl(manifest[field])) throw new Error('invalid URL')
    } catch {
      diagnostics.push(
        diagnostic('error', 'invalid-url', `${field} must be an HTTP(S) URL`),
      )
    }
  }

  if (manifest.locales !== undefined) {
    if (!isObject(manifest.locales)) {
      diagnostics.push(diagnostic('error', 'invalid-locales', 'locales must be an object'))
    } else {
      const locales = Object.entries(manifest.locales)
      if (locales.length > contract.limits.tappLocales) {
        diagnostics.push(
          diagnostic('error', 'invalid-locales', `locales accepts at most ${contract.limits.tappLocales} languages`),
        )
      }
      for (const [locale, entry] of locales) {
        if (locale.length > contract.limits.localeTagLength || !LOCALE_TAG.test(locale)) {
          diagnostics.push(
            diagnostic('error', 'invalid-locale', `Invalid BCP-47 locale: ${locale}`),
          )
        }
        if (!isObject(entry)) {
          diagnostics.push(
            diagnostic('error', 'invalid-locale', `locales.${locale} must be an object`),
          )
          continue
        }
        validateFields(entry, schemaFields('TappManifestLocaleEntry'), `locales.${locale}`, diagnostics)
        if (
          entry.name !== undefined &&
          (typeof entry.name !== 'string' || !entry.name.trim() || entry.name.length > contract.limits.tappNameLength)
        ) {
          diagnostics.push(
            diagnostic('error', 'invalid-locale', `locales.${locale}.name is invalid`),
          )
        }
        if (
          entry.description !== undefined &&
          (typeof entry.description !== 'string' || entry.description.length > contract.limits.tappDescriptionLength)
        ) {
          diagnostics.push(
            diagnostic('error', 'invalid-locale', `locales.${locale}.description is invalid`),
          )
        }
      }
    }
  }

  if (manifest.cssMode !== undefined && !CSS_MODES.has(manifest.cssMode)) {
    diagnostics.push(
      diagnostic('error', 'invalid-css-mode', 'cssMode must be unified or separated'),
    )
  }
  if (manifest.hasPage !== undefined && typeof manifest.hasPage !== 'boolean') {
    diagnostics.push(diagnostic('error', 'invalid-has-page', 'hasPage must be boolean'))
  }
  if (manifest.backgroundRequirements !== undefined && (
    !Array.isArray(manifest.backgroundRequirements) ||
    manifest.backgroundRequirements.length > contract.limits.backgroundRequirements ||
    manifest.backgroundRequirements.some((value) => !BACKGROUND_REQUIREMENTS.has(value)) ||
    hasDuplicates(manifest.backgroundRequirements)
  )) {
    diagnostics.push(
      diagnostic(
        'error',
        'invalid-background-requirements',
        'backgroundRequirements contains an unknown value',
      ),
    )
  }

  if (manifest.settings !== undefined) validateSettings(manifest.settings, 'settings', diagnostics)

  if (manifest.pageModules !== undefined) {
    if (
      !Array.isArray(manifest.pageModules) ||
      manifest.pageModules.length > contract.limits.pageModules ||
      manifest.pageModules.some((module) => typeof module !== 'string' || module.length > contract.limits.tappIdLength || !SAFE_COMPONENT.test(module) || !module.endsWith(contract.rules.resourceExtensions.pageModule)) ||
      hasDuplicates(manifest.pageModules)
    ) {
      diagnostics.push(diagnostic('error', 'invalid-page-modules', `pageModules must contain at most ${contract.limits.pageModules} unique ${contract.rules.resourceExtensions.pageModule} filenames`))
    }
  }

  if (manifest.assets !== undefined && (
    !Array.isArray(manifest.assets) ||
    manifest.assets.some((asset) => typeof asset !== 'string') ||
    hasDuplicates(manifest.assets)
  )) {
    diagnostics.push(diagnostic('error', 'invalid-assets', 'assets must contain unique resource paths'))
  }

  if (manifest.widgets !== undefined && !Array.isArray(manifest.widgets)) {
    diagnostics.push(diagnostic('error', 'invalid-widgets', 'widgets must be an array'))
  } else if (manifest.widgets?.length) {
    if (manifest.widgets.length > contract.limits.widgets) {
      diagnostics.push(diagnostic('error', 'invalid-widgets', `widgets accepts at most ${contract.limits.widgets} entries`))
    }
    const ids = new Set()
    for (const [widgetIndex, widget] of manifest.widgets.entries()) {
      validateFields(widget, schemaFields('TappWidgetDef'), `widgets[${widgetIndex}]`, diagnostics)
      if (!isObject(widget) || typeof widget.id !== 'string' || widget.id.length > contract.limits.tappIdLength || !SAFE_COMPONENT.test(widget.id)) {
        diagnostics.push(diagnostic('error', 'invalid-widget', 'Widget id is invalid'))
        continue
      }
      if (ids.has(widget.id)) {
        diagnostics.push(
          diagnostic('error', 'duplicate-widget', `Duplicate Widget id: ${widget.id}`),
        )
      }
      ids.add(widget.id)
      if (typeof widget.name !== 'string' || !widget.name) {
        diagnostics.push(
          diagnostic('error', 'invalid-widget', `Widget ${widget.id} requires a name`),
        )
      }
      if (widget.name?.length > contract.limits.tappNameLength) {
        diagnostics.push(diagnostic('error', 'invalid-widget', `Widget ${widget.id} name exceeds ${contract.limits.tappNameLength} characters`))
      }
      if (widget.category !== undefined && !WIDGET_CATEGORIES.has(widget.category)) {
        diagnostics.push(diagnostic('error', 'invalid-widget-category', `Widget ${widget.id} category is invalid`))
      }
      if (
        !Array.isArray(widget.sizes) ||
        widget.sizes.length === 0 ||
        widget.sizes.length > contract.limits.widgetSizes ||
        widget.sizes.some((size) => !WIDGET_SIZES.has(size)) ||
        !widget.sizes.includes(widget.defaultSize)
      ) {
        diagnostics.push(
          diagnostic('error', 'invalid-widget-sizes', `Widget ${widget.id} sizes are invalid`),
        )
      }
      if (widget.templates !== undefined && !isObject(widget.templates)) {
        diagnostics.push(
          diagnostic('error', 'invalid-widget-templates', `Widget ${widget.id} templates are invalid`),
        )
      } else if (widget.templates) {
        for (const [size, path] of Object.entries(widget.templates)) {
          if (!WIDGET_SIZES.has(size) || !widget.sizes?.includes(size) || !validateResourcePath(path) || extname(path) !== contract.rules.resourceExtensions.widgetTemplate) {
            diagnostics.push(diagnostic('error', 'invalid-widget-template', `Widget ${widget.id} template ${size} is invalid`))
          }
        }
      }
      if (widget.settings !== undefined) {
        validateSettings(widget.settings, `widgets[${widgetIndex}].settings`, diagnostics)
      }
      if (widget.refreshPolicy !== undefined) {
        const path = `widgets[${widgetIndex}].refreshPolicy`
        if (!validateFields(widget.refreshPolicy, schemaFields('TappWidgetRefreshPolicy'), path, diagnostics)) {
          diagnostics.push(diagnostic('error', 'invalid-widget-refresh', `${path} must be an object`))
        } else if (
          !WIDGET_REFRESH_MODES.has(widget.refreshPolicy.mode) ||
          (widget.refreshPolicy.refreshOnVisible !== undefined && typeof widget.refreshPolicy.refreshOnVisible !== 'boolean') ||
          (widget.refreshPolicy.mode === WIDGET_REFRESH_EVENT_MODE && widget.refreshPolicy.intervalSeconds !== undefined) ||
          (widget.refreshPolicy.mode === WIDGET_REFRESH_INTERVAL_MODE && (!Number.isInteger(widget.refreshPolicy.intervalSeconds) || widget.refreshPolicy.intervalSeconds < contract.limits.widgetRefreshIntervalMinSeconds || widget.refreshPolicy.intervalSeconds > contract.limits.widgetRefreshIntervalMaxSeconds))
        ) {
          diagnostics.push(diagnostic('error', 'invalid-widget-refresh', `Widget ${widget.id} refreshPolicy is invalid`))
        }
      }
    }
  }

  if (manifest.apis !== undefined && !isObject(manifest.apis)) {
    diagnostics.push(diagnostic('error', 'invalid-apis', 'apis must be an object'))
  } else if (manifest.apis) {
    if (Object.keys(manifest.apis).length > contract.limits.tappApis) {
      diagnostics.push(diagnostic('error', 'invalid-apis', `apis accepts at most ${contract.limits.tappApis} entries`))
    }
    for (const [name, definition] of Object.entries(manifest.apis)) {
      if (!isNamedValue(name) || !isObject(definition)) {
        diagnostics.push(diagnostic('error', 'invalid-api', `API declaration ${name} is invalid`))
        continue
      }
      validateFields(definition, schemaFields('TappApiDef'), `apis.${name}`, diagnostics)
      if (definition.access !== undefined && !API_ACCESS_LEVELS.has(definition.access)) {
        diagnostics.push(diagnostic('error', 'invalid-api', `API ${name} access is invalid`))
      }
      const method = definition.method || DEFAULT_HTTP_METHOD
      if (typeof method !== 'string' || method.length > contract.limits.apiMethodLength || !HTTP_METHOD.test(method)) {
        diagnostics.push(diagnostic('error', 'invalid-api', `API ${name} HTTP method is invalid`))
      }
      if (definition.inject !== undefined && !isObject(definition.inject)) {
        diagnostics.push(diagnostic('error', 'invalid-api', `API ${name} inject must be an object`))
      } else if (definition.inject) {
        if (Object.keys(definition.inject).length > contract.limits.apiInjectAliases) {
          diagnostics.push(diagnostic('error', 'invalid-api', `API ${name} inject accepts at most ${contract.limits.apiInjectAliases} aliases`))
        }
        for (const [alias, template] of Object.entries(definition.inject)) {
          if (!isNamedValue(alias) || contract.rules.apiInjectReservedPrefixes.some((prefix) => alias.startsWith(prefix)) || typeof template !== 'string' || !template || template.length > contract.limits.apiInjectTemplateLength) {
            diagnostics.push(diagnostic('error', 'invalid-api', `API ${name} inject alias ${alias} is invalid`))
          }
        }
      }
      if (definition.cacheTtl !== undefined && (!Number.isInteger(definition.cacheTtl) || definition.cacheTtl < 0 || definition.cacheTtl > contract.limits.apiCacheTtlSeconds)) {
        diagnostics.push(diagnostic('error', 'invalid-api', `API ${name} cacheTtl must be between 0 and ${contract.limits.apiCacheTtlSeconds}`))
      }
      if (JSON.stringify(definition).includes('{{secrets.')) {
        diagnostics.push(diagnostic('error', 'invalid-api', `API ${name} cannot reference host secret templates`))
      }
      const type = definition.type || DEFAULT_API_TYPE
      if (!API_TYPES.has(type)) {
        diagnostics.push(diagnostic('error', 'invalid-api', `API ${name} type must be one of: ${contract.rules.apiTypes.join(', ')}`))
        continue
      }
      if (type === HTTP_API_TYPE) {
        addRequiredPermission(
          requiredPermissions,
          contract.rules.httpApiPermission,
          `manifest.apis.${name} uses HTTP`,
        )
        if (typeof definition.endpoint !== 'string' || !definition.endpoint) {
          diagnostics.push(
            diagnostic('error', 'invalid-api', `HTTP API ${name} requires endpoint`),
          )
        }
        if (definition.builtin !== undefined) {
          diagnostics.push(diagnostic('error', 'invalid-api', `HTTP API ${name} cannot declare builtin`))
        }
      } else if (type === BUILTIN_API_TYPE) {
        if (!API_BUILTINS.has(definition.builtin)) {
          diagnostics.push(
            diagnostic('error', 'invalid-api', `Builtin API ${name} has an unknown builtin`),
          )
        }
        const operation = API_BUILTIN_AI_OPERATIONS[definition.builtin]
        if (operation) {
          addRequiredPermission(
            requiredPermissions,
            API_BUILTIN_PERMISSIONS[definition.builtin],
            `manifest.apis.${name} uses ${definition.builtin}`,
          )
          if (manifest.ai?.protocolVersion !== contract.rules.protocolVersion || !manifest.ai?.operations?.includes(operation) || !manifest.ai?.outputFormats?.includes(contract.rules.aiBuiltinOutputFormat)) {
            diagnostics.push(diagnostic('error', 'invalid-api', `Builtin API ${name} requires matching protocolVersion ${contract.rules.protocolVersion} AI operation and ${contract.rules.aiBuiltinOutputFormat} output`))
          }
        }
        if (contract.rules.httpOnlyApiFields.some((field) => definition[field] !== undefined)) {
          diagnostics.push(diagnostic('error', 'invalid-api', `Builtin API ${name} contains HTTP-only fields`))
        }
      } else {
        diagnostics.push(
          diagnostic('error', 'invalid-api', `API ${name} type must be one of: ${contract.rules.apiTypes.join(', ')}`),
        )
      }
    }
  }

  if (manifest.dataExchange !== undefined) validateDataExchange(manifest.dataExchange, diagnostics)
  if (manifest.ai !== undefined) validateAi(manifest.ai, diagnostics, requiredPermissions)
  if (manifest.events !== undefined) validateEvents(manifest.events, manifest, diagnostics, requiredPermissions)
  if (manifest.agent !== undefined) validateAgent(manifest.agent, diagnostics)
}

function scriptKind(file) {
  if (file.endsWith('.tsx')) return ts.ScriptKind.TSX
  if (file.endsWith('.jsx')) return ts.ScriptKind.JSX
  if (file.endsWith('.ts')) return ts.ScriptKind.TS
  return ts.ScriptKind.JS
}

function unwrapExpression(node) {
  while (
    ts.isParenthesizedExpression(node) ||
    ts.isAsExpression(node) ||
    ts.isTypeAssertionExpression(node) ||
    ts.isNonNullExpression(node) ||
    ts.isSatisfiesExpression(node)
  ) {
    node = node.expression
  }
  return node
}

function staticMemberName(node) {
  if (ts.isPropertyAccessExpression(node)) return node.name.text
  if (ts.isElementAccessExpression(node)) {
    const argument = unwrapExpression(node.argumentExpression)
    if (ts.isStringLiteralLike(argument)) return argument.text
  }
  return undefined
}

function tappCallPath(expression) {
  const path = []
  let current = unwrapExpression(expression)
  while (ts.isPropertyAccessExpression(current) || ts.isElementAccessExpression(current)) {
    const name = staticMemberName(current)
    if (name === undefined) return undefined
    path.unshift(name)
    current = unwrapExpression(current.expression)
  }
  return ts.isIdentifier(current) && current.text === 'Tapp' ? path : undefined
}

function literalString(node) {
  const value = node && unwrapExpression(node)
  return value && ts.isStringLiteralLike(value) ? value.text : undefined
}

function inspectCode(source, file, manifest, diagnostics, requiredPermissions, usedActions, surfaces) {
  const sourceFile = ts.createSourceFile(
    file,
    source,
    ts.ScriptTarget.Latest,
    true,
    scriptKind(file),
  )

  for (const parseDiagnostic of sourceFile.parseDiagnostics) {
    const location = sourceFile.getLineAndCharacterOfPosition(parseDiagnostic.start || 0)
    diagnostics.push(
      diagnostic(
        'error',
        'invalid-source-syntax',
        ts.flattenDiagnosticMessageText(parseDiagnostic.messageText, '\n'),
        file,
        location.line + 1,
        location.character + 1,
      ),
    )
  }

  function visit(node) {
    if (!ts.isCallExpression(node)) {
      ts.forEachChild(node, visit)
      return
    }

    const path = tappCallPath(node.expression)
    if (!path) {
      ts.forEachChild(node, visit)
      return
    }
    const position = sourceFile.getLineAndCharacterOfPosition(node.getStart(sourceFile))
    const location = { line: position.line + 1, column: position.character + 1 }

    if (path.length === 2) {
      const action = path.join('.')
      const permission = catalog.actions[action]
      if (permission) {
        usedActions.push({ action, permission, file, ...location })
        addRequiredPermission(requiredPermissions, permission, `code calls ${action}`, file)
      }
      if (HEADLESS_DENIED_ACTIONS.has(action)) {
        if (surfaces?.headlessOnly) {
          diagnostics.push(
            diagnostic(
              'error',
              'headless-denied-action',
              `${action} is unavailable in headless-only Tapps`,
              file,
              location.line,
              location.column,
            ),
          )
        } else if (surfaces?.headless) {
          diagnostics.push(
            diagnostic(
              'warning',
              'headless-unavailable-action',
              `${action} is unavailable in headless core; keep it in Page/Widget code paths`,
              file,
              location.line,
              location.column,
            ),
          )
        }
      }
    }

    if (path.length === 1 && path[0] === 'api') {
      const name = literalString(node.arguments[0])
      if (name === undefined) {
        diagnostics.push(
          diagnostic(
            'warning',
            'dynamic-api-name',
            'A dynamic Tapp.api name cannot be checked statically',
            file,
            location.line,
            location.column,
          ),
        )
      } else if (!manifest.apis || !Object.hasOwn(manifest.apis, name)) {
        diagnostics.push(
          diagnostic(
            'error',
            'undeclared-api',
            `Tapp.api('${name}') is not declared in manifest.apis`,
            file,
            location.line,
            location.column,
          ),
        )
      }
    }

    if (path.length === 2 && path[0] === 'assets' && ASSET_LITERAL_METHODS.has(path[1])) {
      const assetPath = literalString(node.arguments[0])
      if (assetPath !== undefined) {
        const normalized = assetPath.startsWith(`${ASSET_DIRECTORY}/`)
          ? assetPath
          : `${ASSET_DIRECTORY}/${assetPath}`
        const declaredAssets = new Set(arrayValue(manifest.assets))
        if (!declaredAssets.has(normalized)) {
          diagnostics.push(
            diagnostic(
              'error',
              'undeclared-asset',
              `${normalized} is not declared in manifest.assets`,
              file,
              location.line,
              location.column,
            ),
          )
        }
      }
    }

    ts.forEachChild(node, visit)
  }

  visit(sourceFile)
}

async function pathExists(path) {
  try {
    await access(path)
    return true
  } catch {
    return false
  }
}

async function walkCodeFiles(root, current = root) {
  const files = []
  for (const entry of await readdir(current, { withFileTypes: true })) {
    if (entry.isDirectory() && (SKIP_DIRECTORIES.has(entry.name) || entry.name === 'types')) {
      continue
    }
    const absolute = join(current, entry.name)
    if (entry.isDirectory()) files.push(...(await walkCodeFiles(root, absolute)))
    else if (
      entry.isFile() &&
      CODE_EXTENSIONS.has(extname(entry.name)) &&
      !entry.name.endsWith('.d.ts')
    ) {
      files.push(absolute)
    }
  }
  return files
}

function resourceDeclarations(manifest) {
  const declarations = new Map()
  const add = (path, kind, extension) => {
    if (typeof path === 'string' && path) declarations.set(path, { kind, extension })
  }
  for (const [field, extensionKey] of Object.entries(MANIFEST_RESOURCE_FIELDS)) {
    add(manifest[field], field, contract.rules.resourceExtensions[extensionKey])
  }
  for (const module of arrayValue(manifest.pageModules)) add(`${PAGE_MODULE_DIRECTORY}/${module}`, 'pageModule', contract.rules.resourceExtensions.pageModule)
  for (const asset of arrayValue(manifest.assets)) add(asset, 'asset')
  for (const widget of arrayValue(manifest.widgets)) {
    if (!isObject(widget)) continue
    for (const path of Object.values(widget.templates || {})) {
      add(path, `widgetTemplate:${widget.id}`, contract.rules.resourceExtensions.widgetTemplate)
    }
  }
  for (const interaction of arrayValue(manifest.agent?.interactions)) {
    for (const field of AGENT_SCHEMA_FIELDS) {
      add(interaction?.[field], 'agentSchema', contract.rules.resourceExtensions.agentSchema)
    }
  }
  return declarations
}

async function validateResources(root, manifest, diagnostics) {
  const declarations = resourceDeclarations(manifest)
  const packagePaths = new Set(['manifest.json'])
  let assetBytes = 0

  for (const [path, metadata] of declarations) {
    if (!validateResourcePath(path)) {
      diagnostics.push(
        diagnostic('error', 'invalid-resource-path', `Invalid ${metadata.kind} path: ${path}`),
      )
      continue
    }
    if (metadata.extension && extname(path) !== metadata.extension) {
      diagnostics.push(
        diagnostic(
          'error',
          'invalid-resource-extension',
          `${metadata.kind} must use ${metadata.extension}: ${path}`,
        ),
      )
    }
    const absolute = resolve(root, path)
    if (!absolute.startsWith(`${root}${sep}`) || !(await pathExists(absolute))) {
      diagnostics.push(
        diagnostic('error', 'missing-resource', `Declared resource not found: ${path}`),
      )
      continue
    }
    const info = await lstat(absolute)
    if (!info.isFile()) {
      diagnostics.push(diagnostic('error', 'invalid-resource', `Resource is not a file: ${path}`))
      continue
    }
    packagePaths.add(path)
    if (metadata.kind === 'asset') {
      assetBytes += info.size
      if (!path.startsWith(`${ASSET_DIRECTORY}/`) || contract.rules.assetForbiddenExtensions.includes(extname(path))) {
        diagnostics.push(
          diagnostic('error', 'invalid-asset', `Asset must be under ${ASSET_DIRECTORY}/ and cannot be JS/HTML: ${path}`),
        )
      }
      if (info.size > MAX_ASSET_BYTES) {
        diagnostics.push(
          diagnostic('error', 'asset-too-large', `Asset exceeds ${formatBytes(MAX_ASSET_BYTES)}: ${path}`),
        )
      }
    }
    if (metadata.kind === 'agentSchema') {
      if (info.size > MAX_AGENT_SCHEMA_BYTES) {
        diagnostics.push(diagnostic('error', 'agent-schema-too-large', `${path} exceeds ${formatBytes(MAX_AGENT_SCHEMA_BYTES)}`))
      }
      try {
        const schema = JSON.parse(await readFile(absolute, 'utf8'))
        if (!validateInlineSchema(schema)) throw new Error('unsupported schema')
      } catch {
        diagnostics.push(diagnostic('error', 'invalid-agent-schema', `${path} is not a supported inline JSON Schema`))
      }
    }
  }

  if (arrayValue(manifest.assets).length > contract.limits.assets) {
    diagnostics.push(diagnostic('error', 'too-many-assets', `assets accepts at most ${contract.limits.assets} entries`))
  }
  if (assetBytes > MAX_ASSETS_BYTES) {
    diagnostics.push(diagnostic('error', 'assets-too-large', `Declared assets exceed ${formatBytes(MAX_ASSETS_BYTES)} total`))
  }

  for (const [directory, extension] of Object.entries(PACKAGE_RESOURCE_EXTENSIONS)) {
    const absoluteDirectory = join(root, directory)
    if (!(await pathExists(absoluteDirectory))) continue
    const directoryEntries = await readdir(absoluteDirectory, { withFileTypes: true })
    const fileLimitKey = PACKAGE_RESOURCE_FILE_LIMITS[directory]
    if (fileLimitKey && directoryEntries.length > contract.limits[fileLimitKey]) {
      diagnostics.push(diagnostic('error', `too-many-${directory}-files`, `${directory} accepts at most ${contract.limits[fileLimitKey]} files`))
    }
    for (const entry of directoryEntries) {
      if (PACKAGE_JSON_OBJECT_DIRECTORIES.has(directory) && (!entry.isFile() || extname(entry.name) !== extension)) {
        diagnostics.push(diagnostic('error', `invalid-${directory}`, `${directory}/${entry.name} must be a regular JSON file`))
        continue
      }
      if (!entry.isFile()) continue
      const path = `${directory}/${entry.name}`
      if (!validateResourcePath(path)) {
        diagnostics.push(diagnostic('error', 'invalid-resource-path', `Invalid path: ${path}`))
        continue
      }
      const byteLimitKey = PACKAGE_RESOURCE_BYTE_LIMITS[directory]
      if (byteLimitKey) {
        const info = await stat(join(absoluteDirectory, entry.name))
        if (info.size > contract.limits[byteLimitKey]) {
          diagnostics.push(diagnostic('error', `${directory}-too-large`, `${path} exceeds ${contract.limits[byteLimitKey]} bytes`))
        }
        if (PACKAGE_JSON_OBJECT_DIRECTORIES.has(directory)) {
          try {
            const value = JSON.parse(await readFile(join(absoluteDirectory, entry.name), 'utf8'))
            if (!isObject(value)) throw new Error('resource must be an object')
          } catch {
            diagnostics.push(diagnostic('error', `invalid-${directory}`, `${path} must contain a JSON object`))
          }
        }
      }
      if (!PACKAGE_JSON_OBJECT_DIRECTORIES.has(directory) && extname(entry.name) !== extension) continue
      packagePaths.add(path)
    }
  }
  return [...packagePaths].sort()
}

export async function inspectProject(projectRoot = '.') {
  const root = resolve(projectRoot)
  const diagnostics = []
  const manifestPath = join(root, 'manifest.json')
  let manifest

  try {
    manifest = JSON.parse(await readFile(manifestPath, 'utf8'))
  } catch (error) {
    diagnostics.push(
      diagnostic(
        'error',
        'invalid-manifest',
        error.code === 'ENOENT' ? 'manifest.json was not found' : 'manifest.json is not valid JSON',
      ),
    )
    return {
      root,
      manifest: null,
      diagnostics,
      surfaces: { page: false, widget: false, headless: false, headlessOnly: false },
      permissions: { declared: [], required: [], missing: [], usedActions: [] },
      packageFiles: [],
    }
  }

  if (!isObject(manifest)) {
    diagnostics.push(diagnostic('error', 'invalid-manifest', 'manifest.json must contain an object'))
    return {
      root,
      manifest: null,
      diagnostics,
      surfaces: { page: false, widget: false, headless: false, headlessOnly: false },
      permissions: { declared: [], required: [], missing: [], usedActions: [] },
      packageFiles: [],
    }
  }

  const requiredPermissions = new Map()
  validateManifest(manifest, diagnostics, requiredPermissions)
  const surfaces = validateModeConsistency(manifest, diagnostics, requiredPermissions)
  const packageFiles = await validateResources(root, manifest, diagnostics)
  const usedActions = []

  for (const absolute of await walkCodeFiles(root)) {
    const file = portablePath(relative(root, absolute))
    inspectCode(
      await readFile(absolute, 'utf8'),
      file,
      manifest,
      diagnostics,
      requiredPermissions,
      usedActions,
      surfaces,
    )
  }

  const declared = Array.isArray(manifest.permissions) ? manifest.permissions : []
  const missing = [...requiredPermissions.values()].filter(
    ({ permission }) => !declared.includes(permission),
  )
  for (const entry of missing) {
    diagnostics.push(
      diagnostic(
        'error',
        'missing-permission',
        `Missing permission ${entry.permission}: ${entry.reasons.join('; ')}`,
      ),
    )
  }

  return {
    root,
    manifest,
    diagnostics,
    surfaces,
    permissions: {
      declared: declared.map((permission) => ({
        permission,
        level: catalog.permissionLevels[permission],
      })),
      required: [...requiredPermissions.values()].map((entry) => ({
        ...entry,
        level: catalog.permissionLevels[entry.permission],
      })),
      missing,
      usedActions,
    },
    packageFiles,
  }
}

function starterId(directory) {
  const slug = basename(resolve(directory))
    .toLowerCase()
    .replace(/[^a-z0-9._-]+/g, '-')
    .replace(/^[^a-z0-9]+/, '')
    .replace(/[^a-z0-9]+$/, '')
  return `com.example.${slug || 'my-tapp'}`
}

function starterName(directory) {
  return basename(resolve(directory))
    .split(/[-_.\s]+/)
    .filter(Boolean)
    .map((part) => part[0].toUpperCase() + part.slice(1))
    .join(' ') || 'My Tapp'
}

function pageSource() {
  return `// ========== Page Code ==========
Tapp.lifecycle.onReady(async function () {
  var root = document.getElementById('tapp-root');
  root.querySelector('[data-action="notify"]').addEventListener('click', function () {
    Tapp.ui.showNotification({
      title: 'My Tapp',
      message: 'The Page is running.',
      type: 'success'
    });
  });
});
`
}

function widgetSource() {
  return `// ========== Widget Code ==========
Tapp.widgets['starter'] = {
  render: function (container, props) {
    container.innerHTML = '<section class="widget"><strong>My Tapp</strong><span>' + props.size + '</span></section>';
  }
};
`
}

export async function createProject(directory, options = {}) {
  const root = resolve(directory)
  const type = options.type || 'page'
  if (!['page', 'widget', 'both'].includes(type)) {
    throw new Error('type must be page, widget, or both')
  }
  if (await pathExists(root)) {
    const existing = await readdir(root)
    if (existing.length > 0 && !options.force) {
      throw new Error(`Target directory is not empty: ${root}`)
    }
  }
  await mkdir(root, { recursive: true })

  const hasPage = type === 'page' || type === 'both'
  const hasWidget = type === 'widget' || type === 'both'
  const manifest = {
    id: options.id || starterId(root),
    name: options.name || starterName(root),
    version: '0.1.0',
    description: 'A Myriad Tapp',
    category: 'utility',
    main: 'main.js',
    author: { name: options.author || 'Tapp Developer' },
    permissions: [
      ...(hasPage ? ['ui:notification'] : []),
      ...(hasWidget ? ['widget:register'] : []),
    ],
    ...(hasPage ? { hasPage: true, pageTemplate: 'page.html' } : {}),
    ...(hasWidget
      ? {
          widgets: [
            {
              id: 'starter',
              name: 'Starter Widget',
              defaultSize: '2x2',
              sizes: ['2x2', '4x2'],
              templates: {
                '2x2': 'templates/widget-2x2.html',
                '4x2': 'templates/widget-4x2.html',
              },
            },
          ],
        }
      : {}),
    styles: 'styles.css',
    cssMode: 'unified',
  }

  const source = `${hasWidget ? widgetSource() : ''}${
    hasWidget && hasPage ? '\n' : ''
  }${hasPage ? pageSource() : ''}`
  await writeFile(join(root, 'manifest.json'), `${JSON.stringify(manifest, null, 2)}\n`)
  const typedSource = `/// <reference path="./types/tapp-sdk.d.ts" />\n${source}`
  await writeFile(join(root, 'main.js'), typedSource)
  await mkdir(join(root, 'types'), { recursive: true })
  const sdkDts = await readFile(
    new URL('./generated/tapp-sdk.d.ts', import.meta.url),
    'utf8',
  )
  await writeFile(join(root, 'types/tapp-sdk.d.ts'), sdkDts)
  await writeFile(
    join(root, 'jsconfig.json'),
    `${JSON.stringify(
      {
        compilerOptions: {
          checkJs: true,
          noEmit: true,
          lib: ['ES2022', 'DOM'],
          maxNodeModuleJsDepth: 0,
        },
        include: ['*.js', '**/*.js', 'types/**/*.d.ts'],
      },
      null,
      2,
    )}\n`,
  )
  await writeFile(
    join(root, 'styles.css'),
    `:root { color-scheme: light dark; }
body { margin: 0; font: 14px/1.5 system-ui, sans-serif; }
.page, .widget { box-sizing: border-box; display: grid; gap: 12px; padding: 16px; }
.widget { height: 100%; align-content: center; }
button { width: fit-content; padding: 8px 12px; }
`,
  )
  if (hasPage) {
    await writeFile(
      join(root, 'page.html'),
      `<main id="tapp-root" class="page">
  <h1>${manifest.name}</h1>
  <button type="button" data-action="notify">Show notification</button>
</main>
`,
    )
  }
  if (hasWidget) {
    await mkdir(join(root, 'templates'), { recursive: true })
    const template = `<div data-widget-root="true" class="widget"></div>\n`
    await writeFile(join(root, 'templates/widget-2x2.html'), template)
    await writeFile(join(root, 'templates/widget-4x2.html'), template)
  }
  return { root, type, manifest }
}

export async function packProject(projectRoot = '.', outputPath) {
  const report = await inspectProject(projectRoot)
  if (report.diagnostics.some(({ severity }) => severity === 'error')) {
    const error = new Error('Project validation failed')
    error.report = report
    throw error
  }

  const entries = []
  let uncompressedBytes = 0
  // Pack only installable package files (not local editor helpers like types/).
  const packagePaths = new Set(
    expectedPackagePaths({
      manifest: report.manifest,
      extraPaths: report.packageFiles,
    }),
  )
  // Drop non-package editor scaffolding if it slipped into the walk.
  packagePaths.delete('jsconfig.json')
  for (const path of [...packagePaths]) {
    if (path.startsWith('types/')) packagePaths.delete(path)
  }

  const normalizedManifest = normalizeManifestPaths(report.manifest)
  for (const path of [...packagePaths].sort()) {
    let data
    if (path === 'manifest.json') {
      data = Buffer.from(`${JSON.stringify(normalizedManifest, null, 2)}\n`)
    } else {
      const absolute = join(report.root, path)
      if (!(await pathExists(absolute))) continue
      data = await readFile(absolute)
    }
    uncompressedBytes += data.length
    entries.push({ path, data })
  }
  if (entries.length > MAX_ARCHIVE_FILES) throw new Error(`Package exceeds ${MAX_ARCHIVE_FILES} entries`)
  if (uncompressedBytes > MAX_UNCOMPRESSED_BYTES) {
    throw new Error(`Package exceeds ${formatBytes(MAX_UNCOMPRESSED_BYTES)} uncompressed`)
  }

  const target = resolve(
    outputPath || join(report.root, 'dist', `${report.manifest.id}.tapp`),
  )
  if (zipSize(entries) > MAX_ARCHIVE_BYTES) {
    throw new Error(`Package exceeds ${formatBytes(MAX_ARCHIVE_BYTES)}`)
  }
  const result = await writeZip(target, entries)
  return { ...result, report }
}

export function permissionCatalog() {
  return catalog
}

export function generatedContract() {
  return contract
}

export { expectedPackagePaths, normalizeManifestPaths, PACKAGE_MARKERS }
