/// Fetch artist biography from Wikipedia.
///
/// Given a Wikipedia URL (any language), tries to return a Chinese (zh) extract
/// by first looking up the zh-wiki sitelink via the English article's inter-language links,
/// then calling the zh.wikipedia REST summary endpoint.
pub async fn fetch_artist_biography(http: &reqwest::Client, wikipedia_url: &str) -> Option<String> {
    let (lang, title) = parse_wikipedia_url(wikipedia_url)?;

    if lang == "zh" {
        return fetch_zh_summary(http, &title).await;
    }

    // For non-zh URLs, look up the zh sitelink via MediaWiki action API
    let zh_title = get_zh_title(http, &lang, &title).await?;
    fetch_zh_summary(http, &zh_title).await
}

/// Parse "<https://{lang}.wikipedia.org/wiki/{title>}" → (lang, title).
fn parse_wikipedia_url(url: &str) -> Option<(String, String)> {
    let url = url.trim_end_matches('/');
    // Expected format: https://<lang>.wikipedia.org/wiki/<title>
    let after_https = url.strip_prefix("https://")?;
    let dot_pos = after_https.find('.')?;
    let lang = after_https[..dot_pos].to_string();
    let wiki_pos = after_https.find("/wiki/")?;
    let title = after_https[wiki_pos + 6..].to_string();
    if lang.is_empty() || title.is_empty() {
        return None;
    }
    Some((lang, title))
}

/// Look up the Chinese Wikipedia title for a given article via the inter-language link API.
async fn get_zh_title(http: &reqwest::Client, lang: &str, title: &str) -> Option<String> {
    let api_url = format!("https://{lang}.wikipedia.org/w/api.php");
    let resp = http
        .get(&api_url)
        .query(&[
            ("action", "query"),
            ("prop", "langlinks"),
            ("titles", title),
            ("lllang", "zh"),
            ("llprop", "url"),
            ("format", "json"),
            ("redirects", "1"),
        ])
        .send()
        .await
        .ok()?;

    if !resp.status().is_success() {
        return None;
    }

    let data: serde_json::Value = resp.json().await.ok()?;
    // Navigate: query → pages → first page → langlinks → [0] → *
    let pages = data["query"]["pages"].as_object()?;
    let page = pages.values().next()?;
    let link = page["langlinks"].as_array()?.first()?;
    link["*"].as_str().map(String::from)
}

/// Fetch Chinese Wikipedia REST summary for an article title.
async fn fetch_zh_summary(http: &reqwest::Client, title: &str) -> Option<String> {
    let base = reqwest::Url::parse("https://zh.wikipedia.org/api/rest_v1/page/summary/").ok()?;
    let url = base.join(&title.replace(' ', "_")).ok()?;
    let resp = http.get(url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let data: serde_json::Value = resp.json().await.ok()?;
    data["extract"].as_str().map(String::from)
}
