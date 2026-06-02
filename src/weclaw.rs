//! WeChat iLink (Claw) protocol client.
//! QR code login, message send, and update polling.

use base64::Engine;
use rand::Rng;
use reqwest::Client;
use serde::{Deserialize, Serialize};

const ILINK_BASE: &str = "https://ilinkai.weixin.qq.com";
const CHANNEL_VERSION: &str = "1.0.2";

fn random_wechat_uin() -> String {
    let mut rng = rand::rng();
    let n: u32 = rng.random();
    base64::engine::general_purpose::STANDARD.encode(n.to_string())
}

// --- Internal response types ---

#[derive(Deserialize)]
struct QrCodeResponse {
    ret: i32,
    qrcode: Option<String>,
    qrcode_img_content: Option<String>,
}

#[derive(Deserialize)]
struct QrStatusResponse {
    ret: i32,
    status: Option<String>,
    bot_token: Option<String>,
    ilink_bot_id: Option<String>,
    ilink_user_id: Option<String>,
}

#[derive(Deserialize)]
struct GetUpdatesResponse {
    ret: Option<i32>,
    errcode: Option<i32>,
    /// Messages from the user — contains `context_token`.
    msgs: Option<Vec<WeixinMessage>>,
    get_updates_buf: Option<String>,
}

#[derive(Deserialize)]
struct WeixinMessage {
    from_user_id: Option<String>,
    #[serde(default)]
    message_type: Option<i32>,
    context_token: Option<String>,
    #[serde(default)]
    item_list: Option<Vec<WeixinItem>>,
}

#[derive(Deserialize)]
struct WeixinItem {
    #[serde(rename = "type", default)]
    item_type: Option<i32>,
    #[serde(default)]
    text_item: Option<WeixinTextItem>,
}

#[derive(Deserialize)]
struct WeixinTextItem {
    #[serde(default)]
    text: Option<String>,
}

/// A parsed inbound user message extracted from one poll cycle.
#[derive(Debug, Clone)]
pub struct WeclawInboundMessage {
    /// WeChat user id in iLink format: `xxx@im.wechat`.
    pub from_user_id: String,
    /// Extracted text from the first text item. None if no text item.
    pub text: Option<String>,
    /// Context token for replying to this specific message.
    pub context_token: String,
}

// --- Public output types ---

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WeclawQrSession {
    pub qrcode: String,
    /// URL to the QR code image (hosted by WeChat).
    pub qrcode_url: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WeclawQrStatus {
    pub status: String,
    pub bot_token: Option<String>,
    pub bot_id: Option<String>,
    pub user_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WeclawCredentials {
    pub bot_token: String,
    pub bot_id: String,
    pub user_id: String,
    pub context_token: Option<String>,
    pub get_updates_buf: Option<String>,
}

// --- Public functions ---

pub async fn get_qr_code(client: &Client) -> Result<WeclawQrSession, String> {
    let url = format!("{ILINK_BASE}/ilink/bot/get_bot_qrcode?bot_type=3");
    let resp = client
        .get(&url)
        .header("AuthorizationType", "ilink_bot_token")
        .header("X-WECHAT-UIN", random_wechat_uin())
        .header("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("iLink QR request failed: {e}"))?;

    let body: QrCodeResponse = resp
        .json()
        .await
        .map_err(|e| format!("iLink QR parse failed: {e}"))?;

    if body.ret != 0 {
        return Err(format!("iLink QR error: ret={}", body.ret));
    }

    Ok(WeclawQrSession {
        qrcode: body.qrcode.ok_or("missing qrcode")?,
        qrcode_url: body
            .qrcode_img_content
            .ok_or("missing qrcode_img_content")?,
    })
}

pub async fn poll_qr_status(client: &Client, qrcode: &str) -> Result<WeclawQrStatus, String> {
    let url = format!(
        "{ILINK_BASE}/ilink/bot/get_qrcode_status?qrcode={}",
        urlencoding::encode(qrcode)
    );
    let resp = client
        .get(&url)
        .header("AuthorizationType", "ilink_bot_token")
        .header("X-WECHAT-UIN", random_wechat_uin())
        .header("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("iLink QR status request failed: {e}"))?;

    // Grab raw text first so we can log the exact status string iLink returns
    // — the field values are reverse-engineered and may vary between versions.
    let raw = resp
        .text()
        .await
        .map_err(|e| format!("iLink QR status read failed: {e}"))?;
    tracing::debug!(body = %raw, "iLink qr status raw");

    let body: QrStatusResponse =
        serde_json::from_str(&raw).map_err(|e| format!("iLink QR status parse failed: {e}"))?;

    if body.ret != 0 {
        return Err(format!("iLink QR status error: ret={}", body.ret));
    }

    Ok(WeclawQrStatus {
        status: body.status.unwrap_or_else(|| "unknown".into()),
        bot_token: body.bot_token,
        bot_id: body.ilink_bot_id,
        user_id: body.ilink_user_id,
    })
}

/// Poll for updates from WeChat. Long-polls with configurable timeout.
/// Returns updated credentials (with refreshed `context_token` / cursor) and
/// any inbound user messages received this cycle.
pub async fn poll_updates(
    client: &Client,
    creds: &WeclawCredentials,
) -> Result<(WeclawCredentials, Vec<WeclawInboundMessage>), String> {
    poll_updates_with_timeout(client, creds, 40).await
}

/// Poll for updates with a custom timeout in seconds.
pub async fn poll_updates_with_timeout(
    client: &Client,
    creds: &WeclawCredentials,
    timeout_secs: u64,
) -> Result<(WeclawCredentials, Vec<WeclawInboundMessage>), String> {
    let url = format!("{ILINK_BASE}/ilink/bot/getupdates");
    let body = serde_json::json!({
        "get_updates_buf": creds.get_updates_buf.as_deref().unwrap_or(""),
        "base_info": {
            "channel_version": "1.0.0"
        }
    });

    let resp = client
        .post(&url)
        .header("AuthorizationType", "ilink_bot_token")
        .header("X-WECHAT-UIN", random_wechat_uin())
        .header("Authorization", format!("Bearer {}", creds.bot_token))
        .json(&body)
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .send()
        .await
        .map_err(|e| format!("iLink getupdates failed: {e}"))?;

    let resp_body: GetUpdatesResponse = resp
        .json()
        .await
        .map_err(|e| format!("iLink getupdates parse failed: {e}"))?;

    if resp_body.ret.unwrap_or(0) != 0 || resp_body.errcode.unwrap_or(0) != 0 {
        return Err(format!(
            "iLink getupdates error: ret={:?}, errcode={:?}",
            resp_body.ret, resp_body.errcode
        ));
    }

    let mut updated = creds.clone();
    let mut inbound: Vec<WeclawInboundMessage> = Vec::new();

    if let Some(buf) = resp_body.get_updates_buf {
        updated.get_updates_buf = Some(buf);
    }

    if let Some(msgs) = resp_body.msgs {
        for msg in msgs {
            // Refresh context_token off the latest message that carries one.
            if let Some(ct) = msg.context_token.as_ref()
                && !ct.is_empty()
            {
                updated.context_token = Some(ct.clone());
            }

            // Only forward messages from users (message_type == 1). Bot-origin
            // and system messages are filtered out.
            if msg.message_type.unwrap_or(0) != 1 {
                continue;
            }

            let Some(from_user_id) = msg.from_user_id else {
                continue;
            };
            let Some(ct) = msg.context_token else {
                continue;
            };
            if ct.is_empty() {
                continue;
            }

            // Pull the first text_item text if present.
            let text = msg.item_list.and_then(|items| {
                items.into_iter().find_map(|item| {
                    if item.item_type.unwrap_or(0) == 1 {
                        item.text_item
                            .and_then(|ti| ti.text)
                            .filter(|s| !s.is_empty())
                    } else {
                        None
                    }
                })
            });

            inbound.push(WeclawInboundMessage {
                from_user_id,
                text,
                context_token: ct,
            });
        }
    }

    Ok((updated, inbound))
}

/// Send a text message via iLink.
pub async fn send_message(
    client: &Client,
    creds: &WeclawCredentials,
    text: &str,
) -> Result<(), String> {
    let context_token = creds
        .context_token
        .as_deref()
        .ok_or("context_token not available — user must send a message to ClawBot first")?;

    let client_id = format!(
        "tokimo-weixin:{}-{:08x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        rand::rng().random::<u32>()
    );

    let url = format!("{ILINK_BASE}/ilink/bot/sendmessage");
    let body = serde_json::json!({
        "msg": {
            "from_user_id": "",
            "to_user_id": creds.user_id,
            "client_id": client_id,
            "context_token": context_token,
            "item_list": [{
                "type": 1,
                "text_item": { "text": text }
            }],
            "message_type": 2,
            "message_state": 2
        },
        "base_info": { "channel_version": CHANNEL_VERSION }
    });

    let resp = client
        .post(&url)
        .header("AuthorizationType", "ilink_bot_token")
        .header("X-WECHAT-UIN", random_wechat_uin())
        .header("Authorization", format!("Bearer {}", creds.bot_token))
        .json(&body)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("iLink send failed: {e}"))?;

    let status = resp.status();
    let resp_text = resp.text().await.unwrap_or_default();

    if !status.is_success() {
        return Err(format!(
            "iLink send HTTP {}: {}",
            status.as_u16(),
            resp_text
        ));
    }

    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&resp_text) {
        let ret = parsed
            .get("ret")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0);
        let errcode = parsed
            .get("errcode")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0);
        if ret != 0 || errcode != 0 {
            let errmsg = parsed
                .get("errmsg")
                .or_else(|| parsed.get("err_msg"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            return Err(format!(
                "iLink send error: ret={ret}, errcode={errcode}, msg={errmsg}"
            ));
        }
    }

    Ok(())
}
