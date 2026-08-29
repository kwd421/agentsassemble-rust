use std::{io::Cursor, sync::OnceLock};

use agentsassemble_domain::MAX_RASTER_BYTES;
use chrono::DateTime;
use image::{DynamicImage, ImageFormat, ImageReader, Limits};
use tokio::sync::Semaphore;

use crate::PersistenceError;

const MAX_IMAGE_DIMENSION: u32 = 4096;
const MAX_IMAGE_PIXELS: u64 = 16 * 1024 * 1024;
const MAX_DECODE_ALLOC_BYTES: u64 = 72 * 1024 * 1024;

pub(crate) struct CanonicalRaster {
    pub(crate) filename: String,
    pub(crate) content: Vec<u8>,
}

pub(crate) async fn prepare_raster(
    filename: &str,
    content_type: &str,
    content: Vec<u8>,
) -> Result<(CanonicalRaster, i64), PersistenceError> {
    if content.is_empty() || content.len() > MAX_RASTER_BYTES {
        return Err(rejected(
            "attachment_too_large",
            "Raster attachment must be between 1 byte and 10 MiB.",
        ));
    }
    let declared_format = declared_image_format(content_type)?;
    let filename = sanitize_filename(filename);
    let canonical = canonicalize(filename, declared_format, content).await?;
    let size = i64::try_from(canonical.content.len()).map_err(|_| {
        rejected(
            "attachment_too_large",
            "Canonical raster attachment exceeds the supported size.",
        )
    })?;
    Ok((canonical, size))
}

pub(crate) fn validate_stored_raster(
    content_type: &str,
    size: i64,
    content_length: i64,
    created_at: &str,
) -> Result<(), PersistenceError> {
    if content_type != "image/png"
        || !(1..=i64::try_from(MAX_RASTER_BYTES).unwrap_or(i64::MAX)).contains(&size)
        || size != content_length
        || DateTime::parse_from_rfc3339(created_at).is_err()
    {
        return Err(rejected(
            "invalid_state",
            "Stored raster attachment metadata is invalid.",
        ));
    }
    Ok(())
}

async fn canonicalize(
    filename: String,
    declared_format: ImageFormat,
    content: Vec<u8>,
) -> Result<CanonicalRaster, PersistenceError> {
    let permit = decode_admission()
        .acquire()
        .await
        .map_err(|_| decode_task_failed())?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        canonicalize_blocking(&filename, declared_format, content)
    })
    .await
    .map_err(|_| decode_task_failed())?
}

fn canonicalize_blocking(
    filename: &str,
    declared_format: ImageFormat,
    content: Vec<u8>,
) -> Result<CanonicalRaster, PersistenceError> {
    let mut reader = ImageReader::new(Cursor::new(content))
        .with_guessed_format()
        .map_err(|_| invalid_image())?;
    let detected_format = reader.format().ok_or_else(invalid_image)?;
    if detected_format != declared_format {
        return Err(rejected(
            "attachment_type_mismatch",
            "Raster attachment bytes do not match the declared image type.",
        ));
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_DIMENSION);
    limits.max_image_height = Some(MAX_IMAGE_DIMENSION);
    limits.max_alloc = Some(MAX_DECODE_ALLOC_BYTES);
    reader.limits(limits);
    let decoded = reader.decode().map_err(|_| invalid_image())?;
    validate_pixels(&decoded)?;
    let mut encoded = Cursor::new(Vec::new());
    decoded
        .write_to(&mut encoded, ImageFormat::Png)
        .map_err(|_| invalid_image())?;
    let content = encoded.into_inner();
    if content.len() > MAX_RASTER_BYTES {
        return Err(rejected(
            "attachment_too_large",
            "Canonical raster attachment exceeds the 10 MiB item limit.",
        ));
    }
    Ok(CanonicalRaster {
        filename: canonical_png_filename(filename),
        content,
    })
}

fn validate_pixels(image: &DynamicImage) -> Result<(), PersistenceError> {
    let pixels = u64::from(image.width()).saturating_mul(u64::from(image.height()));
    if image.width() == 0
        || image.height() == 0
        || image.width() > MAX_IMAGE_DIMENSION
        || image.height() > MAX_IMAGE_DIMENSION
        || pixels > MAX_IMAGE_PIXELS
    {
        return Err(rejected(
            "attachment_image_limits",
            "Raster attachment dimensions exceed the decode limits.",
        ));
    }
    Ok(())
}

fn decode_admission() -> &'static Semaphore {
    static ADMISSION: OnceLock<Semaphore> = OnceLock::new();
    ADMISSION.get_or_init(|| Semaphore::new(2))
}

fn declared_image_format(content_type: &str) -> Result<ImageFormat, PersistenceError> {
    match content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "image/png" => Ok(ImageFormat::Png),
        "image/jpeg" => Ok(ImageFormat::Jpeg),
        "image/gif" => Ok(ImageFormat::Gif),
        "image/webp" => Ok(ImageFormat::WebP),
        _ => Err(rejected(
            "attachment_type_unsupported",
            "Profile avatars must be PNG, JPEG, GIF, or WebP.",
        )),
    }
}

pub(crate) fn sanitize_filename(value: &str) -> String {
    let normalized = value.replace('\\', "/");
    let name = normalized.rsplit('/').next().unwrap_or_default();
    let name: String = name
        .chars()
        .filter(|character| !character.is_control() && !matches!(character, '/' | '\\'))
        .collect::<String>()
        .trim()
        .chars()
        .take(120)
        .collect();
    if name.is_empty() || matches!(name.as_str(), "." | "..") {
        "profile.png".to_owned()
    } else {
        name
    }
}

fn canonical_png_filename(filename: &str) -> String {
    let stem = filename
        .rsplit_once('.')
        .map_or(filename, |(stem, _)| stem)
        .trim_matches(['.', ' ']);
    let stem: String = stem.chars().take(116).collect();
    if stem.is_empty() {
        "profile.png".to_owned()
    } else {
        format!("{stem}.png")
    }
}

fn invalid_image() -> PersistenceError {
    rejected(
        "attachment_invalid_image",
        "Raster attachment bytes are not a valid bounded image.",
    )
}

fn decode_task_failed() -> PersistenceError {
    PersistenceError::RuntimeAuthorityTask("profile avatar validation task failed".to_owned())
}

fn rejected(code: &'static str, message: &str) -> PersistenceError {
    PersistenceError::CommandRejected {
        code,
        message: message.to_owned(),
    }
}
