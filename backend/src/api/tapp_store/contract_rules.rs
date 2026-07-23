#![allow(dead_code)]

pub const MAX_TAPP_ID_LEN: usize = 128;
pub const MAX_RESOURCE_PATH_LEN: usize = 256;
pub const MAX_TAPP_ARCHIVE_BYTES: usize = 25 * 1024 * 1024;
pub const MAX_TAPP_ARCHIVE_FILES: usize = 512;
pub const MAX_TAPP_ARCHIVE_UNCOMPRESSED_BYTES: u64 = 100 * 1024 * 1024;
pub const MAX_TAPP_RESOURCE_BYTES: u64 = 25 * 1024 * 1024;
pub const MAX_TAPP_ASSETS: usize = 64;
pub const MAX_TAPP_ASSET_BYTES: u64 = 5 * 1024 * 1024;
pub const MAX_TAPP_ASSETS_TOTAL_BYTES: u64 = 20 * 1024 * 1024;
pub const MAX_TAPP_MANIFEST_BYTES: u64 = 256 * 1024;
pub const MAX_WIDGETS_PER_TAPP: usize = 64;
pub const MAX_DATA_EXCHANGE_DECLARATIONS: usize = 32;
pub const MAX_DATA_EXCHANGE_ID_LEN: usize = 128;
pub const MAX_DATA_EXCHANGE_SCHEMA_BYTES: usize = 64 * 1024;
pub const MAX_DATA_EXCHANGE_RESPONSE_BYTES: usize = 512 * 1024;
pub const MAX_AGENT_SCHEMA_RESOURCE_BYTES: usize = 64 * 1024;
pub const MAX_TAPP_I18N_FILES: usize = 32;
pub const MAX_TAPP_I18N_RESOURCE_BYTES: usize = 1024 * 1024;
pub const MAX_TAPP_NAME_LEN: usize = 255;
pub const MAX_TAPP_DESCRIPTION_LEN: usize = 2_000;
pub const MAX_TAPP_LOCALES: usize = 32;
pub const MAX_LOCALE_TAG_LEN: usize = 35;
pub const MAX_TAPP_ICON_LEN: usize = 2_048;
pub const MAX_TAPP_ICON_SVG_BYTES: usize = 65_536;
pub const MAX_HTTP_URL_LEN: usize = 2_048;
pub const MAX_AUTHOR_EMAIL_LEN: usize = 320;
pub const MAX_STORAGE_KEY_LEN: usize = 256;
pub const MAX_TAPP_PERMISSIONS: usize = 64;
pub const MAX_PAGE_MODULES: usize = 64;
pub const MAX_BACKGROUND_REQUIREMENTS: usize = 16;
pub const MAX_TAPP_SETTINGS: usize = 64;
pub const MAX_SETTING_LABEL_LEN: usize = 255;
pub const MAX_SETTING_OPTIONS: usize = 100;
pub const MAX_SETTING_OPTION_VALUE_LEN: usize = 255;
pub const MAX_WIDGET_SIZES: usize = 10;
pub const MAX_TAPP_APIS: usize = 64;
pub const MAX_API_METHOD_LEN: usize = 16;
pub const MAX_API_CACHE_TTL_SECONDS: u32 = 86_400;
pub const MAX_API_INJECT_ALIASES: usize = 32;
pub const MAX_API_INJECT_TEMPLATE_LEN: usize = 2_048;
pub const MAX_DATA_EXCHANGE_DESCRIPTION_LEN: usize = 500;
pub const MAX_DATA_EXCHANGE_RECORDS: usize = 10_000;
pub const MAX_INLINE_SCHEMA_DEPTH: usize = 32;
pub const MAX_AI_OPERATIONS: usize = 4;
pub const MAX_AI_CONTEXT_SOURCES: usize = 4;
pub const MAX_AI_OUTPUT_FORMATS: usize = 3;
pub const MAX_EVENT_TOPICS: usize = 100;
pub const MAX_AGENT_INTERACTIONS: usize = 32;
pub const MAX_AGENT_INTENTS: usize = 16;
pub const MIN_WIDGET_REFRESH_INTERVAL_SECONDS: u32 = 15;
pub const MAX_WIDGET_REFRESH_INTERVAL_SECONDS: u32 = 86_400;
pub const TAPP_PROTOCOL_VERSION: u8 = 2;

pub const WIDGET_SIZES: &[&str] = &[
    "1x1", "1x2", "2x1", "2x2", "2x3", "3x2", "4x1", "4x2", "2x4", "3x3", "4x4",
];
pub const BACKGROUND_REQUIREMENTS: &[&str] = &[
    "media",
    "sync",
    "notification",
    "scheduler",
    "event-listener",
    "realtime",
];
pub const SETTING_TYPES: &[&str] = &["toggle", "select", "input", "number", "color"];
pub const AGENT_INTENTS: &[&str] = &["ui.open", "report.create", "dataExchange.request"];
pub const API_BUILTINS: &[&str] = &["geo", "ai:chat", "ai:generate"];
pub const API_TYPES: &[&str] = &["http", "builtin"];
pub const HTTP_API_TYPE: &str = "http";
pub const BUILTIN_API_TYPE: &str = "builtin";
pub const DEFAULT_API_TYPE: &str = "http";
pub const DEFAULT_HTTP_METHOD: &str = "GET";
pub const CSS_MODES: &[&str] = &["unified", "separated"];
pub const HTTP_URL_SCHEMES: &[&str] = &["http", "https"];
pub const HTTP_METHOD_PATTERN: &str = r"^[A-Za-z!#$%&'*+.^_`|~-]+$";
pub const RESOURCE_EXTENSIONS: &[(&str, &str)] = &[
    ("main", ".js"),
    ("styles", ".css"),
    ("widgetStyles", ".css"),
    ("pageStyles", ".css"),
    ("pageTemplate", ".html"),
    ("pageModule", ".js"),
    ("widgetTemplate", ".html"),
    ("agentSchema", ".json"),
    ("i18n", ".json"),
];
pub const ASSET_FORBIDDEN_EXTENSIONS: &[&str] = &[".js", ".html"];
pub const PACKAGE_RESOURCE_DIRECTORIES: &[&str] = &["i18n", "page", "schemas"];
pub const PACKAGE_RESOURCE_EXTENSIONS: &[(&str, &str)] =
    &[("i18n", ".json"), ("page", ".js"), ("schemas", ".json")];
pub const PACKAGE_JSON_OBJECT_DIRECTORIES: &[&str] = &["i18n"];
pub const PACKAGE_RESOURCE_FILE_LIMITS: &[(&str, &str)] = &[("i18n", "i18nFiles")];
pub const PACKAGE_RESOURCE_BYTE_LIMITS: &[(&str, &str)] = &[("i18n", "i18nResourceBytes")];
pub const ASSET_DIRECTORY: &str = "assets";
pub const PAGE_MODULE_DIRECTORY: &str = "page";
pub const MANIFEST_RESOURCE_FIELDS: &[(&str, &str)] = &[
    ("main", "main"),
    ("styles", "styles"),
    ("widgetStyles", "widgetStyles"),
    ("pageStyles", "pageStyles"),
    ("pageTemplate", "pageTemplate"),
];
pub const AGENT_SCHEMA_FIELDS: &[&str] = &["inputSchema", "resultSchema"];
pub const URL_FIELDS: &[&str] = &["homepage", "repository"];
pub const DATA_EXCHANGE_DIRECTIONS: &[(&str, &str)] =
    &[("exports", "export"), ("imports", "import")];
pub const EVENT_TOPIC_PREFIXES: &[(&str, &[&str])] = &[
    ("publish", &["tapp.{id}."]),
    ("subscribe", &["tapp.", "system."]),
];
pub const TAPP_CATEGORY_ALIASES: &[&str] = &[
    "data-extension",
    "platform",
    "visualization",
    "development",
    "dev",
    "games",
    "entertainment",
    "music",
    "communication",
    "demo",
    "page",
    "test",
    "tool",
    "tools",
    "utilities",
    "widget",
];
pub const WIDGET_CATEGORY_ALIASES: &[&str] = &["tool"];
pub const WIDGET_MANIFEST_PERMISSION: &str = "widget:register";
pub const HTTP_API_PERMISSION: &str = "network:fetch";
pub const EVENT_PERMISSION_RULES: &[(&str, &str)] = &[
    ("publish", "event:publish"),
    ("subscribe", "event:subscribe"),
];
pub const AI_CONTEXT_PERMISSION_RULES: &[(&str, &str)] =
    &[("platform", "platform:read"), ("report", "report:read")];
pub const AI_BUILTIN_OUTPUT_FORMAT: &str = "text";
pub const REQUIRED_MANIFEST_FIELDS: &[&str] = &["category"];
pub const INLINE_SCHEMA_ROOT_KEYS: &[&str] = &["type", "properties", "enum", "const"];
pub const AI_OPERATION_OUTPUT_RULES: &[(&str, &str)] = &[("image", "image")];
pub const API_BUILTIN_AI_OPERATIONS: &[(&str, &str)] =
    &[("ai:chat", "chat"), ("ai:generate", "generate")];
pub const API_BUILTIN_PERMISSIONS: &[(&str, &str)] =
    &[("ai:chat", "ai:chat"), ("ai:generate", "ai:generate")];
pub const HTTP_ONLY_API_FIELDS: &[&str] = &["endpoint", "headers", "body", "spoof", "inject"];
pub const API_INJECT_RESERVED_PREFIXES: &[&str] = &["user.", "geo.", "secrets.", "params."];
pub const EVENT_SUBSCRIBE_PREFIXES: &[&str] = &["tapp.", "system."];
pub const ASSET_LITERAL_METHODS: &[&str] = &["get", "getUrl", "getArrayBuffer"];
pub const SOURCE_CODE_EXTENSIONS: &[&str] = &[".js", ".mjs", ".cjs", ".ts", ".tsx", ".jsx"];
pub const SOURCE_SCAN_SKIP_DIRECTORIES: &[&str] =
    &[".git", "node_modules", "dist", "build", "coverage"];

pub const SAFE_COMPONENT_PATTERN: &str = r"^[A-Za-z0-9][A-Za-z0-9._-]*$";
pub const LOCALE_TAG_PATTERN: &str = r"^[A-Za-z]{2,3}(?:-[A-Za-z0-9]{1,8})*$";
pub const SEMVER_PATTERN: &str =
    r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$";
pub const NAMED_VALUE_PATTERN: &str = r"^[A-Za-z0-9._-]+$";
pub const STORAGE_KEY_PATTERN: &str = r"^[A-Za-z0-9_.:-]+$";
pub const THEME_COLOR_PATTERN: &str = r"^#[0-9A-Fa-f]{6}$";
pub const SEMVER_PREFIXES: &[&str] = &["v"];
pub const SETTING_FIELD_TYPES: &[(&str, &str)] = &[
    ("options", "select"),
    ("min", "number"),
    ("max", "number"),
    ("step", "number"),
    ("placeholder", "input"),
];
pub const SETTING_DEFAULT_KINDS: &[(&str, &str)] = &[
    ("toggle", "boolean"),
    ("input", "string"),
    ("color", "string"),
    ("select", "option"),
    ("number", "number"),
];
pub const WIDGET_REFRESH_MODES: &[(&str, &str)] = &[("event", "event"), ("interval", "interval")];
