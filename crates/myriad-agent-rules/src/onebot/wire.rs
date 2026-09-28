// Ported from nagisa 0.12.0 — crates/nagisa-onebot/src/wire.rs
// Source: https://github.com/djkcyl/nagisa
// License: MIT OR Apache-2.0 (see LICENSES/Apache-2.0.txt)

//! OneBot v11 wire 层类型(serde)。
//!
//! 所有结构都**宽松**:`#[serde(default)]`、`Option`,wire 枚举带 `#[serde(other)] Unknown` 分支。
//! 我们绝不 `deny_unknown_fields`——协议实现(Lagrange / NapCat / LLOneBot)随时可能加字段,
//! 解不开的事件必须降级为 `Event::Raw`,绝不报错。
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// wire 上的单个消息段:`{ "type": "...", "data": { ... } }`。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WireSegment {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub data: Map<String, Value>,
}

/// 把 `serde_json::Value` 读成字符串,兼容某些端用的 int/number/bool wire 形态(如 `emoji_id`
/// 可能以 JSON 数字到达)。这是规范的「宽松 Value→String」转换,由 [`WireSegment::str_field`]
/// 与 `decode::value_as_string` 共用。注意 `adapter::data_str` 故意更严(只接受真正的 JSON
/// 字符串),不走这里。
pub fn value_as_string(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

impl WireSegment {
    pub fn new(kind: impl Into<String>, data: Map<String, Value>) -> Self {
        WireSegment {
            kind: kind.into(),
            data,
        }
    }
    /// 取一个 `data` 字段作字符串,兼容 int/number wire 形态。
    pub fn str_field(&self, key: &str) -> Option<String> {
        self.data.get(key).and_then(value_as_string)
    }
    /// 取一个 `data` 字段作 i64,兼容数字字符串 wire 形态。
    pub fn i64_field(&self, key: &str) -> Option<i64> {
        match self.data.get(key)? {
            Value::Number(n) => n.as_i64(),
            Value::String(s) => s.parse().ok(),
            _ => None,
        }
    }
    /// 取一个 `data` 字段作 bool,兼容 `"true"`/`1`/`"1"` 等形态。
    pub fn bool_field(&self, key: &str) -> Option<bool> {
        match self.data.get(key)? {
            Value::Bool(b) => Some(*b),
            Value::String(s) => match s.as_str() {
                "true" | "yes" | "1" => Some(true),
                "false" | "no" | "0" => Some(false),
                _ => None,
            },
            Value::Number(n) => n.as_i64().map(|v| v != 0),
            _ => None,
        }
    }
}

/// 消息事件的 `message` 字段——要么是段数组(OneBot 数组格式),要么是裸 CQ 字符串
/// (`message_format: "string"`)。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WireMessage {
    Array(Vec<WireSegment>),
    Cq(String),
}

impl Default for WireMessage {
    fn default() -> Self {
        WireMessage::Array(Vec::new())
    }
}

/// 消息事件的 `sender` 子对象(尽力而为;全可选)。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct WireSender {
    #[serde(default)]
    pub user_id: Option<i64>,
    #[serde(default)]
    pub nickname: Option<String>,
    #[serde(default)]
    pub card: Option<String>,
}

/// 统一的入站事件外层封包。每个字段都保持可选,因为具体形态取决于 `post_type`;`extra` 兜住其余
/// 一切,使 `Event::Raw` 逃生通道无损。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RawEventJson {
    #[serde(default)]
    pub time: i64,
    #[serde(default)]
    pub self_id: i64,
    #[serde(default)]
    pub post_type: Option<String>,

    // 消息
    #[serde(default)]
    pub message_type: Option<String>,
    #[serde(default)]
    pub sub_type: Option<String>,
    /// OneBot 规范写的是 `int32`，但 NapCat 实测会发出大于 `i32::MAX` 的值
    /// （官方 issue #213 的报文里是 `3425462029`）。按 `i32` 读会让整条事件解不开，
    /// 所以这里放宽到 `i64`——宽松层跟现实，不跟规范字面。
    #[serde(default)]
    pub message_id: Option<i64>,
    #[serde(default)]
    pub group_id: Option<i64>,
    #[serde(default)]
    pub user_id: Option<i64>,
    #[serde(default)]
    pub message: Option<WireMessage>,
    #[serde(default)]
    pub raw_message: Option<String>,
    #[serde(default)]
    pub sender: Option<WireSender>,
    #[serde(default)]
    pub font: Option<i32>,
    /// LLOneBot/NapCat 私聊消息的会话内序号（与 message_id 并存的 typed seq）。
    #[serde(default)]
    pub message_seq: Option<i64>,

    // 通知
    #[serde(default)]
    pub notice_type: Option<String>,
    #[serde(default)]
    pub operator_id: Option<i64>,
    #[serde(default)]
    pub target_id: Option<i64>,
    #[serde(default)]
    pub sender_id: Option<i64>,
    #[serde(default)]
    pub duration: Option<i64>,

    // 请求
    #[serde(default)]
    pub request_type: Option<String>,
    #[serde(default)]
    pub comment: Option<String>,
    #[serde(default)]
    pub flag: Option<String>,

    // meta
    #[serde(default)]
    pub meta_event_type: Option<String>,
    #[serde(default)]
    pub interval: Option<i64>,
    // meta 心跳的 status 对象
    #[serde(default)]
    pub status: Option<Value>,

    /// 上面没捕获的一切,使 `Event::Raw` 保留完整载荷。
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// 动作响应封包:`{ status, retcode, data, echo, message }`。
#[derive(Clone, Debug, Default, Deserialize)]
pub struct RespJson {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub retcode: i64,
    #[serde(default)]
    pub data: Value,
    #[serde(default)]
    pub echo: Value,
    /// 部分实现在失败时放一段人类可读消息。三个字段各自独立:NapCat 每个响应都同时带
    /// `message` 和 `wording`,写成别名会让 serde 报重复字段,整帧解不开。
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub wording: Option<String>,
    #[serde(default)]
    pub msg: Option<String>,
}

/// 入站帧 demux:动作响应带 `echo`,事件不带。`#[serde(untagged)]` 先试 `Resp`,再回退到裸
/// 事件。真正的判别字段只有 `echo`——它在 [`EchoEnvelope`] 里是必填(无 `#[serde(default)]`),
/// 故无 `echo` 的事件解 `Resp` 失败、落到 `Event`。`status`/`retcode` 都带默认值,不参与判别。
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum Inbound {
    Resp(EchoEnvelope),
    Event(Box<RawEventJson>),
}

/// 响应的最小视图,纯粹用于提取 `echo` 关联键。找到对应槽后再把完整正文重新按 `RespJson` 解析。
#[derive(Clone, Debug, Deserialize)]
pub struct EchoEnvelope {
    /// 当且仅当此帧是动作响应时存在。`echo` 是任意 JSON,由服务端原样回传。
    pub echo: Value,
    #[serde(default)]
    pub status: Value,
    #[serde(default)]
    pub retcode: Value,
    #[serde(flatten)]
    pub rest: Map<String, Value>,
}
