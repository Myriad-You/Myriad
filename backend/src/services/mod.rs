// Service layer modules
pub mod activity_event_service;
pub mod agent;
pub mod agent_interaction; // Agent ↔ Tapp interaction create surface
pub mod agora_convo; // Shengwang Conversational AI join/leave
pub mod agora_rtc_token; // Agora AccessToken2
pub mod ai;
pub mod ai_config; // Cached AI provider config (text + image tiers)
pub mod ai_cost_ledger; // Append-only AI cost ledger writes
pub mod ai_quota; // Daily quota reserve/settle/usage ledger
pub mod ai_task_context; // Context refs resolve (platform/report/profile)
pub mod ai_task_execute; // Run loop (provider + quota + ledger)
pub mod ai_task_prepare; // Prompt assemble + structured-output normalize
pub mod ai_task_provider; // Text/image provider execution for AI Tasks
pub mod ai_task_registry; // Cross-replica register/persist
pub mod ai_task_runtime; // Process-local AI_TASKS map + state transitions
pub mod analyzer;
pub mod avatar; // 头像来源枚举/解析 + 出口统一代理（单一解析处）
pub mod background_processor;
pub mod bilibili_utils;
pub mod config_service;
pub mod content_databases; // Preset anime/game/artist databases
pub mod data_key; // Config-key / federation private-key envelope
pub mod data_paths;
pub mod enka_assets; // Enka character metadata for Hoyoverse cards
pub mod fetcher;
pub mod gemini_media; // Gemini generateContent image + speech
pub mod governed_text; // Governed AI text sink (scheduler + declared-API builtins)
pub mod http_client; // Shared HTTP client with proxy support
pub mod image_generation; // OpenAI / OpenRouter / Volcengine / Gemini image providers
pub mod image_proxy_urls; // Shared image proxy URL rewrite (profile/export)
pub mod json_schema_subset;
pub mod kugou_service; // Kugou lyrics (KRC) supplement
pub mod library_items; // Library item pure builders (Bangumi/MAL/preferences)
pub mod memory_profile; // default vs memory-saver process budgets
pub mod merope_rig; // Site-wide compiled 2.5D face package
pub mod metadata_service;
pub mod minimax_speech; // MiniMax T2A speech synthesis
pub mod module_visibility; // Module visibility for Agent (no api::config import)
pub mod netease_service;
pub mod netease_utils;
pub mod oauth;
pub mod openai_compatible_speech; // OpenAI / OpenRouter file STT + TTS
pub mod outbound_security; // Outbound URL validation, DNS pinning, redirect policy
pub mod permission_service;
pub mod platform_auto_refresh; // Core platform auto-refresh via Tapp scheduler
pub mod platform_cache; // Platform filtered-JSON cache
pub mod platform_items; // Cache → uniform items[] projection
pub mod platform_refresh; // Platform fetch/cache (profile HTTP + scheduler)
pub mod profile_text; // 名称/简介文案来源（与 avatar 独立）
pub mod see_through; // Remote See-through layered-PSD decomposition
pub mod server_location; // Egress location dual-source probe
pub mod site_owner;
pub mod smart_filter;
pub mod speech_runtime; // Provider resolve + test/status
pub mod spoof_utils; // Region IP/UA spoofing helpers
pub mod standalone_tts; // Standalone TTS (cache + Tencent) for HTTP + agent
pub mod store_stats_beacon; // Official store install/update edge stats beacon
pub mod tapp_agent_interaction; // Agent interaction registry + state machine
pub mod tapp_api_service; // Declared-API execution (public/protected)
pub mod tapp_catalog; // Catalog/detail list projection (role-filtered)
pub mod tapp_components; // Host-managed component registry (_component:)
pub mod tapp_context; // Runtime context payloads + subject role projection
pub mod tapp_credentials; // Installation-scoped write-only credential bindings
pub mod tapp_data_exchange; // One-shot consent-gated data exchange
pub mod tapp_data_transform; // Pure declarative data.transform pipeline
pub mod tapp_declared_api; // Manifest declared-API catalog + parse cache
pub mod tapp_events; // Manifest-scoped at-most-once event broker
pub mod tapp_federation_feed; // Federation feed merge + item projection
pub mod tapp_hmac; // Shared HMAC-SHA256 for outbound sign + inbound verify
pub mod tapp_host_attribution; // Host-proxied route→permission maps
pub mod tapp_inbound_guard; // Inbound /tapi pause + IP-fingerprint blocks
pub mod tapp_inbound_route; // Declared inbound /tapi verify + nonce ledger
pub mod tapp_install; // Install/update source mode + CSS channels + approved perms
pub mod tapp_install_resources; // Post-stage declared resource + archive entry checks
pub mod tapp_lifecycle; // Start/stop/uninstall + recent/widget pure rules
pub mod tapp_list_card_sizes; // Per-user list page card sizes (1x1|2x1)
pub mod tapp_notification;
pub mod tapp_ownership;
pub mod tapp_package_fs; // Install-dir lifecycle artifacts + orphan/path rules
pub mod tapp_package_read; // Installed package resource path plans
pub mod tapp_playground_knowledge; // Playground Agent read-only contract retrieval
pub mod tapp_prepared_package; // Prepared package validate + resource overrides
pub mod tapp_rate_limit;
pub mod tapp_registry; // Runtime registry/mailbox (workspace crate + DB adapter)
pub mod tapp_reports; // Platform report catalog + payload projection
pub mod tapp_runtime_grant;
pub mod tapp_scheduler;
pub mod tapp_shortcuts; // Host-managed shortcut registry (_shortcut:)
pub mod tapp_storage;
pub mod tapp_store_package; // Remote store path/index pure mapping
pub mod tapp_store_sources; // Store source admin policy + projection
pub mod tapp_validation; // Manifest/package pure validators
pub mod tapp_ws_ticket; // One-time federation WS tickets
pub mod tencent_speech_service;
pub mod tripo; // Tripo v3 3D generation + Web GLB persistence
pub mod updater_client;

// Brew reading system
pub mod brew_parser;
pub mod brew_scheduler;
pub mod brew_topics;
pub mod icon_service;
pub mod image_cache; // e.g. Notion temporary URLs
pub mod notion_service;
pub mod rig_chest_analysis; // One-shot vision profile for Anime2.5D chest motion
pub mod rsshub_service;
