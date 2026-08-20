//! 腾讯云语音服务模块
//!
//! 提供腾讯云 TTS（文本转语音）和 ASR（语音转文本）功能
//!
//! 参考文档:
//! - TTS: https://cloud.tencent.com/document/product/1073/37995
//! - ASR: https://cloud.tencent.com/document/product/1093/35646

use chrono::{DateTime, Utc};
use hmac::{Hmac, KeyInit, Mac};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::Duration;

use super::http_client::ProxyConfig;
use crate::GLOBAL_DYNAMIC_CONFIG;

/// 腾讯云语音服务错误
#[derive(Debug)]
pub enum TencentSpeechError {
    /// API 密钥未配置
    ApiKeyNotConfigured,
    /// 网络请求失败
    NetworkError(String),
    /// API 返回错误
    ApiError { code: String, message: String },
    /// JSON 解析错误
    ParseError(String),
    /// 音频数据无效
    InvalidAudioData(String),
    /// 文本过长
    TextTooLong,
}

impl std::fmt::Display for TencentSpeechError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TencentSpeechError::ApiKeyNotConfigured => {
                write!(f, "Tencent Cloud SecretId/SecretKey not configured")
            }
            TencentSpeechError::NetworkError(msg) => write!(f, "Network error: {}", msg),
            TencentSpeechError::ApiError { code, message } => {
                write!(f, "API error [{}]: {}", code, message)
            }
            TencentSpeechError::ParseError(msg) => write!(f, "Parse error: {}", msg),
            TencentSpeechError::InvalidAudioData(msg) => write!(f, "Invalid audio data: {}", msg),
            TencentSpeechError::TextTooLong => write!(f, "Text too long for TTS synthesis"),
        }
    }
}

impl std::error::Error for TencentSpeechError {}

// TTS 文本转语音

/// TTS 请求参数
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct TtsRequest {
    /// 合成语音的源文本 (中文最大150字，英文最大500字母)
    pub text: String,
    /// 会话ID，用于标识请求
    pub session_id: String,
    /// 音量大小 [-10, 10]，默认0
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume: Option<f32>,
    /// 语速 [-2, 6]，默认0 (1.0倍速)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed: Option<f32>,
    /// 项目ID，默认0
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<i32>,
    /// 音色ID
    /// 大模型音色默认 `voice_types::AI_XIAO_XI`（601000）
    /// 更多音色参见: https://cloud.tencent.com/document/product/1073/34079
    #[serde(skip_serializing_if = "Option::is_none")]
    pub voice_type: Option<i32>,
    /// 主语言类型: 1-中文(默认), 2-英文
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_language: Option<i32>,
    /// 音频采样率: 8000, 16000(默认), 24000
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sample_rate: Option<i32>,
    /// 返回音频格式: wav(默认), mp3, pcm
    #[serde(skip_serializing_if = "Option::is_none")]
    pub codec: Option<String>,
    /// 是否开启时间戳功能
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_subtitle: Option<bool>,
    /// 情感类别 (仅多情感音色支持)
    /// neutral, sad, happy, angry, fear, news, story, radio, poetry, call, sajiao, disgusted, amaze, peaceful, exciting, aojiao, jieshuo
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emotion_category: Option<String>,
    /// 情感强度 [50, 200]，默认100
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emotion_intensity: Option<i32>,
}

impl Default for TtsRequest {
    fn default() -> Self {
        Self {
            text: String::new(),
            session_id: uuid::Uuid::new_v4().to_string(),
            volume: Some(0.0),
            speed: Some(0.0),
            project_id: Some(0),
            voice_type: Some(voice_types::AI_XIAO_XI),
            primary_language: Some(1),  // 中文
            sample_rate: Some(16000),
            codec: Some("mp3".to_string()),
            enable_subtitle: None,
            emotion_category: None,
            emotion_intensity: None,
        }
    }
}

/// TTS 响应时间戳
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[allow(dead_code)]
pub struct TtsSubtitle {
    /// 文本
    pub text: Option<String>,
    /// 开始时间(ms)
    pub begin_time: Option<i32>,
    /// 结束时间(ms)
    pub end_time: Option<i32>,
    /// 开始索引
    pub begin_index: Option<i32>,
    /// 结束索引
    pub end_index: Option<i32>,
    /// 音素
    pub phoneme: Option<String>,
}

/// TTS 响应
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[allow(dead_code)]
pub struct TtsResponse {
    /// Base64编码的音频数据
    pub audio: Option<String>,
    /// 会话ID
    pub session_id: Option<String>,
    /// 时间戳列表
    pub subtitles: Option<Vec<TtsSubtitle>>,
    /// 请求ID
    pub request_id: Option<String>,
}

// ASR 语音转文本

/// ASR 一句话识别请求参数
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct AsrRequest {
    /// 引擎模型类型
    /// 16k_zh: 中文通用, 16k_en: 英语, 16k_yue: 粤语
    /// 8k_zh: 中文电话, 8k_en: 英文电话
    pub eng_ser_vice_type: String,
    /// 语音数据来源: 0-语音URL, 1-语音数据(post body)
    pub source_type: i32,
    /// 音频格式: wav, pcm, ogg-opus, speex, silk, mp3, m4a, aac, amr
    pub voice_format: String,
    /// 用户音频标识
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usr_audio_key: Option<String>,
    /// 语音URL (SourceType=0时必填)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// 语音数据Base64编码 (SourceType=1时必填)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    /// 数据长度 (SourceType=1时必填，未Base64编码时的长度)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_len: Option<i32>,
    /// 是否显示词级别时间戳: 0-不显示, 1-显示(不含标点), 2-显示(含标点)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub word_info: Option<i32>,
    /// 是否过滤脏词: 0-不过滤, 1-过滤, 2-替换为*
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter_dirty: Option<i32>,
    /// 是否过滤语气词: 0-不过滤, 1-部分过滤, 2-严格过滤
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter_modal: Option<i32>,
    /// 是否过滤标点: 0-不过滤, 1-过滤句末标点, 2-过滤所有标点
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter_punc: Option<i32>,
    /// 阿拉伯数字智能转换: 0-不转换, 1-智能转换(默认)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub convert_num_mode: Option<i32>,
    /// 热词表ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hotword_id: Option<String>,
    /// 临时热词表 (格式: "热词1|权重,热词2|权重")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hotword_list: Option<String>,
}

impl Default for AsrRequest {
    fn default() -> Self {
        Self {
            eng_ser_vice_type: "16k_zh".to_string(),
            source_type: 1, // 默认使用语音数据
            voice_format: "wav".to_string(),
            usr_audio_key: Some(uuid::Uuid::new_v4().to_string()),
            url: None,
            data: None,
            data_len: None,
            word_info: Some(0),
            filter_dirty: Some(0),
            filter_modal: Some(0),
            filter_punc: Some(0),
            convert_num_mode: Some(1),
            hotword_id: None,
            hotword_list: None,
        }
    }
}

/// ASR 词时间戳
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct AsrWord {
    /// 词内容
    pub word: Option<String>,
    /// 开始时间(ms)
    pub start_time: Option<i32>,
    /// 结束时间(ms)
    pub end_time: Option<i32>,
}

/// ASR 一句话识别响应
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[allow(dead_code)]
pub struct AsrResponse {
    /// 识别结果
    pub result: Option<String>,
    /// 音频时长(ms)
    pub audio_duration: Option<i32>,
    /// 词列表长度
    pub word_size: Option<i32>,
    /// 词时间戳列表
    pub word_list: Option<Vec<AsrWord>>,
    /// 请求ID
    pub request_id: Option<String>,
}

// 腾讯云API通用响应

/// 腾讯云API错误
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct TencentApiError {
    code: Option<String>,
    message: Option<String>,
}

/// 腾讯云API响应包装
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct TencentApiResponse<T> {
    response: T,
}

/// 带错误的响应
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[allow(dead_code)]
struct ResponseWithError<T> {
    #[serde(flatten)]
    data: T,
    error: Option<TencentApiError>,
    request_id: Option<String>,
}

// 腾讯云语音服务

/// 腾讯云语音服务
pub struct TencentSpeechService {
    client: Client,
    secret_id: String,
    secret_key: String,
    region: String,
}

impl TencentSpeechService {
    /// 创建腾讯云语音服务实例
    pub async fn new() -> Result<Self, TencentSpeechError> {
        let config = GLOBAL_DYNAMIC_CONFIG.read().await;

        let source = config.find_vendor_source(&config.speech_source);
        let tencent = source
            .as_ref()
            .filter(|item| item.kind == "tencent");
        let secret_id = tencent
            .and_then(|item| crate::config::DynamicConfig::nonempty_opt(item.secret_id.as_ref()))
            .or_else(|| {
                config
                    .tencent_secret_id
                    .clone()
                    .map(|k| k.trim().to_string())
                    .filter(|k| !k.is_empty())
            })
            .ok_or(TencentSpeechError::ApiKeyNotConfigured)?;

        let secret_key = tencent
            .and_then(|item| crate::config::DynamicConfig::nonempty_opt(item.secret_key.as_ref()))
            .or_else(|| {
                config
                    .tencent_secret_key
                    .clone()
                    .map(|k| k.trim().to_string())
                    .filter(|k| !k.is_empty())
            })
            .ok_or(TencentSpeechError::ApiKeyNotConfigured)?;

        let region = tencent
            .and_then(|item| crate::config::DynamicConfig::nonempty_opt(item.region.as_ref()))
            .or_else(|| {
                config
                    .tencent_region
                    .clone()
                    .map(|r| r.trim().to_string())
                    .filter(|r| !r.is_empty())
            })
            .unwrap_or_else(|| "ap-guangzhou".to_string());

        drop(config);

        tracing::info!(
            "Initializing Tencent Speech Service: SecretId={}..., Region={}",
            &secret_id[..8.min(secret_id.len())],
            region
        );

        // 创建 HTTP 客户端（带代理支持）
        let proxy_config = ProxyConfig::from_dynamic_config().await;
        let client = Self::create_client(&proxy_config)?;

        Ok(Self {
            client,
            secret_id,
            secret_key,
            region,
        })
    }

    /// 创建HTTP客户端（含 proxy + NoProxy bypass，与全局 HTTP 客户端一致）
    pub(crate) fn create_client(proxy_config: &ProxyConfig) -> Result<Client, TencentSpeechError> {
        let builder = Client::builder()
            .timeout(Duration::from_secs(60))
            .connect_timeout(Duration::from_secs(30))
            .user_agent("Myriad/1.0");

        crate::services::http_client::apply_proxy(builder, proxy_config)
            .and_then(|b| b.build())
            .map_err(|e| TencentSpeechError::NetworkError(e.to_string()))
    }

    /// 生成腾讯云API签名 (TC3-HMAC-SHA256)
    fn sign_request(
        &self,
        service: &str,
        host: &str,
        _action: &str,
        payload: &str,
        timestamp: i64,
    ) -> String {
        let date = DateTime::<Utc>::from_timestamp(timestamp, 0)
            .unwrap()
            .format("%Y-%m-%d")
            .to_string();

        // 步骤1: 拼接规范请求串
        let http_request_method = "POST";
        let canonical_uri = "/";
        let canonical_query_string = "";
        let canonical_headers = format!("content-type:application/json\nhost:{}\n", host);
        let signed_headers = "content-type;host";
        let hashed_request_payload = hex::encode(Sha256::digest(payload.as_bytes()));

        let canonical_request = format!(
            "{}\n{}\n{}\n{}\n{}\n{}",
            http_request_method,
            canonical_uri,
            canonical_query_string,
            canonical_headers,
            signed_headers,
            hashed_request_payload
        );

        // 步骤2: 拼接待签名字符串
        let algorithm = "TC3-HMAC-SHA256";
        let credential_scope = format!("{}/{}/tc3_request", date, service);
        let hashed_canonical_request = hex::encode(Sha256::digest(canonical_request.as_bytes()));

        let string_to_sign = format!(
            "{}\n{}\n{}\n{}",
            algorithm, timestamp, credential_scope, hashed_canonical_request
        );

        // 步骤3: 计算签名
        let secret_date = Self::hmac_sha256(format!("TC3{}", self.secret_key).as_bytes(), &date);
        let secret_service = Self::hmac_sha256(&secret_date, service);
        let secret_signing = Self::hmac_sha256(&secret_service, "tc3_request");
        let signature = hex::encode(Self::hmac_sha256(&secret_signing, &string_to_sign));

        // 步骤4: 拼接 Authorization
        format!(
            "{} Credential={}/{}, SignedHeaders={}, Signature={}",
            algorithm, self.secret_id, credential_scope, signed_headers, signature
        )
    }

    /// HMAC-SHA256
    fn hmac_sha256(key: &[u8], data: &str) -> Vec<u8> {
        let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC error");
        mac.update(data.as_bytes());
        mac.finalize().into_bytes().to_vec()
    }

    /// 发送腾讯云API请求
    async fn send_request<T: serde::de::DeserializeOwned>(
        &self,
        service: &str,
        action: &str,
        version: &str,
        payload: &str,
    ) -> Result<T, TencentSpeechError> {
        let host = format!("{}.tencentcloudapi.com", service);
        let url = format!("https://{}", host);
        let timestamp = Utc::now().timestamp();

        let authorization = self.sign_request(service, &host, action, payload, timestamp);

        tracing::debug!("Calling Tencent {} API: {}", service, action);
        tracing::debug!(
            "SecretId: {}...",
            &self.secret_id[..8.min(self.secret_id.len())]
        );
        tracing::debug!("Timestamp: {}", timestamp);
        tracing::debug!("Region: {}", self.region);

        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Host", &host)
            .header("X-TC-Action", action)
            .header("X-TC-Version", version)
            .header("X-TC-Timestamp", timestamp.to_string())
            .header("X-TC-Region", &self.region)
            .header("Authorization", &authorization)
            .body(payload.to_string())
            .send()
            .await
            .map_err(|e| TencentSpeechError::NetworkError(e.to_string()))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| TencentSpeechError::NetworkError(e.to_string()))?;

        tracing::debug!("Tencent {} API response status: {}", service, status);
        if !status.is_success() || body.contains("Error") {
            tracing::warn!("Tencent API response body: {}", body);
        }

        if !status.is_success() {
            return Err(TencentSpeechError::NetworkError(format!(
                "HTTP {}: {}",
                status, body
            )));
        }

        // 解析响应
        let api_response: TencentApiResponse<ResponseWithError<T>> = serde_json::from_str(&body)
            .map_err(|e| {
                TencentSpeechError::ParseError(format!(
                    "Failed to parse response: {} - {}",
                    e, body
                ))
            })?;

        // 检查错误
        if let Some(error) = api_response.response.error {
            return Err(TencentSpeechError::ApiError {
                code: error.code.unwrap_or_default(),
                message: error.message.unwrap_or_default(),
            });
        }

        Ok(api_response.response.data)
    }

    // TTS 文本转语音

    /// 文本转语音
    ///
    /// # Arguments
    /// * `request` - TTS请求参数
    ///
    /// # Returns
    /// * `TtsResponse` - 包含Base64编码的音频数据
    ///
    /// # Example
    /// ```ignore
    /// use base64::{engine::general_purpose::STANDARD, Engine as _};
    /// let service = TencentSpeechService::new().await?;
    /// let request = TtsRequest {
    /// text: "你好，世界".to_string(),
    /// ..Default::default()
    /// };
    /// let response = service.text_to_speech(request).await?;
    /// let audio_bytes = STANDARD.decode(response.audio.as_ref().unwrap())?;
    /// ```
    pub async fn text_to_speech(
        &self,
        request: TtsRequest,
    ) -> Result<TtsResponse, TencentSpeechError> {
        // 验证文本长度
        let char_count = request.text.chars().count();
        if char_count > 150 {
            return Err(TencentSpeechError::TextTooLong);
        }

        let payload = serde_json::to_string(&request)
            .map_err(|e| TencentSpeechError::ParseError(e.to_string()))?;

        self.send_request::<TtsResponse>("tts", "TextToVoice", "2019-08-23", &payload)
            .await
    }

    // ASR 语音转文本

    /// 一句话语音识别
    ///
    /// # Arguments
    /// * `request` - ASR请求参数
    ///
    /// # Returns
    /// * `AsrResponse` - 包含识别结果
    ///
    /// # Example
    /// ```ignore
    /// use base64::{engine::general_purpose::STANDARD, Engine as _};
    /// let service = TencentSpeechService::new().await?;
    /// let audio_data = std::fs::read("audio.wav")?;
    /// let request = AsrRequest {
    /// data: Some(STANDARD.encode(&audio_data)),
    /// data_len: Some(audio_data.len() as i32),
    /// ..Default::default()
    /// };
    /// let response = service.speech_to_text(request).await?;
    /// println!("识别结果: {}", response.result.unwrap());
    /// ```
    pub async fn speech_to_text(
        &self,
        request: AsrRequest,
    ) -> Result<AsrResponse, TencentSpeechError> {
        // 验证请求
        if request.source_type == 0 && request.url.is_none() {
            return Err(TencentSpeechError::InvalidAudioData(
                "URL is required when source_type is 0".to_string(),
            ));
        }
        if request.source_type == 1 && (request.data.is_none() || request.data_len.is_none()) {
            return Err(TencentSpeechError::InvalidAudioData(
                "Data and data_len are required when source_type is 1".to_string(),
            ));
        }

        let payload = serde_json::to_string(&request)
            .map_err(|e| TencentSpeechError::ParseError(e.to_string()))?;

        self.send_request::<AsrResponse>("asr", "SentenceRecognition", "2019-06-14", &payload)
            .await
    }
}

// 音色ID定义

/// 腾讯云TTS音色分类
#[allow(dead_code)]
pub mod voice_types {
    // 超自然大模型音色 (最高品质)
    /// 智小虎 - 聊天童声 (超自然大模型)
    pub const ZHI_XIAO_HU: i32 = 502007;
    /// 智小悟 - 聊天男声 (超自然大模型)
    pub const ZHI_XIAO_WU: i32 = 502006;
    /// 智小解 - 解说男声 (超自然大模型)
    pub const ZHI_XIAO_JIE: i32 = 502005;
    /// 智小满 - 营销女声 (超自然大模型)
    pub const ZHI_XIAO_MAN: i32 = 502004;
    /// 智小敏 - 聊天女声 (超自然大模型)
    pub const ZHI_XIAO_MIN: i32 = 502003;
    /// 智小柔 - 聊天女声 (超自然大模型)
    pub const ZHI_XIAO_ROU: i32 = 502001;
    /// 暖心阿灿 - 聊天男声 (超自然大模型)
    pub const NUAN_XIN_A_CAN: i32 = 602004;
    /// 专业梓欣 - 聊天女声 (超自然大模型)
    pub const ZHUAN_YE_ZI_XIN: i32 = 602005;
    /// 懂事少年 - 特色男声 (超自然大模型)
    pub const DONG_SHI_SHAO_NIAN: i32 = 603000;
    /// 潇湘妹妹 - 特色女声 (超自然大模型)
    pub const XIAO_XIANG_MEI_MEI: i32 = 603001;
    /// 软萌心心 - 特色男童声 (超自然大模型)
    pub const RUAN_MENG_XIN_XIN: i32 = 603002;
    /// 随和老李 - 聊天男声 (超自然大模型)
    pub const SUI_HE_LAO_LI: i32 = 603003;
    /// 温柔小柠 - 聊天女声 (超自然大模型)
    pub const WEN_ROU_XIAO_NING: i32 = 603004;
    /// 知心大林 - 聊天男声 (超自然大模型)
    pub const ZHI_XIN_DA_LIN: i32 = 603005;
    /// 爱小悠 - 聊天女声 (超自然大模型)
    pub const AI_XIAO_YOU: i32 = 602003;

    // 大模型音色 (高品质)
    /// 智斌 - 阅读男声 (大模型)
    pub const ZHI_BIN: i32 = 501000;
    /// 智兰 - 资讯女声 (大模型)
    pub const ZHI_LAN: i32 = 501001;
    /// 智菊 - 阅读女声 (大模型)
    pub const ZHI_JU: i32 = 501002;
    /// 智宇 - 阅读男声 (大模型)
    pub const ZHI_YU_LLM: i32 = 501003;
    /// 月华 - 聊天女声 (大模型)
    pub const YUE_HUA: i32 = 501004;
    /// 飞镜 - 聊天男声 (大模型)
    pub const FEI_JING: i32 = 501005;
    /// 千嶂 - 聊天男声 (大模型)
    pub const QIAN_ZHANG: i32 = 501006;
    /// 浅草 - 聊天男声 (大模型)
    pub const QIAN_CAO: i32 = 501007;
    /// WeJames - 外语男声 (大模型)
    pub const WE_JAMES: i32 = 501008;
    /// WeWinny - 外语女声 (大模型)
    pub const WE_WINNY: i32 = 501009;
    /// 爱小溪 - 聊天女声 (大模型，多情感)
    pub const AI_XIAO_XI: i32 = 601000;
    /// 爱小洛 - 阅读女声 (大模型，多情感)
    pub const AI_XIAO_LUO: i32 = 601001;
    /// 爱小辰 - 聊天男声 (大模型，多情感)
    pub const AI_XIAO_CHEN: i32 = 601002;
    /// 爱小荷 - 阅读女声 (大模型，多情感)
    pub const AI_XIAO_HE: i32 = 601003;
    /// 爱小树 - 资讯男声 (大模型，多情感)
    pub const AI_XIAO_SHU: i32 = 601004;
    /// 爱小静 - 聊天女声 (大模型，多情感)
    pub const AI_XIAO_JING: i32 = 601005;
    /// 爱小耀 - 阅读男声 (大模型，多情感)
    pub const AI_XIAO_YAO: i32 = 601006;
    /// 爱小叶 - 聊天女声 (大模型，多情感)
    pub const AI_XIAO_YE: i32 = 601007;
    /// 爱小豪 - 聊天男声 (大模型，多情感)
    pub const AI_XIAO_HAO: i32 = 601008;
    /// 爱小芊 - 聊天女声 (大模型，多情感)
    pub const AI_XIAO_QIAN: i32 = 601009;
    /// 爱小娇 - 聊天女声 (大模型，多情感)
    pub const AI_XIAO_JIAO: i32 = 601010;
    /// 爱小川 - 聊天男声 (大模型)
    pub const AI_XIAO_CHUAN: i32 = 601011;
    /// 爱小璟 - 特色女声 (大模型)
    pub const AI_XIAO_JING2: i32 = 601012;
    /// 爱小伊 - 阅读女声 (大模型)
    pub const AI_XIAO_YI: i32 = 601013;
    /// 爱小简 - 聊天男声 (大模型)
    pub const AI_XIAO_JIAN: i32 = 601014;
    /// 爱小童 - 男童声 (大模型，多情感)
    pub const AI_XIAO_TONG: i32 = 601015;

    // 精品音色 (中等品质)
    /// 智云 - 通用男声 (精品)
    pub const ZHI_YUN: i32 = 101004;
    /// 智瑜 - 情感女声 (精品)
    pub const ZHI_YU: i32 = 101001;
    /// 智燕 - 新闻女声 (精品)
    pub const ZHI_YAN: i32 = 101011;
    /// 智辉 - 新闻男声 (精品)
    pub const ZHI_HUI: i32 = 101013;
    /// 智萌 - 男童声 (精品)
    pub const ZHI_MENG: i32 = 101015;
    /// 智甜 - 女童声 (精品)
    pub const ZHI_TIAN: i32 = 101016;
    /// 智彤 - 粤语女声 (精品)
    pub const ZHI_TONG: i32 = 101019;
    /// 智瑞 - 新闻男声 (精品)
    pub const ZHI_RUI: i32 = 101021;
    /// 智希 - 通用女声 (精品)
    pub const ZHI_XI: i32 = 101026;
    /// 智梅 - 通用女声 (精品)
    pub const ZHI_MEI: i32 = 101027;
    /// 智柯 - 通用男声 (精品)
    pub const ZHI_KE: i32 = 101030;
    /// WeJack - 英文男声 (精品)
    pub const WE_JACK: i32 = 101050;
    /// 智友 - 通用男声 (精品)
    pub const ZHI_YOU: i32 = 101054;
    /// 智付 - 通用女声 (精品)
    pub const ZHI_FU: i32 = 101055;
    /// 爱小静 - 对话女声 (精品)
    pub const AI_XIAO_JING_PREMIUM: i32 = 301037;
}

/// ASR引擎类型
pub mod asr_engines {
    /// 16k中文通用
    pub const ZH_16K: &str = "16k_zh";
    /// 16k英文通用
    pub const EN_16K: &str = "16k_en";
    /// 16k粤语
    pub const YUE_16K: &str = "16k_yue";
    /// 16k日语
    pub const JA_16K: &str = "16k_ja";
    /// 16k韩语
    pub const KO_16K: &str = "16k_ko";
    /// 8k中文电话
    pub const ZH_8K: &str = "8k_zh";
    /// 8k英文电话
    pub const EN_8K: &str = "8k_en";
    /// 16k中英粤混合
    pub const ZH_PY_16K: &str = "16k_zh-PY";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tts_request_default() {
        let request = TtsRequest::default();
        assert!(!request.session_id.is_empty());
        assert_eq!(request.voice_type, Some(voice_types::AI_XIAO_XI));
        assert_eq!(request.codec, Some("mp3".to_string()));
    }

    #[test]
    fn test_asr_request_default() {
        let request = AsrRequest::default();
        assert_eq!(request.eng_ser_vice_type, "16k_zh");
        assert_eq!(request.source_type, 1);
        assert_eq!(request.voice_format, "wav");
    }

    #[test]
    fn create_client_honors_proxy_and_noproxy_bypass() {
        let config = ProxyConfig {
            enabled: true,
            proxy_url: Some("http://127.0.0.1:9".to_string()),
            bypass_list: vec!["localhost".into(), "127.0.0.1".into(), "tencentcloudapi.com".into()],
        };
        TencentSpeechService::create_client(&config).expect("proxy client with NoProxy");
        let direct = ProxyConfig { enabled: false, proxy_url: None, bypass_list: vec![] };
        TencentSpeechService::create_client(&direct).expect("direct client");
    }

    #[test]
    fn analyzer_and_tencent_source_wire_apply_proxy() {
        assert!(include_str!("analyzer.rs").contains("apply_proxy"));
        assert!(include_str!("tencent_speech_service.rs").contains("apply_proxy"));
        // MYR-019: analyzer must fail closed, not silently direct-connect.
        assert!(include_str!("analyzer.rs").contains("proxy_is_required"));
        assert!(include_str!("analyzer.rs").contains("fail-closed"));
        assert!(include_str!("http_client.rs").contains("resolve_client_or_fail_closed"));
    }

    /// HMAC-SHA256 roundtrip for digest-generation alignment (hmac 0.13 + sha2 0.11).
    #[test]
    fn hmac_sha256_roundtrip_matches_known_vector() {
        // RFC 4231 test case 1 (truncated to HMAC-SHA256)
        let key = b"\x0b\x0b\x0b\x0b\x0b\x0b\x0b\x0b\x0b\x0b\x0b\x0b\x0b\x0b\x0b\x0b\x0b\x0b\x0b\x0b";
        let data = "Hi There";
        let tag = TencentSpeechService::hmac_sha256(key, data);
        assert_eq!(
            hex::encode(&tag),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );

        // Same key+data yields same tag; different data does not.
        let again = TencentSpeechService::hmac_sha256(key, data);
        assert_eq!(tag, again);
        let other = TencentSpeechService::hmac_sha256(key, "Hi There!");
        assert_ne!(tag, other);
    }

    /// TC3-HMAC-SHA256 Authorization header is stable for fixed inputs.
    #[test]
    fn tc3_sign_request_is_deterministic_and_well_formed() {
        let svc = TencentSpeechService {
            client: Client::new(),
            secret_id: "AKIDtestSecretId".into(),
            secret_key: "testSecretKey".into(),
            region: "ap-guangzhou".into(),
        };
        let ts = 1_700_000_000_i64;
        let host = "tts.tencentcloudapi.com";
        let payload = r#"{"Text":"hello"}"#;
        let auth1 = svc.sign_request("tts", host, "TextToVoice", payload, ts);
        let auth2 = svc.sign_request("tts", host, "TextToVoice", payload, ts);
        assert_eq!(auth1, auth2);
        assert!(auth1.starts_with("TC3-HMAC-SHA256 Credential=AKIDtestSecretId/"));
        assert!(auth1.contains("SignedHeaders=content-type;host"));
        assert!(auth1.contains("Signature="));
        // Timestamp change must change the signature portion.
        let auth3 = svc.sign_request("tts", host, "TextToVoice", payload, ts + 1);
        assert_ne!(auth1, auth3);
    }
}
