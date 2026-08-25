use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::SystemTime,
};

use serde_json::Value;

use crate::{antigravity::clean_identifier, runtime::DriverError};

const MAX_MESSAGE_CHARS: usize = 12_000;
const MAX_TRANSCRIPT_CANDIDATES: usize = 20;
const MAX_CACHE_BYTES: u64 = 64 * 1024;
const MAX_TURN_TRANSCRIPT_BYTES: usize = 2 * 1024 * 1024;
const MAX_JSONL_LINE_BYTES: usize = 256 * 1024;
const MAX_JSONL_EVENTS: usize = 4_096;
const TRUNCATION_SUFFIX_CHARS: usize = 512;
const TRUNCATION_MIN_ANCHOR_CHARS: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TranscriptSnapshot {
    pub(super) content: String,
    pub(super) observed_model_id: String,
    pub(super) provider_session_id: String,
}

pub(super) struct AntigravityTranscript {
    home: PathBuf,
    workspace: PathBuf,
    offsets: HashMap<PathBuf, u64>,
    ignored_existing_paths: HashSet<PathBuf>,
    active_paths: HashSet<PathBuf>,
    turn_input_seen_paths: HashSet<PathBuf>,
    bound_path: Option<PathBuf>,
    expected_turn_input: String,
    observed_models: HashMap<PathBuf, String>,
}

impl AntigravityTranscript {
    pub(super) fn new(home: PathBuf, workspace: PathBuf) -> Self {
        Self {
            home,
            workspace,
            offsets: HashMap::new(),
            ignored_existing_paths: HashSet::new(),
            active_paths: HashSet::new(),
            turn_input_seen_paths: HashSet::new(),
            bound_path: None,
            expected_turn_input: String::new(),
            observed_models: HashMap::new(),
        }
    }

    pub(super) fn prepare_start(
        &mut self,
        provider_session_id: Option<&str>,
    ) -> Result<(), DriverError> {
        self.offsets.clear();
        self.active_paths.clear();
        self.turn_input_seen_paths.clear();
        self.expected_turn_input.clear();
        self.observed_models.clear();
        if let Some(provider_session_id) = provider_session_id {
            let bound = self.transcript_path(provider_session_id)?;
            if !bound.is_file() {
                return Err(transcript_error());
            }
            self.bound_path = Some(bound);
            self.ignored_existing_paths.clear();
        } else {
            self.bound_path = None;
            self.ignored_existing_paths = self.candidate_paths()?.into_iter().collect();
        }
        Ok(())
    }

    pub(super) fn begin_turn(&mut self, expected_input: &str) -> Result<(), DriverError> {
        self.offsets.clear();
        self.turn_input_seen_paths.clear();
        self.expected_turn_input = normalize_turn_input(expected_input);
        for path in self.visible_candidate_paths()? {
            self.offsets.insert(path.clone(), file_size(&path));
        }
        Ok(())
    }

    pub(super) fn poll(&mut self) -> Result<Option<TranscriptSnapshot>, DriverError> {
        let mut latest = None;
        for path in self.visible_candidate_paths()? {
            let start = self.offsets.get(&path).copied().unwrap_or_default();
            let (text, next_offset) = read_complete_jsonl(&path, start)?;
            self.offsets.insert(path.clone(), next_offset);
            if text.is_empty() {
                continue;
            }
            let scoped = if self.turn_input_seen_paths.contains(&path) {
                text.as_str()
            } else {
                let Some(offset) = self.turn_input_offset(&text, &path) else {
                    continue;
                };
                self.turn_input_seen_paths.insert(path.clone());
                &text[offset..]
            };
            if self.bound_path.as_ref().is_some_and(|bound| bound != &path) {
                continue;
            }
            self.active_paths.insert(path.clone());
            self.observe_model(&path, &text);
            if let Some(content) = final_message(scoped) {
                latest = Some((path, content));
            }
        }
        if self.bound_path.is_none() && self.turn_input_seen_paths.len() > 1 {
            return Err(transcript_error());
        }
        let Some((path, content)) = latest else {
            return Ok(None);
        };
        if self.bound_path.is_none() {
            self.bound_path = Some(path.clone());
        }
        if self.bound_path.as_ref() != Some(&path) {
            return Ok(None);
        }
        Ok(Some(TranscriptSnapshot {
            content,
            observed_model_id: self.observed_models.get(&path).cloned().unwrap_or_default(),
            provider_session_id: provider_session_id_from_path(&path)?,
        }))
    }

    fn visible_candidate_paths(&self) -> Result<Vec<PathBuf>, DriverError> {
        if let Some(bound) = &self.bound_path {
            return Ok(if bound.is_file() {
                vec![bound.clone()]
            } else {
                Vec::new()
            });
        }
        Ok(self
            .candidate_paths()?
            .into_iter()
            .filter(|path| {
                !self.ignored_existing_paths.contains(path) || self.active_paths.contains(path)
            })
            .collect())
    }

    fn candidate_paths(&self) -> Result<Vec<PathBuf>, DriverError> {
        let root = self.home.join(".gemini/antigravity-cli/brain");
        let mut paths = match std::fs::read_dir(&root) {
            Ok(entries) => entries
                .filter_map(Result::ok)
                .map(|entry| entry.path().join(".system_generated/logs/transcript.jsonl"))
                .filter(|path| path.is_file())
                .collect::<Vec<_>>(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(_) => return Err(transcript_error()),
        };
        paths.sort_by(|left, right| {
            modified_time(right)
                .cmp(&modified_time(left))
                .then_with(|| right.cmp(left))
        });
        paths.truncate(MAX_TRANSCRIPT_CANDIDATES);
        if let Some(preferred) = self.preferred_transcript(&root)?
            && preferred.is_file()
            && paths.first() != Some(&preferred)
        {
            paths.retain(|path| path != &preferred);
            paths.insert(0, preferred);
            paths.truncate(MAX_TRANSCRIPT_CANDIDATES);
        }
        Ok(paths)
    }

    fn preferred_transcript(&self, root: &Path) -> Result<Option<PathBuf>, DriverError> {
        let cache = self
            .home
            .join(".gemini/antigravity-cli/cache/last_conversations.json");
        let mut document = String::new();
        let file = match File::open(cache) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(transcript_error()),
        };
        if file.metadata().map_err(|_| transcript_error())?.len() > MAX_CACHE_BYTES {
            return Err(transcript_error());
        }
        file.take(MAX_CACHE_BYTES + 1)
            .read_to_string(&mut document)
            .map_err(|_| transcript_error())?;
        if document.len() as u64 > MAX_CACHE_BYTES {
            return Err(transcript_error());
        }
        let payload: Value = serde_json::from_str(&document).map_err(|_| transcript_error())?;
        let Some(id) = payload
            .as_object()
            .and_then(|entries| entries.get(self.workspace.to_string_lossy().as_ref()))
            .and_then(Value::as_str)
            .map(clean_identifier)
            .filter(|id| !id.is_empty())
        else {
            return Ok(None);
        };
        Ok(Some(
            root.join(id)
                .join(".system_generated/logs/transcript.jsonl"),
        ))
    }

    fn transcript_path(&self, provider_session_id: &str) -> Result<PathBuf, DriverError> {
        let provider_session_id = clean_identifier(provider_session_id);
        if provider_session_id.is_empty() {
            return Err(transcript_error());
        }
        Ok(self
            .home
            .join(".gemini/antigravity-cli/brain")
            .join(provider_session_id)
            .join(".system_generated/logs/transcript.jsonl"))
    }

    fn turn_input_offset(&self, text: &str, path: &Path) -> Option<usize> {
        let bound = self.bound_path.as_deref() == Some(path);
        let mut offset = 0;
        for line in text.split_inclusive('\n') {
            let inputs = turn_inputs(line);
            let exact = inputs.iter().any(|input| {
                turn_input_matches(&self.expected_turn_input, &normalize_turn_input(input))
            });
            if exact
                || (bound
                    && inputs
                        .iter()
                        .any(|input| !normalize_turn_input(input).is_empty()))
            {
                return Some(offset);
            }
            offset += line.len();
        }
        None
    }

    fn observe_model(&mut self, path: &Path, text: &str) {
        for line in text.lines() {
            let Ok(entry) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            let Some(content) = entry.get("content").and_then(Value::as_str) else {
                continue;
            };
            if let Some(model) = selected_model(content) {
                self.observed_models.insert(path.to_path_buf(), model);
            }
        }
    }
}

fn final_message(text: &str) -> Option<String> {
    let mut latest = None;
    for line in text.lines() {
        let Ok(entry) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if entry.get("source").and_then(Value::as_str) != Some("MODEL")
            || entry.get("type").and_then(Value::as_str) != Some("PLANNER_RESPONSE")
            || entry
                .get("tool_calls")
                .and_then(Value::as_array)
                .is_some_and(|calls| !calls.is_empty())
            || entry
                .get("status")
                .and_then(Value::as_str)
                .is_some_and(|status| !status.is_empty() && status != "DONE")
        {
            continue;
        }
        let content = entry
            .get("content")
            .and_then(Value::as_str)
            .map(|value| truncate_chars(value.trim(), MAX_MESSAGE_CHARS))
            .unwrap_or_default();
        if !content.is_empty() {
            latest = Some(content);
        }
    }
    latest
}

fn turn_inputs(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|entry| {
            entry.get("source").and_then(Value::as_str) == Some("USER_EXPLICIT")
                || entry.get("type").and_then(Value::as_str) == Some("USER_INPUT")
        })
        .filter_map(|entry| {
            entry
                .get("content")
                .and_then(Value::as_str)
                .map(user_request)
        })
        .collect()
}

fn user_request(content: &str) -> String {
    let body = tagged_body(content, "USER_REQUEST");
    let body = body.trim();
    let body = body
        .strip_prefix("/plan ")
        .or_else(|| body.strip_prefix("/PLAN "))
        .unwrap_or(body);
    body.strip_suffix(" /plan")
        .or_else(|| body.strip_suffix(" /PLAN"))
        .unwrap_or(body)
        .trim()
        .to_owned()
}

fn turn_input_matches(expected: &str, observed: &str) -> bool {
    if expected == observed {
        return true;
    }
    let Some(marker_start) = observed.find("<truncated ") else {
        return false;
    };
    let Some(marker_end) = observed[marker_start..].find(" bytes>") else {
        return false;
    };
    let marker_end = marker_start + marker_end + " bytes>".len();
    let prefix = observed[..marker_start].trim_end_matches('\n');
    let suffix = observed[marker_end..].trim_start_matches('\n');
    let suffix_anchor = suffix
        .chars()
        .rev()
        .take(TRUNCATION_SUFFIX_CHARS)
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>();
    prefix.chars().count() >= TRUNCATION_MIN_ANCHOR_CHARS
        && suffix_anchor.chars().count() >= TRUNCATION_MIN_ANCHOR_CHARS
        && expected.starts_with(prefix)
        && expected.ends_with(&suffix_anchor)
}

fn selected_model(content: &str) -> Option<String> {
    let body = tagged_body(content, "USER_SETTINGS_CHANGE");
    let marker = "changed setting `Model Selection` from ";
    let after = body
        .find(marker)
        .map(|index| &body[index + marker.len()..])?;
    let to = after.find(" to ").map(|index| &after[index + 4..])?;
    let model = to.split(". No need to comment").next()?.trim();
    (!model.is_empty()).then(|| truncate_chars(model, 128))
}

fn tagged_body<'a>(content: &'a str, tag: &str) -> &'a str {
    let opening = format!("<{tag}>");
    let closing = format!("</{tag}>");
    let Some(start) = content.find(&opening) else {
        return content;
    };
    let body_start = start + opening.len();
    let Some(end) = content[body_start..].find(&closing) else {
        return content;
    };
    &content[body_start..body_start + end]
}

fn read_complete_jsonl(path: &Path, offset: u64) -> Result<(String, u64), DriverError> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok((String::new(), offset));
        }
        Err(_) => return Err(transcript_error()),
    };
    let length = file.metadata().map_err(|_| transcript_error())?.len();
    let remaining = length.checked_sub(offset).ok_or_else(transcript_error)?;
    if remaining > MAX_TURN_TRANSCRIPT_BYTES as u64 {
        return Err(transcript_error());
    }
    file.seek(SeekFrom::Start(offset))
        .map_err(|_| transcript_error())?;
    let mut data = Vec::new();
    file.take((MAX_TURN_TRANSCRIPT_BYTES + 1) as u64)
        .read_to_end(&mut data)
        .map_err(|_| transcript_error())?;
    if data.len() > MAX_TURN_TRANSCRIPT_BYTES {
        return Err(transcript_error());
    }
    if data.is_empty() {
        return Ok((String::new(), offset));
    }
    let complete_len = if data.ends_with(b"\n")
        || data.ends_with(b"\r")
        || serde_json::from_slice::<Value>(&data).is_ok_and(|value| value.is_object())
    {
        data.len()
    } else {
        data.iter()
            .rposition(|byte| matches!(byte, b'\n' | b'\r'))
            .map_or(0, |index| index + 1)
    };
    data.truncate(complete_len);
    validate_jsonl_bounds(&data)?;
    let text = String::from_utf8(data).map_err(|_| transcript_error())?;
    let next_offset = offset
        .checked_add(complete_len as u64)
        .ok_or_else(transcript_error)?;
    Ok((text, next_offset))
}

fn validate_jsonl_bounds(data: &[u8]) -> Result<(), DriverError> {
    let mut events = 0_usize;
    for line in data.split(|byte| matches!(byte, b'\n' | b'\r')) {
        if line.is_empty() {
            continue;
        }
        events = events.checked_add(1).ok_or_else(transcript_error)?;
        if events > MAX_JSONL_EVENTS || line.len() > MAX_JSONL_LINE_BYTES {
            return Err(transcript_error());
        }
    }
    Ok(())
}

fn provider_session_id_from_path(path: &Path) -> Result<String, DriverError> {
    let id = path
        .ancestors()
        .nth(3)
        .and_then(Path::file_name)
        .and_then(|value| value.to_str())
        .map(clean_identifier)
        .unwrap_or_default();
    if id.is_empty() {
        return Err(transcript_error());
    }
    Ok(id)
}

fn normalize_turn_input(value: &str) -> String {
    value
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim()
        .to_owned()
}

fn truncate_chars(value: &str, limit: usize) -> String {
    value
        .chars()
        .take(limit)
        .collect::<String>()
        .trim()
        .to_owned()
}

fn file_size(path: &Path) -> u64 {
    path.metadata().map_or(0, |metadata| metadata.len())
}

fn modified_time(path: &Path) -> SystemTime {
    path.metadata()
        .and_then(|metadata| metadata.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

const fn transcript_error() -> DriverError {
    DriverError::new(
        "provider_transcript_unavailable",
        "The Antigravity transcript authority could not be read.",
    )
}
