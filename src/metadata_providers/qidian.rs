//! Qidian (起点中文网) metadata client.
//!
//! Scrapes book metadata from qidian.com, the largest Chinese web novel platform.
//! Uses HTML scraping on the search and book detail pages; may break if page structure changes.
//!
//! - Search: `https://www.qidian.com/so/{keyword}.html`
//! - Detail: `https://book.qidian.com/info/{bookId}/`

use std::time::Duration;

use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::cache::RequestCache;
use crate::error::ClientError;

const DEFAULT_CACHE_TTL: Duration = Duration::from_hours(1);
const SEARCH_BASE: &str = "https://www.qidian.com/so";
const DETAIL_BASE: &str = "https://book.qidian.com/info";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

pub struct QidianConfig {
    pub http_client: reqwest::Client,
    pub cache_ttl: Option<Duration>,
}

pub struct QidianClient {
    http: reqwest::Client,
    cache: RequestCache,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QidianSearchItem {
    pub qidian_id: String,
    pub title: String,
    pub author: Option<String>,
    pub cover_url: Option<String>,
    pub category: Option<String>,
    pub intro: Option<String>,
    pub word_count: Option<String>,
    pub serial_status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QidianBookDetail {
    pub qidian_id: String,
    pub title: String,
    pub author: Option<String>,
    pub cover_url: Option<String>,
    pub intro: Option<String>,
    pub word_count: Option<String>,
    pub serial_status: Option<String>,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub last_chapter: Option<String>,
}

impl QidianClient {
    pub fn new(config: QidianConfig) -> Self {
        Self {
            http: config.http_client,
            cache: RequestCache::new(config.cache_ttl.unwrap_or(DEFAULT_CACHE_TTL)),
        }
    }

    /// Search books by title.
    pub async fn search_books(&self, query: &str) -> Result<Vec<QidianSearchItem>, ClientError> {
        let cache_key = format!("qidian:search:{query}");
        if let Some(cached) = self.cache.get::<Vec<QidianSearchItem>>(&cache_key).await {
            return Ok(cached);
        }

        let items = self.scrape_search(query).await.unwrap_or_else(|e| {
            warn!("Qidian search failed: {e}");
            vec![]
        });
        self.cache.set(&cache_key, &items).await;
        Ok(items)
    }

    /// Get book detail by Qidian book ID.
    pub async fn get_book_detail(
        &self,
        qidian_id: &str,
    ) -> Result<Option<QidianBookDetail>, ClientError> {
        let cache_key = format!("qidian:detail:{qidian_id}");
        if let Some(cached) = self.cache.get::<Option<QidianBookDetail>>(&cache_key).await {
            return Ok(cached);
        }

        let detail = self.scrape_detail(qidian_id).await.unwrap_or_else(|e| {
            warn!("Qidian detail failed for {qidian_id}: {e}");
            None
        });
        self.cache.set(&cache_key, &detail).await;
        Ok(detail)
    }

    // ---- Internal scraping ----

    async fn fetch_html(&self, url: &str) -> Result<String, ClientError> {
        let resp = self
            .http
            .get(url)
            .header("User-Agent", USER_AGENT)
            .header(
                "Accept",
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            )
            .header("Accept-Language", "zh-CN,zh;q=0.9,en;q=0.8")
            .header("Referer", "https://www.qidian.com/")
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(ClientError::Api {
                status: resp.status().as_u16(),
                message: format!("Qidian request failed: {url}"),
            });
        }

        Ok(resp.text().await?)
    }

    async fn scrape_search(&self, query: &str) -> Result<Vec<QidianSearchItem>, ClientError> {
        let encoded = urlencoding::encode(query);
        let url = format!("{SEARCH_BASE}/{encoded}.html");
        let html = self.fetch_html(&url).await?;
        Ok(parse_search_html(&html))
    }

    async fn scrape_detail(
        &self,
        qidian_id: &str,
    ) -> Result<Option<QidianBookDetail>, ClientError> {
        let url = format!("{DETAIL_BASE}/{qidian_id}/");
        let html = self.fetch_html(&url).await?;
        Ok(parse_detail_html(&html, qidian_id))
    }
}

// ---- HTML parsers ----

/// Parse search results from `www.qidian.com/so/{keyword}.html`.
fn parse_search_html(html: &str) -> Vec<QidianSearchItem> {
    let doc = Html::parse_document(html);
    let mut items = Vec::new();

    // Try multiple selector strategies for robustness
    let sel_item = sel(".res-book-item");
    let sel_bid = sel("[data-bid]");
    let sel_title = sel("h2 a, .book-mid-info h2 a, h3 a");
    let sel_author = sel(".author .name, p.author a:first-child");
    let sel_cover = sel(".book-img-box img, .book-img img");
    let sel_intro = sel("p.intro, .intro");
    let sel_update = sel("p.update span, .book-state span");
    let sel_cat = sel(".author a:nth-child(2), p.author em + a");

    // Strategy 1: .res-book-item elements
    let result_elements: Vec<_> = doc.select(&sel_item).collect();
    let elements = if result_elements.is_empty() {
        // Strategy 2: [data-bid] elements
        doc.select(&sel_bid).collect()
    } else {
        result_elements
    };

    for el in elements {
        let bid = el
            .value()
            .attr("data-bid")
            .map(std::string::ToString::to_string)
            .or_else(|| {
                // Try extracting from link href: /book/{id}/
                el.select(&sel_title)
                    .next()
                    .and_then(|a| a.value().attr("href"))
                    .and_then(extract_book_id_from_href)
            });

        let bid = match bid {
            Some(id) if !id.is_empty() => id,
            _ => continue,
        };

        let title = el
            .select(&sel_title)
            .next()
            .map(|e| text_content(&e))
            .unwrap_or_default();
        if title.is_empty() {
            continue;
        }

        let author = el.select(&sel_author).next().map(|e| text_content(&e));
        let cover_url = el
            .select(&sel_cover)
            .next()
            .and_then(|e| e.value().attr("src").map(normalize_url));
        let intro = el.select(&sel_intro).next().map(|e| text_content(&e));

        let mut serial_status = None;
        let mut word_count = None;
        for span in el.select(&sel_update) {
            let t = text_content(&span);
            if t.contains('字') || t.contains("万") {
                word_count = Some(t);
            } else if t.contains("连载") || t.contains("完本") || t.contains("完结") {
                serial_status = Some(t);
            }
        }

        let category = el.select(&sel_cat).next().map(|e| text_content(&e));

        items.push(QidianSearchItem {
            qidian_id: bid,
            title,
            author,
            cover_url,
            category,
            intro,
            word_count,
            serial_status,
        });
    }

    items
}

/// Parse book detail from `book.qidian.com/info/{bookId}/`.
fn parse_detail_html(html: &str, qidian_id: &str) -> Option<QidianBookDetail> {
    let doc = Html::parse_document(html);

    // Title — from <h1>, og:title, or <title>
    let title = doc
        .select(&sel("h1 em, h1 span, .book-info h1"))
        .next()
        .map(|e| text_content(&e))
        .or_else(|| meta_content(&doc, "og:title"))
        .or_else(|| {
            doc.select(&sel("title"))
                .next()
                .map(|e| text_content(&e))
                .map(|t| t.split('_').next().unwrap_or("").trim().to_string())
        })
        .filter(|s| !s.is_empty())?;

    // Author
    let author = doc
        .select(&sel(
            ".writer-name, .book-info .writer a, a.writer-name, .book-information .writer a",
        ))
        .next()
        .map(|e| text_content(&e))
        .or_else(|| {
            // Fallback: search meta keywords "书名,作者,..."
            meta_content(&doc, "keywords").and_then(|kw| {
                let parts: Vec<&str> = kw.split(',').collect();
                parts.get(1).map(|s| s.trim().to_string())
            })
        });

    // Cover
    let cover_url = doc
        .select(&sel("#bookImg img, .book-img img, .book-information img"))
        .next()
        .and_then(|e| e.value().attr("src").map(normalize_url))
        .or_else(|| meta_content(&doc, "og:image").map(|s| normalize_url(&s)));

    // Intro / description
    let intro = doc
        .select(&sel(
            ".book-intro p, .book-intro .desc-content, #intro .desc-content, .book-info-detail .book-desc p",
        ))
        .next()
        .map(|e| text_content(&e))
        .or_else(|| meta_content(&doc, "og:description"))
        .or_else(|| meta_content(&doc, "description"));

    // Status and word count from the state/info section
    let mut serial_status = None;
    let mut word_count = None;
    for el in doc.select(&sel(
        ".book-state span, .book-info .tag span, .count, p.tag span",
    )) {
        let t = text_content(&el);
        if t.contains('字') || t.contains("万") {
            word_count = Some(t);
        } else if t.contains("连载") || t.contains("完本") || t.contains("完结") {
            serial_status = Some(t);
        }
    }

    // Category
    let category = doc
        .select(&sel(
            ".book-state a, p.tag a:first-child, .book-info .tag a",
        ))
        .next()
        .map(|e| text_content(&e));

    // Tags
    let tags: Vec<String> = doc
        .select(&sel("p.tag a, .book-info .tag a, .book-state a"))
        .map(|e| text_content(&e))
        .filter(|s| !s.is_empty())
        .collect();

    // Last chapter
    let last_chapter = doc
        .select(&sel(
            ".update a, .book-latest-chapter a, .detail-latest-chapter a",
        ))
        .next()
        .map(|e| text_content(&e));

    Some(QidianBookDetail {
        qidian_id: qidian_id.to_string(),
        title,
        author,
        cover_url,
        intro,
        word_count,
        serial_status,
        category,
        tags,
        last_chapter,
    })
}

// ---- Helpers ----

/// Shorthand for creating a CSS selector, panicking on invalid selector.
fn sel(s: &str) -> Selector {
    Selector::parse(s).expect("invalid CSS selector")
}

/// Collect all text from an element, trimmed.
fn text_content(el: &scraper::ElementRef) -> String {
    el.text().collect::<String>().trim().to_string()
}

/// Get content of a `<meta property="X">` or `<meta name="X">` tag.
fn meta_content(doc: &Html, name: &str) -> Option<String> {
    let sel_prop = Selector::parse(&format!("meta[property=\"{name}\"]")).ok()?;
    let sel_name = Selector::parse(&format!("meta[name=\"{name}\"]")).ok()?;

    doc.select(&sel_prop)
        .chain(doc.select(&sel_name))
        .next()
        .and_then(|e| e.value().attr("content"))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Normalize a URL (add https: if protocol-relative).
fn normalize_url(url: &str) -> String {
    if url.starts_with("//") {
        format!("https:{url}")
    } else {
        url.to_string()
    }
}

/// Extract book ID from href like `//www.qidian.com/book/1115277/`.
fn extract_book_id_from_href(href: &str) -> Option<String> {
    let re = regex::Regex::new(r"/book/(\d+)").ok()?;
    re.captures(href)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
}

mod urlencoding {
    pub fn encode(s: &str) -> String {
        url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
    }
}
