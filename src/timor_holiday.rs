use serde::Deserialize;

use crate::error::ClientError;

// ── Response types ───────────────────────────────────────────────────────────

/// Response from `GET /api/holiday/next/$date`.
/// Returns the next upcoming holiday from the current date.
#[derive(Debug, Deserialize)]
pub struct TimorNextHolidayResponse {
    pub code: i32,
    pub holiday: Option<TimorHolidayEntry>,
}

#[derive(Debug, Deserialize)]
pub struct TimorHolidayEntry {
    /// Always true for this endpoint.
    pub holiday: bool,
    /// Chinese holiday name (e.g. "国庆节").
    pub name: String,
    /// Wage multiplier (e.g. 3 = triple pay).
    pub wage: i32,
    /// ISO date string (e.g. "2026-10-01").
    pub date: String,
    /// Days remaining until this holiday.
    pub rest: i32,
}

/// Response from `GET /api/holiday/info/$date`.
#[derive(Debug, Deserialize)]
pub struct TimorDayInfoResponse {
    pub code: i32,
    #[serde(rename = "type")]
    pub day_type: Option<TimorDayType>,
    pub holiday: Option<TimorDayHoliday>,
}

#[derive(Debug, Deserialize)]
pub struct TimorDayType {
    /// 0 = workday, 1 = weekend, 2 = holiday, 3 = compensatory workday (调休/补班).
    #[serde(rename = "type")]
    pub kind: i32,
    pub name: String,
    pub week: i32,
}

#[derive(Debug, Deserialize)]
pub struct TimorDayHoliday {
    pub holiday: bool,
    pub name: String,
    pub wage: i32,
    pub date: Option<String>,
    pub rest: Option<i32>,
}

// ── Client ───────────────────────────────────────────────────────────────────

const BASE_URL: &str = "https://timor.tech/api/holiday";

pub struct TimorHolidayClient {
    http: reqwest::Client,
}

impl TimorHolidayClient {
    pub fn new(http: reqwest::Client) -> Self {
        Self { http }
    }

    /// Get the next upcoming holiday from a given date (or today if `None`).
    pub async fn next_holiday(&self, date: Option<&str>) -> Result<TimorNextHolidayResponse, ClientError> {
        let url = match date {
            Some(d) => format!("{BASE_URL}/next/{d}"),
            None => format!("{BASE_URL}/next"),
        };
        let resp = self
            .http
            .get(&url)
            .send()
            .await?
            .error_for_status()
            .map_err(ClientError::Http)?;
        let body = resp.json::<TimorNextHolidayResponse>().await?;
        if body.code != 0 {
            return Err(ClientError::Api {
                status: 500,
                message: format!("timor API returned code {}", body.code),
            });
        }
        Ok(body)
    }

    /// Get holiday/work-type info for a specific date (or today if `None`).
    pub async fn day_info(&self, date: Option<&str>) -> Result<TimorDayInfoResponse, ClientError> {
        let url = match date {
            Some(d) => format!("{BASE_URL}/info/{d}"),
            None => format!("{BASE_URL}/info"),
        };
        let resp = self
            .http
            .get(&url)
            .send()
            .await?
            .error_for_status()
            .map_err(ClientError::Http)?;
        let body = resp.json::<TimorDayInfoResponse>().await?;
        if body.code != 0 {
            return Err(ClientError::Api {
                status: 500,
                message: format!("timor API returned code {}", body.code),
            });
        }
        Ok(body)
    }
}
