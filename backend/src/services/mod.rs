// Service layer modules
pub mod activity_event_service;
pub mod agent;
pub mod agent_interaction; // Agent ↔ Tapp interaction create surface
pub mod agora_chat; // Realtime transport bindings to shared Agent Chat runs
pub mod agora_convo; // Shengwang Conversational AI join/leave/interrupt
pub mod agora_rtc_token; // Agora AccessToken2
pub mod ai;
pub mod ai_config; // Cached AI provider config (text tiers + image)
pub mod ai_cost_ledger; // Append-only AI cost ledger writes
pub mod ai_quota; // Daily quota reserve/settle/usage ledger
pub mod ai_task_context; // Context refs resolve (platform/report/profile/custom)
pub mod ai_task_execute; // Run loop (provider + quota + ledger)
pub mod ai_task_image; // Bounded reference-image input and local resolution
pub mod ai_task_prepare; // Prompt assemble + structured-output normalize
pub mod ai_task_provider; // Text/image provider execution for AI Tasks
pub mod ai_task_registry; // Cross-replica register/persist
pub mod ai_task_runtime; // Process-local AI_TASKS map + state transitions
pub mod analyzer;
pub mod avatar; // 头像来源枚举/解析（单一解析处；HTTP JSON 走代理，联邦 Actor 不经代理 URL）
pub mod background_processor;
pub mod config_service;
pub mod content_databases; // Preset anime/game/artist databases
pub mod data_key; // Config-key / federation private-key envelope
pub mod data_paths;
pub mod enka_assets; // Enka character metadata for Hoyoverse cards
pub mod federation_gate; // Egress-location decision: may this server federate?
pub mod fetcher;
pub mod gemini_media; // Gemini generateContent image + speech
pub mod governed_text; // Governed AI text sink (scheduler + declared-API builtins)
pub mod http_client; // Shared HTTP client with proxy support
pub mod image_generation; // OpenAI / OpenRouter / Volcengine / Gemini image providers
pub mod image_proxy_urls; // Shared image proxy URL rewrite (profile/export/library)
pub mod json_schema_subset;
pub mod kugou_service; // Kugou lyrics (KRC) supplement
pub mod library_items; // Library item models, paging, Bangumi/MAL builders, preferences, assembly cache
pub mod memory_profile; // default vs memory-saver process budgets
pub mod merope_rig; // Anime2.5D rig store (live pointer is worn outfit)
pub mod metadata_service;
pub mod minimax_speech; // MiniMax T2A speech synthesis
pub mod module_visibility; // Module visibility for Agent (no api::config import)
pub mod music_player_view; // Player playlist projection (slim cache + Song fields)
pub mod netease_service;
pub mod oauth;
pub mod openai_compatible_speech; // OpenAI / OpenRouter file STT + TTS
pub mod outbound_security; // Outbound URL validation, DNS pinning, redirect policy
pub mod permission_service;
pub mod platform_auto_refresh; // Core platform auto-refresh via Tapp scheduler
pub mod platform_cache; // Platform filtered-JSON cache
pub mod platform_items; // Cache → uniform items[] projection
pub mod platform_refresh; // Platform fetch/cache (profile HTTP + scheduler)
pub mod profile_text; // 名称/简介文案来源（与 avatar 独立）
pub mod retired_configuration; // Backup denylist for retired configuration keys
pub mod see_through; // Remote See-through layered-PSD decomposition
pub mod server_location; // Egress location dual-source probe
pub mod site_owner;
pub mod smart_filter;
pub mod speech_runtime; // Provider resolve + test/status
pub mod standalone_tts; // Standalone TTS (cache + configured provider) for HTTP + agent
pub mod sticker_cutout; // Local alpha fallback for home stickers
pub mod store_stats_beacon; // Official store install/update edge stats beacon
pub mod tapp_agent_interaction; // Agent interaction registry + state machine
pub mod tapp_api_service; // Declared-API execution (public/protected/manager)
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

pub(crate) mod bot_ingress;
pub mod channel_pairing; // Shared pairing mint/consume/unbind
pub mod channel_work; // Shared private-chat Work: session, pending, delivery
pub mod discord_bot; // Discord DM Gateway worker
pub mod discord_pairing; // Discord DM pairing codes + user_identities
pub mod discord_work; // Discord DM send adapter + Work entry
pub mod feishu_bot; // Feishu p2p long-connection worker
pub mod feishu_bot_api; // Feishu OpenAPI send / upload / download
pub mod feishu_pairing; // Feishu p2p pairing codes + user_identities
pub mod feishu_work; // Feishu p2p send adapter + Work entry
pub mod feishu_ws; // Feishu pbbp2 Frame + WSS session
pub mod icon_service;
pub mod image_cache; // e.g. Notion temporary URLs
pub mod media;
pub mod media_catalog;
pub mod note_authors;
pub mod note_publish;
pub mod notion_service;
pub mod phantasi_parser;
pub mod phantasi_scheduler;
pub mod phantasi_topics;
pub mod qq_bot; // QQ C2C Gateway worker
pub mod qq_pairing; // QQ C2C pairing codes + user_identities
pub mod qq_work; // QQ C2C send adapter + Work entry
pub mod rig_chest_analysis; // One-shot vision profile for Anime2.5D chest motion
pub mod rsshub_service;
pub mod telegram_bot; // Telegram DM getUpdates worker
pub mod telegram_pairing; // Telegram DM pairing codes + user_identities
pub mod telegram_work; // Telegram DM send adapter + Work entry

pub(crate) mod keyed_lock;
pub(crate) mod retained_cache;
