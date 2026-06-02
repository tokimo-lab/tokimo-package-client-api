//! 115 Pan QR-code login flow.
//!
//! Flow: start QR session → poll status → exchange token for cookie.

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use reqwest::Client;
use serde::{Deserialize, Serialize};

const PAN115_QR_TOKEN_URL: &str = "https://qrcodeapi.115.com/api/1.0/web/1.0/token";
const PAN115_QR_STATUS_URL: &str = "https://qrcodeapi.115.com/get/status/";
const PAN115_QR_IMAGE_URL: &str = "https://qrcodeapi.115.com/api/1.0/mac/1.0/qrcode";
const PAN115_QR_LOGIN_API_PREFIX: &str = "https://passportapi.115.com/app/1.0";
const PAN115_USER_AGENT: &str = "Mozilla/5.0 115Browser/27.0.5.7";

const ENCODE_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

fn url_encode(s: &str) -> String {
    utf8_percent_encode(s, ENCODE_SET).to_string()
}

// ── Internal API response types ────────────────────────────────────────────

#[derive(Deserialize)]
#[allow(dead_code)]
struct QrBaseResponse {
    state: i32,
    message: Option<String>,
    msg: Option<String>,
    error: Option<String>,
    errno: Option<i32>,
}

#[derive(Deserialize)]
struct QrTokenData {
    qrcode: String,
    sign: String,
    time: i64,
    uid: String,
}

#[derive(Deserialize)]
struct QrTokenResponse {
    state: i32,
    message: Option<String>,
    msg: Option<String>,
    error: Option<String>,
    errno: Option<i32>,
    data: Option<QrTokenData>,
}

#[derive(Deserialize)]
struct QrStatusData {
    msg: String,
    status: i32,
}

#[derive(Deserialize)]
struct QrStatusResponse {
    state: i32,
    message: Option<String>,
    msg: Option<String>,
    error: Option<String>,
    errno: Option<i32>,
    data: Option<QrStatusData>,
}

// ── Public output types ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pan115QrSession {
    pub uid: String,
    pub sign: String,
    pub time: i64,
    pub qrcode_content: String,
    pub image_url: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pan115QrStatus {
    pub status: String,
    pub message: Option<String>,
}

// ── Helpers ────────────────────────────────────────────────────────────────

fn build_api_error(prefix: &str, resp: &QrBaseResponse) -> String {
    let detail = resp
        .message
        .as_deref()
        .or(resp.msg.as_deref())
        .or(resp.error.as_deref())
        .unwrap_or("unknown error");
    format!("{prefix}: {detail}")
}

fn map_qr_status(status: i32) -> Result<&'static str, String> {
    match status {
        0 => Ok("waiting"),
        1 => Ok("scanned"),
        2 => Ok("allowed"),
        -1 => Ok("expired"),
        -2 => Ok("canceled"),
        other => Err(format!("未知的 115 二维码状态: {other}")),
    }
}

// ── Public API ─────────────────────────────────────────────────────────────

/// Start a new QR login session. Returns session metadata including the QR content.
pub async fn start_qr_session() -> Result<Pan115QrSession, String> {
    let client = Client::new();
    let resp = client
        .get(PAN115_QR_TOKEN_URL)
        .header("User-Agent", PAN115_USER_AGENT)
        .header("Accept", "application/json")
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("115 QR API request failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!(
            "115 QR API failed: {} {}",
            resp.status().as_u16(),
            resp.status().canonical_reason().unwrap_or("Unknown")
        ));
    }

    let payload: QrTokenResponse = resp
        .json()
        .await
        .map_err(|e| format!("115 QR API parse failed: {e}"))?;

    if payload.state != 1 {
        let base = QrBaseResponse {
            state: payload.state,
            message: payload.message,
            msg: payload.msg,
            error: payload.error,
            errno: payload.errno,
        };
        return Err(build_api_error("115 QR session start failed", &base));
    }

    let data = payload.data.ok_or("115 QR token response missing data")?;

    Ok(Pan115QrSession {
        uid: data.uid.clone(),
        sign: data.sign,
        time: data.time,
        qrcode_content: data.qrcode,
        image_url: format!("{}?uid={}", PAN115_QR_IMAGE_URL, url_encode(&data.uid)),
    })
}

/// Poll the QR login status. Call repeatedly until status is "allowed" or "expired".
pub async fn get_qr_status(uid: &str, sign: &str, time: i64) -> Result<Pan115QrStatus, String> {
    let client = Client::new();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();

    let url = format!(
        "{}?uid={}&sign={}&time={}&_={}",
        PAN115_QR_STATUS_URL,
        url_encode(uid),
        url_encode(sign),
        time,
        now,
    );

    let resp = client
        .get(&url)
        .header("User-Agent", PAN115_USER_AGENT)
        .header("Accept", "application/json")
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("115 QR status request failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!(
            "115 QR API failed: {} {}",
            resp.status().as_u16(),
            resp.status().canonical_reason().unwrap_or("Unknown")
        ));
    }

    let payload: QrStatusResponse = resp
        .json()
        .await
        .map_err(|e| format!("115 QR status parse failed: {e}"))?;

    if payload.state != 1 {
        let base = QrBaseResponse {
            state: payload.state,
            message: payload.message,
            msg: payload.msg,
            error: payload.error,
            errno: payload.errno,
        };
        return Err(build_api_error("115 QR status failed", &base));
    }

    let data = payload.data.ok_or("115 QR status response missing data")?;
    let status = map_qr_status(data.status)?;

    Ok(Pan115QrStatus {
        status: status.to_string(),
        message: Some(data.msg)
            .filter(|s| !s.is_empty())
            .or(payload.msg)
            .or(payload.message),
    })
}

/// Exchange a QR token for a 115 login cookie.
/// Call immediately after QR status becomes "allowed".
pub async fn exchange_qr_token(uid: &str, qrcode_source: &str) -> Result<String, String> {
    let client = Client::new();
    let resp = client
        .post(format!(
            "{PAN115_QR_LOGIN_API_PREFIX}/{qrcode_source}/1.0/login/qrcode"
        ))
        .header("User-Agent", PAN115_USER_AGENT)
        .form(&[("account", uid), ("app", qrcode_source)])
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("115 QR login request failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!(
            "115 QR login API failed: {} {}",
            resp.status().as_u16(),
            resp.status().canonical_reason().unwrap_or("Unknown")
        ));
    }

    let payload: QrExchangeResponse = resp
        .json()
        .await
        .map_err(|e| format!("115 QR login parse failed: {e}"))?;

    if payload.state != 1 {
        return Err(format!(
            "115 QR login failed: {}",
            payload.message.unwrap_or_else(|| "unknown error".into())
        ));
    }

    payload
        .data
        .cookie
        .to_cookie_string()
        .ok_or_else(|| "115 QR login did not return valid credentials".into())
}

// ── QR token → Cookie exchange types ──────────────────────────────────────

#[derive(Deserialize, Default)]
struct QrLoginData {
    #[serde(default)]
    cookie: QrCredential,
}

#[derive(Default, Deserialize)]
struct QrCredential {
    #[serde(default, rename = "UID")]
    uid: String,
    #[serde(default, rename = "CID")]
    cid: String,
    #[serde(default, rename = "SEID")]
    seid: String,
    #[serde(default, rename = "KID")]
    kid: String,
}

#[derive(Deserialize)]
struct QrExchangeResponse {
    state: i32,
    message: Option<String>,
    #[serde(default)]
    data: QrLoginData,
}

impl QrCredential {
    fn to_cookie_string(&self) -> Option<String> {
        if self.uid.is_empty() || self.cid.is_empty() || self.seid.is_empty() {
            return None;
        }
        Some(format!(
            "UID={};CID={};SEID={};KID={}",
            self.uid, self.cid, self.seid, self.kid
        ))
    }
}
