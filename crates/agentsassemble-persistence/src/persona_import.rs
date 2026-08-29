use std::{
    collections::BTreeMap,
    io::{BufReader, Cursor},
};

use agentsassemble_domain::{
    MAX_ATTACHMENT_BYTES, PersonaAssetKind, PersonaCard, PersonaLoreEntry, PersonaLoreSettings,
    canonical_persona_id, trim_persona_card_text,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Map, Value};
use thiserror::Error;

use crate::raster_assets::{PNG_SIGNATURE, prepare_raster};

pub(super) const MAX_CARD_JSON_BYTES: usize = 5 * 1024 * 1024;
const MAX_PNG_TEXT_BYTES: usize = 5 * 1024 * 1024;
type ThumbnailCandidate = (String, &'static str, Vec<u8>);

struct CardAssets<'a> {
    items: Vec<&'a Map<String, Value>>,
    use_default_source: bool,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PersonaImportError {
    #[error("persona file is empty or exceeds the 10 MiB import limit")]
    InvalidSize,
    #[error("persona filename or format is unsupported")]
    UnsupportedFormat,
    #[error("persona card is malformed: {0}")]
    InvalidCard(&'static str),
    #[error("Risu module is malformed: {0}")]
    InvalidModule(&'static str),
    #[error("CHARX archive is malformed: {0}")]
    InvalidArchive(&'static str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedPersonaAsset {
    pub(crate) card: PersonaCard,
    pub(crate) thumbnail: Option<Vec<u8>>,
}

/// Parses one `CCv3` JSON, PNG, or APNG upload and canonicalizes only the UI thumbnail.
///
/// # Errors
///
/// Rejects unsupported, malformed, oversized, or invalid-image input.
pub async fn import_ccv3_asset(
    filename: &str,
    content: Vec<u8>,
) -> Result<ImportedPersonaAsset, PersonaImportError> {
    if content.is_empty() || content.len() > MAX_ATTACHMENT_BYTES {
        return Err(PersonaImportError::InvalidSize);
    }
    let extension = filename
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .unwrap_or_default();
    let (payload, embedded, source_thumbnail) = match extension.as_str() {
        "json" => (parse_ccv3_json_object(&content)?, BTreeMap::new(), None),
        "png" | "apng" => {
            let (payload, embedded) = parse_png_card(&content)?;
            (payload, embedded, Some(content))
        }
        _ => return Err(PersonaImportError::UnsupportedFormat),
    };
    finish_ccv3_import(filename, &payload, embedded, source_thumbnail).await
}

pub(super) async fn finish_ccv3_import(
    filename: &str,
    payload: &Value,
    embedded: BTreeMap<String, Vec<u8>>,
    source_thumbnail: Option<Vec<u8>>,
) -> Result<ImportedPersonaAsset, PersonaImportError> {
    let mut normalized = normalize_ccv3(payload, filename)?;
    let assets = asset_specs(payload)?;
    let mut ignored = normalized.ignored_features.clone();
    let (candidate, asset_count) = select_thumbnail(
        &assets.items,
        assets.use_default_source,
        &embedded,
        source_thumbnail.as_deref(),
        &mut ignored,
    );
    let thumbnail = if let Some((candidate_name, content_type, content)) = candidate {
        if let Ok((canonical, _)) = prepare_raster(&candidate_name, content_type, content).await {
            Some(canonical.content)
        } else {
            add_count(&mut ignored, "invalid_thumbnail", 1);
            None
        }
    } else {
        None
    };
    normalized.asset_count = asset_count;
    normalized.ignored_features = ignored;
    Ok(ImportedPersonaAsset {
        card: normalized,
        thumbnail,
    })
}

pub(super) fn parse_ccv3_json_object(content: &[u8]) -> Result<Value, PersonaImportError> {
    if content.len() > MAX_CARD_JSON_BYTES {
        return Err(PersonaImportError::InvalidCard("card JSON exceeds 5 MiB"));
    }
    let payload: Value = serde_json::from_slice(content)
        .map_err(|_| PersonaImportError::InvalidCard("card JSON is invalid"))?;
    if payload.is_object() {
        Ok(payload)
    } else {
        Err(PersonaImportError::InvalidCard(
            "card root must be an object",
        ))
    }
}

fn parse_png_card(
    content: &[u8],
) -> Result<(Value, BTreeMap<String, Vec<u8>>), PersonaImportError> {
    let mut png_decoder = png::Decoder::new(BufReader::new(Cursor::new(content)));
    png_decoder.set_limits(png::Limits {
        bytes: MAX_ATTACHMENT_BYTES.saturating_mul(2),
    });
    let mut reader = png_decoder
        .read_info()
        .map_err(|_| PersonaImportError::InvalidCard("PNG structure is invalid"))?;
    reader
        .finish()
        .map_err(|_| PersonaImportError::InvalidCard("PNG structure is invalid"))?;
    let mut cards = BTreeMap::new();
    let mut assets = BTreeMap::new();
    for chunk in &reader.info().uncompressed_latin1_text {
        if chunk.text.len() > MAX_PNG_TEXT_BYTES {
            continue;
        }
        if matches!(chunk.keyword.as_str(), "ccv3" | "chara") {
            cards.insert(chunk.keyword.as_str(), chunk.text.as_str());
        } else if let Some(index) = png_asset_index(&chunk.keyword)
            && let Ok(content) = decode_base64_bounded(&chunk.text)
        {
            assets.insert(index.to_owned(), content);
        }
    }
    let encoded =
        cards
            .get("ccv3")
            .or_else(|| cards.get("chara"))
            .ok_or(PersonaImportError::InvalidCard(
                "PNG is missing a ccv3 or chara text chunk",
            ))?;
    let card_bytes = decode_base64_bounded(encoded)?;
    Ok((parse_ccv3_json_object(&card_bytes)?, assets))
}

fn png_asset_index(keyword: &str) -> Option<&str> {
    keyword
        .strip_prefix("chara-ext-asset_:")
        .or_else(|| keyword.strip_prefix("chara-ext-asset_"))
        .filter(|index| !index.is_empty())
}

fn decode_base64_bounded(value: &str) -> Result<Vec<u8>, PersonaImportError> {
    if value.len() > MAX_ATTACHMENT_BYTES.div_ceil(3).saturating_mul(4) {
        return Err(PersonaImportError::InvalidCard(
            "embedded payload is too large",
        ));
    }
    let decoded = STANDARD
        .decode(value)
        .map_err(|_| PersonaImportError::InvalidCard("embedded base64 is invalid"))?;
    if decoded.len() > MAX_ATTACHMENT_BYTES {
        return Err(PersonaImportError::InvalidCard(
            "embedded payload is too large",
        ));
    }
    Ok(decoded)
}

fn normalize_ccv3(payload: &Value, source_name: &str) -> Result<PersonaCard, PersonaImportError> {
    let root = object(payload)?;
    if text(root.get("spec")) != "chara_card_v3" {
        return Err(PersonaImportError::InvalidCard(
            "spec must be chara_card_v3",
        ));
    }
    let data = root
        .get("data")
        .and_then(Value::as_object)
        .ok_or(PersonaImportError::InvalidCard("data must be an object"))?;
    let fallback_name = source_name
        .rsplit_once('.')
        .map_or(source_name, |(stem, _)| stem);
    let display_name = nonempty_text(data.get("name"))
        .or_else(|| (!fallback_name.is_empty()).then(|| fallback_name.to_owned()))
        .unwrap_or_else(|| "Persona".to_owned());
    let character_book = data.get("character_book").and_then(Value::as_object);
    let lorebook: Vec<PersonaLoreEntry> = character_book
        .and_then(|book| book.get("entries"))
        .and_then(Value::as_array)
        .map(|entries| entries.iter().filter_map(normalize_lore).collect())
        .unwrap_or_default();
    let ignored_features = ignored_ccv3_features(data, &lorebook);
    Ok(PersonaCard {
        id: canonical_persona_id(&display_name),
        display_name,
        description: text(data.get("description")),
        system_prompt: text(data.get("system_prompt")),
        personality: text(data.get("personality")),
        scenario: text(data.get("scenario")),
        first_message: text(data.get("first_mes")),
        example_messages: text(data.get("mes_example")),
        post_history_instructions: text(data.get("post_history_instructions")),
        lorebook,
        lore_settings: normalize_lore_settings(character_book),
        asset_kind: PersonaAssetKind::Card,
        source_kind: "ccv3".to_owned(),
        asset_count: 0,
        ignored_features,
        tag_count: data
            .get("tags")
            .and_then(Value::as_array)
            .map_or(0, |tags| tags.iter().filter(|tag| tag.is_string()).count()),
    })
}

pub(super) fn normalize_lore(value: &Value) -> Option<PersonaLoreEntry> {
    let entry = value.as_object()?;
    let extensions = entry.get("extensions").and_then(Value::as_object);
    let insert_order = integer(
        entry
            .get("insert_order")
            .or_else(|| entry.get("insertorder"))
            .or_else(|| entry.get("insertion_order")),
    );
    Some(PersonaLoreEntry {
        key: nonempty_text(entry.get("key")).unwrap_or_else(|| joined_strings(entry.get("keys"))),
        content: raw_text(entry.get("content")),
        secondary_key: nonempty_text(entry.get("secondkey"))
            .unwrap_or_else(|| joined_strings(entry.get("secondary_keys"))),
        comment: nonempty_text(entry.get("comment")).unwrap_or_else(|| text(entry.get("name"))),
        always_active: boolean(
            entry
                .get("always_active")
                .or_else(|| entry.get("alwaysActive"))
                .or_else(|| entry.get("constant")),
        ),
        selective: boolean(entry.get("selective")),
        use_regex: boolean(entry.get("use_regex").or_else(|| entry.get("useRegex"))),
        insert_order,
        enabled: entry
            .get("enabled")
            .is_none_or(|value| boolean(Some(value))),
        case_sensitive: entry.get("case_sensitive").map_or_else(
            || boolean(extensions.and_then(|value| value.get("risu_case_sensitive"))),
            |value| boolean(Some(value)),
        ),
        priority: entry
            .get("priority")
            .map_or(insert_order, |value| integer(Some(value))),
    })
}

fn normalize_lore_settings(character_book: Option<&Map<String, Value>>) -> PersonaLoreSettings {
    let Some(book) = character_book else {
        return PersonaLoreSettings::default();
    };
    let extensions = book.get("extensions").and_then(Value::as_object);
    PersonaLoreSettings {
        scan_depth: usize::try_from(integer(book.get("scan_depth"))).unwrap_or_default(),
        recursive_scanning: boolean(book.get("recursive_scanning")),
        full_word_matching: boolean(
            extensions.and_then(|value| value.get("risu_fullWordMatching")),
        ),
    }
}

fn ignored_ccv3_features(
    data: &Map<String, Value>,
    lorebook: &[PersonaLoreEntry],
) -> BTreeMap<String, u32> {
    let mut ignored = BTreeMap::new();
    let risu = data
        .get("extensions")
        .and_then(Value::as_object)
        .and_then(|extensions| extensions.get("risuai"))
        .and_then(Value::as_object);
    if let Some(risu) = risu {
        for (source, target) in [
            ("triggerscript", "trigger"),
            ("trigger", "trigger"),
            ("customScripts", "customScripts"),
            ("regex", "customScripts"),
        ] {
            if let Some(count) = risu.get(source).and_then(Value::as_array).map(Vec::len) {
                add_count(&mut ignored, target, count);
            }
        }
        for name in ["lowLevelAccess", "cjs", "mcp", "backgroundHTML"] {
            if risu.get(name).is_some_and(truthy) {
                add_count(&mut ignored, name, 1);
            }
        }
    }
    add_count(
        &mut ignored,
        "lorebook_regex_matching",
        lorebook.iter().filter(|entry| entry.use_regex).count(),
    );
    ignored
}

fn asset_specs(payload: &Value) -> Result<CardAssets<'_>, PersonaImportError> {
    let data = object(payload)?
        .get("data")
        .and_then(Value::as_object)
        .ok_or(PersonaImportError::InvalidCard("data must be an object"))?;
    let Some(assets) = data.get("assets").and_then(Value::as_array) else {
        return Ok(CardAssets {
            items: Vec::new(),
            use_default_source: true,
        });
    };
    Ok(CardAssets {
        items: assets.iter().filter_map(Value::as_object).collect(),
        use_default_source: false,
    })
}

fn select_thumbnail(
    assets: &[&Map<String, Value>],
    use_default_asset: bool,
    embedded: &BTreeMap<String, Vec<u8>>,
    source: Option<&[u8]>,
    ignored: &mut BTreeMap<String, u32>,
) -> (Option<ThumbnailCandidate>, usize) {
    let mut selected = None;
    let mut selected_preferred = false;
    let mut count = 0;
    let default_asset = Map::from_iter([
        ("type".to_owned(), Value::String("icon".to_owned())),
        ("uri".to_owned(), Value::String("ccdefault:".to_owned())),
    ]);
    let items: Vec<&Map<String, Value>> = if use_default_asset {
        vec![&default_asset]
    } else {
        assets.to_vec()
    };
    for asset in items {
        let uri = text(asset.get("uri"));
        let payload = asset_payload(&uri, embedded, source, ignored);
        let Some(payload) = payload else { continue };
        count += 1;
        let Some(content_type) = raster_content_type(&payload) else {
            continue;
        };
        let preferred = matches!(
            text(asset.get("type")).to_ascii_lowercase().as_str(),
            "icon" | "avatar" | "portrait"
        );
        if selected.is_none() || (preferred && !selected_preferred) {
            selected = Some(("persona-thumbnail".to_owned(), content_type, payload));
            selected_preferred = preferred;
        }
    }
    (selected, count)
}

fn asset_payload(
    uri: &str,
    embedded: &BTreeMap<String, Vec<u8>>,
    source: Option<&[u8]>,
    ignored: &mut BTreeMap<String, u32>,
) -> Option<Vec<u8>> {
    let (payload, reason) = if uri == "ccdefault:" {
        (source.map(<[u8]>::to_vec), "missing_asset")
    } else if let Some(index) = uri.strip_prefix("__asset:") {
        (embedded.get(index).cloned(), "missing_asset")
    } else if let Some(path) = uri.strip_prefix("embeded://") {
        let Some(path) = safe_embedded_path(path) else {
            add_count(ignored, "unsafe_asset_uri", 1);
            return None;
        };
        (embedded.get(&path).cloned(), "missing_asset")
    } else if uri.starts_with("data:") {
        let payload = uri
            .split_once(',')
            .and_then(|(_, encoded)| decode_base64_bounded(encoded).ok());
        (payload, "oversized_or_invalid_data_uri")
    } else if uri.starts_with("http://") || uri.starts_with("https://") {
        (None, "remote_asset_uri")
    } else if uri.starts_with("file:") || uri.contains(':') {
        (None, "unsupported_asset_uri")
    } else {
        (
            None,
            if uri.is_empty() {
                "missing_asset"
            } else {
                "unsupported_asset_uri"
            },
        )
    };
    if payload.is_none() {
        add_count(ignored, reason, 1);
    }
    payload
}

pub(super) fn safe_embedded_path(value: &str) -> Option<String> {
    if value.is_empty()
        || value.starts_with('/')
        || value.ends_with('/')
        || value.contains('\0')
        || value.contains('\\')
        || value.contains("//")
    {
        return None;
    }
    let mut parts = value.split('/');
    let first = parts.next()?;
    if first.is_empty() || first.contains(':') || matches!(first, "." | "..") {
        return None;
    }
    if parts
        .clone()
        .any(|part| part.is_empty() || matches!(part, "." | ".."))
    {
        return None;
    }
    Some(value.to_owned())
}

fn raster_content_type(content: &[u8]) -> Option<&'static str> {
    if content.starts_with(PNG_SIGNATURE) {
        Some("image/png")
    } else if content.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if content.starts_with(b"GIF87a") || content.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if content.get(..4) == Some(b"RIFF") && content.get(8..12) == Some(b"WEBP") {
        Some("image/webp")
    } else {
        None
    }
}

fn object(value: &Value) -> Result<&Map<String, Value>, PersonaImportError> {
    value.as_object().ok_or(PersonaImportError::InvalidCard(
        "card root must be an object",
    ))
}

pub(super) fn text(value: Option<&Value>) -> String {
    trim_persona_card_text(value.and_then(Value::as_str).unwrap_or_default()).to_owned()
}

pub(super) fn nonempty_text(value: Option<&Value>) -> Option<String> {
    let value = text(value);
    (!value.is_empty()).then_some(value)
}

pub(super) fn raw_text(value: Option<&Value>) -> String {
    value.and_then(Value::as_str).unwrap_or_default().to_owned()
}

fn joined_strings(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
}

pub(super) fn boolean(value: Option<&Value>) -> bool {
    value.and_then(Value::as_bool).unwrap_or(false)
}

pub(super) fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::String(value) => !value.is_empty(),
        Value::Array(value) => !value.is_empty(),
        Value::Object(value) => !value.is_empty(),
        Value::Number(value) => value.as_f64().is_some_and(|value| value != 0.0),
    }
}

pub(super) fn integer(value: Option<&Value>) -> i64 {
    value
        .and_then(|value| value.as_i64().or_else(|| value.as_str()?.parse().ok()))
        .unwrap_or_default()
}

pub(super) fn add_count(counts: &mut BTreeMap<String, u32>, name: &str, count: usize) {
    if count == 0 {
        return;
    }
    let count = u32::try_from(count).unwrap_or(u32::MAX);
    counts
        .entry(name.to_owned())
        .and_modify(|value| *value = value.saturating_add(count))
        .or_insert(count);
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use serde_json::json;

    use super::import_ccv3_asset;

    #[tokio::test]
    async fn imports_png_card_with_safe_thumbnail_and_inert_features() {
        let card = json!({
            "spec": "chara_card_v3",
            "spec_version": "3.0",
            "data": {
                "name": "\u{001F}Harbor Guide\u{001F}",
                "description": "Keeps watch.",
                "first_mes": "Hello {{char}}",
                "tags": ["guide"],
                "character_book": {"entries": [
                    {"keys": ["harbor"], "content": "The bell rings."},
                    {"key": ".*", "content": "must stay inert", "use_regex": true}
                ]},
                "extensions": {"risuai": {"customScripts": [{"in": ".*"}], "mcp": {"server": "private"}}}
            }
        });
        let mut png_bytes = Cursor::new(Vec::new());
        {
            let mut png_encoder = png::Encoder::new(&mut png_bytes, 1, 1);
            png_encoder.set_color(png::ColorType::Rgba);
            png_encoder.set_depth(png::BitDepth::Eight);
            png_encoder
                .add_text_chunk("ccv3".to_owned(), STANDARD.encode(card.to_string()))
                .unwrap_or_else(|error| panic!("add card chunk: {error}"));
            let mut writer = png_encoder
                .write_header()
                .unwrap_or_else(|error| panic!("write PNG header: {error}"));
            writer
                .write_image_data(&[0, 0, 0, 255])
                .unwrap_or_else(|error| panic!("write PNG pixel: {error}"));
        }
        let imported = import_ccv3_asset("guide.png", png_bytes.into_inner())
            .await
            .unwrap_or_else(|error| panic!("import PNG card: {error}"));
        assert_eq!(imported.card.id, "Harbor-Guide");
        assert_eq!(imported.card.display_name, "Harbor Guide");
        assert_eq!(imported.card.lorebook.len(), 2);
        assert_eq!(imported.card.ignored_features["customScripts"], 1);
        assert_eq!(imported.card.ignored_features["mcp"], 1);
        assert_eq!(imported.card.ignored_features["lorebook_regex_matching"], 1);
        assert!(imported.thumbnail.is_some());
    }

    #[tokio::test]
    async fn rejects_non_ccv3_json_without_substituting_placeholder_data() {
        let Err(error) = import_ccv3_asset("card.json", br#"{"name":"not-v3"}"#.to_vec()).await
        else {
            panic!("non-CCv3 JSON must fail");
        };
        assert!(error.to_string().contains("spec must be chara_card_v3"));
    }

    #[tokio::test]
    async fn counts_resolved_assets_after_the_first_preferred_thumbnail() {
        let icon = png_data_uri([0, 0, 0, 255]);
        let background = png_data_uri([255, 255, 255, 255]);
        let card = json!({
            "spec": "chara_card_v3",
            "spec_version": "3.0",
            "data": {
                "name": "Asset Guide",
                "assets": [
                    {"type": "icon", "uri": icon},
                    {"type": "background", "uri": background}
                ]
            }
        });

        let imported = import_ccv3_asset("assets.json", card.to_string().into_bytes())
            .await
            .unwrap_or_else(|error| panic!("import card: {error}"));

        assert_eq!(imported.card.asset_count, 2);
        assert!(imported.thumbnail.is_some());
    }

    fn png_data_uri(pixel: [u8; 4]) -> String {
        let mut png_bytes = Cursor::new(Vec::new());
        {
            let mut png_encoder = png::Encoder::new(&mut png_bytes, 1, 1);
            png_encoder.set_color(png::ColorType::Rgba);
            png_encoder.set_depth(png::BitDepth::Eight);
            let mut writer = png_encoder
                .write_header()
                .unwrap_or_else(|error| panic!("write PNG header: {error}"));
            writer
                .write_image_data(&pixel)
                .unwrap_or_else(|error| panic!("write PNG pixel: {error}"));
        }
        format!(
            "data:image/png;base64,{}",
            STANDARD.encode(png_bytes.into_inner())
        )
    }
}
