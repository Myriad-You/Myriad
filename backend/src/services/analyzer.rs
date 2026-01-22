// AI analysis service using Google Gemini API or OpenAI-compatible API
use anyhow::{Context, Result};
use reqwest::{Client, Proxy};
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::services::http_client::{GeminiApiUrl, ProxyConfig};

// ============= Gemini API Structures =============
#[derive(Debug, Serialize)]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
}

#[derive(Debug, Serialize)]
struct GeminiContent {
    parts: Vec<GeminiPart>,
}

#[derive(Debug, Serialize)]
struct GeminiPart {
    text: String,
}

#[derive(Debug, Deserialize)]
struct GeminiResponse {
    candidates: Vec<GeminiCandidate>,
}

#[derive(Debug, Deserialize)]
struct GeminiCandidate {
    content: GeminiCandidateContent,
}

#[derive(Debug, Deserialize)]
struct GeminiCandidateContent {
    parts: Vec<GeminiResponsePart>,
}

#[derive(Debug, Deserialize)]
struct GeminiResponsePart {
    text: String,
}

// ============= OpenAI-compatible API Structures =============
#[derive(Debug, Serialize)]
struct OpenAIRequest {
    model: String,
    messages: Vec<OpenAIMessage>,
}

#[derive(Debug, Serialize)]
struct OpenAIMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct OpenAIResponse {
    choices: Vec<OpenAIChoice>,
}

#[derive(Debug, Deserialize)]
struct OpenAIChoice {
    message: OpenAIResponseMessage,
}

#[derive(Debug, Deserialize)]
struct OpenAIResponseMessage {
    content: String,
}

// ============= AI Provider Enum =============
#[derive(Debug, Clone, PartialEq)]
pub enum AiProvider {
    Gemini,
    OpenAI,
}

impl AiProvider {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "openai" => Self::OpenAI,
            _ => Self::Gemini,
        }
    }
}

pub struct AiAnalyzer {
    client: Client,
    provider: AiProvider,
    api_key: String,
    model: String,
    base_url: Option<String>, // For OpenAI-compatible APIs
}

impl AiAnalyzer {
    pub async fn new(
        provider: AiProvider,
        api_key: String,
        model: String,
        base_url: Option<String>,
    ) -> Self {
        let proxy_config = ProxyConfig::from_dynamic_config().await;

        let mut builder = Client::builder()
            .timeout(Duration::from_secs(120))
            .connect_timeout(Duration::from_secs(30))
            .user_agent("Myriad/1.0");

        if proxy_config.should_use_proxy() {
            if let Some(proxy_url) = proxy_config.proxy_url.as_deref() {
                if let Ok(proxy) = Proxy::all(proxy_url) {
                    builder = builder.proxy(proxy);
                }
            }
        }

        let client = builder.build().unwrap_or_else(|_| Client::new());
        Self {
            client,
            provider,
            api_key,
            model,
            base_url,
        }
    }

    pub async fn analyze_profile(&self, profile_data: &serde_json::Value) -> Result<String> {
        match self.provider {
            AiProvider::Gemini => self.analyze_with_gemini(profile_data).await,
            AiProvider::OpenAI => self.analyze_with_openai(profile_data).await,
        }
    }

    async fn analyze_with_gemini(&self, profile_data: &serde_json::Value) -> Result<String> {
        // 检查是否提供了自定义提示词
        let user_prompt = if let Some(prompt) = profile_data.get("prompt").and_then(|p| p.as_str())
        {
            prompt.to_string()
        } else {
            let system_prompt = "You are an expert data analyst specializing in social media and professional profiles.";
            format!(
                "{}\n\nAnalyze the following user profile data and provide insights on their professional background, skills, interests, and online presence:\n\n{}",
                system_prompt,
                serde_json::to_string_pretty(profile_data)?
            )
        };

        let request_body = GeminiRequest {
            contents: vec![GeminiContent {
                parts: vec![GeminiPart { text: user_prompt }],
            }],
        };

        let url = GeminiApiUrl::generate_content_url(&self.model).await;

        tracing::info!("🔗 Calling Gemini API (model: {})", self.model);

        let response = self
            .client
            .post(&url)
            .query(&[("key", &self.api_key)])
            .json(&request_body)
            .send()
            .await
            .context("Failed to send request to Gemini API")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(anyhow::anyhow!(
                "Gemini API error {}: {}",
                status,
                error_text
            ));
        }

        let gemini_response: GeminiResponse = response
            .json()
            .await
            .context("Failed to parse Gemini API response")?;

        let analysis = gemini_response
            .candidates
            .first()
            .and_then(|c| c.content.parts.first())
            .map(|p| p.text.clone())
            .unwrap_or_else(|| "No analysis generated".to_string());

        Ok(analysis)
    }

    async fn analyze_with_openai(&self, profile_data: &serde_json::Value) -> Result<String> {
        // 检查是否提供了自定义提示词
        let user_content = if let Some(prompt) = profile_data.get("prompt").and_then(|p| p.as_str())
        {
            prompt.to_string()
        } else {
            format!(
                "Analyze the following user profile data and provide insights on their professional background, skills, interests, and online presence:\n\n{}",
                serde_json::to_string_pretty(profile_data)?
            )
        };

        let request_body = OpenAIRequest {
            model: self.model.clone(),
            messages: vec![
                OpenAIMessage {
                    role: "system".to_string(),
                    content: "You are an expert data analyst specializing in social media and professional profiles.".to_string(),
                },
                OpenAIMessage {
                    role: "user".to_string(),
                    content: user_content,
                },
            ],
        };

        let base_url = self
            .base_url
            .as_deref()
            .unwrap_or("https://api.openai.com/v1");

        // 智能处理 base_url：如果已经包含 /chat/completions，直接使用；否则拼接
        let url = if base_url.ends_with("/chat/completions") {
            tracing::debug!("Base URL already contains /chat/completions, using as-is");
            base_url.to_string()
        } else if base_url.ends_with('/') {
            tracing::debug!("Base URL ends with /, appending chat/completions");
            format!("{}chat/completions", base_url)
        } else {
            tracing::debug!("Base URL needs path separator, appending /chat/completions");
            format!("{}/chat/completions", base_url)
        };

        tracing::info!(
            "🔗 Calling OpenAI-compatible API: {} (model: {})",
            url,
            self.model
        );

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .context("Failed to send request to OpenAI API")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(anyhow::anyhow!(
                "OpenAI API error {}: {}",
                status,
                error_text
            ));
        }

        let openai_response: OpenAIResponse = response
            .json()
            .await
            .context("Failed to parse OpenAI API response")?;

        let analysis = openai_response
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_else(|| "No analysis generated".to_string());

        Ok(analysis)
    }

    /// 简单的分析方法（用于 Tapp API）
    pub async fn analyze(&self, prompt: &str) -> Result<String> {
        let data = serde_json::json!({ "prompt": prompt });
        self.analyze_profile(&data).await
    }

    /// 带系统提示的分析方法（用于 Tapp API）
    pub async fn analyze_with_system(&self, system: &str, prompt: &str) -> Result<String> {
        match self.provider {
            AiProvider::Gemini => {
                let full_prompt = format!("{}\n\n{}", system, prompt);
                let data = serde_json::json!({ "prompt": full_prompt });
                self.analyze_profile(&data).await
            }
            AiProvider::OpenAI => {
                let request_body = OpenAIRequest {
                    model: self.model.clone(),
                    messages: vec![
                        OpenAIMessage {
                            role: "system".to_string(),
                            content: system.to_string(),
                        },
                        OpenAIMessage {
                            role: "user".to_string(),
                            content: prompt.to_string(),
                        },
                    ],
                };

                let base_url = self.base_url.as_deref().unwrap_or("https://api.openai.com");
                let url = format!("{}/v1/chat/completions", base_url.trim_end_matches('/'));

                let response = self
                    .client
                    .post(&url)
                    .header("Authorization", format!("Bearer {}", self.api_key))
                    .header("Content-Type", "application/json")
                    .json(&request_body)
                    .send()
                    .await
                    .context("Failed to send request to OpenAI API")?;

                if !response.status().is_success() {
                    let status = response.status();
                    let error_text = response
                        .text()
                        .await
                        .unwrap_or_else(|_| "Unknown error".to_string());
                    return Err(anyhow::anyhow!(
                        "OpenAI API error {}: {}",
                        status,
                        error_text
                    ));
                }

                let openai_response: OpenAIResponse = response
                    .json()
                    .await
                    .context("Failed to parse OpenAI API response")?;

                let analysis = openai_response
                    .choices
                    .first()
                    .map(|c| c.message.content.clone())
                    .unwrap_or_else(|| "No analysis generated".to_string());

                Ok(analysis)
            }
        }
    }

    // TODO: Add more analysis methods
    // pub async fn generate_summary(&self, profiles: Vec<serde_json::Value>) -> Result<String>
    // pub async fn extract_skills(&self, profile_data: &serde_json::Value) -> Result<Vec<String>>
}
