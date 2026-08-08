use serde_json::{Map, Value};
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct ReleaseCard {
    pub id: i64,
    pub title: String,
    pub subtitle: Option<String>,
    pub year: Option<i64>,
    pub description: Option<String>,
    pub poster_url: Option<String>,
    pub rating: Option<f64>,
    pub episodes_released: Option<i64>,
    pub episodes_total: Option<i64>,
    pub duration_minutes: Option<i64>,
    pub status: Option<String>,
    pub country: Option<String>,
    pub season: Option<String>,
    pub age_rating: Option<String>,
    pub studio: Option<String>,
    pub genres: Vec<String>,
}

impl ReleaseCard {
    pub fn merge_prefer(self, newer: ReleaseCard) -> ReleaseCard {
        ReleaseCard {
            id: self.id,
            title: prefer_string(Some(self.title), Some(newer.title)).unwrap_or_default(),
            subtitle: prefer_string(self.subtitle, newer.subtitle),
            year: newer.year.or(self.year),
            description: prefer_string(self.description, newer.description),
            poster_url: prefer_string(self.poster_url, newer.poster_url),
            rating: newer.rating.or(self.rating),
            episodes_released: newer.episodes_released.or(self.episodes_released),
            episodes_total: newer.episodes_total.or(self.episodes_total),
            duration_minutes: newer.duration_minutes.or(self.duration_minutes),
            status: prefer_string(self.status, newer.status),
            country: prefer_string(self.country, newer.country),
            season: prefer_string(self.season, newer.season),
            age_rating: prefer_string(self.age_rating, newer.age_rating),
            studio: prefer_string(self.studio, newer.studio),
            genres: if newer.genres.is_empty() { self.genres } else { newer.genres },
        }
    }
}

#[derive(Debug, Clone)]
pub struct Voiceover {
    pub id: i64,
    pub name: String,
    pub episodes_count: Option<i64>,
    pub is_sub: bool,
    pub quality: Option<i64>,
    pub icon_url: Option<String>,
    pub view_count: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct EpisodeSource {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct EpisodeItem {
    pub position: i64,
    pub name: String,
    pub url: Option<String>,
    pub iframe: bool,
    pub is_watched: bool,
}

pub fn extract_release_cards(root: &Value) -> Vec<ReleaseCard> {
    // Prefer the actual result arrays. The old recursive scanner was useful while
    // reverse engineering, but it could also mistake nested recommendation,
    // comment, or metadata objects for releases after API schema changes.
    for key in ["content", "releases", "items", "results"] {
        if let Some(items) = direct_result_array(root, key) {
            let direct = items
                .iter()
                .filter_map(release_from_result_value)
                .collect::<Vec<_>>();
            if !direct.is_empty() {
                return dedup_releases(direct);
            }
        }
    }

    let mut found = Vec::new();
    let mut seen = HashSet::new();
    walk_releases(root, &mut found, &mut seen);
    found
}

pub fn extract_release_detail(root: &Value, id: i64) -> Option<ReleaseCard> {
    let mut candidates = Vec::new();
    walk_release_candidates(root, id, &mut candidates);
    candidates.into_iter().max_by_key(release_richness)
}

pub fn extract_voiceovers(root: &Value) -> Vec<Voiceover> {
    let Some(items) = find_named_array(root, &["types", "voiceovers", "dubbers"]) else {
        return Vec::new();
    };

    items
        .iter()
        .filter_map(Value::as_object)
        .filter_map(|map| {
            let id = integer(map, &["id", "type_id", "typeId", "dubber_id", "dubberId"])?;
            Some(Voiceover {
                id,
                name: text(map, &["name", "title"])
                    .unwrap_or_else(|| format!("Voiceover {id}")),
                episodes_count: integer(map, &["episodes_count", "episodesCount", "count"]),
                is_sub: boolean(map, &["is_sub", "isSub"]).unwrap_or(false),
                quality: integer(map, &["quality"]),
                icon_url: http_text(map, &["icon", "icon_url", "iconUrl", "image", "avatar"]),
                view_count: integer(map, &["view_count", "viewCount", "views"]),
            })
        })
        .collect()
}

pub fn extract_episode_sources(root: &Value) -> Vec<EpisodeSource> {
    let Some(items) = find_named_array(root, &["sources", "video_sources", "videoSources"]) else {
        return Vec::new();
    };

    items
        .iter()
        .filter_map(Value::as_object)
        .filter_map(|map| {
            let id = integer(map, &["id", "source_id", "sourceId"])?;
            Some(EpisodeSource {
                id,
                name: text(map, &["name", "title", "label"])
                    .unwrap_or_else(|| format!("Source {id}")),
            })
        })
        .collect()
}

pub fn extract_episode_items(root: &Value) -> Vec<EpisodeItem> {
    let Some(items) = find_named_array(root, &["episodes"]) else {
        return Vec::new();
    };

    items
        .iter()
        .enumerate()
        .filter_map(|(index, value)| {
            let map = value.as_object()?;
            let position = integer(
                map,
                &["position", "episode_position", "episodePosition", "number", "episode"],
            )
            .unwrap_or((index + 1) as i64);

            let url = http_text(
                map,
                &["url", "link", "player_url", "playerUrl", "iframe_url", "iframeUrl"],
            );

            Some(EpisodeItem {
                position,
                name: text(map, &["name", "title", "episode_name", "episodeName"])
                    .unwrap_or_else(|| format!("Episode {position}")),
                url,
                iframe: boolean(map, &["iframe", "is_iframe", "isIframe"]).unwrap_or(false),
                is_watched: boolean(map, &["is_watched", "isWatched", "watched"])
                    .unwrap_or(false),
            })
        })
        .collect()
}

fn direct_result_array<'a>(root: &'a Value, key: &str) -> Option<&'a Vec<Value>> {
    root.get(key)
        .and_then(Value::as_array)
        .or_else(|| root.get("data")?.get(key)?.as_array())
        .or_else(|| root.get("result")?.get(key)?.as_array())
}

fn dedup_releases(items: Vec<ReleaseCard>) -> Vec<ReleaseCard> {
    let mut seen = HashSet::new();
    items
        .into_iter()
        .filter(|item| seen.insert(item.id))
        .collect()
}

fn find_named_array<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a Vec<Value>> {
    match value {
        Value::Object(map) => {
            for key in keys {
                if let Some(array) = map.get(*key).and_then(Value::as_array) {
                    return Some(array);
                }
            }
            map.values().find_map(|child| find_named_array(child, keys))
        }
        Value::Array(items) => items.iter().find_map(|child| find_named_array(child, keys)),
        _ => None,
    }
}

fn walk_releases(value: &Value, out: &mut Vec<ReleaseCard>, seen: &mut HashSet<i64>) {
    match value {
        Value::Object(map) => {
            if let Some(card) = release_from_result_map(map) {
                if seen.insert(card.id) {
                    out.push(card);
                }
            }
            for child in map.values() {
                walk_releases(child, out, seen);
            }
        }
        Value::Array(items) => {
            for child in items {
                walk_releases(child, out, seen);
            }
        }
        _ => {}
    }
}

fn walk_release_candidates(value: &Value, id: i64, out: &mut Vec<ReleaseCard>) {
    match value {
        Value::Object(map) => {
            if integer(map, &["release_id", "releaseId", "id"]) == Some(id) {
                if let Some(card) = release_from_map(map) {
                    out.push(card);
                }
            }
            for child in map.values() {
                walk_release_candidates(child, id, out);
            }
        }
        Value::Array(items) => {
            for child in items {
                walk_release_candidates(child, id, out);
            }
        }
        _ => {}
    }
}

fn release_richness(card: &ReleaseCard) -> usize {
    usize::from(card.subtitle.is_some())
        + usize::from(card.year.is_some())
        + usize::from(card.description.is_some()) * 3
        + usize::from(card.poster_url.is_some()) * 3
        + usize::from(card.rating.is_some())
        + usize::from(card.episodes_released.is_some())
        + usize::from(card.episodes_total.is_some())
        + usize::from(card.duration_minutes.is_some())
        + usize::from(card.status.is_some())
        + usize::from(card.country.is_some())
        + usize::from(card.season.is_some())
        + usize::from(card.age_rating.is_some())
        + usize::from(card.studio.is_some())
        + card.genres.len()
}

fn release_from_result_value(value: &Value) -> Option<ReleaseCard> {
    release_from_result_map(value.as_object()?)
}

/// Discover endpoints wrap a real release in an `Interesting`/feed object.
/// Those wrappers have their own large database `id` values (for example
/// 8,013,030) which are *not* valid `/release/{id}` or `/episode/{id}` IDs.
/// Prefer a nested release object or an explicit release_id before falling
/// back to the container's generic `id` field.
fn release_from_result_map(map: &Map<String, Value>) -> Option<ReleaseCard> {
    for key in ["release", "release_model", "releaseModel"] {
        if let Some(release) = map.get(key).and_then(Value::as_object) {
            if let Some(card) = release_from_map(release) {
                return Some(card);
            }
        }
    }

    let mut card = release_from_map(map)?;
    // Beta 21's Interesting entity is a navigation/banner wrapper with fields
    // `id`, `title`, `description`, `image`, `type` and string `action`.
    // Its own id is not necessarily a release id. If the action explicitly
    // targets a release, recover the actual release id from that action.
    if let Some(release_id) = release_id_from_action(map) {
        card.id = release_id;
    }
    Some(card)
}

fn release_id_from_action(map: &Map<String, Value>) -> Option<i64> {
    let action = text(map, &["action"])?;
    let lower = action.to_ascii_lowercase();
    if !(lower.contains("release") || lower.contains("anime")) {
        return None;
    }

    // Accept URI-like, path-like and JSON-ish action formats without tying the
    // parser to one beta representation. The last positive integer is usually
    // the target release id, e.g. `.../release/19939` or `release:19939`.
    let mut numbers = Vec::new();
    let mut current = String::new();
    for ch in action.chars() {
        if ch.is_ascii_digit() {
            current.push(ch);
        } else if !current.is_empty() {
            if let Ok(value) = current.parse::<i64>() {
                numbers.push(value);
            }
            current.clear();
        }
    }
    if !current.is_empty() {
        if let Ok(value) = current.parse::<i64>() {
            numbers.push(value);
        }
    }
    numbers.into_iter().rev().find(|value| *value > 0)
}

fn release_from_map(map: &Map<String, Value>) -> Option<ReleaseCard> {
    let id = integer(map, &["release_id", "releaseId", "id"])?;
    let title = text(
        map,
        &[
            "title_ru",
            "titleRu",
            "title",
            "name",
            "title_original",
            "titleOriginal",
        ],
    )?;

    let looks_release_like = map.contains_key("title_ru")
        || map.contains_key("titleRu")
        || map.contains_key("title_original")
        || map.contains_key("titleOriginal")
        || map.contains_key("year")
        || map.contains_key("image")
        || map.contains_key("poster")
        || map.contains_key("description");
    if !looks_release_like {
        return None;
    }

    let subtitle = text(map, &["title_original", "titleOriginal", "title_en", "titleEn"])
        .filter(|candidate| candidate != &title);
    let year = integer(map, &["year"]);
    let description = text(
        map,
        &["description", "description_sitename", "descriptionSitename", "description_title"],
    );

    let poster_url = http_text(
        map,
        &[
            "image_original",
            "imageOriginal",
            "image",
            "poster",
            "image_url",
            "imageUrl",
            "poster_url",
            "posterUrl",
            "image_preview",
            "imagePreview",
        ],
    );

    let rating = number(map, &["grade", "rating", "score", "average"]);
    let episodes_released = integer(
        map,
        &[
            "episodes_released",
            "episodesReleased",
            "episodes_current",
            "episodesCurrent",
            "episode_current",
            "episodeCurrent",
        ],
    );
    let episodes_total = integer(
        map,
        &[
            "episodes_total",
            "episodesTotal",
            "episode_total",
            "episodeTotal",
            "episodes_count",
            "episodesCount",
        ],
    );
    let duration_minutes = integer(map, &["duration", "duration_minutes", "durationMinutes"]);
    let status = text(map, &["status", "status_name", "statusName"]);
    let country = text(map, &["country", "country_name", "countryName"]);
    let season = text(map, &["season", "season_name", "seasonName"]);
    let age_rating = text(map, &["age_rating", "ageRating", "age", "rating_age"])
        .or_else(|| integer(map, &["age_rating", "ageRating", "age"]).map(|n| format!("{n}+")));
    let studio = text(map, &["studio", "studio_name", "studioName"]);
    let genres = string_list(map, &["genres", "genre", "genre_names", "genreNames"]);

    Some(ReleaseCard {
        id,
        title,
        subtitle,
        year,
        description,
        poster_url,
        rating,
        episodes_released,
        episodes_total,
        duration_minutes,
        status,
        country,
        season,
        age_rating,
        studio,
        genres,
    })
}

fn prefer_string(old: Option<String>, new: Option<String>) -> Option<String> {
    new.filter(|s| !s.trim().is_empty()).or(old)
}

fn text(map: &Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        map.get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
    })
}

fn http_text(map: &Map<String, Value>, keys: &[&str]) -> Option<String> {
    text(map, keys).filter(|value| value.starts_with("https://") || value.starts_with("http://"))
}

fn integer(map: &Map<String, Value>, keys: &[&str]) -> Option<i64> {
    keys.iter().find_map(|key| {
        let value = map.get(*key)?;
        value
            .as_i64()
            .or_else(|| value.as_u64().and_then(|n| i64::try_from(n).ok()))
            .or_else(|| value.as_str().and_then(|n| n.parse().ok()))
    })
}

fn number(map: &Map<String, Value>, keys: &[&str]) -> Option<f64> {
    keys.iter().find_map(|key| {
        let value = map.get(*key)?;
        value
            .as_f64()
            .or_else(|| value.as_i64().map(|n| n as f64))
            .or_else(|| value.as_u64().map(|n| n as f64))
            .or_else(|| value.as_str().and_then(|n| n.parse().ok()))
    })
}

fn boolean(map: &Map<String, Value>, keys: &[&str]) -> Option<bool> {
    keys.iter().find_map(|key| {
        let value = map.get(*key)?;
        value
            .as_bool()
            .or_else(|| value.as_i64().map(|n| n != 0))
            .or_else(|| match value.as_str()?.to_ascii_lowercase().as_str() {
                "true" | "1" | "yes" => Some(true),
                "false" | "0" | "no" => Some(false),
                _ => None,
            })
    })
}

fn string_list(map: &Map<String, Value>, keys: &[&str]) -> Vec<String> {
    for key in keys {
        let Some(value) = map.get(*key) else { continue };
        match value {
            Value::Array(items) => {
                let values = items
                    .iter()
                    .filter_map(|item| {
                        item.as_str().map(ToOwned::to_owned).or_else(|| {
                            item.as_object().and_then(|obj| text(obj, &["name", "title"]))
                        })
                    })
                    .filter(|item| !item.trim().is_empty())
                    .collect::<Vec<_>>();
                if !values.is_empty() {
                    return values;
                }
            }
            Value::String(value) if !value.trim().is_empty() => {
                return value
                    .split(',')
                    .map(str::trim)
                    .filter(|part| !part.is_empty())
                    .map(ToOwned::to_owned)
                    .collect();
            }
            _ => {}
        }
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_release_without_binding_to_one_api_schema() {
        let value = json!({
            "code": 0,
            "content": [
                {"id": 42, "title_ru": "Example", "title_original": "Example EN", "year": 2026, "image": "https://example.test/poster.jpg"},
                {"id": 7, "name": "not a release"}
            ]
        });
        let cards = extract_release_cards(&value);
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].id, 42);
        assert!(cards[0].poster_url.is_some());
    }

    #[test]
    fn extracts_beta_voiceover_types_with_icons() {
        let value = json!({
            "code": 0,
            "types": [
                {"id": 2, "name": "AnimeVost", "episodes_count": 6, "is_sub": false, "quality": 0, "icon": "https://example.test/icon.jpg", "view_count": 69000}
            ]
        });
        let items = extract_voiceovers(&value);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].name, "AnimeVost");
        assert_eq!(items[0].view_count, Some(69000));
    }

    #[test]
    fn extracts_sources_then_episodes() {
        let sources = extract_episode_sources(&json!({
            "sources": [{"id": 11, "name": "Kodik"}]
        }));
        assert_eq!(sources[0].id, 11);

        let episodes = extract_episode_items(&json!({
            "episodes": [{"position": 1, "name": "Episode 1", "url": "https://example.test/embed/1", "iframe": true}]
        }));
        assert_eq!(episodes.len(), 1);
        assert!(episodes[0].iframe);
    }
}

#[cfg(test)]
mod release_identity_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn discover_wrapper_uses_nested_release_id() {
        let value = json!({
            "code": 0,
            "content": [
                {
                    "id": 8013030,
                    "title": "Interesting wrapper title",
                    "image": "https://example.invalid/banner.jpg",
                    "release": {
                        "id": 19939,
                        "title_ru": "Реинкарнация безработного 3",
                        "title_original": "Mushoku Tensei III",
                        "image_original": "https://example.invalid/poster.jpg"
                    }
                }
            ]
        });

        let cards = extract_release_cards(&value);
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].id, 19939);
        assert_eq!(cards[0].title, "Реинкарнация безработного 3");
        assert_eq!(cards[0].poster_url.as_deref(), Some("https://example.invalid/poster.jpg"));
    }

    #[test]
    fn explicit_release_id_beats_container_id() {
        let value = json!({
            "code": 0,
            "content": [
                {
                    "id": 8013030,
                    "release_id": 19939,
                    "title_ru": "Реинкарнация безработного 3",
                    "image": "https://example.invalid/poster.jpg"
                }
            ]
        });

        let cards = extract_release_cards(&value);
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].id, 19939);
    }

    #[test]
    fn interesting_action_recovers_release_id() {
        let value = json!({
            "code": 0,
            "content": [
                {
                    "id": 8013030,
                    "title": "Реинкарнация безработного 3",
                    "description": "Home/Discover wrapper",
                    "image": "https://example.invalid/banner.jpg",
                    "action": "anixart://open/release/19939"
                }
            ]
        });

        let cards = extract_release_cards(&value);
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].id, 19939);
    }
}
