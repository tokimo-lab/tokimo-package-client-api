# tokimo-package-client-api

External API clients for media metadata services — TMDB, OMDb, Douban, Bangumi, Spotify, MusicBrainz, and more.

## Features

| Category | Providers |
|---|---|
| **Metadata** | TMDB · OMDb · Douban · Bangumi · Spotify · MusicBrainz · Fanart.tv · TheTVDB · StashDB · TPDB |
| **Music** | Netease · QQ Music · Deezer · LRCLIB |
| **Downloaders** | qBittorrent · Transmission · Aria2 · Deluge · rTorrent · 115 Pan · Xunlei |
| **Subtitle** | ASSRT · Shooter |
| **Geocoding** | Nominatim · Open-Meteo |
| **Other** | GitHub Releases · JavDB · JavBus · Wikipedia · Qidian |

## Usage

```rust
use tokimo_package_client_api::metadata_providers::tmdb::TmdbClient;

let client = TmdbClient::new("your_api_key");
let movie = client.get_movie(550).await?;
```

## Add to Cargo.toml

```toml
tokimo-package-client-api = { git = "https://github.com/tokimo-lab/tokimo-package-client-api" }
```

## License

MIT
