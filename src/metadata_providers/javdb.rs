use std::time::Duration;

use crate::cache::RequestCache;
use crate::cloudflare::{CloudflareBypassClient, is_under_challenge};
use crate::error::ClientError;
use crate::types::{AdultMetadata, AdultSeriesVideo};

const CACHE_TTL: Duration = Duration::from_hours(24);

pub struct JavDBConfig {
    pub base_url: Option<String>,
    pub language: Option<String>,
    pub cookie: Option<String>,
    pub flaresolverr_url: Option<String>,
}

pub struct JavDBClient {
    base_url: String,
    language: String,
    cookie: String,
    bypass: CloudflareBypassClient,
    cache: RequestCache,
}

impl JavDBClient {
    pub fn new(config: JavDBConfig) -> Self {
        let base_cookie = "over18=1; locale=zh";
        let cookie = match config.cookie {
            Some(c) => format!("{base_cookie}; {c}"),
            None => base_cookie.to_string(),
        };

        Self {
            base_url: config
                .base_url
                .unwrap_or_else(|| "https://javdb.com".to_string())
                .trim_end_matches('/')
                .to_string(),
            language: config.language.unwrap_or_else(|| "zh".to_string()),
            cookie,
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

    /// Search by video ID.
    pub async fn search_by_video_id(&self, video_id: &str) -> Result<Option<AdultMetadata>, ClientError> {
        let cache_key = format!("javdb:{video_id}");
        if let Some(cached) = self.cache.get::<Option<AdultMetadata>>(&cache_key).await {
            return Ok(cached);
        }

        let result = self.fetch_by_video_id(video_id).await;
        let metadata = result.unwrap_or(None);
        self.cache.set(&cache_key, &metadata).await;
        Ok(metadata)
    }

    async fn fetch_by_video_id(&self, video_id: &str) -> Result<Option<AdultMetadata>, ClientError> {
        let locale = if self.language == "ja" { "ja" } else { "zh" };
        let search_url = format!(
            "{}/search?q={}&locale={locale}",
            self.base_url,
            urlencoding::encode(video_id)
        );

        let search_result = self.bypass.fetch_html(&search_url, Some(&self.cookie)).await?;
        check_cloudflare(&search_result.body)?;
        if search_result.status != 200 {
            return Ok(None);
        }

        let Some(detail_path) = extract_detail_path(&search_result.body, video_id) else {
            return Ok(None);
        };

        let detail_url = format!("{}{}?locale={locale}", self.base_url, detail_path);
        let detail_result = self.bypass.fetch_html(&detail_url, Some(&self.cookie)).await?;
        check_cloudflare(&detail_result.body)?;
        if detail_result.status != 200 {
            return Ok(None);
        }

        Ok(parse_detail_page(
            &detail_result.body,
            video_id,
            &detail_url,
            &self.base_url,
        ))
    }

    /// Search all videos by series prefix (multi-page).
    pub async fn search_by_prefix(&self, prefix: &str, max_pages: u32) -> Result<Vec<AdultSeriesVideo>, ClientError> {
        let cache_key = format!("javdb:prefix:{prefix}");
        if let Some(cached) = self.cache.get::<Vec<AdultSeriesVideo>>(&cache_key).await {
            return Ok(cached);
        }

        let result = self.fetch_series_pages(prefix, max_pages).await?;
        self.cache.set(&cache_key, &result).await;
        Ok(result)
    }

    async fn fetch_series_pages(&self, prefix: &str, max_pages: u32) -> Result<Vec<AdultSeriesVideo>, ClientError> {
        let normalized = prefix.to_uppercase();
        let locale = if self.language == "ja" { "ja" } else { "zh" };
        let mut all_videos: Vec<AdultSeriesVideo> = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for page in 1..=max_pages {
            let url = format!(
                "{}/search?q={}&f=all&page={page}&locale={locale}",
                self.base_url,
                urlencoding::encode(&normalized)
            );
            let result = self.bypass.fetch_html(&url, Some(&self.cookie)).await?;
            check_cloudflare(&result.body)?;
            if result.status != 200 {
                break;
            }

            let videos = parse_search_page_list(&result.body, &normalized, &self.base_url);
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

            if !added_new || !has_next_page_link(&result.body) {
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
}

fn check_cloudflare(body: &str) -> Result<(), ClientError> {
    if is_under_challenge(body) {
        Err(ClientError::CloudflareChallenge)
    } else {
        Ok(())
    }
}

fn extract_detail_path(html: &str, video_id: &str) -> Option<String> {
    let document = scraper::Html::parse_document(html);
    let normalized = video_id.to_uppercase();

    let link_sel = scraper::Selector::parse(".movie-list .item a, .grid-item a").expect("valid CSS selector");
    let uid_sel = scraper::Selector::parse(".uid, .video-title strong").expect("valid CSS selector");

    // Exact match
    for el in document.select(&link_sel) {
        let uid = el
            .select(&uid_sel)
            .next()
            .map(|u| u.text().collect::<String>().trim().to_uppercase());
        if uid.as_deref() == Some(&normalized)
            && let Some(href) = el.value().attr("href")
        {
            return Some(href.to_string());
        }
    }

    // Fallback: first result
    document
        .select(&link_sel)
        .next()
        .and_then(|el| el.value().attr("href"))
        .map(String::from)
}

fn parse_detail_page(html: &str, video_id: &str, source_url: &str, base_url: &str) -> Option<AdultMetadata> {
    let document = scraper::Html::parse_document(html);

    // Title
    let title_sel1 = scraper::Selector::parse("h2.title strong.current-title").expect("valid CSS selector");
    let title_sel2 = scraper::Selector::parse("h2.title").expect("valid CSS selector");
    let title = document
        .select(&title_sel1)
        .next()
        .or_else(|| document.select(&title_sel2).next())
        .map(|el| el.text().collect::<String>().trim().to_string())?;

    if title.is_empty() {
        return None;
    }

    // Poster
    let poster_sel = scraper::Selector::parse(".video-cover img, .column-video-cover img").expect("valid CSS selector");
    let poster_url = document
        .select(&poster_sel)
        .next()
        .and_then(|el| el.value().attr("src"))
        .map(|u| resolve_url(u, base_url));

    // Rating
    let rating_sel = scraper::Selector::parse(".score .value").expect("valid CSS selector");
    let rating = document
        .select(&rating_sel)
        .next()
        .and_then(|el| el.text().collect::<String>().trim().parse::<f64>().ok());

    // Metadata panels
    let panel_sel = scraper::Selector::parse(".movie-panel-info .panel-block, .video-meta-panel .panel-block").expect("valid CSS selector");
    let strong_sel = scraper::Selector::parse("strong, .header").expect("valid CSS selector");
    let value_sel = scraper::Selector::parse(".value, span:not(.header)").expect("valid CSS selector");
    let a_sel = scraper::Selector::parse("a").expect("valid CSS selector");

    let mut actors: Vec<String> = Vec::new();
    let mut genres: Vec<String> = Vec::new();
    let mut release_date = None;
    let mut studio = None;
    let mut duration = None;

    let date_re = regex::Regex::new(r"\d{4}-\d{2}-\d{2}").expect("valid regex");
    let num_re = regex::Regex::new(r"(\d+)").expect("valid regex");
    for el in document.select(&panel_sel) {
        let label = el
            .select(&strong_sel)
            .next()
            .map(|s| s.text().collect::<String>())
            .unwrap_or_default();
        let value = el
            .select(&value_sel)
            .next()
            .map(|s| s.text().collect::<String>())
            .unwrap_or_default();

        if label.contains("日期") || label.contains("Date") {
            release_date = date_re.find(&value).map(|m| m.as_str().to_string());
        } else if label.contains("片商") || label.contains("Maker") {
            studio = el
                .select(&a_sel)
                .next()
                .map(|a| a.text().collect::<String>().trim().to_string())
                .filter(|s| !s.is_empty());
        } else if label.contains("時長") || label.contains("時间") || label.contains("Duration") {
            duration = num_re
                .captures(&value)
                .and_then(|c| c.get(1))
                .and_then(|m| m.as_str().parse().ok());
        } else if label.contains("類別") || label.contains("类别") || label.contains("Genre") {
            for a in el.select(&a_sel) {
                let g = a.text().collect::<String>().trim().to_string();
                if !g.is_empty() {
                    genres.push(g);
                }
            }
        } else if label.contains("演員") || label.contains("演员") || label.contains("Actor") {
            for a in el.select(&a_sel) {
                let name = a.text().collect::<String>().trim().to_string();
                if !name.is_empty() {
                    actors.push(name);
                }
            }
        }
    }

    Some(AdultMetadata {
        video_id: video_id.to_string(),
        title: Some(title.replace(video_id, "").trim().to_string())
            .filter(|s| !s.is_empty())
            .or(Some(title)),
        poster_url,
        cover_url: None,
        source_url: Some(source_url.to_string()),
        actors: if actors.is_empty() { None } else { Some(actors) },
        genres: if genres.is_empty() { None } else { Some(genres) },
        release_date,
        studio,
        duration,
        rating,
        source: "javdb".to_string(),
    })
}

fn parse_search_page_list(html: &str, prefix: &str, base_url: &str) -> Vec<AdultSeriesVideo> {
    let document = scraper::Html::parse_document(html);
    let item_sel = scraper::Selector::parse(".movie-list .item, .grid-item").expect("valid CSS selector");
    let uid_sel = scraper::Selector::parse(".uid, .video-title strong").expect("valid CSS selector");
    let strong_sel = scraper::Selector::parse("strong").expect("valid CSS selector");
    let img_sel = scraper::Selector::parse("img").expect("valid CSS selector");
    let meta_sel = scraper::Selector::parse(".meta, .has-text-grey-dark").expect("valid CSS selector");
    let a_sel = scraper::Selector::parse("a").expect("valid CSS selector");

    let mut videos = Vec::new();
    let date_re = regex::Regex::new(r"\d{4}-\d{2}-\d{2}").expect("valid regex");

    for el in document.select(&item_sel) {
        let link = if el.value().name() == "a" {
            Some(el)
        } else {
            el.select(&a_sel).next()
        };

        let Some(link) = link else { continue };

        let video_id = link
            .select(&uid_sel)
            .next()
            .or_else(|| link.select(&strong_sel).next())
            .map(|u| u.text().collect::<String>().trim().to_uppercase())
            .unwrap_or_default();

        if !video_id.starts_with(prefix) {
            continue;
        }

        let poster_url = link
            .select(&img_sel)
            .next()
            .and_then(|img| img.value().attr("src"))
            .map(|u| resolve_url(u, base_url));

        let title = link
            .select(&scraper::Selector::parse(".video-title").expect("valid CSS selector"))
            .next()
            .map(|t| t.text().collect::<String>().trim().to_string());

        let date_text = link
            .select(&meta_sel)
            .next()
            .map(|m| m.text().collect::<String>())
            .unwrap_or_default();
        let release_date = date_re.find(&date_text).map(|m| m.as_str().to_string());

        videos.push(AdultSeriesVideo {
            video_id,
            title,
            poster_url,
            release_date,
        });
    }

    videos
}

fn has_next_page_link(html: &str) -> bool {
    let document = scraper::Html::parse_document(html);
    let sel = scraper::Selector::parse(".pagination-next:not([disabled]), .pagination a.is-current + a").expect("valid CSS selector");
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
