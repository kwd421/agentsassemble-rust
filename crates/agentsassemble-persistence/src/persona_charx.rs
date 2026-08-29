use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Cursor, Read},
};

use agentsassemble_domain::{MAX_ATTACHMENT_BYTES, PersonaLoreSettings};
use serde_json::Value;
use zip::ZipArchive;

use crate::{
    persona_import::{
        ImportedPersonaAsset, PersonaImportError, add_count, finish_ccv3_import, safe_embedded_path,
    },
    persona_risu::decode_risum_module,
};

const MAX_ENTRY_COUNT: usize = 512;
const MAX_TOTAL_EXPANDED_BYTES: u64 = 80 * 1024 * 1024;
const MAX_COMPRESSION_RATIO: u64 = 200;
const MAX_CARD_BYTES: usize = 5 * 1024 * 1024;

/// Parses one bounded `CHARX` archive without extracting paths onto the host filesystem.
///
/// # Errors
///
/// Rejects malformed archives, unsafe or duplicate paths, encrypted entries, and bounded
/// expansion violations. A malformed optional Risu module remains inert on a valid card.
pub async fn import_charx_asset(
    filename: &str,
    content: &[u8],
) -> Result<ImportedPersonaAsset, PersonaImportError> {
    if content.is_empty() || content.len() > MAX_ATTACHMENT_BYTES {
        return Err(PersonaImportError::InvalidSize);
    }
    let mut archive =
        ZipArchive::new(Cursor::new(content)).map_err(|_| invalid("ZIP is invalid"))?;
    validate_archive(&mut archive)?;
    let card_bytes = read_member(&mut archive, "card.json", MAX_CARD_BYTES)?;
    let payload: Value = serde_json::from_slice(&card_bytes)
        .map_err(|_| PersonaImportError::InvalidCard("card JSON is invalid"))?;
    if !payload.is_object() {
        return Err(PersonaImportError::InvalidCard(
            "card root must be an object",
        ));
    }
    let referenced = referenced_asset_paths(&payload);
    let mut embedded = BTreeMap::new();
    for path in referenced {
        if archive.index_for_name(&path).is_some() {
            embedded.insert(
                path.clone(),
                read_member(&mut archive, &path, MAX_ATTACHMENT_BYTES)?,
            );
        }
    }
    let module = if archive.index_for_name("module.risum").is_some() {
        Some(
            read_member(&mut archive, "module.risum", MAX_ATTACHMENT_BYTES)
                .and_then(|bytes| decode_risum_module("module.risum", &bytes)),
        )
    } else {
        None
    };
    let mut imported = finish_ccv3_import(filename, &payload, embedded, None).await?;
    if let Some(module) = module {
        match module {
            Ok(module) => {
                if module.lorebook_present {
                    imported.card.lorebook = module.card.lorebook;
                    imported.card.lore_settings = PersonaLoreSettings::default();
                    imported
                        .card
                        .ignored_features
                        .remove("lorebook_regex_matching");
                }
                for (name, count) in module.card.ignored_features {
                    imported
                        .card
                        .ignored_features
                        .entry(name)
                        .and_modify(|value| *value = value.saturating_add(count))
                        .or_insert(count);
                }
            }
            Err(_) => add_count(
                &mut imported.card.ignored_features,
                "embedded_module_unreadable",
                1,
            ),
        }
    }
    Ok(imported)
}

fn validate_archive(archive: &mut ZipArchive<Cursor<&[u8]>>) -> Result<(), PersonaImportError> {
    if archive.len() > MAX_ENTRY_COUNT {
        return Err(invalid("archive contains too many entries"));
    }
    if archive
        .has_overlapping_files()
        .map_err(|_| invalid("entry ranges are invalid"))?
    {
        return Err(invalid("entry ranges overlap"));
    }
    let mut names = BTreeSet::new();
    let mut total_expanded = 0_u64;
    for index in 0..archive.len() {
        let entry = archive
            .by_index_raw(index)
            .map_err(|_| invalid("entry metadata is invalid"))?;
        let name = entry.name();
        let path = if entry.is_dir() {
            name.strip_suffix('/').unwrap_or(name)
        } else {
            name
        };
        if safe_embedded_path(path).is_none() {
            return Err(invalid("entry path is unsafe"));
        }
        if !names.insert(path.to_owned()) {
            return Err(invalid("archive contains duplicate entry paths"));
        }
        if entry.encrypted() {
            return Err(invalid("encrypted entries are unsupported"));
        }
        let size = usize::try_from(entry.size()).map_err(|_| invalid("entry size is invalid"))?;
        if size > MAX_ATTACHMENT_BYTES {
            return Err(invalid("entry exceeds 10 MiB"));
        }
        let size_u64 = u64::try_from(size).map_err(|_| invalid("entry size is invalid"))?;
        total_expanded = total_expanded
            .checked_add(size_u64)
            .ok_or_else(|| invalid("expanded size overflows"))?;
        if total_expanded > MAX_TOTAL_EXPANDED_BYTES {
            return Err(invalid("archive expands beyond 80 MiB"));
        }
        let compressed = entry.compressed_size();
        if size_u64 > 0
            && (compressed == 0 || size_u64 > compressed.saturating_mul(MAX_COMPRESSION_RATIO))
        {
            return Err(invalid("entry exceeds the compression-ratio limit"));
        }
    }
    if !names.contains("card.json") {
        return Err(invalid("card.json is missing from the archive root"));
    }
    Ok(())
}

fn read_member(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    name: &str,
    limit: usize,
) -> Result<Vec<u8>, PersonaImportError> {
    let mut entry = archive
        .by_name(name)
        .map_err(|_| invalid("required entry cannot be read"))?;
    let declared = usize::try_from(entry.size()).map_err(|_| invalid("entry size is invalid"))?;
    if declared > limit {
        return Err(invalid("entry exceeds its read limit"));
    }
    let mut content = Vec::with_capacity(declared);
    entry
        .by_ref()
        .take(u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1))
        .read_to_end(&mut content)
        .map_err(|_| invalid("entry decompression failed"))?;
    if content.len() != declared || content.len() > limit {
        return Err(invalid("entry length does not match its metadata"));
    }
    Ok(content)
}

fn referenced_asset_paths(payload: &Value) -> BTreeSet<String> {
    payload
        .get("data")
        .and_then(Value::as_object)
        .and_then(|data| data.get("assets"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .filter_map(|asset| asset.get("uri").and_then(Value::as_str))
        .filter_map(|uri| uri.strip_prefix("embeded://"))
        .filter_map(safe_embedded_path)
        .collect()
}

const fn invalid(message: &'static str) -> PersonaImportError {
    PersonaImportError::InvalidArchive(message)
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Write};

    use serde_json::json;
    use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

    use super::import_charx_asset;
    use crate::persona_risu::RPACK_DECODE;

    #[tokio::test]
    async fn imports_charx_thumbnail_and_embedded_risu_lore() {
        let icon = png_icon();
        let card = json!({
            "spec": "chara_card_v3",
            "spec_version": "3.0",
            "data": {
                "name": "Archive Guide",
                "character_book": {"entries": [{"key": "old", "content": "old lore"}]},
                "assets": [{"type": "icon", "uri": "embeded://assets/icon.png", "name": "main", "ext": "png"}]
            }
        });
        let module = risu_module();
        let archive = zip(&[
            ("card.json", card.to_string().as_bytes()),
            ("assets/icon.png", &icon),
            ("module.risum", &module),
        ]);
        let imported = import_charx_asset("guide.charx", &archive)
            .await
            .unwrap_or_else(|error| panic!("import CHARX: {error}"));
        assert_eq!(imported.card.id, "Archive-Guide");
        assert_eq!(imported.card.lorebook[0].key, "harbor");
        assert_eq!(imported.card.ignored_features["regex"], 1);
        assert_eq!(imported.card.asset_count, 1);
        assert!(imported.thumbnail.is_some());
    }

    #[tokio::test]
    async fn rejects_archive_traversal_before_reading_card() {
        let archive = zip(&[("../card.json", b"{}")]);
        let Err(error) = import_charx_asset("unsafe.charx", &archive).await else {
            panic!("unsafe archive path must fail");
        };
        assert!(error.to_string().contains("entry path is unsafe"));
    }

    #[tokio::test]
    async fn embedded_module_without_lore_keeps_card_lore() {
        let card = json!({
            "spec": "chara_card_v3",
            "spec_version": "3.0",
            "data": {
                "name": "Archive Guide",
                "character_book": {"entries": [
                    {"key": ".*", "content": "card lore", "use_regex": true}
                ]}
            }
        });
        let module = risu_module_from(&json!({
            "type": "risuModule",
            "module": {"name": "module", "regex": [{"in": ".*"}]}
        }));
        let archive = zip(&[
            ("card.json", card.to_string().as_bytes()),
            ("module.risum", &module),
        ]);

        let imported = import_charx_asset("guide.charx", &archive)
            .await
            .unwrap_or_else(|error| panic!("import CHARX: {error}"));

        assert_eq!(imported.card.lorebook[0].content, "card lore");
        assert_eq!(imported.card.ignored_features["lorebook_regex_matching"], 1);
        assert_eq!(imported.card.ignored_features["regex"], 1);
    }

    fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for (name, content) in entries {
            writer
                .start_file(*name, options)
                .unwrap_or_else(|error| panic!("start ZIP entry: {error}"));
            writer
                .write_all(content)
                .unwrap_or_else(|error| panic!("write ZIP entry: {error}"));
        }
        writer
            .finish()
            .unwrap_or_else(|error| panic!("finish ZIP: {error}"))
            .into_inner()
    }

    fn png_icon() -> Vec<u8> {
        let mut output = Cursor::new(Vec::new());
        {
            let mut encoder = png::Encoder::new(&mut output, 1, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder
                .write_header()
                .unwrap_or_else(|error| panic!("write PNG header: {error}"));
            writer
                .write_image_data(&[0, 0, 0, 255])
                .unwrap_or_else(|error| panic!("write PNG pixel: {error}"));
        }
        output.into_inner()
    }

    fn risu_module() -> Vec<u8> {
        risu_module_from(&json!({
            "type": "risuModule",
            "module": {
                "name": "module",
                "lorebook": [{"key": "harbor", "content": "module lore"}],
                "regex": [{"in": ".*"}]
            }
        }))
    }

    fn risu_module_from(payload: &serde_json::Value) -> Vec<u8> {
        let json = serde_json::to_vec(payload)
            .unwrap_or_else(|error| panic!("serialize Risu fixture: {error}"));
        let encoded = rpack_encode(&json);
        let length = u32::try_from(encoded.len())
            .unwrap_or_else(|error| panic!("fixture length fits u32: {error}"));
        let mut module = vec![111, 0];
        module.extend_from_slice(&length.to_le_bytes());
        module.extend_from_slice(&encoded);
        module.push(0);
        module
    }

    fn rpack_encode(content: &[u8]) -> Vec<u8> {
        let mut inverse = [0_u8; 256];
        for (encoded, decoded) in RPACK_DECODE.iter().copied().enumerate() {
            inverse[usize::from(decoded)] = u8::try_from(encoded).unwrap_or_default();
        }
        content
            .iter()
            .map(|byte| inverse[usize::from(*byte)])
            .collect()
    }
}
