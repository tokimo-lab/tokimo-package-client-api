mod types;

pub use types::*;

use std::time::Duration;

use crate::cache::RequestCache;
use crate::error::ClientError;

const DEFAULT_CACHE_TTL: Duration = Duration::from_mins(30);

pub struct TmdbConfig {
    pub api_key: String,
    pub language: Option<String>,
    pub base_url: Option<String>,
    pub image_base_url: Option<String>,
    pub cache_ttl: Option<Duration>,
    pub http_client: reqwest::Client,
}

pub struct TmdbClient {
    api_key: String,
    language: String,
    base_url: String,
    image_base_url: String,
    http: reqwest::Client,
    cache: RequestCache,
}

impl TmdbClient {
    pub fn new(config: TmdbConfig) -> Self {
        Self {
            api_key: config.api_key,
            language: config.language.unwrap_or_else(|| "zh-CN".to_string()),
            base_url: config
                .base_url
                .unwrap_or_else(|| "https://api.themoviedb.org/3".to_string()),
            image_base_url: config
                .image_base_url
                .unwrap_or_else(|| "https://image.tmdb.org/t/p".to_string()),
            http: config.http_client,
            cache: RequestCache::new(config.cache_ttl.unwrap_or(DEFAULT_CACHE_TTL)),
        }
    }

    pub async fn clear_cache(&self) {
        self.cache.clear().await;
    }

    pub async fn cache_size(&self) -> usize {
        self.cache.size().await
    }

    async fn request<T: serde::de::DeserializeOwned + serde::Serialize>(
        &self,
        path: &str,
        params: &[(&str, &str)],
    ) -> Result<T, ClientError> {
        let mut url =
            url::Url::parse(&format!("{}{}", self.base_url, path)).map_err(|e| ClientError::Other(e.to_string()))?;

        url.query_pairs_mut()
            .append_pair("api_key", &self.api_key)
            .append_pair("language", &self.language);

        for (key, value) in params {
            url.query_pairs_mut().append_pair(key, value);
        }

        let cache_key = url.to_string();

        if let Some(cached) = self.cache.get::<T>(&cache_key).await {
            return Ok(cached);
        }

        let resp = self.http.get(url).send().await?;

        if !resp.status().is_success() {
            return Err(ClientError::Api {
                status: resp.status().as_u16(),
                message: resp.text().await.unwrap_or_default(),
            });
        }

        let data: T = resp.json().await?;
        self.cache.set(&cache_key, &data).await;
        Ok(data)
    }

    // ---- Search ----

    pub async fn search_multi(&self, query: &str, page: u32) -> Result<Vec<TmdbMedia>, ClientError> {
        let page_str = page.to_string();
        let data: types::TmdbSearchResult = self
            .request(
                "/search/multi",
                &[("query", query), ("page", &page_str), ("include_adult", "false")],
            )
            .await?;

        Ok(data
            .results
            .into_iter()
            .filter(|item| matches!(item.media_type.as_deref(), Some("movie" | "tv")))
            .map(|item| self.transform_media(item))
            .collect())
    }

    pub async fn search_movies(
        &self,
        query: &str,
        year: Option<u32>,
        page: u32,
    ) -> Result<Vec<TmdbMedia>, ClientError> {
        let page_str = page.to_string();
        let year_str = year.map(|y| y.to_string());
        let mut params = vec![("query", query), ("page", &page_str), ("include_adult", "false")];
        if let Some(ref y) = year_str {
            params.push(("year", y));
        }

        let data: types::TmdbSearchResult = self.request("/search/movie", &params).await?;
        Ok(data
            .results
            .into_iter()
            .map(|mut item| {
                item.media_type = Some("movie".to_string());
                self.transform_media(item)
            })
            .collect())
    }

    pub async fn search_tv(&self, query: &str, year: Option<u32>, page: u32) -> Result<Vec<TmdbMedia>, ClientError> {
        let page_str = page.to_string();
        let year_str = year.map(|y| y.to_string());
        let mut params = vec![("query", query), ("page", &page_str), ("include_adult", "false")];
        if let Some(ref y) = year_str {
            params.push(("first_air_date_year", y));
        }

        let data: types::TmdbSearchResult = self.request("/search/tv", &params).await?;
        Ok(data
            .results
            .into_iter()
            .map(|mut item| {
                item.media_type = Some("tv".to_string());
                self.transform_media(item)
            })
            .collect())
    }

    // ---- Genres ----

    pub async fn list_genres(&self, media_type: &str) -> Result<Vec<TmdbGenre>, ClientError> {
        let data: types::TmdbGenreListResponse = self.request(&format!("/genre/{media_type}/list"), &[]).await?;
        Ok(data.genres)
    }

    // ---- Movie detail ----

    pub async fn get_movie_detail(&self, movie_id: i64) -> Result<TmdbMediaDetail, ClientError> {
        let data: types::TmdbMovieDetailRaw = self
            .request(&format!("/movie/{movie_id}"), &[("append_to_response", "credits")])
            .await?;

        let mut base_item = data.base;
        base_item.media_type = Some("movie".to_string());
        let base = self.transform_media(base_item);

        let cast = data.credits.and_then(|c| c.cast).map(|mut members| {
            members.sort_by_key(|m| m.order.unwrap_or(999));
            members
                .into_iter()
                .map(|c| TmdbCastInfo {
                    name: c.name,
                    role: c.character,
                    tmdb_id: c.id,
                    thumb: c.profile_path.map(|p| format!("{}/w185{p}", self.image_base_url)),
                })
                .collect()
        });

        Ok(TmdbMediaDetail {
            base,
            imdb_id: data.imdb_id,
            runtime: data.runtime,
            status: data.status,
            tagline: data.tagline,
            budget: data.budget,
            revenue: data.revenue,
            homepage: data.homepage,
            number_of_seasons: None,
            number_of_episodes: None,
            genres: data.genres,
            origin_country: data
                .production_countries
                .map(|cs| cs.into_iter().map(|c| c.iso_3166_1).collect()),
            production_companies: data.production_companies.map(|cs| {
                cs.into_iter()
                    .map(|c| TmdbCompany {
                        id: c.id,
                        name: c.name,
                        logo_path: c.logo_path.map(|p| format!("{}/w200{p}", self.image_base_url)),
                    })
                    .collect()
            }),
            cast,
        })
    }

    // ---- TV detail ----

    pub async fn get_tv_detail(&self, tv_id: i64) -> Result<TmdbMediaDetail, ClientError> {
        let data: types::TmdbTvDetailRaw = self
            .request(
                &format!("/tv/{tv_id}"),
                &[("append_to_response", "external_ids,credits")],
            )
            .await?;

        let mut base_item = data.base;
        base_item.media_type = Some("tv".to_string());
        let base = self.transform_media(base_item);

        let cast = data.credits.and_then(|c| c.cast).map(|mut members| {
            members.sort_by_key(|m| m.order.unwrap_or(999));
            members
                .into_iter()
                .map(|c| TmdbCastInfo {
                    name: c.name,
                    role: c.character,
                    tmdb_id: c.id,
                    thumb: c.profile_path.map(|p| format!("{}/w185{p}", self.image_base_url)),
                })
                .collect()
        });

        Ok(TmdbMediaDetail {
            base,
            imdb_id: data.external_ids.and_then(|e| e.imdb_id),
            runtime: None,
            status: data.status,
            tagline: data.tagline,
            budget: None,
            revenue: None,
            homepage: data.homepage,
            number_of_seasons: data.number_of_seasons,
            number_of_episodes: data.number_of_episodes,
            genres: data.genres,
            origin_country: data.origin_country,
            production_companies: data.production_companies.map(|cs| {
                cs.into_iter()
                    .map(|c| TmdbCompany {
                        id: c.id,
                        name: c.name,
                        logo_path: c.logo_path.map(|p| format!("{}/w200{p}", self.image_base_url)),
                    })
                    .collect()
            }),
            cast,
        })
    }

    // ---- Find by IMDb ID ----

    pub async fn find_by_imdb_id(&self, imdb_id: &str) -> Result<Option<TmdbMedia>, ClientError> {
        let data: types::TmdbFindResult = self
            .request(&format!("/find/{imdb_id}"), &[("external_source", "imdb_id")])
            .await?;

        if let Some(mut movie) = data.movie_results.into_iter().next() {
            movie.media_type = Some("movie".to_string());
            return Ok(Some(self.transform_media(movie)));
        }
        if let Some(mut tv) = data.tv_results.into_iter().next() {
            tv.media_type = Some("tv".to_string());
            return Ok(Some(self.transform_media(tv)));
        }

        // Season-level IMDb ID → resolve to parent show
        if let Some(season) = data.tv_season_results.into_iter().next()
            && let Some(show_id) = season.show_id
            && let Ok(tv_data) = self
                .request::<types::TmdbMediaItem>(&format!("/tv/{show_id}"), &[])
                .await
        {
            let mut item = tv_data;
            item.media_type = Some("tv".to_string());
            return Ok(Some(self.transform_media(item)));
        }

        // Episode-level IMDb ID → resolve to parent show
        if let Some(episode) = data.tv_episode_results.into_iter().next()
            && let Some(show_id) = episode.show_id
            && let Ok(tv_data) = self
                .request::<types::TmdbMediaItem>(&format!("/tv/{show_id}"), &[])
                .await
        {
            let mut item = tv_data;
            item.media_type = Some("tv".to_string());
            return Ok(Some(self.transform_media(item)));
        }

        Ok(None)
    }

    /// Resolve an `IMDb` ID to its canonical parent-level `IMDb` ID, TMDB ID, and media type.
    pub async fn resolve_imdb_id(&self, imdb_id: &str) -> Result<Option<ResolvedImdbId>, ClientError> {
        let data: types::TmdbFindResult = self
            .request(&format!("/find/{imdb_id}"), &[("external_source", "imdb_id")])
            .await?;

        if let Some(movie) = data.movie_results.first() {
            let movie_id = movie.id;
            let detail: Result<types::TmdbMovieDetailRaw, _> = self.request(&format!("/movie/{movie_id}"), &[]).await;
            let resolved_imdb = detail
                .ok()
                .and_then(|d| d.imdb_id)
                .unwrap_or_else(|| imdb_id.to_string());
            return Ok(Some(ResolvedImdbId {
                imdb_id: resolved_imdb,
                tmdb_id: movie_id,
                media_type: "movie".to_string(),
            }));
        }

        if let Some(tv) = data.tv_results.first() {
            return Ok(Some(self.resolve_tv_imdb(tv.id, imdb_id).await));
        }

        if let Some(season) = data.tv_season_results.first()
            && let Some(show_id) = season.show_id
        {
            return Ok(Some(self.resolve_tv_imdb(show_id, imdb_id).await));
        }

        if let Some(episode) = data.tv_episode_results.first()
            && let Some(show_id) = episode.show_id
        {
            return Ok(Some(self.resolve_tv_imdb(show_id, imdb_id).await));
        }

        Ok(None)
    }

    async fn resolve_tv_imdb(&self, tv_id: i64, fallback_imdb: &str) -> ResolvedImdbId {
        let detail: Result<types::TmdbTvDetailRaw, _> = self
            .request(&format!("/tv/{tv_id}"), &[("append_to_response", "external_ids")])
            .await;
        let resolved_imdb = detail
            .ok()
            .and_then(|d| d.external_ids)
            .and_then(|e| e.imdb_id)
            .unwrap_or_else(|| fallback_imdb.to_string());
        ResolvedImdbId {
            imdb_id: resolved_imdb,
            tmdb_id: tv_id,
            media_type: "tv".to_string(),
        }
    }

    // ---- Popular ----

    pub async fn get_popular_movies(&self, page: u32) -> Result<Vec<TmdbMedia>, ClientError> {
        let page_str = page.to_string();
        let data: types::TmdbSearchResult = self.request("/movie/popular", &[("page", &page_str)]).await?;
        Ok(data
            .results
            .into_iter()
            .map(|mut item| {
                item.media_type = Some("movie".to_string());
                self.transform_media(item)
            })
            .collect())
    }

    pub async fn get_popular_tv(&self, page: u32) -> Result<Vec<TmdbMedia>, ClientError> {
        let page_str = page.to_string();
        let data: types::TmdbSearchResult = self.request("/tv/popular", &[("page", &page_str)]).await?;
        Ok(data
            .results
            .into_iter()
            .map(|mut item| {
                item.media_type = Some("tv".to_string());
                self.transform_media(item)
            })
            .collect())
    }

    // ---- Season detail ----

    pub async fn get_tv_season_detail(&self, tv_id: i64, season_number: i32) -> Result<TmdbSeasonDetail, ClientError> {
        self.request(
            &format!("/tv/{tv_id}/season/{season_number}"),
            &[("append_to_response", "credits")],
        )
        .await
    }

    // ---- Person ----

    pub async fn get_person_detail(&self, person_id: i64) -> Result<TmdbPersonDetail, ClientError> {
        self.request(
            &format!("/person/{person_id}"),
            &[("append_to_response", "combined_credits,external_ids")],
        )
        .await
    }

    pub async fn get_movie_cast(&self, movie_id: i64) -> Result<Vec<SimpleCastMember>, ClientError> {
        let data: types::TmdbCredits = self.request(&format!("/movie/{movie_id}/credits"), &[]).await?;
        Ok(data
            .cast
            .unwrap_or_default()
            .into_iter()
            .map(|c| SimpleCastMember {
                id: c.id,
                name: c.name,
                original_name: c.original_name,
            })
            .collect())
    }

    pub async fn get_tv_cast(&self, tv_id: i64) -> Result<Vec<SimpleCastMember>, ClientError> {
        let data: types::TmdbCredits = self.request(&format!("/tv/{tv_id}/credits"), &[]).await?;
        Ok(data
            .cast
            .unwrap_or_default()
            .into_iter()
            .map(|c| SimpleCastMember {
                id: c.id,
                name: c.name,
                original_name: c.original_name,
            })
            .collect())
    }

    pub async fn search_person(&self, query: &str) -> Result<Vec<TmdbPersonSearchItem>, ClientError> {
        let data: types::TmdbPersonSearchResponse = self.request("/search/person", &[("query", query)]).await?;
        Ok(data.results)
    }

    // ---- Images ----

    pub async fn get_movie_images(&self, movie_id: i64) -> Result<TmdbImagesResponse, ClientError> {
        self.request(
            &format!("/movie/{movie_id}/images"),
            &[("include_image_language", "zh,en,null")],
        )
        .await
    }

    pub async fn get_tv_images(&self, tv_id: i64) -> Result<TmdbImagesResponse, ClientError> {
        self.request(
            &format!("/tv/{tv_id}/images"),
            &[("include_image_language", "zh,en,null")],
        )
        .await
    }

    // ---- Image URL builders ----

    pub fn get_poster_url(&self, poster_path: Option<&str>, size: &str) -> Option<String> {
        poster_path.map(|p| format!("{}/{size}{p}", self.image_base_url))
    }

    pub fn get_backdrop_url(&self, backdrop_path: Option<&str>, size: &str) -> Option<String> {
        backdrop_path.map(|p| format!("{}/{size}{p}", self.image_base_url))
    }

    // ---- Transform ----

    #[allow(clippy::unused_self)]
    fn transform_media(&self, item: types::TmdbMediaItem) -> TmdbMedia {
        let is_movie = item.media_type.as_deref() == Some("movie");
        TmdbMedia {
            id: item.id,
            media_type: if is_movie { "movie" } else { "tv" }.to_string(),
            title: if is_movie {
                item.title.unwrap_or_default()
            } else {
                item.name.unwrap_or_default()
            },
            original_title: if is_movie {
                item.original_title
            } else {
                item.original_name
            },
            overview: item.overview,
            poster_path: item.poster_path,
            backdrop_path: item.backdrop_path,
            release_date: if is_movie {
                item.release_date
            } else {
                item.first_air_date
            },
            vote_average: item.vote_average,
            vote_count: item.vote_count,
            popularity: item.popularity,
            original_language: item.original_language,
            genre_ids: item.genre_ids,
        }
    }
}
