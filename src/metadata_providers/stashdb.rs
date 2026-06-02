use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::cache::RequestCache;
use crate::error::ClientError;
use crate::types::AdultMetadata;

const CACHE_TTL: Duration = Duration::from_hours(24);

const SCENE_SEARCH_QUERY: &str = r"
    query SearchScenes($term: String!) {
        queryScenes(input: { text: $term, per_page: 5 }) {
            count
            scenes {
                id
                title
                date
                duration
                code
                images { url }
                studio { name }
                performers { performer { name } }
                tags { name }
            }
        }
    }
";

pub struct StashDBConfig {
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub cache_ttl: Option<Duration>,
    pub http_client: reqwest::Client,
}

#[derive(Deserialize)]
struct GraphQLResponse {
    data: Option<GraphQLData>,
}

#[derive(Deserialize)]
struct GraphQLData {
    #[serde(rename = "queryScenes")]
    query_scenes: Option<QueryScenes>,
}

#[derive(Deserialize)]
struct QueryScenes {
    scenes: Option<Vec<StashDBScene>>,
}

#[derive(Deserialize)]
struct StashDBScene {
    id: String,
    title: Option<String>,
    date: Option<String>,
    duration: Option<i64>,
    code: Option<String>,
    images: Option<Vec<SceneImage>>,
    studio: Option<SceneStudio>,
    performers: Option<Vec<ScenePerformer>>,
    tags: Option<Vec<SceneTag>>,
}

#[derive(Deserialize)]
struct SceneImage {
    url: Option<String>,
}

#[derive(Deserialize)]
struct SceneStudio {
    name: Option<String>,
}

#[derive(Deserialize)]
struct ScenePerformer {
    performer: Option<PerformerInner>,
}

#[derive(Deserialize)]
struct PerformerInner {
    name: Option<String>,
}

#[derive(Deserialize)]
struct SceneTag {
    name: Option<String>,
}

pub struct StashDBClient {
    api_key: Option<String>,
    base_url: String,
    http: reqwest::Client,
    cache: RequestCache,
}

impl StashDBClient {
    pub fn new(config: StashDBConfig) -> Self {
        Self {
            api_key: config.api_key,
            base_url: config
                .base_url
                .unwrap_or_else(|| "https://stashdb.org/graphql".to_string())
                .trim_end_matches('/')
                .to_string(),
            http: config.http_client,
            cache: RequestCache::new(config.cache_ttl.unwrap_or(CACHE_TTL)),
        }
    }

    pub async fn clear_cache(&self) {
        self.cache.clear().await;
    }

    pub async fn search_by_video_id(
        &self,
        video_id: &str,
    ) -> Result<Option<AdultMetadata>, ClientError> {
        let cache_key = format!("stashdb:{video_id}");
        if let Some(cached) = self.cache.get::<Option<AdultMetadata>>(&cache_key).await {
            return Ok(cached);
        }

        let result = self.fetch_by_video_id(video_id).await;
        let metadata = result.unwrap_or(None);
        self.cache.set(&cache_key, &metadata).await;
        Ok(metadata)
    }

    async fn fetch_by_video_id(
        &self,
        video_id: &str,
    ) -> Result<Option<AdultMetadata>, ClientError> {
        #[derive(Serialize)]
        struct GqlRequest {
            query: &'static str,
            variables: GqlVars,
        }

        #[derive(Serialize)]
        struct GqlVars {
            term: String,
        }

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("Content-Type", "application/json".parse().unwrap());
        headers.insert("Accept", "application/json".parse().unwrap());
        if let Some(ref key) = self.api_key {
            headers.insert("ApiKey", key.parse().unwrap());
        }

        let resp = self
            .http
            .post(&self.base_url)
            .headers(headers)
            .json(&GqlRequest {
                query: SCENE_SEARCH_QUERY,
                variables: GqlVars {
                    term: video_id.to_string(),
                },
            })
            .timeout(Duration::from_secs(10))
            .send()
            .await?;

        if !resp.status().is_success() {
            return Ok(None);
        }

        let gql_resp: GraphQLResponse = resp.json().await?;
        let scenes = gql_resp
            .data
            .and_then(|d| d.query_scenes)
            .and_then(|qs| qs.scenes)
            .unwrap_or_default();

        if scenes.is_empty() {
            return Ok(None);
        }

        let scene = find_best_match(&scenes, video_id);
        Ok(scene.map(|s| transform_scene(s, video_id, &self.base_url)))
    }
}

fn find_best_match<'a>(scenes: &'a [StashDBScene], video_id: &str) -> Option<&'a StashDBScene> {
    let normalized = video_id.to_uppercase().replace(['-', '_', ' '], "");

    // Match on code field first
    for scene in scenes {
        if let Some(ref code) = scene.code
            && code.to_uppercase().replace(['-', '_', ' '], "") == normalized
        {
            return Some(scene);
        }
    }

    // Title contains video ID
    for scene in scenes {
        if let Some(ref title) = scene.title
            && title
                .to_uppercase()
                .replace(['-', '_', ' '], "")
                .contains(&normalized)
        {
            return Some(scene);
        }
    }

    scenes.first()
}

fn transform_scene(scene: &StashDBScene, video_id: &str, base_url: &str) -> AdultMetadata {
    let web_base = base_url.replace("/graphql", "");
    AdultMetadata {
        video_id: video_id.to_string(),
        title: scene.title.clone(),
        poster_url: scene
            .images
            .as_ref()
            .and_then(|imgs| imgs.first())
            .and_then(|img| img.url.clone()),
        cover_url: None,
        source_url: Some(format!("{web_base}/scenes/{}", scene.id)),
        actors: scene.performers.as_ref().map(|ps| {
            ps.iter()
                .filter_map(|p| p.performer.as_ref().and_then(|pp| pp.name.clone()))
                .collect()
        }),
        genres: scene
            .tags
            .as_ref()
            .map(|ts| ts.iter().filter_map(|t| t.name.clone()).collect()),
        release_date: scene.date.clone(),
        studio: scene.studio.as_ref().and_then(|s| s.name.clone()),
        duration: scene.duration.map(|d| (d / 60) as u32),
        rating: None,
        source: "stashdb".to_string(),
    }
}
