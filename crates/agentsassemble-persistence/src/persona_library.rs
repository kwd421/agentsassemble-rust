use agentsassemble_domain::{
    MAX_ATTACHMENT_BYTES, MAX_PERSONA_ID_CHARACTERS, PersonaAssetKind, PersonaAssetSummary,
    PersonaCard, canonical_persona_id,
};
use caseless::default_case_fold_str;
use sqlx::{Row, Sqlite, Transaction};

use crate::{ImportedPersonaAsset, PersistenceError, SqliteStore, raster_assets::PNG_SIGNATURE};

impl SqliteStore {
    /// Atomically replaces one normalized persona card and its optional canonical thumbnail.
    ///
    /// # Errors
    ///
    /// Rejects invalid typed importer output or a database write failure. The previous exact
    /// persona row remains unchanged when validation or the single replacement statement fails.
    pub async fn replace_persona_asset(
        &self,
        imported: ImportedPersonaAsset,
    ) -> Result<PersonaAssetSummary, PersistenceError> {
        let ImportedPersonaAsset { card, thumbnail } = imported;
        validate_card(&card)?;
        validate_thumbnail(&card, thumbnail.as_deref())?;
        let card_json = serde_json::to_string(&card)?;
        if card_json.len() > MAX_ATTACHMENT_BYTES {
            return Err(PersistenceError::InvalidPersonaAsset);
        }
        let summary = card.summary(thumbnail.is_some());
        sqlx::query(
            "INSERT INTO persona_assets(persona_id, card_json, thumbnail_png) VALUES (?, ?, ?) ON CONFLICT(persona_id) DO UPDATE SET card_json = excluded.card_json, thumbnail_png = excluded.thumbnail_png",
        )
        .bind(&card.id)
        .bind(card_json)
        .bind(thumbnail)
        .execute(&self.pool)
        .await?;
        Ok(summary)
    }

    /// Lists safe summaries without loading optional thumbnail BLOBs.
    ///
    /// # Errors
    ///
    /// Fails closed when any durable card is corrupt instead of hiding part of the library.
    pub async fn persona_assets(&self) -> Result<Vec<PersonaAssetSummary>, PersistenceError> {
        let rows = sqlx::query(
            "SELECT persona_id, card_json, thumbnail_png IS NOT NULL AS has_thumbnail FROM persona_assets",
        )
        .fetch_all(&self.pool)
        .await?;
        let mut summaries = Vec::with_capacity(rows.len());
        for row in rows {
            let card = decode_card(
                row.get::<String, _>("persona_id").as_str(),
                row.get::<String, _>("card_json").as_str(),
            )?;
            let has_thumbnail = row.get::<bool, _>("has_thumbnail");
            validate_thumbnail_kind(&card, has_thumbnail)?;
            summaries.push(card.summary(has_thumbnail));
        }
        summaries.sort_by_cached_key(|summary| {
            (
                asset_order(summary.asset_kind),
                default_case_fold_str(&summary.display_name),
                summary.id.clone(),
            )
        });
        Ok(summaries)
    }

    /// Loads one private normalized card for selection or provider-neutral prompt rendering.
    ///
    /// # Errors
    ///
    /// Returns `PersonaAssetMissing` for an invalid or absent ID and fails closed for corrupt
    /// stored card state.
    pub async fn persona_asset(&self, persona_id: &str) -> Result<PersonaCard, PersistenceError> {
        if !valid_persona_id(persona_id) {
            return Err(PersistenceError::PersonaAssetMissing);
        }
        let card_json = sqlx::query_scalar::<_, String>(
            "SELECT card_json FROM persona_assets WHERE persona_id = ?",
        )
        .bind(persona_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(PersistenceError::PersonaAssetMissing)?;
        decode_card(persona_id, &card_json)
    }

    /// Loads one canonical PNG without reading the private card body.
    ///
    /// # Errors
    ///
    /// Returns `PersonaAssetMissing` when the ID or thumbnail is unavailable and rejects corrupt
    /// stored bytes rather than serving them as an image.
    pub async fn persona_thumbnail(&self, persona_id: &str) -> Result<Vec<u8>, PersistenceError> {
        if !valid_persona_id(persona_id) {
            return Err(PersistenceError::PersonaAssetMissing);
        }
        let content = sqlx::query_scalar::<_, Option<Vec<u8>>>(
            "SELECT thumbnail_png FROM persona_assets WHERE persona_id = ?",
        )
        .bind(persona_id)
        .fetch_optional(&self.pool)
        .await?
        .flatten()
        .ok_or(PersistenceError::PersonaAssetMissing)?;
        if !valid_png(&content) {
            return Err(PersistenceError::InvalidPersonaAsset);
        }
        Ok(content)
    }
}

pub(crate) async fn resolve_persona_selection(
    transaction: &mut Transaction<'_, Sqlite>,
    persona_id: &str,
) -> Result<Option<Box<PersonaAssetSummary>>, PersistenceError> {
    if persona_id.is_empty() {
        return Ok(None);
    }
    if !valid_persona_id(persona_id) {
        return Err(persona_not_found());
    }
    let row = sqlx::query(
        "SELECT card_json, thumbnail_png IS NOT NULL AS has_thumbnail FROM persona_assets WHERE persona_id = ?",
    )
    .bind(persona_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(persona_not_found)?;
    let card = decode_card(persona_id, row.get::<String, _>("card_json").as_str())
        .map_err(|_| persona_not_found())?;
    let has_thumbnail = row.get::<bool, _>("has_thumbnail");
    validate_thumbnail_kind(&card, has_thumbnail).map_err(|_| persona_not_found())?;
    Ok(Some(Box::new(card.summary(has_thumbnail))))
}

fn decode_card(persona_id: &str, card_json: &str) -> Result<PersonaCard, PersistenceError> {
    let card: PersonaCard =
        serde_json::from_str(card_json).map_err(|_| PersistenceError::InvalidPersonaAsset)?;
    if card.id != persona_id || !valid_persona_id(&card.id) {
        return Err(PersistenceError::InvalidPersonaAsset);
    }
    Ok(card)
}

fn validate_card(card: &PersonaCard) -> Result<(), PersistenceError> {
    if valid_persona_id(&card.id) {
        Ok(())
    } else {
        Err(PersistenceError::InvalidPersonaAsset)
    }
}

fn validate_thumbnail(
    card: &PersonaCard,
    thumbnail: Option<&[u8]>,
) -> Result<(), PersistenceError> {
    validate_thumbnail_kind(card, thumbnail.is_some())?;
    if thumbnail.is_none_or(valid_png) {
        Ok(())
    } else {
        Err(PersistenceError::InvalidPersonaAsset)
    }
}

fn validate_thumbnail_kind(
    card: &PersonaCard,
    has_thumbnail: bool,
) -> Result<(), PersistenceError> {
    if has_thumbnail && card.asset_kind != PersonaAssetKind::Card {
        Err(PersistenceError::InvalidPersonaAsset)
    } else {
        Ok(())
    }
}

fn valid_png(content: &[u8]) -> bool {
    !content.is_empty()
        && content.len() <= MAX_ATTACHMENT_BYTES
        && content.starts_with(PNG_SIGNATURE)
}

fn valid_persona_id(persona_id: &str) -> bool {
    persona_id.chars().count() <= MAX_PERSONA_ID_CHARACTERS
        && canonical_persona_id(persona_id) == persona_id
}

fn persona_not_found() -> PersistenceError {
    PersistenceError::CommandRejected {
        code: "persona_not_found",
        message: "The selected bot card or module is unavailable.".to_owned(),
    }
}

const fn asset_order(kind: PersonaAssetKind) -> u8 {
    match kind {
        PersonaAssetKind::Card => 0,
        PersonaAssetKind::Module => 1,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use agentsassemble_domain::{PersonaAssetKind, PersonaLoreSettings};

    use super::*;

    #[tokio::test]
    async fn replacement_is_atomic_sorted_and_durable_without_orphan_thumbnail() {
        let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
        let path = directory.path().join("runtime.sqlite3");
        let store = SqliteStore::open_path(&path)
            .await
            .unwrap_or_else(|error| panic!("open store: {error}"));
        store
            .replace_persona_asset(imported(
                card("guide", "Zulu", PersonaAssetKind::Card),
                Some(png()),
            ))
            .await
            .unwrap_or_else(|error| panic!("store card: {error}"));
        store
            .replace_persona_asset(imported(
                card("module", "Alpha", PersonaAssetKind::Module),
                None,
            ))
            .await
            .unwrap_or_else(|error| panic!("store module: {error}"));

        let mut invalid = card("guide", "must not replace", PersonaAssetKind::Card);
        invalid.description = "private failed replacement".to_owned();
        assert!(
            store
                .replace_persona_asset(imported(invalid, Some(vec![1, 2, 3])))
                .await
                .is_err()
        );
        assert_eq!(
            store
                .persona_asset("guide")
                .await
                .unwrap_or_else(|error| panic!("load retained card: {error}"))
                .display_name,
            "Zulu"
        );

        let items = store
            .persona_assets()
            .await
            .unwrap_or_else(|error| panic!("list assets: {error}"));
        assert_eq!(
            items
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            ["guide", "module"]
        );
        assert!(items[0].thumbnail_url.ends_with("/guide/thumbnail"));
        assert!(
            store
                .persona_thumbnail("guide")
                .await
                .unwrap_or_else(|error| panic!("load thumbnail: {error}"))
                .starts_with(PNG_SIGNATURE)
        );

        store
            .replace_persona_asset(imported(
                card("guide", "Beta", PersonaAssetKind::Card),
                None,
            ))
            .await
            .unwrap_or_else(|error| panic!("replace card: {error}"));
        assert!(matches!(
            store.persona_thumbnail("guide").await,
            Err(PersistenceError::PersonaAssetMissing)
        ));
        drop(store);

        let reopened = SqliteStore::open_path(&path)
            .await
            .unwrap_or_else(|error| panic!("reopen store: {error}"));
        assert_eq!(
            reopened
                .persona_asset("guide")
                .await
                .unwrap_or_else(|error| panic!("load reopened card: {error}"))
                .display_name,
            "Beta"
        );
        assert_eq!(
            reopened
                .persona_assets()
                .await
                .unwrap_or_else(|error| panic!("list reopened assets: {error}"))
                .len(),
            2
        );
    }

    fn imported(card: PersonaCard, thumbnail: Option<Vec<u8>>) -> ImportedPersonaAsset {
        ImportedPersonaAsset { card, thumbnail }
    }

    fn card(id: &str, display_name: &str, asset_kind: PersonaAssetKind) -> PersonaCard {
        PersonaCard {
            id: id.to_owned(),
            display_name: display_name.to_owned(),
            description: "private description".to_owned(),
            system_prompt: String::new(),
            personality: String::new(),
            scenario: String::new(),
            first_message: String::new(),
            example_messages: String::new(),
            post_history_instructions: String::new(),
            lorebook: Vec::new(),
            lore_settings: PersonaLoreSettings::default(),
            asset_kind,
            source_kind: "fixture".to_owned(),
            asset_count: 0,
            ignored_features: BTreeMap::new(),
            tag_count: 0,
        }
    }

    fn png() -> Vec<u8> {
        let mut output = std::io::Cursor::new(Vec::new());
        {
            let mut encoder = png::Encoder::new(&mut output, 1, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder
                .write_header()
                .unwrap_or_else(|error| panic!("write PNG header: {error}"));
            writer
                .write_image_data(&[1, 2, 3, 255])
                .unwrap_or_else(|error| panic!("write PNG pixel: {error}"));
        }
        output.into_inner()
    }
}
