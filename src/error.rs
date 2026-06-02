use std::fmt;

#[derive(Debug)]
pub enum ClientError {
    Http(reqwest::Error),
    Json(serde_json::Error),
    Api { status: u16, message: String },
    NotFound,
    Auth(String),
    CloudflareChallenge,
    Other(String),
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Http(e) => write!(f, "HTTP request failed: {e}"),
            Self::Json(e) => write!(f, "JSON error: {e}"),
            Self::Api { status, message } => write!(f, "API error {status}: {message}"),
            Self::NotFound => write!(f, "Not found"),
            Self::Auth(msg) => write!(f, "Authentication failed: {msg}"),
            Self::CloudflareChallenge => write!(f, "Cloudflare challenge detected"),
            Self::Other(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for ClientError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Http(e) => Some(e),
            Self::Json(e) => Some(e),
            _ => None,
        }
    }
}

impl From<reqwest::Error> for ClientError {
    fn from(e: reqwest::Error) -> Self {
        Self::Http(e)
    }
}

impl From<serde_json::Error> for ClientError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}

impl From<tokimo_web_fetch::FetchError> for ClientError {
    fn from(e: tokimo_web_fetch::FetchError) -> Self {
        match e {
            tokimo_web_fetch::FetchError::Http(err) => Self::Http(err),
            tokimo_web_fetch::FetchError::CloudflareChallenge => Self::CloudflareChallenge,
            other => Self::Other(other.to_string()),
        }
    }
}
