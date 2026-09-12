//! Feishu long-connection Frame (pbbp2) and session loop.
//!
//! Framing matches `larksuite-oapi-sdk-rs` 0.2.0 `proto/ws.proto` (MIT).
//! Handshake: POST `/callback/ws/endpoint` with AppID/AppSecret, then WSS.
//! method 0 = control (ping/pong); method 1 = data (event + ACK).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::{SinkExt, StreamExt};
use myriad_agent_rules::channel::{
    classify_feishu_handshake, parse_feishu_event_envelope, ConnectFailureKind,
    FEISHU_CARD_ACTION_TRIGGER, FEISHU_MESSAGE_RECEIVE_V1,
};
use myriad_error::redact_secrets;
use prost::Message as ProstMessage;
use tokio::sync::{watch, Mutex};
use tokio_tungstenite::tungstenite::Message;
use tracing::{info, warn};

const METHOD_CONTROL: i32 = 0;
const METHOD_DATA: i32 = 1;
const MSG_TYPE_EVENT: &str = "event";
const MSG_TYPE_PING: &str = "ping";
const MSG_TYPE_PONG: &str = "pong";
const HEADER_TYPE: &str = "type";
const HEADER_SUM: &str = "sum";
const HEADER_SEQ: &str = "seq";
const HEADER_MESSAGE_ID: &str = "message_id";
const HEADER_BIZ_RT: &str = "biz_rt";
const HEADER_HANDSHAKE_STATUS: &str = "Handshake-Status";
const HEADER_HANDSHAKE_MSG: &str = "Handshake-Msg";
const HEADER_HANDSHAKE_AUTH_ERR_CODE: &str = "Handshake-Autherrcode";
const WS_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone, PartialEq, prost::Message)]
pub struct Header {
    #[prost(string, required, tag = "1")]
    pub key: String,
    #[prost(string, required, tag = "2")]
    pub value: String,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct Frame {
    #[prost(uint64, required, tag = "1")]
    pub seq_id: u64,
    #[prost(uint64, required, tag = "2")]
    pub log_id: u64,
    #[prost(int32, required, tag = "3")]
    pub service: i32,
    #[prost(int32, required, tag = "4")]
    pub method: i32,
    #[prost(message, repeated, tag = "5")]
    pub headers: Vec<Header>,
    #[prost(string, optional, tag = "6")]
    pub payload_encoding: Option<String>,
    #[prost(string, optional, tag = "7")]
    pub payload_type: Option<String>,
    #[prost(bytes, optional, tag = "8")]
    pub payload: Option<Vec<u8>>,
    #[prost(string, optional, tag = "9")]
    pub log_id_new: Option<String>,
}

struct FragEntry {
    frames: Vec<Option<Vec<u8>>>,
    created: Instant,
}

impl FragEntry {
    fn new(sum: usize) -> Self {
        Self {
            frames: vec![None; sum],
            created: Instant::now(),
        }
    }

    fn insert(&mut self, index: usize, data: Vec<u8>) {
        if index < self.frames.len() {
            self.frames[index] = Some(data);
        }
    }

    fn complete(&self) -> bool {
        self.frames.iter().all(|f| f.is_some())
    }

    fn assemble(self) -> Vec<u8> {
        self.frames.into_iter().flatten().flatten().collect()
    }

    fn expired(&self) -> bool {
        self.created.elapsed() > Duration::from_secs(5)
    }
}

fn get_header(headers: &[Header], key: &str) -> Option<String> {
    headers
        .iter()
        .find(|h| h.key == key)
        .map(|h| h.value.clone())
}

fn get_header_int(headers: &[Header], key: &str) -> i32 {
    get_header(headers, key)
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

fn make_header(key: &str, value: &str) -> Header {
    Header {
        key: key.to_string(),
        value: value.to_string(),
    }
}

pub async fn run_session(
    ws_url: &str,
    service_id: i32,
    ping_interval: Duration,
    mut cancel: watch::Receiver<bool>,
) -> Result<(), ConnectFailureKind> {
    info!(gateway = %sanitize_ws_url(ws_url), "Feishu long-connection connecting");
    let (ws, resp) = tokio::select! {
        _ = cancel.changed() => return Ok(()),
        result = tokio::time::timeout(
            WS_CONNECT_TIMEOUT,
            tokio_tungstenite::connect_async(ws_url),
        ) => {
            match result {
                Ok(Ok(stream)) => stream,
                Ok(Err(err)) => {
                    warn!(
                        error = %redact_secrets(&err.to_string()),
                        "Feishu WebSocket connect failed"
                    );
                    return Err(ConnectFailureKind::Transient);
                }
                Err(_) => {
                    warn!(
                        timeout_secs = WS_CONNECT_TIMEOUT.as_secs(),
                        "Feishu WebSocket connect timed out"
                    );
                    return Err(ConnectFailureKind::Transient);
                }
            }
        }
    };
    if resp.status().as_u16() != 101 {
        return Err(classify_handshake(&resp));
    }

    crate::services::feishu_bot::publish_phase_online().await;
    info!("Feishu long-connection online");

    let (write, mut read) = ws.split();
    let write = Arc::new(Mutex::new(write));
    let ping_write = write.clone();
    let ping_secs = ping_interval.max(Duration::from_secs(1)).as_secs();
    let ping_handle = tokio::spawn(async move {
        let mut timer = tokio::time::interval(Duration::from_secs(ping_secs));
        timer.tick().await;
        loop {
            timer.tick().await;
            let frame = Frame {
                seq_id: 0,
                log_id: 0,
                service: service_id,
                method: METHOD_CONTROL,
                headers: vec![make_header(HEADER_TYPE, MSG_TYPE_PING)],
                payload_encoding: None,
                payload_type: None,
                payload: None,
                log_id_new: None,
            };
            let encoded = frame.encode_to_vec();
            let mut w = ping_write.lock().await;
            if (*w).send(Message::Binary(encoded.into())).await.is_err() {
                break;
            }
        }
    });

    let mut pending_frags: HashMap<String, FragEntry> = HashMap::new();
    let result = loop {
        tokio::select! {
            _ = cancel.changed() => {
                let mut w = write.lock().await;
                let _ = (*w).close().await;
                break Ok(());
            }
            msg = read.next() => {
                match msg {
                    Some(Ok(Message::Binary(data))) => {
                        if let Err(kind) = handle_binary(
                            &data,
                            &mut pending_frags,
                            &write,
                        )
                        .await
                        {
                            break Err(kind);
                        }
                    }
                    Some(Ok(Message::Close(_))) => break Err(ConnectFailureKind::Transient),
                    Some(Ok(_)) => {}
                    Some(Err(err)) => {
                        warn!(
                            error = %redact_secrets(&err.to_string()),
                            "Feishu WebSocket stream error"
                        );
                        break Err(ConnectFailureKind::Transient);
                    }
                    None => break Err(ConnectFailureKind::Transient),
                }
            }
        }
    };
    ping_handle.abort();
    result
}

fn classify_handshake(
    resp: &tokio_tungstenite::tungstenite::handshake::client::Response,
) -> ConnectFailureKind {
    let headers = resp.headers();
    let status = headers
        .get(HEADER_HANDSHAKE_STATUS)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let auth_err = headers
        .get(HEADER_HANDSHAKE_AUTH_ERR_CODE)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let msg = headers
        .get(HEADER_HANDSHAKE_MSG)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    warn!(status, auth_err, msg, "Feishu WebSocket handshake rejected");
    classify_feishu_handshake(status, auth_err)
}

async fn handle_binary<S>(
    data: &[u8],
    pending_frags: &mut HashMap<String, FragEntry>,
    write: &Arc<Mutex<S>>,
) -> Result<(), ConnectFailureKind>
where
    S: futures::Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    let frame = Frame::decode(data).map_err(|_| ConnectFailureKind::Transient)?;
    match frame.method {
        METHOD_CONTROL => {
            let msg_type = get_header(&frame.headers, HEADER_TYPE).unwrap_or_default();
            if msg_type == MSG_TYPE_PING {
                let pong = Frame {
                    method: METHOD_CONTROL,
                    service: frame.service,
                    seq_id: frame.seq_id,
                    log_id: frame.log_id,
                    headers: vec![make_header(HEADER_TYPE, MSG_TYPE_PONG)],
                    payload_encoding: None,
                    payload_type: None,
                    payload: None,
                    log_id_new: None,
                };
                let encoded = pong.encode_to_vec();
                let mut w = write.lock().await;
                let _ = (*w).send(Message::Binary(encoded.into())).await;
            }
            Ok(())
        }
        METHOD_DATA => {
            let msg_type = get_header(&frame.headers, HEADER_TYPE).unwrap_or_default();
            if msg_type != MSG_TYPE_EVENT {
                return Ok(());
            }
            let sum = get_header_int(&frame.headers, HEADER_SUM);
            let seq = get_header_int(&frame.headers, HEADER_SEQ);
            let msg_id = get_header(&frame.headers, HEADER_MESSAGE_ID).unwrap_or_default();
            let payload = if sum <= 1 {
                frame.payload.clone().unwrap_or_default()
            } else {
                pending_frags.retain(|_, v| !v.expired());
                let entry = pending_frags
                    .entry(msg_id.clone())
                    .or_insert_with(|| FragEntry::new(sum as usize));
                entry.insert(seq as usize, frame.payload.clone().unwrap_or_default());
                if entry.complete() {
                    match pending_frags.remove(&msg_id) {
                        Some(e) => e.assemble(),
                        None => return Ok(()),
                    }
                } else {
                    return Ok(());
                }
            };
            let start = Instant::now();
            dispatch_event(payload);
            let biz_rt = start.elapsed().as_millis().to_string();
            let ack_payload =
                serde_json::to_vec(&serde_json::json!({ "code": 200u16 })).unwrap_or_default();
            let mut ack_headers = frame.headers.clone();
            ack_headers.push(make_header(HEADER_BIZ_RT, &biz_rt));
            let ack = Frame {
                seq_id: frame.seq_id,
                log_id: frame.log_id,
                service: frame.service,
                method: frame.method,
                headers: ack_headers,
                payload_encoding: frame.payload_encoding.clone(),
                payload_type: frame.payload_type.clone(),
                payload: Some(ack_payload),
                log_id_new: frame.log_id_new.clone(),
            };
            let encoded = ack.encode_to_vec();
            let mut w = write.lock().await;
            let _ = (*w).send(Message::Binary(encoded.into())).await;
            Ok(())
        }
        _ => Ok(()),
    }
}

fn dispatch_event(payload: Vec<u8>) {
    let Some((event_type, event_id, event)) = parse_feishu_event_envelope(&payload) else {
        return;
    };
    tokio::spawn(async move {
        crate::services::feishu_bot::mark_inbound().await;
        match event_type.as_str() {
            FEISHU_MESSAGE_RECEIVE_V1 => {
                if let Some(parsed) =
                    myriad_agent_rules::channel::parse_feishu_message_receive(&event_id, &event)
                {
                    crate::services::feishu_pairing::handle_inbound(parsed).await;
                }
            }
            FEISHU_CARD_ACTION_TRIGGER => {
                if let Some(parsed) =
                    myriad_agent_rules::channel::parse_feishu_card_callback(&event_id, &event)
                {
                    crate::services::feishu_pairing::handle_callback(parsed).await;
                }
            }
            _ => {}
        }
    });
}

fn sanitize_ws_url(url: &str) -> String {
    match url.split_once('?') {
        Some((base, _)) => format!("{base}?<redacted>"),
        None => url.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prost::Message as ProstMessage;

    #[test]
    fn frame_roundtrip_preserves_method_and_payload() {
        let frame = Frame {
            seq_id: 3,
            log_id: 9,
            service: 7,
            method: METHOD_DATA,
            headers: vec![make_header(HEADER_TYPE, MSG_TYPE_EVENT)],
            payload_encoding: None,
            payload_type: Some("json".into()),
            payload: Some(b"{\"a\":1}".to_vec()),
            log_id_new: None,
        };
        let bytes = frame.encode_to_vec();
        let decoded = Frame::decode(bytes.as_slice()).expect("decode");
        assert_eq!(decoded.method, METHOD_DATA);
        assert_eq!(decoded.service, 7);
        assert_eq!(
            get_header(&decoded.headers, HEADER_TYPE).as_deref(),
            Some(MSG_TYPE_EVENT)
        );
        assert_eq!(decoded.payload.as_deref(), Some(b"{\"a\":1}".as_slice()));
    }

    #[test]
    fn fragment_assembles_in_seq_order() {
        let mut entry = FragEntry::new(2);
        entry.insert(1, b"b".to_vec());
        entry.insert(0, b"a".to_vec());
        assert!(entry.complete());
        assert_eq!(entry.assemble(), b"ab");
    }
}
