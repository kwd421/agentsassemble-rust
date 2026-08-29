use std::collections::BTreeMap;

use agentsassemble_domain::{
    MAX_ATTACHMENT_BYTES, PersonaAssetKind, PersonaCard, PersonaLoreEntry, PersonaLoreSettings,
    canonical_persona_id,
};
use serde_json::{Map, Value};

use crate::persona_import::{
    ImportedPersonaAsset, PersonaImportError, add_count, boolean, integer, nonempty_text,
    normalize_lore, raw_text, text, truthy,
};

const RISU_MAGIC: u8 = 111;
const RISU_VERSION: u8 = 0;
const ASSET_MARKER: u8 = 1;
const EOF_MARKER: u8 = 0;
const MAX_MAIN_BYTES: usize = 5 * 1024 * 1024;
const MAX_ASSET_BYTES: usize = 8 * 1024 * 1024;
const MAX_ASSET_COUNT: usize = 256;

// RPack v0's byte permutation is wire-format data. Its inverse was verified against the
// official 512-byte encode/decode map (SHA-256 428e939c41617140ef2fad0420d9163cc80ce7c9b2f5e620223a25acc8afd498).
const RPACK_DECODE: [u8; 256] = [
    0x2c, 0xf7, 0x84, 0x8b, 0xc9, 0x65, 0xfb, 0xb6, 0x9f, 0xae, 0xb3, 0x03, 0x2d, 0x01, 0x69, 0x74,
    0x1f, 0xe4, 0xa3, 0xec, 0xee, 0x5c, 0x34, 0x21, 0x93, 0x4a, 0x0f, 0x6a, 0xe2, 0x62, 0x02, 0x9e,
    0x22, 0x9c, 0xfd, 0x3c, 0xfc, 0x71, 0xc7, 0xc6, 0xad, 0x59, 0x67, 0x05, 0x70, 0x6d, 0x8a, 0x44,
    0x12, 0xfa, 0x24, 0x86, 0x5f, 0xaf, 0xd1, 0x7a, 0x47, 0xce, 0xfe, 0x50, 0x63, 0xdd, 0x51, 0x06,
    0x6f, 0x18, 0xe0, 0x52, 0xa8, 0x09, 0x9d, 0x56, 0x73, 0x4c, 0xb8, 0x53, 0x6c, 0xc3, 0xa0, 0x0e,
    0x19, 0xcf, 0x3e, 0x0d, 0x7e, 0x07, 0x32, 0x68, 0x46, 0xea, 0x48, 0xf9, 0x99, 0x2e, 0xab, 0xa4,
    0x49, 0x20, 0x5e, 0x55, 0x35, 0x38, 0x0c, 0xbc, 0xd3, 0xb1, 0x58, 0x16, 0x79, 0x28, 0x0a, 0x1a,
    0xe1, 0xf2, 0xcd, 0xc4, 0x39, 0xdb, 0xa2, 0xba, 0x60, 0x72, 0x76, 0x7d, 0x95, 0xef, 0x7f, 0xc8,
    0xc0, 0xde, 0x37, 0x94, 0xbf, 0xb5, 0x14, 0x81, 0x92, 0x25, 0x45, 0xac, 0xe7, 0xf5, 0x66, 0xa7,
    0x2b, 0x36, 0x5a, 0xc1, 0x13, 0xe3, 0x4b, 0x3a, 0xe8, 0x8d, 0x83, 0x1b, 0x7c, 0x27, 0xb0, 0x9a,
    0x42, 0xeb, 0x87, 0xaa, 0xdc, 0x54, 0x8e, 0x78, 0x26, 0xd2, 0x57, 0x29, 0xd4, 0xb7, 0xf8, 0x2f,
    0x8f, 0x89, 0x75, 0xf0, 0x41, 0x77, 0xc2, 0x1e, 0xff, 0xd8, 0x15, 0x11, 0xe5, 0x04, 0x97, 0x17,
    0xf3, 0x31, 0xd0, 0x9b, 0x00, 0xd7, 0xca, 0xb4, 0x4f, 0x2a, 0x3b, 0xd9, 0xb2, 0x6b, 0xda, 0x5d,
    0xa1, 0x3f, 0x30, 0x61, 0xbd, 0x91, 0x3d, 0x4e, 0xe6, 0xdf, 0xbe, 0x4d, 0x82, 0x8c, 0x1d, 0x23,
    0x10, 0x98, 0x64, 0xf4, 0x85, 0x33, 0x7b, 0x90, 0x43, 0xbb, 0xa9, 0x88, 0xf1, 0xd6, 0xa5, 0x1c,
    0xf6, 0xcc, 0x6e, 0xb9, 0x5b, 0x0b, 0x96, 0xed, 0xd5, 0xe9, 0xc5, 0xcb, 0x08, 0xa6, 0x80, 0x40,
];

/// Parses a standalone Risu module without filesystem or environment lookups.
///
/// # Errors
///
/// Rejects unsupported, malformed, oversized, or ambiguously terminated modules.
pub fn import_risum_asset(
    filename: &str,
    content: &[u8],
) -> Result<ImportedPersonaAsset, PersonaImportError> {
    let card = decode_risum_card(filename, content)?;
    Ok(ImportedPersonaAsset {
        card,
        thumbnail: None,
    })
}

pub(super) fn decode_risum_card(
    source_name: &str,
    content: &[u8],
) -> Result<PersonaCard, PersonaImportError> {
    if content.is_empty() || content.len() > MAX_ATTACHMENT_BYTES {
        return Err(PersonaImportError::InvalidSize);
    }
    if content.len() < 7 || content[0] != RISU_MAGIC {
        return Err(invalid("magic byte is invalid"));
    }
    if content[1] != RISU_VERSION {
        return Err(invalid("version is unsupported"));
    }
    let mut offset = 2;
    let main_length = read_length(content, &mut offset)?;
    if main_length > MAX_MAIN_BYTES {
        return Err(invalid("main payload exceeds 5 MiB"));
    }
    let encoded = read_record(content, &mut offset, main_length)?;
    let decoded = encoded
        .iter()
        .map(|byte| RPACK_DECODE[usize::from(*byte)])
        .collect::<Vec<_>>();
    let root: Value = serde_json::from_slice(&decoded).map_err(|_| invalid("JSON is invalid"))?;
    let root = root
        .as_object()
        .ok_or_else(|| invalid("payload root must be an object"))?;
    if text(root.get("type")) != "risuModule" {
        return Err(invalid("payload type must be risuModule"));
    }
    let module = root
        .get("module")
        .and_then(Value::as_object)
        .ok_or_else(|| invalid("module must be an object"))?;
    let asset_records = validate_asset_records(content, &mut offset)?;
    Ok(normalize_module(module, source_name, asset_records))
}

fn validate_asset_records(content: &[u8], offset: &mut usize) -> Result<usize, PersonaImportError> {
    let mut asset_count = 0_usize;
    let mut asset_bytes = 0_usize;
    loop {
        let marker = *content
            .get(*offset)
            .ok_or_else(|| invalid("EOF marker is missing"))?;
        *offset += 1;
        if marker == EOF_MARKER {
            if *offset != content.len() {
                return Err(invalid("trailing bytes follow the EOF marker"));
            }
            return Ok(asset_count);
        }
        if marker != ASSET_MARKER {
            return Err(invalid("asset record marker is invalid"));
        }
        if asset_count >= MAX_ASSET_COUNT {
            return Err(invalid("too many asset records"));
        }
        let length = read_length(content, offset)?;
        asset_bytes = asset_bytes
            .checked_add(length)
            .ok_or_else(|| invalid("asset sizes overflow"))?;
        if asset_bytes > MAX_ASSET_BYTES {
            return Err(invalid("asset records exceed 8 MiB"));
        }
        let _ = read_record(content, offset, length)?;
        asset_count += 1;
    }
}

fn normalize_module(
    module: &Map<String, Value>,
    source_name: &str,
    asset_records: usize,
) -> PersonaCard {
    let fallback_name = source_name
        .rsplit_once('.')
        .map_or(source_name, |(stem, _)| stem);
    let display_name = nonempty_text(module.get("name"))
        .or_else(|| (!fallback_name.is_empty()).then(|| fallback_name.to_owned()))
        .unwrap_or_else(|| "Persona".to_owned());
    let id_source = nonempty_text(module.get("id")).unwrap_or_else(|| display_name.clone());
    let lorebook: Vec<PersonaLoreEntry> = module
        .get("lorebook")
        .and_then(Value::as_array)
        .map(|entries| entries.iter().filter_map(normalize_lore).collect())
        .unwrap_or_default();
    let declared_assets = module
        .get("assets")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    PersonaCard {
        id: canonical_persona_id(&id_source),
        display_name,
        description: text(module.get("description")),
        system_prompt: first_text(module, "systemPrompt", "system_prompt"),
        personality: text(module.get("personality")),
        scenario: text(module.get("scenario")),
        first_message: first_text(module, "firstMessage", "first_message"),
        example_messages: first_text(module, "exampleMessage", "example_messages"),
        post_history_instructions: first_text(
            module,
            "postHistoryInstructions",
            "post_history_instructions",
        ),
        lore_settings: module_lore_settings(module),
        ignored_features: ignored_module_features(module, &lorebook),
        lorebook,
        asset_kind: PersonaAssetKind::Module,
        source_kind: "risu_module".to_owned(),
        asset_count: declared_assets.max(asset_records),
        tag_count: 0,
    }
}

fn module_lore_settings(module: &Map<String, Value>) -> PersonaLoreSettings {
    PersonaLoreSettings {
        scan_depth: usize::try_from(last_integer(module, "scanDepth", "scan_depth"))
            .unwrap_or_default(),
        recursive_scanning: last_boolean(module, "recursiveScanning", "recursive_scanning"),
        full_word_matching: last_boolean(module, "fullWordMatching", "full_word_matching"),
    }
}

fn ignored_module_features(
    module: &Map<String, Value>,
    lorebook: &[PersonaLoreEntry],
) -> BTreeMap<String, u32> {
    let mut ignored = BTreeMap::new();
    for name in ["regex", "trigger"] {
        if let Some(count) = module.get(name).and_then(Value::as_array).map(Vec::len) {
            add_count(&mut ignored, name, count);
        }
    }
    if !raw_text(module.get("cjs")).is_empty() {
        add_count(&mut ignored, "cjs", 1);
    }
    for name in ["lowLevelAccess", "customModuleToggle", "mcp"] {
        if module.get(name).is_some_and(truthy) {
            add_count(&mut ignored, name, 1);
        }
    }
    add_count(
        &mut ignored,
        "lorebook_regex_matching",
        lorebook.iter().filter(|entry| entry.use_regex).count(),
    );
    ignored
}

fn first_text(module: &Map<String, Value>, primary: &str, secondary: &str) -> String {
    nonempty_text(module.get(primary)).unwrap_or_else(|| text(module.get(secondary)))
}

fn last_integer(module: &Map<String, Value>, primary: &str, secondary: &str) -> i64 {
    module.get(secondary).map_or_else(
        || integer(module.get(primary)),
        |value| integer(Some(value)),
    )
}

fn last_boolean(module: &Map<String, Value>, primary: &str, secondary: &str) -> bool {
    module.get(secondary).map_or_else(
        || boolean(module.get(primary)),
        |value| boolean(Some(value)),
    )
}

fn read_length(content: &[u8], offset: &mut usize) -> Result<usize, PersonaImportError> {
    let end = offset
        .checked_add(4)
        .ok_or_else(|| invalid("record offset overflowed"))?;
    let bytes: [u8; 4] = content
        .get(*offset..end)
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| invalid("record length is truncated"))?;
    *offset = end;
    Ok(u32::from_le_bytes(bytes) as usize)
}

fn read_record<'a>(
    content: &'a [u8],
    offset: &mut usize,
    length: usize,
) -> Result<&'a [u8], PersonaImportError> {
    let end = offset
        .checked_add(length)
        .ok_or_else(|| invalid("record offset overflowed"))?;
    let record = content
        .get(*offset..end)
        .ok_or_else(|| invalid("record body is truncated"))?;
    *offset = end;
    Ok(record)
}

const fn invalid(message: &'static str) -> PersonaImportError {
    PersonaImportError::InvalidModule(message)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{RPACK_DECODE, import_risum_asset};

    #[test]
    fn imports_bounded_risu_module_without_executing_runtime_features() {
        let payload = json!({
            "type": "risuModule",
            "module": {
                "id": "guide/module",
                "name": "Risu Guide",
                "description": "Keeps watch.",
                "lorebook": [
                    {"key": "harbor", "content": "The bell rings."},
                    {"key": ".*", "content": "must stay inert", "useRegex": true}
                ],
                "assets": [["icon", "", "png"]],
                "regex": [{"in": ".*"}],
                "trigger": [{"type": "manual"}],
                "cjs": "private code",
                "mcp": {"server": "private"}
            }
        });
        let json = serde_json::to_vec(&payload)
            .unwrap_or_else(|error| panic!("serialize module fixture: {error}"));
        let encoded = encode(&json);
        let mut module = vec![111, 0];
        let encoded_length = u32::try_from(encoded.len())
            .unwrap_or_else(|error| panic!("fixture length fits u32: {error}"));
        module.extend_from_slice(&encoded_length.to_le_bytes());
        module.extend_from_slice(&encoded);
        module.push(1);
        module.extend_from_slice(&3_u32.to_le_bytes());
        module.extend_from_slice(&encode(&[1, 2, 3]));
        module.push(0);

        let imported = import_risum_asset("guide.risum", &module)
            .unwrap_or_else(|error| panic!("import Risu module: {error}"));
        assert_eq!(imported.card.id, "guide-module");
        assert_eq!(imported.card.asset_count, 1);
        assert_eq!(imported.card.ignored_features["regex"], 1);
        assert_eq!(imported.card.ignored_features["trigger"], 1);
        assert_eq!(imported.card.ignored_features["cjs"], 1);
        assert_eq!(imported.card.ignored_features["mcp"], 1);
        assert_eq!(imported.card.ignored_features["lorebook_regex_matching"], 1);
        assert!(imported.thumbnail.is_none());
    }

    fn encode(content: &[u8]) -> Vec<u8> {
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
