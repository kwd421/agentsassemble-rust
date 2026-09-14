//! Selected-directory file operations for the builtin API tool owner.
use std::{
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
    time::Instant,
};

use agentsassemble_domain::stable_identity_hash;
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt, OpenOptionsSyncExt};
use cap_std::fs::{Dir, OpenOptions};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

const FILE_BYTES: u64 = 1_000_000;
pub(super) const RESULT_BYTES: usize = 128 * 1024;

#[derive(Deserialize, Serialize)]
#[serde(tag = "operation", deny_unknown_fields)]
pub(super) enum FileOperation {
    #[serde(rename = "list_workspace_files")]
    List {
        #[serde(default = "root_path")]
        path: String,
    },
    #[serde(rename = "read_workspace_file")]
    Read {
        path: String,
        #[serde(default = "first_line")]
        start_line: usize,
        end_line: Option<usize>,
        #[serde(default)]
        offset: usize,
    },
    #[serde(rename = "search_workspace_text")]
    Search {
        #[serde(default = "root_path")]
        path: String,
        query: String,
    },
    #[serde(rename = "write_workspace_file")]
    Write { path: String, content: String },
    #[serde(rename = "replace_workspace_text")]
    Replace {
        path: String,
        old_text: String,
        new_text: String,
        #[serde(default = "first_line")]
        expected_replacements: usize,
    },
}

fn root_path() -> String {
    ".".to_owned()
}
const fn first_line() -> usize {
    1
}
fn invalid() -> io::Error {
    io::Error::other("workspace file operation rejected")
}

impl FileOperation {
    pub(super) fn path(&self) -> &str {
        match self {
            Self::List { path }
            | Self::Read { path, .. }
            | Self::Search { path, .. }
            | Self::Write { path, .. }
            | Self::Replace { path, .. } => path,
        }
    }

    pub(super) fn resolve(&mut self, root: &Dir) -> io::Result<()> {
        let mut path = relative(self.path())?;
        let mut missing = Vec::new();
        let mut resolved = loop {
            match root.canonicalize(if path.as_os_str().is_empty() {
                Path::new(".")
            } else {
                &path
            }) {
                Ok(path) => break path,
                Err(error) if self.writes() && error.kind() == io::ErrorKind::NotFound => {
                    missing.push(path.file_name().ok_or_else(invalid)?.to_owned());
                    if !path.pop() {
                        return Err(error);
                    }
                }
                Err(error) => return Err(error),
            }
        };
        resolved.extend(missing.into_iter().rev());
        let resolved = resolved.to_str().ok_or_else(invalid)?;
        relative(if resolved.is_empty() { "." } else { resolved })?;
        let target = match self {
            Self::List { path }
            | Self::Read { path, .. }
            | Self::Search { path, .. }
            | Self::Write { path, .. }
            | Self::Replace { path, .. } => path,
        };
        *target = if resolved.is_empty() {
            ".".to_owned()
        } else {
            resolved.to_owned()
        };
        Ok(())
    }

    pub(super) const fn writes(&self) -> bool {
        matches!(self, Self::Write { .. } | Self::Replace { .. })
    }

    pub(super) fn validate(&self) -> io::Result<()> {
        let path = relative(self.path())?;
        if !matches!(self, Self::List { .. } | Self::Search { .. }) && path.file_name().is_none() {
            return Err(invalid());
        }
        let valid = match self {
            Self::List { .. } => true,
            Self::Read {
                start_line,
                end_line,
                ..
            } => *start_line > 0 && end_line.is_none_or(|end| end >= *start_line),
            Self::Search { query, .. } => !query.is_empty() && query.chars().count() <= 1000,
            Self::Write { content, .. } => content.len() as u64 <= FILE_BYTES,
            Self::Replace {
                old_text,
                new_text,
                expected_replacements,
                ..
            } => {
                !old_text.is_empty()
                    && old_text.len() as u64 <= FILE_BYTES
                    && new_text.len() as u64 <= FILE_BYTES
                    && (1..=100).contains(expected_replacements)
            }
        };
        if valid { Ok(()) } else { Err(invalid()) }
    }
}

pub(super) fn bind(workspace: &str, identity: &str) -> io::Result<Dir> {
    let dir = Dir::open_ambient_dir(workspace, cap_std::ambient_authority())?;
    let handle = same_file::Handle::from_file(dir.try_clone()?.into_std_file())?;
    if stable_identity_hash(&handle) != identity {
        return Err(invalid());
    }
    Ok(dir)
}

fn relative(path: &str) -> io::Result<PathBuf> {
    if path.is_empty()
        || path.len() > 1000
        || path.contains(['\\', ':'])
        || path.chars().any(char::is_control)
    {
        return Err(invalid());
    }
    let mut result = PathBuf::new();
    for component in Path::new(path).components() {
        match component {
            Component::CurDir => {}
            Component::Normal(name)
                if ![".git", ".hg", ".svn"]
                    .iter()
                    .any(|blocked| name.eq_ignore_ascii_case(blocked)) =>
            {
                result.push(name);
            }
            _ => return Err(invalid()),
        }
    }
    Ok(result)
}

fn directory(root: &Dir, path: &Path, create: bool) -> io::Result<Dir> {
    let mut dir = root.try_clone()?;
    for name in path.components() {
        if create {
            match dir.create_dir(name.as_os_str()) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        dir = dir.open_dir_nofollow(name.as_os_str())?;
    }
    Ok(dir)
}

pub(super) fn read(root: &Dir, path: &str, maximum: u64) -> io::Result<String> {
    let path = relative(path)?;
    let name = path.file_name().ok_or_else(invalid)?;
    let parent = directory(root, path.parent().ok_or_else(invalid)?, false)?;
    read_at(&parent, name, maximum)
}

fn read_at(parent: &Dir, name: &std::ffi::OsStr, maximum: u64) -> io::Result<String> {
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No).nonblock(true);
    let file = parent.open_with(name, &options)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > maximum {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    file.take(maximum + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > maximum {
        return Err(invalid());
    }
    String::from_utf8(bytes).map_err(|_| invalid())
}

pub(super) fn execute(
    root: &Dir,
    operation: &FileOperation,
    before: Option<&str>,
    cancel: &CancellationToken,
) -> io::Result<Value> {
    if cancel.is_cancelled() {
        return Err(invalid());
    }
    match operation {
        FileOperation::List { path } => discover(root, path, None, cancel),
        FileOperation::Search { path, query } => discover(root, path, Some(query), cancel),
        FileOperation::Read {
            path,
            start_line,
            end_line,
            offset,
        } => {
            let text = read(root, path, 2_000_000)?;
            let end = end_line.unwrap_or_else(|| start_line.saturating_add(399));
            let lines: Vec<_> = text
                .split_inclusive('\n')
                .skip(start_line - 1)
                .take(end.saturating_sub(*start_line).saturating_add(1))
                .collect();
            let selected = lines.concat();
            let remaining: String = selected.chars().skip(*offset).collect();
            let mut result = json!({"path":path,"start_line":start_line,"end_line":start_line.saturating_add(lines.len()).saturating_sub(1),"truncated":false,"content":"","next_offset":offset.saturating_add(remaining.chars().count())});
            // Reserve the full JSON envelope, including the largest continuation offset.
            let budget = RESULT_BYTES
                .checked_sub(result.to_string().len())
                .ok_or_else(invalid)?;
            let content = json_text_prefix(&remaining, budget, 100_000);
            let more = content.len() < remaining.len();
            result["next_offset"] = if more {
                json!(offset.saturating_add(content.chars().count()))
            } else {
                Value::Null
            };
            result["truncated"] = json!(more || end < text.lines().count());
            result["content"] = json!(content);
            Ok(result)
        }
        FileOperation::Write { path, content } => {
            write(root, path, content, None, cancel)?;
            Ok(json!({"path":path,"bytes_written":content.len()}))
        }
        FileOperation::Replace {
            path,
            old_text,
            new_text,
            expected_replacements,
        } => {
            let previous = before.ok_or_else(invalid)?;
            if previous.matches(old_text).count() != *expected_replacements {
                return Err(invalid());
            }
            let replaced_bytes = old_text.len().saturating_mul(*expected_replacements);
            let added_bytes = new_text.len().saturating_mul(*expected_replacements);
            if previous
                .len()
                .saturating_sub(replaced_bytes)
                .saturating_add(added_bytes) as u64
                > FILE_BYTES
            {
                return Err(invalid());
            }
            let content = previous.replace(old_text, new_text);
            if content.len() as u64 > FILE_BYTES {
                return Err(invalid());
            }
            write(root, path, &content, Some(previous), cancel)?;
            Ok(json!({"path":path,"replacements":expected_replacements}))
        }
    }
}

fn write(
    root: &Dir,
    path: &str,
    content: &str,
    before: Option<&str>,
    cancel: &CancellationToken,
) -> io::Result<()> {
    let path = relative(path)?;
    let name = path.file_name().ok_or_else(invalid)?;
    if cancel.is_cancelled() {
        return Err(invalid());
    }
    let parent = directory(root, path.parent().ok_or_else(invalid)?, true)?;
    if let Some(before) = before
        && read_at(&parent, name, FILE_BYTES)? != before
    {
        return Err(invalid());
    }
    let permissions = match parent.symlink_metadata(name) {
        Ok(metadata) if metadata.is_file() => Some(metadata.permissions()),
        Ok(_) => return Err(invalid()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    let temporary = format!(".agentsassemble-write-{}", uuid::Uuid::new_v4());
    let mut options = OpenOptions::new();
    options
        .write(true)
        .create_new(true)
        .follow(FollowSymlinks::No);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = parent.open_with(&temporary, &options)?;
    let result = (|| {
        if let Some(permissions) = permissions {
            file.set_permissions(permissions)?;
        }
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
        drop(file);
        if cancel.is_cancelled() {
            return Err(invalid());
        }
        parent.rename(&temporary, &parent, name)
    })();
    if result.is_err() {
        parent.remove_file(&temporary)?;
    }
    result
}

fn discover(
    root: &Dir,
    path: &str,
    query: Option<&str>,
    cancel: &CancellationToken,
) -> io::Result<Value> {
    let started = Instant::now();
    let initial = relative(path)?;
    let mut pending = vec![initial];
    let mut result = Vec::new();
    let mut entries_seen = 0;
    let mut bytes_read = 0;
    let mut truncated = false;
    let key = if query.is_some() { "matches" } else { "files" };
    // false is one byte longer than true, so either final truncation flag fits.
    let mut result_bytes = json!({key:[],"truncated":false}).to_string().len();
    'walk: while let Some(path) = pending.pop() {
        for entry in directory(root, &path, false)?.entries()? {
            if cancel.is_cancelled() {
                return Err(invalid());
            }
            entries_seen += 1;
            if entries_seen > 5000 || started.elapsed().as_millis() >= 750 {
                truncated = true;
                break 'walk;
            }
            let entry = entry?;
            let child = path.join(entry.file_name());
            let Some(encoded) = child.to_str() else {
                truncated = true;
                continue;
            };
            if relative(encoded).is_err() {
                continue;
            }
            let kind = entry.file_type()?;
            if kind.is_dir() {
                pending.push(child);
            } else if kind.is_file() {
                if let Some(query) = query {
                    let Ok(text) = read(root, encoded, FILE_BYTES) else {
                        truncated = true;
                        continue;
                    };
                    bytes_read += text.len();
                    if bytes_read > 10_000_000 {
                        truncated = true;
                        break 'walk;
                    }
                    for (line, text) in text
                        .lines()
                        .enumerate()
                        .filter(|(_, text)| text.contains(query))
                    {
                        let item = json!({"path":encoded,"line":line+1,"text":text.chars().take(500).collect::<String>()});
                        if !push_bounded(&mut result, item, &mut result_bytes) {
                            truncated = true;
                            break 'walk;
                        }
                        if result.len() >= 200 {
                            truncated = true;
                            break 'walk;
                        }
                    }
                } else {
                    if !push_bounded(&mut result, json!(encoded), &mut result_bytes) {
                        truncated = true;
                        break 'walk;
                    }
                    if result.len() >= 500 {
                        truncated = true;
                        break 'walk;
                    }
                }
            }
        }
    }
    Ok(if query.is_some() {
        json!({"matches":result,"truncated":truncated})
    } else {
        json!({"files":result,"truncated":truncated})
    })
}

// The JSON string encoder escapes controls, quotes and backslashes in addition
// to preserving UTF-8; a character count alone cannot bound the tool wire result.
fn json_text_prefix(text: &str, budget: usize, maximum_chars: usize) -> &str {
    let mut bytes = 0;
    let mut end = 0;
    for (index, character) in text.char_indices().take(maximum_chars) {
        let cost = match character {
            '"' | '\\' | '\n' | '\r' | '\t' | '\u{0008}' | '\u{000c}' => 2,
            '\u{0000}'..='\u{001f}' => 6,
            _ => character.len_utf8(),
        };
        if bytes + cost > budget {
            break;
        }
        bytes += cost;
        end = index + character.len_utf8();
    }
    &text[..end]
}

fn push_bounded(items: &mut Vec<Value>, item: Value, bytes: &mut usize) -> bool {
    let added = item.to_string().len() + usize::from(!items.is_empty());
    if *bytes + added > RESULT_BYTES {
        return false;
    }
    *bytes += added;
    items.push(item);
    true
}
