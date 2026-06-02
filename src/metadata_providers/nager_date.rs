//! Nager.Date — free public holiday API.
//!
//! <https://date.nager.at/> — no authentication required.
//! Returns public holidays for a given country and year.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::cache::RequestCache;
use crate::error::ClientError;

const BASE_URL: &str = "https://date.nager.at/api/v3";
/// Cache holidays for 24 hours (data rarely changes).
const DEFAULT_CACHE_TTL: Duration = Duration::from_hours(24);

/// A single public holiday entry from the Nager.Date API.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicHoliday {
    /// ISO date string, e.g. "2026-01-01"
    pub date: String,
    /// Localized name in the country's language, e.g. "元旦"
    pub local_name: String,
    /// English name, e.g. "New Year's Day"
    pub name: String,
    /// ISO 3166-1 alpha-2 country code
    pub country_code: String,
    /// Whether this is a nationwide holiday
    pub global: bool,
    /// Holiday types (Public, Bank, School, etc.)
    pub types: Vec<String>,
}

/// A long weekend entry from the Nager.Date API.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LongWeekend {
    /// ISO date string, e.g. "2026-01-01"
    pub start_date: String,
    /// ISO date string, e.g. "2026-01-04"
    pub end_date: String,
    /// Number of consecutive days off
    pub day_count: u8,
    /// Whether a bridge day (vacation day) is needed
    pub need_bridge_day: bool,
}

/// A country entry from the Nager.Date available countries endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailableCountry {
    /// ISO 3166-1 alpha-2 country code
    pub country_code: String,
    /// Country name in English
    pub name: String,
}

pub struct NagerDateClient {
    http: reqwest::Client,
    cache: RequestCache,
}

impl NagerDateClient {
    pub fn new(http_client: reqwest::Client) -> Self {
        Self {
            http: http_client,
            cache: RequestCache::new(DEFAULT_CACHE_TTL),
        }
    }

    /// Get all public holidays for a given country and year.
    ///
    /// `country_code` is ISO 3166-1 alpha-2 (e.g. "CN", "US", "JP").
    pub async fn get_public_holidays(&self, year: u16, country_code: &str) -> Result<Vec<PublicHoliday>, ClientError> {
        let cache_key = format!("nager:holidays:{country_code}:{year}");
        if let Some(cached) = self.cache.get::<Vec<PublicHoliday>>(&cache_key).await {
            return Ok(cached);
        }

        let url = format!("{BASE_URL}/PublicHolidays/{year}/{country_code}");
        let resp = self.http.get(&url).send().await?;

        if !resp.status().is_success() {
            return Err(ClientError::Api {
                status: resp.status().as_u16(),
                message: resp.text().await.unwrap_or_default(),
            });
        }

        let holidays: Vec<PublicHoliday> = resp.json().await?;
        self.cache.set(&cache_key, &holidays).await;
        Ok(holidays)
    }

    /// Get long weekends for a given country and year.
    pub async fn get_long_weekends(&self, year: u16, country_code: &str) -> Result<Vec<LongWeekend>, ClientError> {
        let cache_key = format!("nager:long_weekends:{country_code}:{year}");
        if let Some(cached) = self.cache.get::<Vec<LongWeekend>>(&cache_key).await {
            return Ok(cached);
        }

        let url = format!("{BASE_URL}/LongWeekend/{year}/{country_code}");
        let resp = self.http.get(&url).send().await?;

        if !resp.status().is_success() {
            return Err(ClientError::Api {
                status: resp.status().as_u16(),
                message: resp.text().await.unwrap_or_default(),
            });
        }

        let weekends: Vec<LongWeekend> = resp.json().await?;
        self.cache.set(&cache_key, &weekends).await;
        Ok(weekends)
    }

    /// Get all countries supported by the Nager.Date API.
    pub async fn get_available_countries(&self) -> Result<Vec<AvailableCountry>, ClientError> {
        let cache_key = "nager:countries".to_string();
        if let Some(cached) = self.cache.get::<Vec<AvailableCountry>>(&cache_key).await {
            return Ok(cached);
        }

        let url = format!("{BASE_URL}/AvailableCountries");
        let resp = self.http.get(&url).send().await?;

        if !resp.status().is_success() {
            return Err(ClientError::Api {
                status: resp.status().as_u16(),
                message: resp.text().await.unwrap_or_default(),
            });
        }

        let countries: Vec<AvailableCountry> = resp.json().await?;
        self.cache.set(&cache_key, &countries).await;
        Ok(countries)
    }

    /// Get the next upcoming public holidays for a given country.
    pub async fn get_next_public_holidays(&self, country_code: &str) -> Result<Vec<PublicHoliday>, ClientError> {
        let cache_key = format!("nager:next:{country_code}");
        if let Some(cached) = self.cache.get::<Vec<PublicHoliday>>(&cache_key).await {
            return Ok(cached);
        }

        let url = format!("{BASE_URL}/NextPublicHolidays/{country_code}");
        let resp = self.http.get(&url).send().await?;

        if !resp.status().is_success() {
            return Err(ClientError::Api {
                status: resp.status().as_u16(),
                message: resp.text().await.unwrap_or_default(),
            });
        }

        let holidays: Vec<PublicHoliday> = resp.json().await?;
        self.cache.set(&cache_key, &holidays).await;
        Ok(holidays)
    }
}
