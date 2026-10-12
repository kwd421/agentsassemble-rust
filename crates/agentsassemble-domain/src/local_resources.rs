use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Private on-demand observation, separate from room events.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct LocalResourceStatus {
    pub attachment_storage: AttachmentStorageUsage,
    pub observed_at: DateTime<Utc>,
    pub cpu_sample_seconds: Option<f64>,
    pub cpu_count: Option<usize>,
    pub total_memory_bytes: Option<u64>,
    pub available_memory_bytes: Option<u64>,
    pub load_average: Option<[f64; 3]>,
    pub matching_process_count: usize,
    pub processes: Vec<LocalResourceProcess>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct LocalResourceProcess {
    pub pid: u32,
    pub label: String,
    pub cpu_percent: Option<f32>,
    pub memory_bytes: Option<u64>,
}

/// Retained attachment bytes, excluding database overhead and persona bodies.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct AttachmentStorageUsage {
    pub rooms: Vec<RoomAttachmentStorageUsage>,
    pub other: AttachmentStorageCount,
    pub total: AttachmentStorageCount,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct RoomAttachmentStorageUsage {
    pub room_id: String,
    pub label: String,
    pub files: i64,
    pub bytes: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct AttachmentStorageCount {
    pub files: i64,
    pub bytes: i64,
}
