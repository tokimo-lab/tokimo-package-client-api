use serde::Deserialize;

use crate::error::ClientError;

// ── Open-Meteo forecast ──────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct OmForecastResponse {
    pub utc_offset_seconds: i32,
    pub current: OmCurrent,
    pub hourly: OmHourly,
    pub daily: OmDaily,
}

#[derive(Deserialize)]
pub struct OmCurrent {
    pub time: String,
    pub temperature_2m: f64,
    pub relative_humidity_2m: f64,
    pub apparent_temperature: f64,
    pub is_day: f64,
    pub precipitation: f64,
    pub rain: f64,
    pub snowfall: f64,
    pub weather_code: f64,
    pub cloud_cover: f64,
    pub pressure_msl: f64,
    pub surface_pressure: f64,
    pub wind_speed_10m: f64,
    pub wind_direction_10m: f64,
    pub wind_gusts_10m: f64,
}

#[derive(Deserialize)]
pub struct OmHourly {
    pub time: Vec<String>,
    pub temperature_2m: Vec<f64>,
    pub relative_humidity_2m: Vec<f64>,
    pub apparent_temperature: Vec<f64>,
    pub precipitation_probability: Vec<f64>,
    pub precipitation: Vec<f64>,
    pub rain: Vec<f64>,
    pub snowfall: Vec<f64>,
    pub weather_code: Vec<f64>,
    pub cloud_cover: Vec<f64>,
    pub visibility: Vec<f64>,
    pub wind_speed_10m: Vec<f64>,
    pub wind_direction_10m: Vec<f64>,
    pub pressure_msl: Vec<f64>,
    pub is_day: Vec<f64>,
}

#[derive(Deserialize)]
pub struct OmDaily {
    pub time: Vec<String>,
    pub weather_code: Vec<f64>,
    pub temperature_2m_max: Vec<f64>,
    pub temperature_2m_min: Vec<f64>,
    pub sunrise: Vec<String>,
    pub sunset: Vec<String>,
    pub precipitation_sum: Vec<f64>,
    pub rain_sum: Vec<f64>,
    pub snowfall_sum: Vec<f64>,
    pub precipitation_probability_max: Vec<f64>,
    pub wind_speed_10m_max: Vec<f64>,
    pub uv_index_max: Vec<f64>,
}

// ── Open-Meteo air quality ───────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct OmAirQualityResponse {
    pub current: OmAirQualityCurrent,
}

#[derive(Deserialize)]
pub struct OmAirQualityCurrent {
    pub european_aqi: Option<f64>,
    pub us_aqi: Option<f64>,
    pub pm10: Option<f64>,
    pub pm2_5: Option<f64>,
    pub carbon_monoxide: Option<f64>,
    pub nitrogen_dioxide: Option<f64>,
    pub sulphur_dioxide: Option<f64>,
    pub ozone: Option<f64>,
    pub dust: Option<f64>,
    pub uv_index: Option<f64>,
}

// ── Client ───────────────────────────────────────────────────────────────────

pub struct OpenMeteoConfig {
    pub http_client: reqwest::Client,
}

pub struct OpenMeteoClient {
    http: reqwest::Client,
}

impl OpenMeteoClient {
    pub fn new(config: OpenMeteoConfig) -> Self {
        Self {
            http: config.http_client,
        }
    }

    pub async fn fetch_forecast(&self, lat: f64, lon: f64) -> Result<OmForecastResponse, ClientError> {
        let url = format!(
            "https://api.open-meteo.com/v1/forecast\
             ?latitude={lat}&longitude={lon}\
             &current=temperature_2m,relative_humidity_2m,apparent_temperature,is_day,\
             precipitation,rain,snowfall,weather_code,cloud_cover,pressure_msl,\
             surface_pressure,wind_speed_10m,wind_direction_10m,wind_gusts_10m\
             &hourly=temperature_2m,relative_humidity_2m,apparent_temperature,\
             precipitation_probability,precipitation,rain,snowfall,weather_code,\
             cloud_cover,visibility,wind_speed_10m,wind_direction_10m,pressure_msl,is_day\
             &daily=weather_code,temperature_2m_max,temperature_2m_min,\
             sunrise,sunset,precipitation_sum,rain_sum,snowfall_sum,\
             precipitation_probability_max,wind_speed_10m_max,uv_index_max\
             &wind_speed_unit=ms&timezone=auto&forecast_hours=48"
        );
        let resp = self.http.get(&url).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ClientError::Api {
                status: status.as_u16(),
                message: format!("Open-Meteo error: {body}"),
            });
        }
        Ok(resp.json::<OmForecastResponse>().await?)
    }

    pub async fn fetch_air_quality(&self, lat: f64, lon: f64) -> Result<OmAirQualityResponse, ClientError> {
        let url = format!(
            "https://air-quality-api.open-meteo.com/v1/air-quality\
             ?latitude={lat}&longitude={lon}\
             &current=european_aqi,us_aqi,pm10,pm2_5,carbon_monoxide,\
             nitrogen_dioxide,sulphur_dioxide,ozone,dust,uv_index\
             &timezone=auto"
        );
        let resp = self.http.get(&url).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ClientError::Api {
                status: status.as_u16(),
                message: format!("Air Quality API error: {body}"),
            });
        }
        Ok(resp.json::<OmAirQualityResponse>().await?)
    }
}
