//! 语音服务 API
//!
//! 提供 TTS（文本转语音）和 ASR（语音转文本）的 HTTP API。
//! 探测/单条 TTS 走 `configured_provider`（`speech_source` 优先，否则 `speech_provider`）。
//! batch 播客路径固定 `TencentSpeechService`。

use crate::middleware::auth::Claims;
use axum::{
    Json, Router,
    extract::{Extension, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post},
};
use sea_orm::DatabaseConnection;

use crate::services::data_paths::paths;
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::fs;

fn require_speech_user_id(claims: &Claims) -> Option<i32> {
    crate::services::tapp_ownership::positive_user_id(&claims.sub)
}

use crate::services::tencent_speech_service::{
    AsrRequest, TencentSpeechError, TencentSpeechService, TtsRequest,
};

/// TTS 子目录名
const TTS_SUBDIR: &str = "tts";

/// 创建语音服务 API 路由
pub fn create_speech_routes(app_state: crate::state::AppState) -> Router<crate::state::AppState> {
    use axum::middleware::from_fn_with_state;
    Router::<crate::state::AppState>::new()
        // TTS 文本转语音
        .route("/tts", post(text_to_speech))
        // 批量 TTS（用于播客）
        .route("/tts/batch", post(batch_text_to_speech))
        // ASR 语音转文本
        .route("/asr", post(speech_to_text))
        // 服务状态
        .route("/status", get(get_speech_status))
        // 设置页可用性测试（TTS + 可选 ASR）
        .route("/test", post(test_speech_service))
        .route("/convo/start", post(start_convo_session))
        .route("/convo/stop", post(stop_convo_session))
        .route("/convo/interrupt", post(interrupt_convo_session))
        .route(
            "/convo/events",
            get(super::speech_conversation::conversation_events),
        )
        // 可用音色列表
        .route("/voices", get(get_voice_list))
        // 可用引擎列表
        .route("/engines", get(get_engine_list))
        // 文章缓存管理
        .route("/cache/article", get(get_article_cache_info))
        .route("/cache/article/voice", delete(clear_article_voice_cache))
        // Tapp 运行时携带 Grant 头时做服务端归因与权限强制（在认证之后执行）
        .route_layer(from_fn_with_state(
            app_state.clone(),
            crate::api::tapp_runtime::speech_host_attribution,
        ))
        // 以上路由走 auth_middleware
        .route_layer(from_fn_with_state(
            app_state,
            crate::middleware::auth::auth_middleware,
        ))
        // Cloud callbacks authenticate with a short-lived, server-bound key,
        // never with browser cookies or a provider credential. No CSRF exemption.
        .route(
            "/convo/chat/completions",
            post(super::speech_conversation::chat_completion)
                .layer(axum::extract::DefaultBodyLimit::max(64 * 1024)),
        )
}

// Standalone TTS DTO + synthesis live in services so agent does not depend on this API module.
pub use crate::services::standalone_tts::{
    TtsApiRequest, TtsApiResponse, synthesize_standalone_tts,
};

/// 批量 TTS 请求体（用于播客）
#[derive(Debug, Deserialize)]
pub struct BatchTtsApiRequest {
    /// 订阅源 ID
    pub source_id: i32,
    /// 文章 ID
    pub article_id: i32,
    /// 对话列表
    pub dialogues: Vec<BatchTtsDialogue>,
    /// 音频格式（缺省 mp3；本层不校验枚举）
    #[serde(default)]
    pub codec: Option<String>,
    /// 采样率（缺省 16000；本层不校验取值）
    #[serde(default)]
    pub sample_rate: Option<i32>,
    /// 强制重新生成（跳过 exact 与 any-voice 缓存）
    #[serde(default)]
    pub force_regenerate: bool,
}

/// 批量 TTS 单条对话
#[derive(Debug, Deserialize, Clone)]
pub struct BatchTtsDialogue {
    /// 对话索引（用于排序）
    pub index: usize,
    /// 说话者：`host` → 智斌，其余 → 爱小溪（见 `get_default_voice_for_speaker`）
    pub speaker: String,
    /// 对话文本
    pub text: String,
    /// 音色ID（可选，不提供则根据speaker自动选择）
    #[serde(default)]
    pub voice_type: Option<i32>,
    /// 语速（缺省 0.0；本层不校验区间）
    #[serde(default)]
    pub speed: Option<f32>,
}

/// 获取文章 TTS 目录路径
/// 结构: {phantasi}/{source_id}/{article_id}/tts/
fn get_article_tts_dir(source_id: i32, article_id: i32) -> PathBuf {
    paths()
        .phantasi
        .join(source_id.to_string())
        .join(article_id.to_string())
        .join(TTS_SUBDIR)
}

/// 批量 TTS 响应体
#[derive(Debug, Serialize)]
pub struct BatchTtsApiResponse {
    /// 是否成功
    pub success: bool,
    /// 音频列表
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audios: Option<Vec<BatchTtsAudioItem>>,
    /// 缓存命中数
    pub cache_hits: usize,
    /// 新生成数
    pub generated: usize,
    /// 失败列表
    #[serde(skip_serializing_if = "Option::is_none")]
    pub errors: Option<Vec<BatchTtsError>>,
    /// 总体错误信息
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 批量 TTS 单条音频结果
#[derive(Debug, Serialize)]
pub struct BatchTtsAudioItem {
    /// 对话索引
    pub index: usize,
    /// 说话者
    pub speaker: String,
    /// Base64编码的音频数据
    pub audio: String,
    /// 是否来自缓存
    pub cached: bool,
}

/// 批量 TTS 单条错误
#[derive(Debug, Serialize)]
pub struct BatchTtsError {
    /// 对话索引
    pub index: usize,
    /// 错误信息
    pub error: String,
}

/// 根据说话者获取默认音色
fn get_default_voice_for_speaker(speaker: &str) -> i32 {
    use crate::services::tencent_speech_service::voice_types;
    match speaker.to_lowercase().as_str() {
        "host" => voice_types::ZHI_BIN,     // 主播用大模型阅读男声-智斌
        "guest" => voice_types::AI_XIAO_XI, // 嘉宾用大模型聊天女声-爱小溪
        _ => voice_types::AI_XIAO_XI,
    }
}

/// 获取文章 TTS 文件路径（按音色分文件夹，索引为文件名）
/// 结构: `{phantasi}/{source_id}/{article_id}/tts/{voice_type}/{index}.{codec}`
fn get_article_tts_file_path(
    source_id: i32,
    article_id: i32,
    voice_type: i32,
    dialogue_index: usize,
    codec: &str,
) -> PathBuf {
    get_article_tts_dir(source_id, article_id)
        .join(voice_type.to_string())
        .join(format!("{}.{}", dialogue_index, codec))
}

/// 从文件读取 TTS 音频（返回 base64）
async fn read_tts_file(path: &PathBuf) -> Option<String> {
    match fs::read(path).await {
        Ok(data) => {
            tracing::debug!("TTS file read: {}", path.display());
            Some(BASE64.encode(&data))
        }
        Err(_) => None,
    }
}

// 文章 TTS 缓存

/// 精确查找文章对话 TTS 缓存（指定音色+对话索引）
async fn find_article_exact_tts(
    source_id: i32,
    article_id: i32,
    voice_type: i32,
    dialogue_index: usize,
    codec: &str,
) -> Option<String> {
    let path = get_article_tts_file_path(source_id, article_id, voice_type, dialogue_index, codec);
    if let Some(audio) = read_tts_file(&path).await {
        tracing::info!("TTS exact match: {}", path.display());
        return Some(audio);
    }
    None
}

/// 查找文章对话任意音色的 TTS 缓存（遍历所有音色文件夹找对应索引）
async fn find_article_any_tts(
    source_id: i32,
    article_id: i32,
    dialogue_index: usize,
    codec: &str,
) -> Option<String> {
    let tts_dir = get_article_tts_dir(source_id, article_id);
    let target_filename = format!("{}.{}", dialogue_index, codec);

    // 遍历所有音色文件夹
    let mut entries = match fs::read_dir(&tts_dir).await {
        Ok(entries) => entries,
        Err(_) => return None,
    };

    while let Ok(Some(voice_entry)) = entries.next_entry().await {
        if let Ok(file_type) = voice_entry.file_type().await {
            if file_type.is_dir() {
                // 检查该音色文件夹下是否有对应索引的文件
                let file_path = voice_entry.path().join(&target_filename);
                if let Some(audio) = read_tts_file(&file_path).await {
                    tracing::info!("TTS any-voice match: {}", file_path.display());
                    return Some(audio);
                }
            }
        }
    }

    None
}

/// 写入文章对话 TTS 文件
async fn write_article_tts_file(
    source_id: i32,
    article_id: i32,
    voice_type: i32,
    dialogue_index: usize,
    codec: &str,
    audio_base64: &str,
) -> Result<(), std::io::Error> {
    // 确保音色目录存在
    let voice_dir = get_article_tts_dir(source_id, article_id).join(voice_type.to_string());
    fs::create_dir_all(&voice_dir).await?;

    // 解码 base64 并写入文件
    let audio_data = BASE64
        .decode(audio_base64)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    let file_path =
        get_article_tts_file_path(source_id, article_id, voice_type, dialogue_index, codec);
    fs::write(&file_path, &audio_data).await?;

    tracing::info!(
        "TTS file written: {} ({} bytes)",
        file_path.display(),
        audio_data.len()
    );
    Ok(())
}

/// ASR 请求体
#[derive(Debug, Deserialize)]
pub struct AsrApiRequest {
    /// Base64 音频。与 `url` 同时出现时本层优先用它；非 Tencent 路径只认这个字段。
    #[serde(default)]
    pub audio_data: Option<String>,
    /// 音频 URL。仅 Tencent ASR 使用；非 Tencent 有 URL 无 `audio_data` 会拒。
    #[serde(default)]
    pub url: Option<String>,
    /// 音频格式（缺省 wav；本层不校验枚举）
    #[serde(default)]
    pub format: Option<String>,
    /// 引擎类型（缺省 16k_zh；本层不校验枚举）
    #[serde(default)]
    pub engine: Option<String>,
    /// 词级时间戳（透传给 ASR；缺省不填）
    #[serde(default)]
    pub word_info: Option<i32>,
    /// 脏词过滤（透传给 ASR；缺省不填）
    #[serde(default)]
    pub filter_dirty: Option<i32>,
    /// 临时热词表 (格式: "热词1|权重,热词2|权重")
    #[serde(default)]
    pub hotword_list: Option<String>,
}

/// ASR 响应体
#[derive(Debug, Serialize)]
pub struct AsrApiResponse {
    /// 是否成功
    pub success: bool,
    /// 识别结果文本
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// 音频时长(ms)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<i32>,
    /// 词时间戳列表
    #[serde(skip_serializing_if = "Option::is_none")]
    pub words: Option<Vec<WordInfo>>,
    /// 错误信息
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 词信息
#[derive(Debug, Serialize)]
pub struct WordInfo {
    pub word: String,
    pub start_time: i32,
    pub end_time: i32,
}

/// 语音服务状态响应
#[derive(Debug, Serialize)]
pub struct SpeechStatusResponse {
    pub available: bool,
    pub tts_enabled: bool,
    pub asr_enabled: bool,
    pub convo_enabled: bool,
    /// 人设开口朗读。人设未生效或开关关着时为 false。
    pub persona_speech_enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 可用音色列表响应
#[derive(Debug, Serialize)]
pub struct VoiceListResponse {
    pub voices: Vec<VoiceInfo>,
}

/// 音色信息
#[derive(Debug, Serialize)]
pub struct VoiceInfo {
    pub id: i32,
    pub name: String,
    pub gender: String,
    pub language: String,
    pub description: String,
    /// 音色类型: ultra_natural (超自然大模型), llm (大模型), premium (精品)
    pub voice_type: String,
    /// 是否支持情感控制
    pub emotion_support: bool,
}

/// 将腾讯云语音服务错误转换为HTTP响应
fn speech_error_to_response(error: TencentSpeechError) -> (StatusCode, String) {
    match error {
        TencentSpeechError::ApiKeyNotConfigured => (
            StatusCode::SERVICE_UNAVAILABLE,
            "Speech service is not configured".to_string(),
        ),
        TencentSpeechError::NetworkError(_) => (
            StatusCode::BAD_GATEWAY,
            "Speech service is unreachable".to_string(),
        ),
        TencentSpeechError::ApiError { .. } => (
            StatusCode::BAD_REQUEST,
            "Speech service request failed".to_string(),
        ),
        TencentSpeechError::ParseError(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Speech service request failed".to_string(),
        ),
        TencentSpeechError::InvalidAudioData(_) => {
            (StatusCode::BAD_REQUEST, "Invalid audio data".to_string())
        }
        TencentSpeechError::TextTooLong => (
            StatusCode::BAD_REQUEST,
            "Speech text is too long".to_string(),
        ),
    }
}

/// 文本转语音 API
///
/// POST /api/speech/tts
pub async fn text_to_speech(
    Extension(claims): Extension<Claims>,
    Json(request): Json<TtsApiRequest>,
) -> impl IntoResponse {
    let Some(user_id) = require_speech_user_id(&claims) else {
        return (
            StatusCode::FORBIDDEN,
            Json(TtsApiResponse {
                success: false,
                audio: None,
                session_id: None,
                cached: None,
                error: Some("A durable user account is required".into()),
            }),
        );
    };
    match crate::services::ai_cost_ledger::with_site_ai_ledger(
        user_id,
        "speech",
        "tts",
        synthesize_standalone_tts(&request),
    )
    .await
    {
        Ok(response) => (StatusCode::OK, Json(response)),
        Err(msg) => {
            let status = if msg.contains("empty") {
                StatusCode::BAD_REQUEST
            } else if msg.contains("too long") {
                StatusCode::BAD_REQUEST
            } else if msg.contains("not configured") {
                StatusCode::SERVICE_UNAVAILABLE
            } else {
                StatusCode::BAD_GATEWAY
            };
            (
                status,
                Json(TtsApiResponse {
                    success: false,
                    audio: None,
                    session_id: None,
                    cached: None,
                    error: Some(msg),
                }),
            )
        }
    }
}

/// 批量文本转语音 API（用于播客）
///
/// POST /api/speech/tts/batch
pub async fn batch_text_to_speech(
    Extension(claims): Extension<Claims>,
    Json(request): Json<BatchTtsApiRequest>,
) -> impl IntoResponse {
    let Some(user_id) = require_speech_user_id(&claims) else {
        return (
            StatusCode::FORBIDDEN,
            Json(BatchTtsApiResponse {
                success: false,
                audios: None,
                cache_hits: 0,
                generated: 0,
                errors: None,
                error: Some("A durable user account is required".into()),
            }),
        )
            .into_response();
    };
    crate::services::ai_cost_ledger::with_site_ai_ledger(
        user_id,
        "speech",
        "tts_batch",
        batch_text_to_speech_inner(request),
    )
    .await
    .into_response()
}

async fn batch_text_to_speech_inner(request: BatchTtsApiRequest) -> impl IntoResponse {
    // 验证对话列表
    if request.dialogues.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(BatchTtsApiResponse {
                success: false,
                audios: None,
                cache_hits: 0,
                generated: 0,
                errors: None,
                error: Some("Dialogue list is empty".to_string()),
            }),
        );
    }

    if request.dialogues.len() > 100 {
        return (
            StatusCode::BAD_REQUEST,
            Json(BatchTtsApiResponse {
                success: false,
                audios: None,
                cache_hits: 0,
                generated: 0,
                errors: None,
                error: Some("Too many dialogues (max 100)".to_string()),
            }),
        );
    }

    let codec = request.codec.as_deref().unwrap_or("mp3");
    let sample_rate = request.sample_rate.unwrap_or(16000);

    let mut audios = Vec::new();
    let mut errors = Vec::new();
    let mut cache_hits = 0;
    let mut generated = 0;

    // 创建服务（延迟创建，只有需要生成时才创建）
    let mut service: Option<TencentSpeechService> = None;

    for dialogue in &request.dialogues {
        // 验证文本
        if dialogue.text.is_empty() {
            errors.push(BatchTtsError {
                index: dialogue.index,
                error: "empty_dialogue_text".to_string(),
            });
            continue;
        }

        // 获取音色和参数
        let voice_type = dialogue
            .voice_type
            .unwrap_or_else(|| get_default_voice_for_speaker(&dialogue.speaker));
        let speed = dialogue.speed.unwrap_or(0.0);

        tracing::debug!(
            "Looking for TTS cache: source={}, article={}, voice={}, speed={}, sample_rate={}, codec={}",
            request.source_id,
            request.article_id,
            voice_type,
            speed,
            sample_rate,
            codec
        );

        // force_regenerate：跳过 exact + any-voice 缓存，强制按指定音色重新合成
        if !request.force_regenerate {
            // 1. 精确缓存（音色分文件夹 `{phantasi}/.../tts/{voice_type}/{index}.{codec}`）
            if let Some(cached_audio) = find_article_exact_tts(
                request.source_id,
                request.article_id,
                voice_type,
                dialogue.index,
                codec,
            )
            .await
            {
                tracing::info!("TTS exact cache hit for index {}", dialogue.index);
                audios.push(BatchTtsAudioItem {
                    index: dialogue.index,
                    speaker: dialogue.speaker.clone(),
                    audio: cached_audio,
                    cached: true,
                });
                cache_hits += 1;
                continue;
            }

            // 2. 任意音色文件夹下的 `{index}.{codec}`
            if let Some(cached_audio) =
                find_article_any_tts(request.source_id, request.article_id, dialogue.index, codec)
                    .await
            {
                tracing::info!("TTS any-voice cache hit for index {}", dialogue.index);
                audios.push(BatchTtsAudioItem {
                    index: dialogue.index,
                    speaker: dialogue.speaker.clone(),
                    audio: cached_audio,
                    cached: true,
                });
                cache_hits += 1;
                continue;
            }
        }

        tracing::info!("TTS cache miss for index {}, will generate", dialogue.index);

        // 3. 缓存未命中或 force_regenerate：Tencent 合成
        if service.is_none() {
            match TencentSpeechService::new().await {
                Ok(s) => service = Some(s),
                Err(e) => {
                    let (_, msg) = speech_error_to_response(e);
                    return (
                        StatusCode::SERVICE_UNAVAILABLE,
                        Json(BatchTtsApiResponse {
                            success: false,
                            audios: Some(audios),
                            cache_hits,
                            generated,
                            errors: Some(errors),
                            error: Some(msg),
                        }),
                    );
                }
            }
        }

        // 构建TTS请求
        let tts_request = TtsRequest {
            text: dialogue.text.clone(),
            voice_type: Some(voice_type),
            speed: dialogue.speed,
            volume: None,
            codec: Some(codec.to_string()),
            sample_rate: Some(sample_rate),
            ..Default::default()
        };

        // 调用TTS服务
        match service.as_ref().unwrap().text_to_speech(tts_request).await {
            Ok(response) => {
                crate::services::speech_runtime::note_tts(
                    "tencent",
                    "tts",
                    &dialogue.text,
                    response.audio.is_some(),
                )
                .await;
                if let Some(audio) = response.audio {
                    // 写入文章缓存目录
                    if let Err(e) = write_article_tts_file(
                        request.source_id,
                        request.article_id,
                        voice_type,
                        dialogue.index,
                        codec,
                        &audio,
                    )
                    .await
                    {
                        tracing::warn!(
                            "Failed to write TTS file for index {}: {}",
                            dialogue.index,
                            e
                        );
                    }

                    audios.push(BatchTtsAudioItem {
                        index: dialogue.index,
                        speaker: dialogue.speaker.clone(),
                        audio,
                        cached: false,
                    });
                    generated += 1;
                } else {
                    errors.push(BatchTtsError {
                        index: dialogue.index,
                        error: "Speech service returned no audio".to_string(),
                    });
                }
            }
            Err(e) => {
                crate::services::speech_runtime::note_tts("tencent", "tts", &dialogue.text, false)
                    .await;
                let (_, msg) = speech_error_to_response(e);
                errors.push(BatchTtsError {
                    index: dialogue.index,
                    error: msg,
                });
            }
        }
    }

    // 按索引排序
    audios.sort_by_key(|a| a.index);

    let success = errors.is_empty() && !audios.is_empty();

    (
        if success {
            StatusCode::OK
        } else {
            StatusCode::PARTIAL_CONTENT
        },
        Json(BatchTtsApiResponse {
            success,
            audios: if audios.is_empty() {
                None
            } else {
                Some(audios)
            },
            cache_hits,
            generated,
            errors: if errors.is_empty() {
                None
            } else {
                Some(errors)
            },
            error: None,
        }),
    )
}

/// 语音转文本 API
///
/// POST /api/speech/asr
pub async fn speech_to_text(
    Extension(claims): Extension<Claims>,
    Json(request): Json<AsrApiRequest>,
) -> (StatusCode, Json<AsrApiResponse>) {
    let Some(user_id) = require_speech_user_id(&claims) else {
        return (
            StatusCode::FORBIDDEN,
            Json(AsrApiResponse {
                success: false,
                text: None,
                duration: None,
                words: None,
                error: Some("A durable user account is required".into()),
            }),
        );
    };
    crate::services::ai_cost_ledger::with_site_ai_ledger(
        user_id,
        "speech",
        "stt",
        speech_to_text_inner(request),
    )
    .await
}

async fn speech_to_text_inner(request: AsrApiRequest) -> (StatusCode, Json<AsrApiResponse>) {
    // 验证输入
    if request.audio_data.is_none() && request.url.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(AsrApiResponse {
                success: false,
                text: None,
                duration: None,
                words: None,
                error: Some("Please provide audio data".to_string()),
            }),
        );
    }

    let provider = crate::services::speech_runtime::configured_provider().await;
    if !matches!(
        provider,
        crate::services::speech_runtime::SpeechProviderKind::Tencent
    ) {
        return openai_speech_to_text(request).await;
    }

    // 创建服务
    let service = match TencentSpeechService::new().await {
        Ok(s) => s,
        Err(e) => {
            let (_, msg) = speech_error_to_response(e);
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(AsrApiResponse {
                    success: false,
                    text: None,
                    duration: None,
                    words: None,
                    error: Some(msg),
                }),
            );
        }
    };

    // 构建ASR请求
    let format = request.format.unwrap_or_else(|| "wav".to_string());
    let engine = request.engine.unwrap_or_else(|| "16k_zh".to_string());
    let mut asr_audio_bytes: usize = 0;

    let asr_request = if let Some(audio_data) = &request.audio_data {
        // 解码Base64获取原始数据长度
        let data_len = match BASE64.decode(audio_data) {
            Ok(bytes) => bytes.len() as i32,
            Err(_) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(AsrApiResponse {
                        success: false,
                        text: None,
                        duration: None,
                        words: None,
                        error: Some("Invalid audio data".to_string()),
                    }),
                );
            }
        };
        asr_audio_bytes = data_len as usize;

        AsrRequest {
            eng_ser_vice_type: engine.clone(),
            source_type: 1,
            voice_format: format,
            data: Some(audio_data.clone()),
            data_len: Some(data_len),
            word_info: request.word_info,
            filter_dirty: request.filter_dirty,
            hotword_list: request.hotword_list,
            ..Default::default()
        }
    } else {
        AsrRequest {
            eng_ser_vice_type: engine.clone(),
            source_type: 0,
            voice_format: format,
            url: request.url,
            word_info: request.word_info,
            filter_dirty: request.filter_dirty,
            hotword_list: request.hotword_list,
            ..Default::default()
        }
    };

    // 调用ASR服务
    match service.speech_to_text(asr_request).await {
        Ok(response) => {
            crate::services::speech_runtime::note_stt(
                "tencent",
                &engine,
                asr_audio_bytes,
                response.result.as_deref().unwrap_or(""),
                true,
            )
            .await;
            let words = response.word_list.map(|list| {
                list.into_iter()
                    .filter_map(|w| {
                        Some(WordInfo {
                            word: w.word?,
                            start_time: w.start_time.unwrap_or(0),
                            end_time: w.end_time.unwrap_or(0),
                        })
                    })
                    .collect()
            });

            (
                StatusCode::OK,
                Json(AsrApiResponse {
                    success: true,
                    text: response.result,
                    duration: response.audio_duration,
                    words,
                    error: None,
                }),
            )
        }
        Err(e) => {
            crate::services::speech_runtime::note_stt(
                "tencent",
                &engine,
                asr_audio_bytes,
                "",
                false,
            )
            .await;
            let (status, msg) = speech_error_to_response(e);
            (
                status,
                Json(AsrApiResponse {
                    success: false,
                    text: None,
                    duration: None,
                    words: None,
                    error: Some(msg),
                }),
            )
        }
    }
}

async fn openai_speech_to_text(request: AsrApiRequest) -> (StatusCode, Json<AsrApiResponse>) {
    let format = request.format.unwrap_or_else(|| "wav".to_string());
    let audio = if let Some(audio_data) = request.audio_data {
        match BASE64.decode(&audio_data) {
            Ok(bytes) => bytes,
            Err(_) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(AsrApiResponse {
                        success: false,
                        text: None,
                        duration: None,
                        words: None,
                        error: Some("Invalid audio data".to_string()),
                    }),
                );
            }
        }
    } else {
        return (
            StatusCode::BAD_REQUEST,
            Json(AsrApiResponse {
                success: false,
                text: None,
                duration: None,
                words: None,
                error: Some("Please upload audio data".to_string()),
            }),
        );
    };

    match crate::services::speech_runtime::transcribe_bytes(audio, &format, None).await {
        Ok(text) => (
            StatusCode::OK,
            Json(AsrApiResponse {
                success: true,
                text: Some(text),
                duration: None,
                words: None,
                error: None,
            }),
        ),
        Err(msg) => (
            StatusCode::BAD_GATEWAY,
            Json(AsrApiResponse {
                success: false,
                text: None,
                duration: None,
                words: None,
                error: Some(msg),
            }),
        ),
    }
}

/// 获取语音服务状态
///
/// GET /api/speech/status
pub async fn get_speech_status() -> impl IntoResponse {
    let probe = crate::services::speech_runtime::speech_probe().await;
    let config = crate::GLOBAL_DYNAMIC_CONFIG.read().await;
    let convo_enabled = crate::services::agora_convo::convo_configured(&config);
    let persona_speech_enabled = config.merope_speech_enabled_resolved();
    drop(config);
    let callback_ready = super::speech_conversation::callback_url(
        &crate::oauth_url_builder::SiteConfig::get_base_url().await,
    )
    .is_ok();
    let convo_enabled = convo_enabled && callback_ready;
    Json(SpeechStatusResponse {
        available: probe.available || convo_enabled,
        tts_enabled: probe.tts_enabled,
        asr_enabled: probe.asr_enabled,
        convo_enabled,
        persona_speech_enabled,
        provider: Some(probe.provider),
        error: probe.error,
    })
}

#[derive(Debug, Deserialize)]
pub struct ConvoStartRequest {
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ConvoAgentRequest {
    pub agent_id: String,
}

pub async fn start_convo_session(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(request): Json<ConvoStartRequest>,
) -> impl IntoResponse {
    let language = request.language.unwrap_or_else(|| "en-US".to_string());
    let language = crate::services::agora_convo::conversation_language(&language);
    let Some(user_id) = require_speech_user_id(&claims) else {
        return (
            StatusCode::FORBIDDEN,
            Json(AppError::fail_json("Agent is unavailable")),
        )
            .into_response();
    };
    if crate::services::agent::ensure_agent_usage_allowed(&db, user_id)
        .await
        .is_err()
    {
        return (
            StatusCode::FORBIDDEN,
            Json(AppError::fail_json("Agent is unavailable")),
        )
            .into_response();
    }
    if crate::services::ai::create_strict_lite_ai_analyzer_with_timeout(None)
        .await
        .is_none()
    {
        return convo_error(
            crate::services::agora_convo::AgoraConvoError::NotConfigured(
                "Lite model is not configured for Chat mode".into(),
            ),
        )
        .into_response();
    }
    let url = match super::speech_conversation::callback_url(
        &crate::oauth_url_builder::SiteConfig::get_base_url().await,
    ) {
        Ok(url) => url,
        Err(message) => {
            return convo_error(
                crate::services::agora_convo::AgoraConvoError::NotConfigured(message.into()),
            )
            .into_response();
        }
    };
    let session_id = match super::agent::ensure_session(
        &db,
        request.session_id.as_deref(),
        user_id,
        crate::services::agent::AgentInteractionMode::Chat,
    )
    .await
    {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AppError::fail_json("Could not prepare Chat session")),
            )
                .into_response();
        }
    };
    let (chat, key) = match crate::services::agora_chat::ChatSession::register(
        claims,
        session_id.clone(),
    )
    .await
    {
        Ok(binding) => binding,
        Err(message) => {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                Json(AppError::fail_json(message)),
            )
                .into_response();
        }
    };
    let endpoint = crate::services::agora_convo::LlmEndpoint { url, api_key: key };
    match crate::services::agora_convo::start_session(chat.clone(), language, &endpoint).await {
        Ok(session) => {
            let mut body = json_ok_session(session);
            body["session_id"] = serde_json::json!(session_id);
            (StatusCode::OK, Json(body)).into_response()
        }
        Err(error) => {
            chat.close().await;
            convo_error(error).into_response()
        }
    }
}

pub async fn stop_convo_session(
    Extension(claims): Extension<Claims>,
    Json(request): Json<ConvoAgentRequest>,
) -> impl IntoResponse {
    let Some(user_id) = require_speech_user_id(&claims) else {
        return (
            StatusCode::FORBIDDEN,
            Json(AppError::fail_json("A durable user account is required")),
        )
            .into_response();
    };
    match crate::services::agora_convo::stop_session(user_id, &request.agent_id).await {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({ "success": true }))).into_response(),
        Err(error) => convo_error(error).into_response(),
    }
}

pub async fn interrupt_convo_session(
    Extension(claims): Extension<Claims>,
    Json(request): Json<ConvoAgentRequest>,
) -> impl IntoResponse {
    let Some(user_id) = require_speech_user_id(&claims) else {
        return (
            StatusCode::FORBIDDEN,
            Json(AppError::fail_json("A durable user account is required")),
        )
            .into_response();
    };
    match crate::services::agora_convo::interrupt_session(user_id, &request.agent_id).await {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({ "success": true }))).into_response(),
        Err(error) => convo_error(error).into_response(),
    }
}

fn json_ok_session(session: crate::services::agora_convo::ConvoSession) -> serde_json::Value {
    serde_json::json!({
        "success": true,
        "app_id": session.app_id,
        "channel": session.channel,
        "uid": session.uid,
        "agent_uid": session.agent_uid,
        "token": session.token,
        "agent_id": session.agent_id,
    })
}

fn convo_error(error: crate::services::agora_convo::AgoraConvoError) -> impl IntoResponse {
    use crate::services::agora_convo::AgoraConvoError;
    let (status, message, code) = match &error {
        AgoraConvoError::SessionUnavailable => (
            StatusCode::NOT_FOUND,
            "Realtime session is unavailable".to_string(),
            "realtime_session_unavailable",
        ),
        AgoraConvoError::NotConfigured(msg) => (
            StatusCode::SERVICE_UNAVAILABLE,
            msg.clone(),
            "speech_not_configured",
        ),
        AgoraConvoError::Token(e) => (StatusCode::BAD_REQUEST, e.to_string(), "bad_request"),
        AgoraConvoError::Network(msg) => (
            StatusCode::BAD_GATEWAY,
            format!("Speech service is unreachable: {msg}"),
            "speech_upstream_failed",
        ),
        AgoraConvoError::Api {
            status: http_status,
            ..
        } => {
            // Provider errors may echo the LLM callback key or TTS credentials.
            let detail =
                format!("Realtime voice provider rejected the request (HTTP {http_status})");
            (StatusCode::BAD_GATEWAY, detail, "speech_upstream_failed")
        }
    };
    (
        status,
        Json(
            AppError::from_status_u16(status.as_u16(), message)
                .with_code(code)
                .to_json(),
        ),
    )
}

#[cfg(test)]
mod realtime_error_tests {
    use super::*;
    use axum::body::to_bytes;

    #[test]
    fn speech_handlers_do_not_parse_subject_to_zero() {
        let src = include_str!("speech.rs");
        let production = src.split("#[cfg(test)]").next().expect("production");
        assert!(production.contains("require_speech_user_id"));
        assert!(!production.contains("claims.sub.parse().unwrap_or(0)"));
    }

    #[test]
    fn realtime_session_response_includes_both_transport_identities() {
        let response = json_ok_session(crate::services::agora_convo::ConvoSession {
            app_id: "app".into(),
            channel: "channel".into(),
            uid: 42,
            agent_uid: 43,
            token: "browser-scoped-token".into(),
            agent_id: "agent".into(),
        });
        assert_eq!(response["uid"], 42);
        assert_eq!(response["agent_uid"], 43);
        assert_eq!(response["agent_id"], "agent");
    }

    #[tokio::test]
    async fn provider_error_payload_cannot_echo_callback_or_tts_secrets() {
        let secret = "ephemeral-callback-and-tts-secret";
        let response = convo_error(crate::services::agora_convo::AgoraConvoError::Api {
            status: 400,
            message: secret.to_string(),
        })
        .into_response();
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let body = String::from_utf8(
            to_bytes(response.into_body(), 16 * 1024)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(!body.contains(secret));
        assert!(!body.contains("callback"));
    }
}

/// POST /api/speech/test
pub async fn test_speech_service(Extension(claims): Extension<Claims>) -> impl IntoResponse {
    let Some(user_id) = require_speech_user_id(&claims) else {
        return (
            StatusCode::FORBIDDEN,
            Json(AppError::fail_json("A durable user account is required")),
        )
            .into_response();
    };
    Json(
        crate::services::ai_cost_ledger::with_site_ai_ledger(
            user_id,
            "speech",
            "test",
            crate::services::speech_runtime::test_speech_roundtrip(),
        )
        .await,
    )
    .into_response()
}

/// 获取可用音色列表
///
/// GET /api/speech/voices
pub async fn get_voice_list() -> impl IntoResponse {
    use crate::services::tencent_speech_service::voice_types;

    Json(VoiceListResponse {
        voices: vec![
            // 超自然大模型音色
            VoiceInfo {
                id: voice_types::ZHI_XIAO_WU,
                name: "智小悟".to_string(),
                gender: "male".to_string(),
                language: "zh-en".to_string(),
                description: "Natural male chat voice".to_string(),
                voice_type: "ultra_natural".to_string(),
                emotion_support: false,
            },
            VoiceInfo {
                id: voice_types::ZHI_XIAO_JIE,
                name: "智小解".to_string(),
                gender: "male".to_string(),
                language: "zh-en".to_string(),
                description: "Male narrator, good for podcasts".to_string(),
                voice_type: "ultra_natural".to_string(),
                emotion_support: false,
            },
            VoiceInfo {
                id: voice_types::ZHI_XIAO_ROU,
                name: "智小柔".to_string(),
                gender: "female".to_string(),
                language: "zh-en".to_string(),
                description: "Gentle female chat voice".to_string(),
                voice_type: "ultra_natural".to_string(),
                emotion_support: false,
            },
            VoiceInfo {
                id: voice_types::ZHI_XIAO_MIN,
                name: "智小敏".to_string(),
                gender: "female".to_string(),
                language: "zh-en".to_string(),
                description: "Bright, clear female chat voice".to_string(),
                voice_type: "ultra_natural".to_string(),
                emotion_support: false,
            },
            VoiceInfo {
                id: voice_types::ZHI_XIAO_MAN,
                name: "智小满".to_string(),
                gender: "female".to_string(),
                language: "zh-en".to_string(),
                description: "Warm female marketing voice".to_string(),
                voice_type: "ultra_natural".to_string(),
                emotion_support: false,
            },
            VoiceInfo {
                id: voice_types::NUAN_XIN_A_CAN,
                name: "暖心阿灿".to_string(),
                gender: "male".to_string(),
                language: "zh-en".to_string(),
                description: "Warm, friendly male chat voice".to_string(),
                voice_type: "ultra_natural".to_string(),
                emotion_support: false,
            },
            VoiceInfo {
                id: voice_types::ZHUAN_YE_ZI_XIN,
                name: "专业梓欣".to_string(),
                gender: "female".to_string(),
                language: "zh-en".to_string(),
                description: "Steady professional female chat voice".to_string(),
                voice_type: "ultra_natural".to_string(),
                emotion_support: false,
            },
            VoiceInfo {
                id: voice_types::SUI_HE_LAO_LI,
                name: "随和老李".to_string(),
                gender: "male".to_string(),
                language: "zh-en".to_string(),
                description: "Calm, easygoing male chat voice".to_string(),
                voice_type: "ultra_natural".to_string(),
                emotion_support: false,
            },
            VoiceInfo {
                id: voice_types::WEN_ROU_XIAO_NING,
                name: "温柔小柠".to_string(),
                gender: "female".to_string(),
                language: "zh-en".to_string(),
                description: "Soft, sweet female chat voice".to_string(),
                voice_type: "ultra_natural".to_string(),
                emotion_support: false,
            },
            VoiceInfo {
                id: voice_types::ZHI_XIN_DA_LIN,
                name: "知心大林".to_string(),
                gender: "male".to_string(),
                language: "zh-en".to_string(),
                description: "Thoughtful, steady male chat voice".to_string(),
                voice_type: "ultra_natural".to_string(),
                emotion_support: false,
            },
            VoiceInfo {
                id: voice_types::ZHI_XIAO_HU,
                name: "智小虎".to_string(),
                gender: "boy".to_string(),
                language: "zh-en".to_string(),
                description: "Lively child chat voice".to_string(),
                voice_type: "ultra_natural".to_string(),
                emotion_support: false,
            },
            // 大模型音色
            VoiceInfo {
                id: voice_types::ZHI_BIN,
                name: "智斌".to_string(),
                gender: "male".to_string(),
                language: "zh-en".to_string(),
                description: "Male reading voice for news".to_string(),
                voice_type: "llm".to_string(),
                emotion_support: false,
            },
            VoiceInfo {
                id: voice_types::ZHI_LAN,
                name: "智兰".to_string(),
                gender: "female".to_string(),
                language: "zh-en".to_string(),
                description: "Clear female news voice".to_string(),
                voice_type: "llm".to_string(),
                emotion_support: false,
            },
            VoiceInfo {
                id: voice_types::ZHI_JU,
                name: "智菊".to_string(),
                gender: "female".to_string(),
                language: "zh-en".to_string(),
                description: "Calm, full female reading voice".to_string(),
                voice_type: "llm".to_string(),
                emotion_support: false,
            },
            VoiceInfo {
                id: voice_types::ZHI_YU_LLM,
                name: "智宇".to_string(),
                gender: "male".to_string(),
                language: "zh-en".to_string(),
                description: "Deep, steady male reading voice".to_string(),
                voice_type: "llm".to_string(),
                emotion_support: false,
            },
            VoiceInfo {
                id: voice_types::YUE_HUA,
                name: "月华".to_string(),
                gender: "female".to_string(),
                language: "zh-en".to_string(),
                description: "Gentle female chat voice".to_string(),
                voice_type: "llm".to_string(),
                emotion_support: false,
            },
            VoiceInfo {
                id: voice_types::FEI_JING,
                name: "飞镜".to_string(),
                gender: "male".to_string(),
                language: "zh-en".to_string(),
                description: "Bright, upbeat male chat voice".to_string(),
                voice_type: "llm".to_string(),
                emotion_support: false,
            },
            VoiceInfo {
                id: voice_types::QIAN_ZHANG,
                name: "千嶂".to_string(),
                gender: "male".to_string(),
                language: "zh-en".to_string(),
                description: "Mature, steady male chat voice".to_string(),
                voice_type: "llm".to_string(),
                emotion_support: false,
            },
            VoiceInfo {
                id: voice_types::QIAN_CAO,
                name: "浅草".to_string(),
                gender: "male".to_string(),
                language: "zh-en".to_string(),
                description: "Fresh, natural male chat voice".to_string(),
                voice_type: "llm".to_string(),
                emotion_support: false,
            },
            VoiceInfo {
                id: voice_types::AI_XIAO_XI,
                name: "爱小溪".to_string(),
                gender: "female".to_string(),
                language: "zh".to_string(),
                description: "Female chat voice with emotions".to_string(),
                voice_type: "llm".to_string(),
                emotion_support: true,
            },
            VoiceInfo {
                id: voice_types::AI_XIAO_LUO,
                name: "爱小洛".to_string(),
                gender: "female".to_string(),
                language: "zh".to_string(),
                description: "Female reading voice with emotions".to_string(),
                voice_type: "llm".to_string(),
                emotion_support: true,
            },
            VoiceInfo {
                id: voice_types::AI_XIAO_CHEN,
                name: "爱小辰".to_string(),
                gender: "male".to_string(),
                language: "zh".to_string(),
                description: "Male chat voice with emotions".to_string(),
                voice_type: "llm".to_string(),
                emotion_support: true,
            },
            VoiceInfo {
                id: voice_types::AI_XIAO_HE,
                name: "爱小荷".to_string(),
                gender: "female".to_string(),
                language: "zh".to_string(),
                description: "Female reading voice for stories, radio, and poetry".to_string(),
                voice_type: "llm".to_string(),
                emotion_support: true,
            },
            VoiceInfo {
                id: voice_types::AI_XIAO_SHU,
                name: "爱小树".to_string(),
                gender: "male".to_string(),
                language: "zh".to_string(),
                description: "Male news voice with emotions".to_string(),
                voice_type: "llm".to_string(),
                emotion_support: true,
            },
            VoiceInfo {
                id: voice_types::AI_XIAO_JING,
                name: "爱小静".to_string(),
                gender: "female".to_string(),
                language: "zh".to_string(),
                description: "Female chat voice with emotions".to_string(),
                voice_type: "llm".to_string(),
                emotion_support: true,
            },
            VoiceInfo {
                id: voice_types::AI_XIAO_HAO,
                name: "爱小豪".to_string(),
                gender: "male".to_string(),
                language: "zh".to_string(),
                description: "Male chat voice with emotions".to_string(),
                voice_type: "llm".to_string(),
                emotion_support: true,
            },
            VoiceInfo {
                id: voice_types::AI_XIAO_TONG,
                name: "爱小童".to_string(),
                gender: "boy".to_string(),
                language: "zh".to_string(),
                description: "Boy voice with emotions".to_string(),
                voice_type: "llm".to_string(),
                emotion_support: true,
            },
            VoiceInfo {
                id: voice_types::WE_JAMES,
                name: "WeJames".to_string(),
                gender: "male".to_string(),
                language: "en".to_string(),
                description: "English male voice".to_string(),
                voice_type: "llm".to_string(),
                emotion_support: false,
            },
            VoiceInfo {
                id: voice_types::WE_WINNY,
                name: "WeWinny".to_string(),
                gender: "female".to_string(),
                language: "en".to_string(),
                description: "English female voice".to_string(),
                voice_type: "llm".to_string(),
                emotion_support: false,
            },
            // 精品音色
            VoiceInfo {
                id: voice_types::ZHI_YUN,
                name: "智云".to_string(),
                gender: "male".to_string(),
                language: "zh".to_string(),
                description: "Steady general male voice".to_string(),
                voice_type: "premium".to_string(),
                emotion_support: false,
            },
            VoiceInfo {
                id: voice_types::ZHI_YU,
                name: "智瑜".to_string(),
                gender: "female".to_string(),
                language: "zh".to_string(),
                description: "Soft emotional female voice".to_string(),
                voice_type: "premium".to_string(),
                emotion_support: false,
            },
            VoiceInfo {
                id: voice_types::ZHI_XI,
                name: "智希".to_string(),
                gender: "female".to_string(),
                language: "zh".to_string(),
                description: "Fresh general female voice".to_string(),
                voice_type: "premium".to_string(),
                emotion_support: false,
            },
            VoiceInfo {
                id: voice_types::ZHI_KE,
                name: "智柯".to_string(),
                gender: "male".to_string(),
                language: "zh".to_string(),
                description: "Young, energetic male voice".to_string(),
                voice_type: "premium".to_string(),
                emotion_support: false,
            },
            VoiceInfo {
                id: voice_types::ZHI_HUI,
                name: "智辉".to_string(),
                gender: "male".to_string(),
                language: "zh".to_string(),
                description: "Professional male news voice".to_string(),
                voice_type: "premium".to_string(),
                emotion_support: false,
            },
            VoiceInfo {
                id: voice_types::ZHI_YAN,
                name: "智燕".to_string(),
                gender: "female".to_string(),
                language: "zh".to_string(),
                description: "Poised female news voice".to_string(),
                voice_type: "premium".to_string(),
                emotion_support: false,
            },
            VoiceInfo {
                id: voice_types::ZHI_TONG,
                name: "智彤".to_string(),
                gender: "female".to_string(),
                language: "yue".to_string(),
                description: "Cantonese female voice".to_string(),
                voice_type: "premium".to_string(),
                emotion_support: false,
            },
            VoiceInfo {
                id: voice_types::WE_JACK,
                name: "WeJack".to_string(),
                gender: "male".to_string(),
                language: "en".to_string(),
                description: "English male voice, standard accent".to_string(),
                voice_type: "premium".to_string(),
                emotion_support: false,
            },
        ],
    })
}

/// 获取可用ASR引擎列表
///
/// GET /api/speech/engines
pub async fn get_engine_list() -> impl IntoResponse {
    use crate::services::tencent_speech_service::asr_engines;

    #[derive(Serialize)]
    struct EngineInfo {
        id: String,
        name: String,
        language: String,
        description: String,
    }

    #[derive(Serialize)]
    struct EngineListResponse {
        engines: Vec<EngineInfo>,
    }

    Json(EngineListResponse {
        engines: vec![
            EngineInfo {
                id: asr_engines::ZH_16K.to_string(),
                name: "Chinese general".to_string(),
                language: "zh".to_string(),
                description: "16 kHz, general Chinese speech recognition".to_string(),
            },
            EngineInfo {
                id: asr_engines::EN_16K.to_string(),
                name: "English general".to_string(),
                language: "en".to_string(),
                description: "16 kHz, general English speech recognition".to_string(),
            },
            EngineInfo {
                id: asr_engines::YUE_16K.to_string(),
                name: "Cantonese".to_string(),
                language: "yue".to_string(),
                description: "16 kHz, Cantonese speech recognition".to_string(),
            },
            EngineInfo {
                id: asr_engines::JA_16K.to_string(),
                name: "Japanese".to_string(),
                language: "ja".to_string(),
                description: "16 kHz, Japanese speech recognition".to_string(),
            },
            EngineInfo {
                id: asr_engines::KO_16K.to_string(),
                name: "Korean".to_string(),
                language: "ko".to_string(),
                description: "16 kHz, Korean speech recognition".to_string(),
            },
            EngineInfo {
                id: asr_engines::ZH_PY_16K.to_string(),
                name: "Chinese/English/Cantonese mix".to_string(),
                language: "mixed".to_string(),
                description: "16 kHz, mixed Chinese, English, and Cantonese recognition"
                    .to_string(),
            },
            EngineInfo {
                id: asr_engines::ZH_8K.to_string(),
                name: "Chinese telephony".to_string(),
                language: "zh".to_string(),
                description: "8 kHz, Chinese telephone-audio recognition".to_string(),
            },
            EngineInfo {
                id: asr_engines::EN_8K.to_string(),
                name: "English telephony".to_string(),
                language: "en".to_string(),
                description: "8 kHz, English telephone-audio recognition".to_string(),
            },
        ],
    })
}

/// 格式化文件大小
fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// 清除缓存响应
#[derive(Debug, Serialize)]
pub struct ClearCacheResponse {
    pub success: bool,
    pub deleted_files: usize,
    pub deleted_dirs: usize,
    pub freed_size: u64,
    pub freed_size_formatted: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// 文章缓存管理 API

/// 文章缓存信息请求
#[derive(Debug, Deserialize)]
pub struct ArticleCacheQuery {
    pub source_id: i32,
    pub article_id: i32,
}

/// 音色缓存信息
#[derive(Debug, Serialize)]
pub struct VoiceCacheInfo {
    /// 音色 ID
    pub voice_id: i32,
    /// 音色名称（如果已知）
    pub voice_name: Option<String>,
    /// 角色类型：偶索引全 host、奇索引全 guest，否则 mixed
    pub role: String,
    /// 文件数量
    pub file_count: usize,
    /// 总大小
    pub total_size: u64,
    /// 格式化大小
    pub size_formatted: String,
    /// 对话索引列表
    pub indices: Vec<usize>,
}

/// 文章缓存信息响应
#[derive(Debug, Serialize)]
pub struct ArticleCacheResponse {
    pub source_id: i32,
    pub article_id: i32,
    /// 各音色的缓存信息
    pub voices: Vec<VoiceCacheInfo>,
    /// 总文件数
    pub total_files: usize,
    /// 总大小
    pub total_size: u64,
    pub total_size_formatted: String,
}

/// 获取文章缓存信息
///
/// GET /api/speech/cache/article?source_id=1&article_id=2
pub async fn get_article_cache_info(Query(query): Query<ArticleCacheQuery>) -> impl IntoResponse {
    let tts_dir = get_article_tts_dir(query.source_id, query.article_id);
    let mut voices: Vec<VoiceCacheInfo> = Vec::new();
    let mut total_files = 0usize;
    let mut total_size = 0u64;

    // 遍历音色文件夹
    if let Ok(mut entries) = fs::read_dir(&tts_dir).await {
        while let Ok(Some(voice_entry)) = entries.next_entry().await {
            if let Ok(file_type) = voice_entry.file_type().await {
                if file_type.is_dir() {
                    let voice_name = voice_entry.file_name();
                    let voice_id_str = voice_name.to_string_lossy();

                    // 解析音色 ID
                    if let Ok(voice_id) = voice_id_str.parse::<i32>() {
                        let voice_dir = voice_entry.path();
                        let mut file_count = 0usize;
                        let mut voice_size = 0u64;
                        let mut indices: Vec<usize> = Vec::new();

                        // 统计该音色文件夹内的文件
                        if let Ok(mut files) = fs::read_dir(&voice_dir).await {
                            while let Ok(Some(file_entry)) = files.next_entry().await {
                                if let Ok(meta) = file_entry.metadata().await {
                                    if meta.is_file() {
                                        file_count += 1;
                                        voice_size += meta.len();

                                        // 解析索引
                                        let file_name = file_entry.file_name();
                                        let name_str = file_name.to_string_lossy();
                                        if let Some(idx_str) = name_str.split('.').next() {
                                            if let Ok(idx) = idx_str.parse::<usize>() {
                                                indices.push(idx);
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        if file_count > 0 {
                            indices.sort();

                            // 判断角色类型（根据索引的奇偶性）
                            let role = if indices.iter().all(|&i| i % 2 == 0) {
                                "host".to_string()
                            } else if indices.iter().all(|&i| i % 2 == 1) {
                                "guest".to_string()
                            } else {
                                "mixed".to_string()
                            };

                            total_files += file_count;
                            total_size += voice_size;

                            voices.push(VoiceCacheInfo {
                                voice_id,
                                voice_name: get_voice_name_by_id(voice_id),
                                role,
                                file_count,
                                total_size: voice_size,
                                size_formatted: format_size(voice_size),
                                indices,
                            });
                        }
                    }
                }
            }
        }
    }

    // 按角色排序（host 在前）
    voices.sort_by(|a, b| {
        if a.role == b.role {
            a.voice_id.cmp(&b.voice_id)
        } else if a.role == "host" {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Greater
        }
    });

    Json(ArticleCacheResponse {
        source_id: query.source_id,
        article_id: query.article_id,
        voices,
        total_files,
        total_size,
        total_size_formatted: format_size(total_size),
    })
}

/// 根据音色 ID 获取名称
fn get_voice_name_by_id(voice_id: i32) -> Option<String> {
    match voice_id {
        // 超自然大模型
        300009 => Some("超自然小溪".to_string()),
        300010 => Some("超自然智斌".to_string()),
        300011 => Some("超自然老铁".to_string()),
        300012 => Some("超自然兮儿".to_string()),
        300013 => Some("超自然思琪".to_string()),
        // 大模型
        502001 => Some("智萱".to_string()),
        502002 => Some("智婷".to_string()),
        502003 => Some("智琪".to_string()),
        502004 => Some("智聆".to_string()),
        502005 => Some("智媛".to_string()),
        502006 => Some("智海".to_string()),
        502007 => Some("智斌".to_string()),
        502008 => Some("智康".to_string()),
        502009 => Some("小仙".to_string()),
        502010 => Some("爱小溪".to_string()),
        502011 => Some("小勇".to_string()),
        502012 => Some("小勤".to_string()),
        _ => None,
    }
}

/// 清除文章特定音色缓存请求
#[derive(Debug, Deserialize)]
pub struct ClearArticleVoiceCacheQuery {
    pub source_id: i32,
    pub article_id: i32,
    pub voice_id: i32,
}

/// 清除文章特定音色缓存（仅管理员可用）
///
/// DELETE /api/speech/cache/article/voice?source_id=1&article_id=2&voice_id=502007
pub async fn clear_article_voice_cache(
    // Route only has auth_middleware: AdminClaims performs the live admin check.
    _admin: crate::extract::AdminClaims,
    Query(query): Query<ClearArticleVoiceCacheQuery>,
) -> impl IntoResponse {

    let voice_dir =
        get_article_tts_dir(query.source_id, query.article_id).join(query.voice_id.to_string());

    let mut deleted_files = 0usize;
    let mut freed_size = 0u64;

    if voice_dir.exists() {
        // 统计并删除文件
        if let Ok(mut entries) = fs::read_dir(&voice_dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                if let Ok(meta) = entry.metadata().await {
                    if meta.is_file() {
                        freed_size += meta.len();
                        if fs::remove_file(entry.path()).await.is_ok() {
                            deleted_files += 1;
                        }
                    }
                }
            }
        }

        // 删除空目录
        let _ = fs::remove_dir(&voice_dir).await;
    }

    tracing::info!(
        "Article voice cache cleared: source={}, article={}, voice={}, {} files, {}",
        query.source_id,
        query.article_id,
        query.voice_id,
        deleted_files,
        format_size(freed_size)
    );

    Json(ClearCacheResponse {
        success: true,
        deleted_files,
        deleted_dirs: if deleted_files > 0 { 1 } else { 0 },
        freed_size,
        freed_size_formatted: format_size(freed_size),
        error: None,
    })
    .into_response()
}
use myriad_error::AppError;
