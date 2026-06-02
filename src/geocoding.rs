use md5::Digest as _;
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use crate::error::ClientError;

/// Reverse-geocoded location data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoLocation {
    pub province: Option<String>,
    pub city: Option<String>,
    pub district: Option<String>,
    pub township: Option<String>,
    pub adcode: Option<String>,
    pub address: Option<String>,
    pub country: Option<String>,
}

// ── Amap (高德) ──────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct AmapResponse {
    status: String,
    info: Option<String>,
    regeocode: Option<AmapRegeocode>,
}

#[derive(Debug, Deserialize)]
struct AmapRegeocode {
    formatted_address: Option<AmapStrOrEmpty>,
    #[serde(rename = "addressComponent")]
    address_component: Option<AmapAddressComponent>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum AmapStrOrEmpty {
    Str(String),
    #[allow(dead_code)]
    List(Vec<serde_json::Value>),
}

impl AmapStrOrEmpty {
    fn as_opt_string(&self) -> Option<String> {
        match self {
            AmapStrOrEmpty::Str(s) if !s.is_empty() => Some(s.clone()),
            _ => None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct AmapAddressComponent {
    province: Option<AmapStrOrEmpty>,
    city: Option<AmapStrOrEmpty>,
    district: Option<AmapStrOrEmpty>,
    township: Option<AmapStrOrEmpty>,
    adcode: Option<AmapStrOrEmpty>,
    country: Option<AmapStrOrEmpty>,
}

// ── QQ Map (腾讯) ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct QqmapResponse {
    status: i32,
    message: Option<String>,
    result: Option<QqmapResult>,
}

#[derive(Debug, Deserialize)]
struct QqmapResult {
    address: Option<String>,
    address_component: Option<QqmapAddressComponent>,
    ad_info: Option<QqmapAdInfo>,
}

#[derive(Debug, Deserialize)]
struct QqmapAddressComponent {
    nation: Option<String>,
    province: Option<String>,
    city: Option<String>,
    district: Option<String>,
}

#[derive(Debug, Deserialize)]
struct QqmapAdInfo {
    adcode: Option<String>,
}

// ── Tianditu (天地图) ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct TiandituResponse {
    status: Option<String>,
    result: Option<TiandituResult>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TiandituResult {
    formatted_address: Option<String>,
    address_component: Option<TiandituAddressComponent>,
}

#[derive(Debug, Deserialize)]
struct TiandituAddressComponent {
    city: Option<String>,
}

// ── Mapbox / MapTiler ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct MapboxResponse {
    features: Option<Vec<MapboxFeature>>,
}

#[derive(Debug, Deserialize)]
struct MapboxFeature {
    place_name: Option<String>,
    context: Option<Vec<MapboxContext>>,
}

#[derive(Debug, Deserialize)]
struct MapboxContext {
    id: Option<String>,
    text: Option<String>,
}

// ── Client ───────────────────────────────────────────────────────────────────

pub struct GeocodingClient {
    http: reqwest::Client,
}

impl GeocodingClient {
    pub fn new(http: reqwest::Client) -> Self {
        Self { http }
    }

    pub async fn amap_reverse_geocode(
        &self,
        api_key: &str,
        secret: Option<&str>,
        lon: f64,
        lat: f64,
    ) -> Result<GeoLocation, ClientError> {
        let location = format!("{lon:.6},{lat:.6}");
        let mut url = format!(
            "https://restapi.amap.com/v3/geocode/regeo?key={api_key}&location={location}&extensions=base"
        );

        if let Some(sec) = secret
            && !sec.is_empty()
        {
            let mut params = [
                ("extensions", "base".to_string()),
                ("key", api_key.to_string()),
                ("location", location.clone()),
            ];
            params.sort_by(|a, b| a.0.cmp(b.0));
            let sign_str: String = params
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join("&");
            let sig = hex::encode(md5::Md5::digest(format!("{sign_str}{sec}").as_bytes()));
            write!(url, "&sig={sig}").ok();
        }

        let resp: AmapResponse = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| ClientError::Other(format!("Amap HTTP error: {e}")))?
            .json()
            .await
            .map_err(|e| ClientError::Other(format!("Amap JSON parse error: {e}")))?;

        if resp.status != "1" {
            let info = resp.info.unwrap_or_default();
            return Err(ClientError::Other(format!("Amap API error: {info}")));
        }

        let regeo = resp
            .regeocode
            .ok_or_else(|| ClientError::Other("Amap: no regeocode in response".into()))?;

        let comp = regeo.address_component;
        Ok(GeoLocation {
            province: comp
                .as_ref()
                .and_then(|c| c.province.as_ref()?.as_opt_string()),
            city: comp.as_ref().and_then(|c| c.city.as_ref()?.as_opt_string()),
            district: comp
                .as_ref()
                .and_then(|c| c.district.as_ref()?.as_opt_string()),
            township: comp
                .as_ref()
                .and_then(|c| c.township.as_ref()?.as_opt_string()),
            adcode: comp
                .as_ref()
                .and_then(|c| c.adcode.as_ref()?.as_opt_string()),
            address: regeo
                .formatted_address
                .as_ref()
                .and_then(AmapStrOrEmpty::as_opt_string),
            country: comp
                .as_ref()
                .and_then(|c| c.country.as_ref()?.as_opt_string()),
        })
    }

    pub async fn qqmap_reverse_geocode(
        &self,
        api_key: &str,
        secret_key: Option<&str>,
        lon: f64,
        lat: f64,
    ) -> Result<GeoLocation, ClientError> {
        let location = format!("{lat:.6},{lon:.6}");
        let url = if let Some(secret) = secret_key {
            let sign_str = format!("/ws/geocoder/v1/?key={api_key}&location={location}{secret}");
            let sig = hex::encode(md5::Md5::digest(sign_str.as_bytes()));
            format!(
                "https://apis.map.qq.com/ws/geocoder/v1/?key={api_key}&location={location}&sig={sig}"
            )
        } else {
            format!("https://apis.map.qq.com/ws/geocoder/v1/?key={api_key}&location={location}")
        };

        let resp: QqmapResponse = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| ClientError::Other(format!("QQ Map HTTP error: {e}")))?
            .json()
            .await
            .map_err(|e| ClientError::Other(format!("QQ Map JSON parse error: {e}")))?;

        if resp.status != 0 {
            let msg = resp.message.unwrap_or_default();
            return Err(ClientError::Other(format!("QQ Map API error: {msg}")));
        }

        let result = resp
            .result
            .ok_or_else(|| ClientError::Other("QQ Map: no result in response".into()))?;

        let comp = result.address_component.as_ref();
        Ok(GeoLocation {
            province: comp
                .and_then(|c| c.province.clone())
                .filter(|s| !s.is_empty()),
            city: comp.and_then(|c| c.city.clone()).filter(|s| !s.is_empty()),
            district: comp
                .and_then(|c| c.district.clone())
                .filter(|s| !s.is_empty()),
            township: None,
            adcode: result
                .ad_info
                .as_ref()
                .and_then(|a| a.adcode.clone())
                .filter(|s| !s.is_empty()),
            address: result.address.filter(|s| !s.is_empty()),
            country: comp
                .and_then(|c| c.nation.clone())
                .filter(|s| !s.is_empty()),
        })
    }

    pub async fn tianditu_reverse_geocode(
        &self,
        server_key: &str,
        lon: f64,
        lat: f64,
    ) -> Result<GeoLocation, ClientError> {
        let post_str = format!("{{'lon':{lon:.6},'lat':{lat:.6},'ver':1}}");
        let url = format!(
            "http://api.tianditu.gov.cn/geocoder?postStr={post_str}&type=geocode&tk={server_key}"
        );

        let resp: TiandituResponse = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| ClientError::Other(format!("Tianditu HTTP error: {e}")))?
            .json()
            .await
            .map_err(|e| ClientError::Other(format!("Tianditu JSON parse error: {e}")))?;

        if resp.status.as_deref() != Some("0") {
            return Err(ClientError::Other(format!(
                "Tianditu API error: status={:?}",
                resp.status
            )));
        }

        let result = resp
            .result
            .ok_or_else(|| ClientError::Other("Tianditu: no result in response".into()))?;

        let city = result
            .address_component
            .as_ref()
            .and_then(|c| c.city.clone())
            .filter(|s| !s.is_empty());

        let province = result.formatted_address.as_deref().and_then(parse_province);

        Ok(GeoLocation {
            province,
            city,
            district: None,
            township: None,
            adcode: None,
            address: result.formatted_address.filter(|s| !s.is_empty()),
            country: None,
        })
    }

    pub async fn mapbox_reverse_geocode(
        &self,
        access_token: &str,
        lon: f64,
        lat: f64,
    ) -> Result<GeoLocation, ClientError> {
        let url = format!(
            "https://api.mapbox.com/geocoding/v5/mapbox.places/{lon:.6},{lat:.6}.json\
             ?access_token={access_token}&language=zh&types=address,place,region,country"
        );

        let resp: MapboxResponse = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| ClientError::Other(format!("Mapbox HTTP error: {e}")))?
            .json()
            .await
            .map_err(|e| ClientError::Other(format!("Mapbox JSON parse error: {e}")))?;

        let feature = resp
            .features
            .as_ref()
            .and_then(|f| f.first())
            .ok_or_else(|| ClientError::Other("Mapbox: no features in response".into()))?;

        let (mut country, mut province, mut city, mut district) = (None, None, None, None);
        if let Some(ctx) = &feature.context {
            for item in ctx {
                let id = item.id.as_deref().unwrap_or_default();
                let text = item.text.clone().filter(|s| !s.is_empty());
                if id.starts_with("country.") {
                    country = text;
                } else if id.starts_with("region.") {
                    province = text;
                } else if id.starts_with("place.") {
                    city = text;
                } else if id.starts_with("district.") {
                    district = text;
                }
            }
        }

        Ok(GeoLocation {
            province,
            city,
            district,
            township: None,
            adcode: None,
            address: feature.place_name.clone().filter(|s| !s.is_empty()),
            country,
        })
    }

    pub async fn maptiler_reverse_geocode(
        &self,
        api_key: &str,
        lon: f64,
        lat: f64,
    ) -> Result<GeoLocation, ClientError> {
        let url = format!(
            "https://api.maptiler.com/geocoding/{lon:.6},{lat:.6}.json?key={api_key}&language=zh"
        );

        let resp: MapboxResponse = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| ClientError::Other(format!("MapTiler HTTP error: {e}")))?
            .json()
            .await
            .map_err(|e| ClientError::Other(format!("MapTiler JSON parse error: {e}")))?;

        let feature = resp
            .features
            .as_ref()
            .and_then(|f| f.first())
            .ok_or_else(|| ClientError::Other("MapTiler: no features in response".into()))?;

        let (mut country, mut province, mut city, mut district) = (None, None, None, None);
        if let Some(ctx) = &feature.context {
            for item in ctx {
                let id = item.id.as_deref().unwrap_or_default();
                let text = item.text.clone().filter(|s| !s.is_empty());
                if id.starts_with("country.") {
                    country = text;
                } else if id.starts_with("region.") {
                    province = text;
                } else if id.starts_with("municipality.") {
                    city = text;
                } else if id.starts_with("municipal_district.") {
                    district = text;
                }
            }
        }

        Ok(GeoLocation {
            province,
            city,
            district,
            township: None,
            adcode: None,
            address: feature.place_name.clone().filter(|s| !s.is_empty()),
            country,
        })
    }
}

/// Extract province from a Chinese formatted address string.
pub fn parse_province(addr: &str) -> Option<String> {
    for m in &["北京市", "天津市", "上海市", "重庆市"] {
        if addr.starts_with(m) {
            return Some((*m).to_string());
        }
    }
    if let Some(idx) = addr.find('省') {
        let prov = &addr[..idx + '省'.len_utf8()];
        if prov.chars().count() <= 10 {
            return Some(prov.to_string());
        }
    }
    for suffix in &["自治区"] {
        if let Some(idx) = addr.find(suffix) {
            let end = idx + suffix.len();
            let region = &addr[..end];
            if region.chars().count() <= 20 {
                return Some(region.to_string());
            }
        }
    }
    for sar in &["香港特别行政区", "澳门特别行政区"] {
        if addr.starts_with(sar) {
            return Some((*sar).to_string());
        }
    }
    None
}
