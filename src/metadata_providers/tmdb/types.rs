use serde::{Deserialize, Serialize};

// ---- Search response ----

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct TmdbSearchResult {
    pub results: Vec<TmdbMediaItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct TmdbMediaItem {
    pub id: i64,
    pub media_type: Option<String>,
    pub title: Option<String>,
    pub name: Option<String>,
    pub original_title: Option<String>,
    pub original_name: Option<String>,
    pub overview: Option<String>,
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
    pub release_date: Option<String>,
    pub first_air_date: Option<String>,
    pub vote_average: Option<f64>,
    pub vote_count: Option<i64>,
    pub popularity: Option<f64>,
    pub original_language: Option<String>,
    pub genre_ids: Option<Vec<i64>>,
}

// ---- Cast ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct TmdbCastMember {
    pub id: i64,
    pub name: String,
    pub original_name: Option<String>,
    pub character: Option<String>,
    pub profile_path: Option<String>,
    pub order: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct TmdbCredits {
    pub cast: Option<Vec<TmdbCastMember>>,
}

// ---- Movie detail ----

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct TmdbMovieDetailRaw {
    #[serde(flatten)]
    pub base: TmdbMediaItem,
    pub imdb_id: Option<String>,
    pub runtime: Option<i32>,
    pub status: Option<String>,
    pub tagline: Option<String>,
    pub budget: Option<i64>,
    pub revenue: Option<i64>,
    pub homepage: Option<String>,
    pub genres: Option<Vec<TmdbGenre>>,
    pub production_countries: Option<Vec<TmdbCountry>>,
    pub production_companies: Option<Vec<TmdbCompanyRaw>>,
    pub credits: Option<TmdbCredits>,
}

// ---- TV detail ----

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct TmdbTvDetailRaw {
    #[serde(flatten)]
    pub base: TmdbMediaItem,
    pub number_of_seasons: Option<i32>,
    pub number_of_episodes: Option<i32>,
    pub status: Option<String>,
    pub tagline: Option<String>,
    pub homepage: Option<String>,
    pub origin_country: Option<Vec<String>>,
    pub genres: Option<Vec<TmdbGenre>>,
    pub production_companies: Option<Vec<TmdbCompanyRaw>>,
    pub external_ids: Option<TmdbExternalIds>,
    pub credits: Option<TmdbCredits>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct TmdbExternalIds {
    pub imdb_id: Option<String>,
}

// ---- Common sub-types ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TmdbGenre {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct TmdbCountry {
    pub iso_3166_1: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct TmdbCompanyRaw {
    pub id: i64,
    pub name: String,
    pub logo_path: Option<String>,
}

// ---- Genre list ----

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct TmdbGenreListResponse {
    pub genres: Vec<TmdbGenre>,
}

// ---- Season detail ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TmdbSeasonDetail {
    pub id: i64,
    pub name: String,
    pub overview: Option<String>,
    pub poster_path: Option<String>,
    pub season_number: i32,
    pub credits: Option<TmdbCreditsPublic>,
    pub episodes: Option<Vec<TmdbEpisode>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TmdbCreditsPublic {
    pub cast: Option<Vec<TmdbCastPublic>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TmdbCastPublic {
    pub id: i64,
    pub name: String,
    pub original_name: Option<String>,
    pub character: Option<String>,
    pub profile_path: Option<String>,
    pub order: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TmdbEpisode {
    pub id: i64,
    pub name: String,
    pub overview: Option<String>,
    pub episode_number: i32,
    pub season_number: i32,
    pub still_path: Option<String>,
    pub air_date: Option<String>,
    pub vote_average: Option<f64>,
}

// ---- Find ----

#[derive(Debug, Serialize, Deserialize)]
#[allow(clippy::struct_field_names)]
pub(crate) struct TmdbFindResult {
    pub movie_results: Vec<TmdbMediaItem>,
    pub tv_results: Vec<TmdbMediaItem>,
    pub tv_season_results: Vec<TmdbFindSeasonResult>,
    pub tv_episode_results: Vec<TmdbFindEpisodeResult>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct TmdbFindSeasonResult {
    pub show_id: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct TmdbFindEpisodeResult {
    pub show_id: Option<i64>,
}

// ---- Images ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TmdbImagesResponse {
    pub id: i64,
    pub backdrops: Vec<TmdbImage>,
    pub posters: Vec<TmdbImage>,
    pub logos: Vec<TmdbImage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TmdbImage {
    pub file_path: String,
    pub width: i32,
    pub height: i32,
}

// ---- Person ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TmdbPersonDetail {
    pub id: i64,
    pub name: String,
    pub also_known_as: Option<Vec<String>>,
    pub biography: Option<String>,
    pub birthday: Option<String>,
    pub deathday: Option<String>,
    pub gender: Option<i32>,
    pub place_of_birth: Option<String>,
    pub profile_path: Option<String>,
    pub popularity: Option<f64>,
    pub known_for_department: Option<String>,
    pub external_ids: Option<TmdbPersonExternalIds>,
    pub combined_credits: Option<TmdbCombinedCredits>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TmdbPersonExternalIds {
    pub imdb_id: Option<String>,
    pub facebook_id: Option<String>,
    pub instagram_id: Option<String>,
    pub twitter_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TmdbCombinedCredits {
    pub cast: Option<Vec<TmdbCombinedCreditItem>>,
    pub crew: Option<Vec<TmdbCombinedCrewItem>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TmdbCombinedCreditItem {
    pub id: i64,
    pub title: Option<String>,
    pub name: Option<String>,
    pub media_type: String,
    pub poster_path: Option<String>,
    pub release_date: Option<String>,
    pub first_air_date: Option<String>,
    pub character: Option<String>,
    pub vote_average: Option<f64>,
    pub popularity: Option<f64>,
    pub order: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TmdbCombinedCrewItem {
    pub id: i64,
    pub title: Option<String>,
    pub name: Option<String>,
    pub media_type: String,
    pub poster_path: Option<String>,
    pub release_date: Option<String>,
    pub first_air_date: Option<String>,
    pub job: Option<String>,
    pub department: Option<String>,
    pub popularity: Option<f64>,
}

// ---- Person search ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TmdbPersonSearchItem {
    pub id: i64,
    pub name: String,
    pub profile_path: Option<String>,
    pub popularity: Option<f64>,
    pub known_for_department: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct TmdbPersonSearchResponse {
    pub results: Vec<TmdbPersonSearchItem>,
}

// ---- Public output types ----

/// Unified TMDB media item (movie or TV).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TmdbMedia {
    pub id: i64,
    pub media_type: String,
    pub title: String,
    pub original_title: Option<String>,
    pub overview: Option<String>,
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
    pub release_date: Option<String>,
    pub vote_average: Option<f64>,
    pub vote_count: Option<i64>,
    pub popularity: Option<f64>,
    pub original_language: Option<String>,
    pub genre_ids: Option<Vec<i64>>,
}

/// Detailed TMDB media info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TmdbMediaDetail {
    #[serde(flatten)]
    pub base: TmdbMedia,
    pub imdb_id: Option<String>,
    pub runtime: Option<i32>,
    pub status: Option<String>,
    pub tagline: Option<String>,
    pub budget: Option<i64>,
    pub revenue: Option<i64>,
    pub homepage: Option<String>,
    pub number_of_seasons: Option<i32>,
    pub number_of_episodes: Option<i32>,
    pub genres: Option<Vec<TmdbGenre>>,
    pub origin_country: Option<Vec<String>>,
    pub production_companies: Option<Vec<TmdbCompany>>,
    pub cast: Option<Vec<TmdbCastInfo>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TmdbCompany {
    pub id: i64,
    pub name: String,
    pub logo_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TmdbCastInfo {
    pub name: String,
    pub role: Option<String>,
    pub tmdb_id: i64,
    pub thumb: Option<String>,
}

/// Resolved IMDB ID result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedImdbId {
    pub imdb_id: String,
    pub tmdb_id: i64,
    pub media_type: String,
}

/// Simple cast member for credits endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimpleCastMember {
    pub id: i64,
    pub name: String,
    pub original_name: Option<String>,
}
