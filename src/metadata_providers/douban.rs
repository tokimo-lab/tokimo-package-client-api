use std::time::Duration;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::cache::RequestCache;
use crate::error::ClientError;

const DEFAULT_CACHE_TTL: Duration = Duration::from_hours(1);
const FRODO_BASE_URL: &str = "https://frodo.douban.com/api/v2";
const FRODO_API_KEY: &str = "0dad551ec0f84ed02907ff5c42e8ec70";
const FRODO_USER_AGENT: &str =
    "MicroMessenger/8.0.44.2502(0x2800002C) NetType/WIFI Language/zh_CN miniProgramSDK/2.33.0";
const DOUBAN_WEB_BASE: &str = "https://movie.douban.com";
const DOUBAN_BOOK_WEB_BASE: &str = "https://book.douban.com";

pub struct DoubanConfig {
    pub cookie: Option<String>,
    pub api_key: Option<String>,
    pub proxy_url: Option<String>,
    pub cache_ttl: Option<Duration>,
    pub http_client: reqwest::Client,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoubanSearchItem {
    pub douban_id: String,
    pub title: String,
    pub original_title: Option<String>,
    pub year: Option<String>,
    pub media_type: String,
    pub poster_url: Option<String>,
    pub rating: Option<f64>,
    pub rating_count: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoubanDetail {
    pub douban_id: String,
    pub title: String,
    pub original_title: Option<String>,
    pub year: Option<String>,
    pub overview: Option<String>,
    pub genres: Option<Vec<String>>,
    pub directors: Option<Vec<String>>,
    pub actors: Option<Vec<DoubanActor>>,
    pub rating: Option<f64>,
    pub rating_count: Option<i64>,
    pub imdb_id: Option<String>,
    pub poster_url: Option<String>,
    pub media_type: String,
    pub episode_count: Option<i32>,
    pub season_count: Option<i32>,
    pub tagline: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoubanActor {
    pub name: String,
    pub role: Option<String>,
}

/// Book-specific detail from Douban Books (`book.douban.com`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoubanBookDetail {
    pub douban_id: String,
    pub title: String,
    pub original_title: Option<String>,
    pub author: Option<String>,
    pub translator: Option<String>,
    pub publisher: Option<String>,
    pub year: Option<String>,
    pub isbn: Option<String>,
    pub pages: Option<i32>,
    pub overview: Option<String>,
    pub cover_url: Option<String>,
    pub rating: Option<f64>,
    pub rating_count: Option<i64>,
    pub tags: Option<Vec<String>>,
    pub binding: Option<String>,
    pub price: Option<String>,
}

// ---- Frodo API response types ----

#[derive(Deserialize)]
struct FrodoSubjectResponse {
    id: Option<String>,
    title: Option<String>,
    original_title: Option<String>,
    year: Option<String>,
    intro: Option<String>,
    #[serde(rename = "type")]
    subject_type: Option<String>,
    subtype: Option<String>,
    genres: Option<Vec<String>>,
    directors: Option<Vec<FrodoPerson>>,
    actors: Option<Vec<FrodoActor>>,
    rating: Option<FrodoRating>,
    imdb: Option<String>,
    pic: Option<FrodoPic>,
    cover_url: Option<String>,
    card_subtitle: Option<String>,
    episodes_count: Option<i32>,
    seasons_count: Option<i32>,
}

#[derive(Deserialize)]
struct FrodoPerson {
    name: Option<String>,
}

#[derive(Deserialize)]
struct FrodoActor {
    name: Option<String>,
    roles: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct FrodoRating {
    value: Option<f64>,
    count: Option<i64>,
}

#[derive(Deserialize)]
struct FrodoPic {
    large: Option<String>,
    normal: Option<String>,
}

// ---- Frodo Book API types ----

#[derive(Deserialize)]
#[allow(dead_code)]
struct FrodoBookResponse {
    id: Option<String>,
    title: Option<String>,
    original_title: Option<String>,
    subtitle: Option<String>,
    author: Option<Vec<String>>,
    translator: Option<Vec<String>>,
    publisher: Option<String>,
    #[serde(rename = "pubdate")]
    pub_date: Option<Vec<String>>,
    isbn: Option<String>,
    pages: Option<String>,
    intro: Option<String>,
    rating: Option<FrodoRating>,
    pic: Option<FrodoPic>,
    cover_url: Option<String>,
    card_subtitle: Option<String>,
    #[serde(default)]
    tags: Vec<FrodoBookTag>,
    binding: Option<String>,
    price: Option<String>,
}

#[derive(Deserialize)]
struct FrodoBookTag {
    name: Option<String>,
}

#[derive(Deserialize)]
struct FrodoBookSearchResponse {
    items: Option<Vec<FrodoBookSearchItem>>,
}

#[derive(Deserialize)]
struct FrodoBookSearchItem {
    target_type: Option<String>,
    target: Option<FrodoSearchTarget>,
}

#[derive(Deserialize)]
struct FrodoSearchResponse {
    items: Option<Vec<FrodoSearchItem>>,
}

#[derive(Deserialize)]
struct FrodoSearchItem {
    target_type: Option<String>,
    target: Option<FrodoSearchTarget>,
}

#[derive(Deserialize)]
struct FrodoSearchTarget {
    id: Option<String>,
    title: Option<String>,
    card_subtitle: Option<String>,
    year: Option<String>,
    cover_url: Option<String>,
    rating: Option<FrodoRating>,
}

#[derive(Deserialize)]
struct SuggestItem {
    id: Option<String>,
    title: Option<String>,
    year: Option<String>,
    #[serde(rename = "type")]
    item_type: Option<String>,
    img: Option<String>,
}

pub struct DoubanClient {
    cookie: Option<String>,
    http: reqwest::Client,
    cache: RequestCache,
}

impl DoubanClient {
    pub fn new(config: DoubanConfig) -> Self {
        Self {
            cookie: config.cookie,
            http: config.http_client,
            cache: RequestCache::new(config.cache_ttl.unwrap_or(DEFAULT_CACHE_TTL)),
        }
    }

    pub async fn clear_cache(&self) {
        self.cache.clear().await;
    }

    pub fn mode(&self) -> &str {
        if self.cookie.is_some() { "frodo" } else { "scraping" }
    }

    // ---- Public API ----

    pub async fn find_by_imdb_id(&self, imdb_id: &str) -> Result<Option<DoubanDetail>, ClientError> {
        let cache_key = format!("douban:imdb:{imdb_id}");
        if let Some(cached) = self.cache.get::<Option<DoubanDetail>>(&cache_key).await {
            return Ok(cached);
        }

        let result = if self.cookie.is_some() {
            self.frodo_find_by_imdb_id(imdb_id).await
        } else {
            self.scraping_find_by_imdb_id(imdb_id).await
        };

        let detail = result.unwrap_or(None);
        self.cache.set(&cache_key, &detail).await;
        Ok(detail)
    }

    pub async fn get_detail(&self, douban_id: &str) -> Result<Option<DoubanDetail>, ClientError> {
        let cache_key = format!("douban:detail:{douban_id}");
        if let Some(cached) = self.cache.get::<Option<DoubanDetail>>(&cache_key).await {
            return Ok(cached);
        }

        let result = if self.cookie.is_some() {
            self.frodo_get_detail(douban_id).await
        } else {
            self.scraping_get_detail(douban_id).await
        };

        let detail = result.unwrap_or(None);
        self.cache.set(&cache_key, &detail).await;
        Ok(detail)
    }

    pub async fn search(&self, query: &str) -> Result<Vec<DoubanSearchItem>, ClientError> {
        let cache_key = format!("douban:search:{query}");
        if let Some(cached) = self.cache.get::<Vec<DoubanSearchItem>>(&cache_key).await {
            return Ok(cached);
        }

        let result = if self.cookie.is_some() {
            self.frodo_search(query).await
        } else {
            self.scraping_search(query).await
        };

        let items = result.unwrap_or_default();
        self.cache.set(&cache_key, &items).await;
        Ok(items)
    }

    pub async fn test_connection(&self) -> Result<bool, ClientError> {
        match self.get_detail("1292052").await {
            Ok(Some(d)) => Ok(!d.title.is_empty()),
            _ => Ok(false),
        }
    }

    // ---- Book API ----

    /// Search books by title/ISBN.
    pub async fn search_books(&self, query: &str) -> Result<Vec<DoubanSearchItem>, ClientError> {
        let cache_key = format!("douban:book:search:{query}");
        if let Some(cached) = self.cache.get::<Vec<DoubanSearchItem>>(&cache_key).await {
            return Ok(cached);
        }

        let result = if self.cookie.is_some() {
            self.frodo_search_books(query).await
        } else {
            self.scraping_search_books(query).await
        };

        let items = result.unwrap_or_default();
        self.cache.set(&cache_key, &items).await;
        Ok(items)
    }

    /// Get book detail by Douban ID.
    pub async fn get_book_detail(&self, douban_id: &str) -> Result<Option<DoubanBookDetail>, ClientError> {
        let cache_key = format!("douban:book:detail:{douban_id}");
        if let Some(cached) = self.cache.get::<Option<DoubanBookDetail>>(&cache_key).await {
            return Ok(cached);
        }

        let result = if self.cookie.is_some() {
            self.frodo_get_book_detail(douban_id).await
        } else {
            self.scraping_get_book_detail(douban_id).await
        };

        let detail = result.unwrap_or(None);
        self.cache.set(&cache_key, &detail).await;
        Ok(detail)
    }

    /// Search book by ISBN.
    pub async fn find_book_by_isbn(&self, isbn: &str) -> Result<Option<DoubanBookDetail>, ClientError> {
        let results = self.search_books(isbn).await?;
        if let Some(first) = results.first() {
            return self.get_book_detail(&first.douban_id).await;
        }
        Ok(None)
    }

    // ---- Frodo API mode ----

    async fn frodo_request<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        params: &[(&str, &str)],
    ) -> Result<T, ClientError> {
        let mut url =
            url::Url::parse(&format!("{FRODO_BASE_URL}{path}")).map_err(|e| ClientError::Other(e.to_string()))?;

        url.query_pairs_mut().append_pair("apikey", FRODO_API_KEY);
        for (k, v) in params {
            url.query_pairs_mut().append_pair(k, v);
        }

        let resp = self
            .http
            .get(url)
            .header("Cookie", self.cookie.as_deref().unwrap_or(""))
            .header("User-Agent", FRODO_USER_AGENT)
            .header(
                "Referer",
                "https://servicewechat.com/wx2f9b06c1de1ccfca/91/page-frame.html",
            )
            .send()
            .await?;

        let status = resp.status().as_u16();
        if status == 401 || status == 403 {
            return Err(ClientError::Auth("豆瓣 Cookie 已过期".into()));
        }
        if !resp.status().is_success() {
            return Err(ClientError::Api {
                status,
                message: resp.text().await.unwrap_or_default(),
            });
        }

        Ok(resp.json().await?)
    }

    async fn frodo_find_by_imdb_id(&self, imdb_id: &str) -> Result<Option<DoubanDetail>, ClientError> {
        match self
            .frodo_request::<FrodoSubjectResponse>(&format!("/movie/imdb/{imdb_id}"), &[])
            .await
        {
            Ok(data) => Ok(parse_frodo_detail(&data)),
            Err(ClientError::Auth(_)) => Err(ClientError::Auth("豆瓣 Cookie 已过期".into())),
            Err(_) => Ok(None),
        }
    }

    async fn frodo_get_detail(&self, douban_id: &str) -> Result<Option<DoubanDetail>, ClientError> {
        match self
            .frodo_request::<FrodoSubjectResponse>(&format!("/movie/{douban_id}"), &[])
            .await
        {
            Ok(data) => Ok(parse_frodo_detail(&data)),
            Err(ClientError::Auth(_)) => Err(ClientError::Auth("豆瓣 Cookie 已过期".into())),
            Err(_) => Ok(None),
        }
    }

    async fn frodo_search(&self, query: &str) -> Result<Vec<DoubanSearchItem>, ClientError> {
        let data: FrodoSearchResponse = self
            .frodo_request("/search/movie", &[("q", query), ("count", "10")])
            .await?;

        let items = data.items.unwrap_or_default();
        Ok(items
            .into_iter()
            .filter(|item| matches!(item.target_type.as_deref(), Some("movie" | "tv")))
            .filter_map(|item| {
                let target = item.target?;
                let id = target.id.filter(|s| !s.is_empty())?;
                Some(DoubanSearchItem {
                    douban_id: id,
                    title: target.title.unwrap_or_default(),
                    original_title: target.card_subtitle,
                    year: target.year,
                    media_type: if item.target_type.as_deref() == Some("tv") {
                        "tv".to_string()
                    } else {
                        "movie".to_string()
                    },
                    poster_url: target.cover_url,
                    rating: target.rating.as_ref().and_then(|r| r.value),
                    rating_count: target.rating.as_ref().and_then(|r| r.count),
                })
            })
            .collect())
    }

    // ---- HTML scraping mode ----

    async fn web_request(&self, path: &str) -> Result<String, ClientError> {
        let url = format!("{DOUBAN_WEB_BASE}{path}");
        let resp = self
            .http
            .get(&url)
            .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            .header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
            .header("Accept-Language", "zh-CN,zh;q=0.9,en;q=0.8")
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(ClientError::Api {
                status: resp.status().as_u16(),
                message: "Douban web request failed".into(),
            });
        }

        Ok(resp.text().await?)
    }

    async fn scraping_find_by_imdb_id(&self, imdb_id: &str) -> Result<Option<DoubanDetail>, ClientError> {
        let suggest_url = format!("{DOUBAN_WEB_BASE}/j/subject_suggest?q={}", urlencoding::encode(imdb_id));
        let resp = self
            .http
            .get(&suggest_url)
            .header(
                "User-Agent",
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
            )
            .send()
            .await;

        if let Ok(resp) = resp
            && resp.status().is_success()
            && let Ok(results) = resp.json::<Vec<SuggestItem>>().await
            && let Some(first) = results.first()
            && let Some(ref id) = first.id
        {
            return self.scraping_get_detail(id).await;
        }

        // Fallback: web search
        if let Ok(html) = self
            .web_request(&format!(
                "/subject_search?search_text={}&cat=1002",
                urlencoding::encode(imdb_id)
            ))
            .await
        {
            let re = Regex::new(r"subject/(\d+)/").unwrap();
            if let Some(caps) = re.captures(&html) {
                return self.scraping_get_detail(&caps[1]).await;
            }
        }

        Ok(None)
    }

    async fn scraping_get_detail(&self, douban_id: &str) -> Result<Option<DoubanDetail>, ClientError> {
        let html = self.web_request(&format!("/subject/{douban_id}/")).await?;
        Ok(parse_detail_html(&html, douban_id))
    }

    async fn scraping_search(&self, query: &str) -> Result<Vec<DoubanSearchItem>, ClientError> {
        let suggest_url = format!("{DOUBAN_WEB_BASE}/j/subject_suggest?q={}", urlencoding::encode(query));
        let resp = self
            .http
            .get(&suggest_url)
            .header(
                "User-Agent",
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
            )
            .send()
            .await?;

        if !resp.status().is_success() {
            return Ok(vec![]);
        }

        let results: Vec<SuggestItem> = resp.json().await.unwrap_or_default();
        Ok(results
            .into_iter()
            .filter(|item| item.item_type.as_deref() == Some("movie"))
            .filter_map(|item| {
                let id = item.id.filter(|s| !s.is_empty())?;
                Some(DoubanSearchItem {
                    douban_id: id,
                    title: item.title.unwrap_or_default(),
                    original_title: None,
                    year: item.year,
                    media_type: "movie".to_string(),
                    poster_url: item.img,
                    rating: None,
                    rating_count: None,
                })
            })
            .collect())
    }

    // ---- Frodo Book API ----

    async fn frodo_search_books(&self, query: &str) -> Result<Vec<DoubanSearchItem>, ClientError> {
        let data: FrodoBookSearchResponse = self
            .frodo_request("/search/book", &[("q", query), ("count", "10")])
            .await?;

        let items = data.items.unwrap_or_default();
        Ok(items
            .into_iter()
            .filter(|item| item.target_type.as_deref() == Some("book"))
            .filter_map(|item| {
                let target = item.target?;
                let id = target.id.filter(|s| !s.is_empty())?;
                Some(DoubanSearchItem {
                    douban_id: id,
                    title: target.title.unwrap_or_default(),
                    original_title: target.card_subtitle,
                    year: target.year,
                    media_type: "book".to_string(),
                    poster_url: target.cover_url,
                    rating: target.rating.as_ref().and_then(|r| r.value),
                    rating_count: target.rating.as_ref().and_then(|r| r.count),
                })
            })
            .collect())
    }

    async fn frodo_get_book_detail(&self, douban_id: &str) -> Result<Option<DoubanBookDetail>, ClientError> {
        match self
            .frodo_request::<FrodoBookResponse>(&format!("/book/{douban_id}"), &[])
            .await
        {
            Ok(data) => Ok(parse_frodo_book_detail(&data)),
            Err(ClientError::Auth(_)) => Err(ClientError::Auth("豆瓣 Cookie 已过期".into())),
            Err(_) => Ok(None),
        }
    }

    // ---- Scraping Book API ----

    async fn book_web_request(&self, path: &str) -> Result<String, ClientError> {
        let url = format!("{DOUBAN_BOOK_WEB_BASE}{path}");
        let resp = self
            .http
            .get(&url)
            .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            .header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
            .header("Accept-Language", "zh-CN,zh;q=0.9,en;q=0.8")
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(ClientError::Api {
                status: resp.status().as_u16(),
                message: "Douban book web request failed".into(),
            });
        }

        Ok(resp.text().await?)
    }

    async fn scraping_search_books(&self, query: &str) -> Result<Vec<DoubanSearchItem>, ClientError> {
        let suggest_url = format!(
            "{DOUBAN_BOOK_WEB_BASE}/j/subject_suggest?q={}",
            urlencoding::encode(query)
        );
        let resp = self
            .http
            .get(&suggest_url)
            .header(
                "User-Agent",
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
            )
            .send()
            .await?;

        if !resp.status().is_success() {
            return Ok(vec![]);
        }

        let results: Vec<SuggestItem> = resp.json().await.unwrap_or_default();
        Ok(results
            .into_iter()
            .filter_map(|item| {
                let id = item.id.filter(|s| !s.is_empty())?;
                Some(DoubanSearchItem {
                    douban_id: id,
                    title: item.title.unwrap_or_default(),
                    original_title: None,
                    year: item.year,
                    media_type: "book".to_string(),
                    poster_url: item.img,
                    rating: None,
                    rating_count: None,
                })
            })
            .collect())
    }

    async fn scraping_get_book_detail(&self, douban_id: &str) -> Result<Option<DoubanBookDetail>, ClientError> {
        let html = self.book_web_request(&format!("/subject/{douban_id}/")).await?;
        Ok(parse_book_detail_html(&html, douban_id))
    }
}

fn parse_frodo_detail(data: &FrodoSubjectResponse) -> Option<DoubanDetail> {
    let id = data.id.as_ref()?;
    let is_tv = data.subject_type.as_deref() == Some("tv")
        || data.subtype.as_deref() == Some("tv")
        || data.episodes_count.unwrap_or(0) > 0;

    Some(DoubanDetail {
        douban_id: id.clone(),
        title: data.title.clone().unwrap_or_default(),
        original_title: data.original_title.clone(),
        year: data.year.clone(),
        overview: data.intro.clone(),
        genres: data.genres.clone(),
        directors: data
            .directors
            .as_ref()
            .map(|ds| ds.iter().filter_map(|d| d.name.clone()).collect()),
        actors: data.actors.as_ref().map(|actors| {
            actors
                .iter()
                .filter_map(|a| {
                    Some(DoubanActor {
                        name: a.name.clone()?,
                        role: a.roles.as_ref().map(|r| r.join("/")),
                    })
                })
                .collect()
        }),
        rating: data.rating.as_ref().and_then(|r| r.value),
        rating_count: data.rating.as_ref().and_then(|r| r.count),
        imdb_id: data.imdb.clone(),
        poster_url: data
            .pic
            .as_ref()
            .and_then(|p| p.large.clone().or(p.normal.clone()))
            .or(data.cover_url.clone()),
        media_type: if is_tv { "tv" } else { "movie" }.to_string(),
        episode_count: data.episodes_count,
        season_count: data.seasons_count,
        tagline: data.card_subtitle.clone(),
    })
}

fn parse_detail_html(html: &str, douban_id: &str) -> Option<DoubanDetail> {
    let mut detail = DoubanDetail {
        douban_id: douban_id.to_string(),
        title: String::new(),
        original_title: None,
        year: None,
        overview: None,
        genres: None,
        directors: None,
        actors: None,
        rating: None,
        rating_count: None,
        imdb_id: None,
        poster_url: None,
        media_type: "movie".to_string(),
        episode_count: None,
        season_count: None,
        tagline: None,
    };

    // Title
    let re = Regex::new(r#"<span\s+property="v:itemreviewed">(.*?)</span>"#).unwrap();
    if let Some(caps) = re.captures(html) {
        detail.title = decode_html_entities(&caps[1]);
    }

    // Year
    let re = Regex::new(r#"<span\s+class="year">\((\d{4})\)</span>"#).unwrap();
    if let Some(caps) = re.captures(html) {
        detail.year = Some(caps[1].to_string());
    }

    // Overview
    let re_full = Regex::new(r#"<span\s+class="all\s+hidden">\s*([\s\S]*?)\s*</span>"#).unwrap();
    let re_short = Regex::new(r#"<span\s+property="v:summary"[^>]*>\s*([\s\S]*?)\s*</span>"#).unwrap();
    let plot_match = re_full.captures(html).or_else(|| re_short.captures(html));
    if let Some(caps) = plot_match {
        let raw = caps[1].to_string();
        let cleaned = Regex::new(r"<br\s*/?>").unwrap().replace_all(&raw, "\n");
        let cleaned = Regex::new(r"<[^>]+>").unwrap().replace_all(&cleaned, "");
        detail.overview = Some(decode_html_entities(cleaned.trim()));
    }

    // Rating
    let re = Regex::new(r#"<strong[^>]*class="ll\s+rating_num"[^>]*>([\d.]+)</strong>"#).unwrap();
    if let Some(caps) = re.captures(html) {
        detail.rating = caps[1].parse().ok();
    }

    // Rating count
    let re = Regex::new(r#"<span\s+property="v:votes">(\d+)</span>"#).unwrap();
    if let Some(caps) = re.captures(html) {
        detail.rating_count = caps[1].parse().ok();
    }

    // IMDb ID
    let re = Regex::new(r"IMDb:.*?(tt\d+)").unwrap();
    if let Some(caps) = re.captures(html) {
        detail.imdb_id = Some(caps[1].to_string());
    }

    // Directors
    let re = Regex::new(r#"<a[^>]*rel="v:directedBy"[^>]*>(.*?)</a>"#).unwrap();
    let directors: Vec<String> = re
        .captures_iter(html)
        .map(|caps| decode_html_entities(&caps[1]))
        .collect();
    if !directors.is_empty() {
        detail.directors = Some(directors);
    }

    // Actors
    let re = Regex::new(r#"<a[^>]*rel="v:starring"[^>]*>(.*?)</a>"#).unwrap();
    let actors: Vec<DoubanActor> = re
        .captures_iter(html)
        .map(|caps| DoubanActor {
            name: decode_html_entities(&caps[1]),
            role: None,
        })
        .collect();
    if !actors.is_empty() {
        detail.actors = Some(actors);
    }

    // Genres
    let re = Regex::new(r#"<span\s+property="v:genre">(.*?)</span>"#).unwrap();
    let genres: Vec<String> = re
        .captures_iter(html)
        .map(|caps| decode_html_entities(&caps[1]))
        .collect();
    if !genres.is_empty() {
        detail.genres = Some(genres);
    }

    // Movie vs TV detection
    if html.contains("集数") || html.contains("季数") || html.contains("首播") {
        detail.media_type = "tv".to_string();
    }

    // Episode count
    let re = Regex::new(r"集数:</span>\s*(\d+)").unwrap();
    if let Some(caps) = re.captures(html) {
        detail.episode_count = caps[1].parse().ok();
    }

    // Original title
    let re = Regex::new(r"原名:</span>\s*(.*?)<br").unwrap();
    if let Some(caps) = re.captures(html) {
        detail.original_title = Some(decode_html_entities(caps[1].trim()));
    }

    if detail.title.is_empty() { None } else { Some(detail) }
}

fn decode_html_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .trim()
        .to_string()
}

mod urlencoding {
    pub fn encode(s: &str) -> String {
        url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
    }
}

// ---- Book parsing helpers ----

fn parse_frodo_book_detail(data: &FrodoBookResponse) -> Option<DoubanBookDetail> {
    let id = data.id.as_ref()?;
    Some(DoubanBookDetail {
        douban_id: id.clone(),
        title: data.title.clone().unwrap_or_default(),
        original_title: data.original_title.clone().or(data.subtitle.clone()),
        author: data.author.as_ref().map(|a| a.join(", ")),
        translator: data.translator.as_ref().map(|t| t.join(", ")),
        publisher: data.publisher.clone(),
        year: data.pub_date.as_ref().and_then(|dates| dates.first().cloned()),
        isbn: data.isbn.clone(),
        pages: data.pages.as_ref().and_then(|p| p.parse().ok()),
        overview: data.intro.clone(),
        cover_url: data
            .pic
            .as_ref()
            .and_then(|p| p.large.clone().or(p.normal.clone()))
            .or(data.cover_url.clone()),
        rating: data.rating.as_ref().and_then(|r| r.value),
        rating_count: data.rating.as_ref().and_then(|r| r.count),
        tags: {
            let tags: Vec<String> = data.tags.iter().filter_map(|t| t.name.clone()).collect();
            if tags.is_empty() { None } else { Some(tags) }
        },
        binding: data.binding.clone(),
        price: data.price.clone(),
    })
}

fn parse_book_detail_html(html: &str, douban_id: &str) -> Option<DoubanBookDetail> {
    let mut detail = DoubanBookDetail {
        douban_id: douban_id.to_string(),
        title: String::new(),
        original_title: None,
        author: None,
        translator: None,
        publisher: None,
        year: None,
        isbn: None,
        pages: None,
        overview: None,
        cover_url: None,
        rating: None,
        rating_count: None,
        tags: None,
        binding: None,
        price: None,
    };

    // Title
    let re = Regex::new(r#"<span\s+property="v:itemreviewed">(.*?)</span>"#).unwrap();
    if let Some(caps) = re.captures(html) {
        detail.title = decode_html_entities(&caps[1]);
    }
    // Fallback title from <title> tag
    if detail.title.is_empty() {
        let re = Regex::new(r"<title>\s*(.*?)\s*\(豆瓣\)").unwrap();
        if let Some(caps) = re.captures(html) {
            detail.title = decode_html_entities(caps[1].trim());
        }
    }

    // The #info div contains structured metadata as spans
    // Extract the #info section
    let info_re = Regex::new(r#"<div[^>]*id="info"[^>]*>([\s\S]*?)</div>"#).unwrap();
    if let Some(info_caps) = info_re.captures(html) {
        let info = &info_caps[1];

        // Author — links inside the author section
        let author_re = Regex::new(r#"作者.*?</span>([\s\S]*?)(?:<br|<span class="pl")"#).unwrap();
        if let Some(caps) = author_re.captures(info) {
            let link_re = Regex::new(r"<a[^>]*>(.*?)</a>").unwrap();
            let authors: Vec<String> = link_re
                .captures_iter(&caps[1])
                .map(|c| decode_html_entities(c[1].trim()))
                .filter(|s| !s.is_empty())
                .collect();
            if !authors.is_empty() {
                detail.author = Some(authors.join(", "));
            }
        }

        // Translator
        let translator_re = Regex::new(r#"译者.*?</span>([\s\S]*?)(?:<br|<span class="pl")"#).unwrap();
        if let Some(caps) = translator_re.captures(info) {
            let link_re = Regex::new(r"<a[^>]*>(.*?)</a>").unwrap();
            let translators: Vec<String> = link_re
                .captures_iter(&caps[1])
                .map(|c| decode_html_entities(c[1].trim()))
                .filter(|s| !s.is_empty())
                .collect();
            if !translators.is_empty() {
                detail.translator = Some(translators.join(", "));
            }
        }

        // Publisher
        let publisher_re = Regex::new(r"出版社:</span>\s*(.*?)(?:<br|</|<span)").unwrap();
        if let Some(caps) = publisher_re.captures(info) {
            let text = Regex::new(r"<[^>]+>").unwrap().replace_all(&caps[1], "");
            let p = decode_html_entities(text.trim());
            if !p.is_empty() {
                detail.publisher = Some(p);
            }
        }

        // Publish date
        let year_re = Regex::new(r"出版年:</span>\s*(.*?)(?:<br|</|<span)").unwrap();
        if let Some(caps) = year_re.captures(info) {
            let text = Regex::new(r"<[^>]+>").unwrap().replace_all(&caps[1], "");
            let y = decode_html_entities(text.trim());
            if !y.is_empty() {
                detail.year = Some(y);
            }
        }

        // ISBN
        let isbn_re = Regex::new(r"ISBN:</span>\s*([\d\-Xx]+)").unwrap();
        if let Some(caps) = isbn_re.captures(info) {
            detail.isbn = Some(caps[1].replace('-', "").trim().to_string());
        }

        // Pages
        let pages_re = Regex::new(r"页数:</span>\s*(\d+)").unwrap();
        if let Some(caps) = pages_re.captures(info) {
            detail.pages = caps[1].parse().ok();
        }

        // Original title
        let orig_re = Regex::new(r"原作名:</span>\s*(.*?)(?:<br|</|<span)").unwrap();
        if let Some(caps) = orig_re.captures(info) {
            let text = Regex::new(r"<[^>]+>").unwrap().replace_all(&caps[1], "");
            let t = decode_html_entities(text.trim());
            if !t.is_empty() {
                detail.original_title = Some(t);
            }
        }

        // Binding
        let binding_re = Regex::new(r"装帧:</span>\s*(.*?)(?:<br|</|<span)").unwrap();
        if let Some(caps) = binding_re.captures(info) {
            let text = Regex::new(r"<[^>]+>").unwrap().replace_all(&caps[1], "");
            let b = decode_html_entities(text.trim());
            if !b.is_empty() {
                detail.binding = Some(b);
            }
        }

        // Price
        let price_re = Regex::new(r"定价:</span>\s*(.*?)(?:<br|</|<span)").unwrap();
        if let Some(caps) = price_re.captures(info) {
            let text = Regex::new(r"<[^>]+>").unwrap().replace_all(&caps[1], "");
            let p = decode_html_entities(text.trim());
            if !p.is_empty() {
                detail.price = Some(p);
            }
        }
    }

    // Overview / summary
    // Douban uses <span class="all hidden"> (full text) or <div class="intro"> / <span class="intro">
    let re_full = Regex::new(r#"<span\s+class="all\s+hidden">\s*([\s\S]*?)\s*</span>"#).unwrap();
    let re_short_div = Regex::new(r#"<div\s+class="intro">\s*([\s\S]*?)\s*</div>"#).unwrap();
    let re_short_span = Regex::new(r#"<span\s+class="intro">\s*([\s\S]*?)\s*</span>"#).unwrap();
    let intro_section =
        Regex::new(r#"内容简介[\s\S]*?(<(?:span|div)\s+class="(?:all hidden|intro)"[\s\S]*?</(?:span|div)>)"#).unwrap();
    if let Some(section) = intro_section.captures(html) {
        let section_html = &section[1];
        let plot_match = re_full
            .captures(section_html)
            .or_else(|| re_short_div.captures(section_html))
            .or_else(|| re_short_span.captures(section_html));
        if let Some(caps) = plot_match {
            let raw = caps[1].to_string();
            let cleaned = Regex::new(r"<br\s*/?>").unwrap().replace_all(&raw, "\n");
            let cleaned = Regex::new(r"<[^>]+>").unwrap().replace_all(&cleaned, "");
            let text = decode_html_entities(cleaned.trim());
            if !text.is_empty() {
                detail.overview = Some(text);
            }
        }
    }

    // Rating — value may have surrounding whitespace like "> 7.0 <"
    let re = Regex::new(r#"<strong[^>]*class="[^"]*rating_num[^"]*"[^>]*>\s*([\d.]+)\s*</strong>"#).unwrap();
    if let Some(caps) = re.captures(html) {
        detail.rating = caps[1].parse().ok();
    }

    // Rating count
    let re = Regex::new(r#"<span\s+property="v:votes">(\d+)</span>"#).unwrap();
    if let Some(caps) = re.captures(html) {
        detail.rating_count = caps[1].parse().ok();
    }

    // Cover image — prefer large (/l/) over small (/s/)
    let re_large = Regex::new(r#"<img[^>]*src="(https://img\d\.doubanio\.com/view/subject/l/[^"]+)"#).unwrap();
    let re_any = Regex::new(r#"<img[^>]*src="(https://img\d\.doubanio\.com/view/subject/[^"]+)"#).unwrap();
    if let Some(caps) = re_large.captures(html).or_else(|| re_any.captures(html)) {
        detail.cover_url = Some(caps[1].to_string());
    }

    if detail.title.is_empty() { None } else { Some(detail) }
}
