use std::time::Duration;

use crate::cache::RequestCache;
use crate::cloudflare::{CloudflareBypassClient, is_under_challenge};
use crate::error::ClientError;
use crate::types::{AdultMetadata, AdultSeriesVideo};

const CACHE_TTL: Duration = Duration::from_hours(24);

pub struct JavBusConfig {
    pub base_url: Option<String>,
    pub language: Option<String>,
    pub cookie: Option<String>,
    pub flaresolverr_url: Option<String>,
}

pub struct JavBusClient {
    base_url: String,
    language: String,
    cookie: String,
    bypass: CloudflareBypassClient,
    cache: RequestCache,
}

impl JavBusClient {
    pub fn new(config: JavBusConfig) -> Self {
        Self {
            base_url: config
                .base_url
                .unwrap_or_else(|| "https://www.javbus.com".to_string())
                .trim_end_matches('/')
                .to_string(),
            language: config.language.unwrap_or_else(|| "zh".to_string()),
            cookie: config.cookie.unwrap_or_default(),
            bypass: CloudflareBypassClient::new(config.flaresolverr_url),
            cache: RequestCache::new(CACHE_TTL),
        }
    }

    pub fn get_base_url(&self) -> &str {
        &self.base_url
    }

    pub async fn clear_cache(&self) {
        self.cache.clear().await;
    }

    /// Search by video ID (e.g., "FC2-PPV-4847315").
    pub async fn search_by_video_id(&self, video_id: &str) -> Result<Option<AdultMetadata>, ClientError> {
        let cache_key = format!("javbus:{video_id}");
        if let Some(cached) = self.cache.get::<Option<AdultMetadata>>(&cache_key).await {
            return Ok(cached);
        }

        let result = self.fetch_detail(video_id).await;
        let metadata = result.unwrap_or(None);
        self.cache.set(&cache_key, &metadata).await;
        Ok(metadata)
    }

    async fn fetch_detail(&self, video_id: &str) -> Result<Option<AdultMetadata>, ClientError> {
        let lang_prefix = if self.language == "ja" { "/ja" } else { "" };
        let url = format!("{}{lang_prefix}/{}", self.base_url, urlencoding::encode(video_id));

        let result = self
            .bypass
            .fetch_html(
                &url,
                if self.cookie.is_empty() {
                    None
                } else {
                    Some(self.cookie.as_str())
                },
            )
            .await?;

        check_cloudflare(&result.body)?;

        if result.status != 200 {
            return Ok(None);
        }

        Ok(parse_detail_page(&result.body, video_id, &url, &self.base_url))
    }

    /// Search all videos by series prefix (multi-page).
    pub async fn search_by_prefix(&self, prefix: &str, max_pages: u32) -> Result<Vec<AdultSeriesVideo>, ClientError> {
        let cache_key = format!("javbus:prefix:{prefix}");
        if let Some(cached) = self.cache.get::<Vec<AdultSeriesVideo>>(&cache_key).await {
            return Ok(cached);
        }

        let result = self.fetch_series_pages(prefix, max_pages).await?;
        self.cache.set(&cache_key, &result).await;
        Ok(result)
    }

    async fn fetch_series_pages(&self, prefix: &str, max_pages: u32) -> Result<Vec<AdultSeriesVideo>, ClientError> {
        let normalized = prefix.to_uppercase();
        let mut all_videos: Vec<AdultSeriesVideo> = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for page in 1..=max_pages {
            let url = format!(
                "{}/search/{}&type=1&parent=ce&page={page}",
                self.base_url,
                urlencoding::encode(&normalized)
            );

            let result = self
                .bypass
                .fetch_html(
                    &url,
                    if self.cookie.is_empty() {
                        None
                    } else {
                        Some(self.cookie.as_str())
                    },
                )
                .await?;

            check_cloudflare(&result.body)?;
            if result.status != 200 {
                break;
            }

            let videos = parse_search_page(&result.body, &normalized, &self.base_url);
            if videos.is_empty() {
                break;
            }

            let mut added_new = false;
            for v in videos {
                if seen.insert(v.video_id.clone()) {
                    all_videos.push(v);
                    added_new = true;
                }
            }

            if !added_new || !has_next_page(&result.body) {
                break;
            }
        }

        all_videos.sort_by(|a, b| {
            let num_a = extract_trailing_number(&a.video_id);
            let num_b = extract_trailing_number(&b.video_id);
            num_a.cmp(&num_b)
        });

        Ok(all_videos)
    }

    /// Get thumbnail URL from search page (fallback when URL derivation fails).
    pub async fn fetch_thumb_url_from_search(&self, video_id: &str) -> Result<Option<String>, ClientError> {
        let url = format!(
            "{}/search/{}&type=1&parent=ce",
            self.base_url,
            urlencoding::encode(video_id)
        );
        let result = self
            .bypass
            .fetch_html(
                &url,
                if self.cookie.is_empty() {
                    None
                } else {
                    Some(self.cookie.as_str())
                },
            )
            .await?;

        check_cloudflare(&result.body)?;
        if result.status != 200 {
            return Ok(None);
        }

        let document = scraper::Html::parse_document(&result.body);
        let upper = video_id.to_uppercase();
        let item_sel = scraper::Selector::parse("#waterfall .item, .movie-box, a.movie-box").expect("valid CSS selector");
        let date_sel = scraper::Selector::parse("date").expect("valid CSS selector");
        let img_sel = scraper::Selector::parse("img").expect("valid CSS selector");

        for el in document.select(&item_sel) {
            let vid = el
                .select(&date_sel)
                .next()
                .map(|d| d.text().collect::<String>().trim().to_uppercase());
            if vid.as_deref() == Some(&upper)
                && let Some(src) = el.select(&img_sel).next().and_then(|img| img.value().attr("src"))
            {
                return Ok(Some(resolve_url(src, &self.base_url)));
            }
        }

        Ok(None)
    }
}

fn check_cloudflare(body: &str) -> Result<(), ClientError> {
    if is_under_challenge(body) {
        Err(ClientError::CloudflareChallenge)
    } else {
        Ok(())
    }
}

fn parse_detail_page(html: &str, video_id: &str, source_url: &str, base_url: &str) -> Option<AdultMetadata> {
    let document = scraper::Html::parse_document(html);

    let h3_sel = scraper::Selector::parse("h3").expect("valid CSS selector");
    let title = document
        .select(&h3_sel)
        .next()
        .map(|el| el.text().collect::<String>().trim().to_string())?;

    if title.is_empty() {
        return None;
    }

    // Cover image
    let big_image_sel = scraper::Selector::parse("a.bigImage").expect("valid CSS selector");
    let big_img_sel = scraper::Selector::parse("a.bigImage img").expect("valid CSS selector");
    let raw_cover = document
        .select(&big_image_sel)
        .next()
        .and_then(|el| el.value().attr("href"))
        .or_else(|| {
            document
                .select(&big_img_sel)
                .next()
                .and_then(|el| el.value().attr("src"))
        });
    let cover_url = raw_cover.map(|u| resolve_url(u, base_url));
    let poster_url = cover_url
        .as_ref()
        .map(|u| u.replace("/pics/cover/", "/pics/thumb/").replace("_b.jpg", ".jpg"));

    // Actors
    let actor_sel = scraper::Selector::parse(".star-name a").expect("valid CSS selector");
    let actors: Vec<String> = document
        .select(&actor_sel)
        .map(|el| el.text().collect::<String>().trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    // Genres
    let genre_sel = scraper::Selector::parse(r#"span.genre a[href*="genre"]"#).expect("valid CSS selector");
    let genres: Vec<String> = document
        .select(&genre_sel)
        .map(|el| el.text().collect::<String>().trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    // Info fields
    let info_sel = scraper::Selector::parse(".info p").expect("valid CSS selector");
    let mut release_date = None;
    let mut studio = None;
    let mut duration = None;
    let a_sel = scraper::Selector::parse("a").expect("valid CSS selector");
    let date_re = regex::Regex::new(r"\d{4}-\d{2}-\d{2}").expect("valid regex");
    let num_re = regex::Regex::new(r"(\d+)").expect("valid regex");

    for el in document.select(&info_sel) {
        let text = el.text().collect::<String>();
        if text.contains("發行日期") || text.contains("发行日期") {
            release_date = date_re.find(&text).map(|m| m.as_str().to_string());
        }
        if text.contains("製作商") || text.contains("制作商") {
            studio = el
                .select(&a_sel)
                .next()
                .map(|a| a.text().collect::<String>().trim().to_string())
                .filter(|s| !s.is_empty());
        }
        if text.contains("長度") || text.contains("长度") {
            duration = num_re
                .captures(&text)
                .and_then(|c| c.get(1))
                .and_then(|m| m.as_str().parse().ok());
        }
    }

    Some(AdultMetadata {
        video_id: video_id.to_string(),
        title: Some(title.replace(video_id, "").trim().to_string())
            .filter(|s| !s.is_empty())
            .or(Some(title)),
        poster_url,
        cover_url,
        source_url: Some(source_url.to_string()),
        actors: if actors.is_empty() { None } else { Some(actors) },
        genres: if genres.is_empty() { None } else { Some(genres) },
        release_date,
        studio,
        duration,
        rating: None,
        source: "javbus".to_string(),
    })
}

fn parse_search_page(html: &str, prefix: &str, base_url: &str) -> Vec<AdultSeriesVideo> {
    let document = scraper::Html::parse_document(html);
    let item_sel = scraper::Selector::parse("#waterfall .item, .movie-box, a.movie-box").expect("valid CSS selector");
    let date_sel = scraper::Selector::parse("date").expect("valid CSS selector");
    let img_sel = scraper::Selector::parse("img").expect("valid CSS selector");

    let mut videos = Vec::new();
    let date_val_re = regex::Regex::new(r"\d{4}-\d{2}-\d{2}").expect("valid regex");

    for el in document.select(&item_sel) {
        let dates: Vec<_> = el.select(&date_sel).collect();
        let video_id = dates
            .first()
            .map(|d| d.text().collect::<String>().trim().to_uppercase())
            .unwrap_or_default();
        let release_date = dates
            .last()
            .map(|d| d.text().collect::<String>().trim().to_string())
            .filter(|s| date_val_re.is_match(s));
        let poster_url = el
            .select(&img_sel)
            .next()
            .and_then(|img| img.value().attr("src"))
            .map(|u| resolve_url(u, base_url));
        let title = el
            .select(&img_sel)
            .next()
            .and_then(|img| img.value().attr("title"))
            .map(String::from);

        if video_id.starts_with(prefix) {
            videos.push(AdultSeriesVideo {
                video_id,
                title,
                poster_url,
                release_date,
            });
        }
    }

    videos
}

fn has_next_page(html: &str) -> bool {
    let document = scraper::Html::parse_document(html);
    let sel = scraper::Selector::parse("a#next, .pagination a[id='next']").expect("valid CSS selector");
    document.select(&sel).next().is_some()
}

fn resolve_url(url: &str, base_url: &str) -> String {
    if url.starts_with("http") {
        url.to_string()
    } else if url.starts_with('/') {
        format!("{base_url}{url}")
    } else {
        format!("{base_url}/{url}")
    }
}

fn extract_trailing_number(s: &str) -> i64 {
    let re = regex::Regex::new(r"(\d+)$").expect("valid regex");
    re.captures(s)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse().ok())
        .unwrap_or(0)
}

mod urlencoding {
    pub fn encode(s: &str) -> String {
        url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
    }
}
