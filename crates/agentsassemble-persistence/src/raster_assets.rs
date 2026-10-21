use std::{io::Cursor, sync::OnceLock};

use agentsassemble_domain::{MAX_ATTACHMENT_BYTES, MAX_PERSONA_UPLOAD_BYTES};
use chrono::DateTime;
use image::{DynamicImage, ImageFormat, ImageReader, Limits};
use tokio::sync::Semaphore;

use crate::PersistenceError;

const MAX_IMAGE_DIMENSION: u32 = 4096;
const MAX_IMAGE_PIXELS: u64 = 16 * 1024 * 1024;
const MAX_CARD_ART_DIMENSION: u32 = 8192;
const CARD_THUMBNAIL_DIMENSION: u32 = 1024;
pub const MAX_RASTER_DECODE_ALLOC_BYTES: u64 = 72 * 1024 * 1024;
pub(crate) const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

pub(crate) struct CanonicalRaster {
    pub(crate) filename: String,
    pub(crate) content: Vec<u8>,
}

pub(crate) async fn prepare_raster(
    filename: &str,
    content_type: &str,
    content: Vec<u8>,
) -> Result<(CanonicalRaster, i64), PersistenceError> {
    if content.is_empty() || content.len() > MAX_ATTACHMENT_BYTES {
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

/// Card art may be larger than an attachment; only a bounded PNG thumbnail is kept.
pub(crate) async fn prepare_card_thumbnail(
    content_type: &str,
    content: Vec<u8>,
) -> Result<Vec<u8>, PersistenceError> {
    if content.is_empty() || content.len() > MAX_PERSONA_UPLOAD_BYTES {
        return Err(invalid_image());
    }
    let declared_format = declared_image_format(content_type)?;
    let permit = decode_permit().await?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        card_thumbnail_blocking(declared_format, content)
    })
    .await
    .map_err(|_| decode_task_failed())?
}

fn card_thumbnail_blocking(
    declared_format: ImageFormat,
    content: Vec<u8>,
) -> Result<Vec<u8>, PersistenceError> {
    let mut limits = decode_limits();
    limits.max_image_width = Some(MAX_CARD_ART_DIMENSION);
    limits.max_image_height = Some(MAX_CARD_ART_DIMENSION);
    let mut reader = ImageReader::new(Cursor::new(content.as_slice()))
        .with_guessed_format()
        .map_err(|_| invalid_image())?;
    reader.limits(limits.clone());
    if reader.format() != Some(declared_format) {
        return Err(invalid_image());
    }
    let (width, height) = reader.into_dimensions().map_err(|_| invalid_image())?;
    let pixels = u64::from(width).saturating_mul(u64::from(height));
    if pixels == 0 || pixels > MAX_IMAGE_PIXELS {
        return Err(invalid_image());
    }
    let decoded = ImageReader::new(Cursor::new(content.as_slice()))
        .with_guessed_format()
        .map_err(|_| invalid_image())
        .and_then(|mut reader| {
            reader.limits(limits);
            reader.decode().map_err(|_| invalid_image())
        })?;
    // The integer box filter allocates only the output, unlike resize's float buffer.
    let thumbnail = if decoded.width().max(decoded.height()) > CARD_THUMBNAIL_DIMENSION {
        decoded.thumbnail(CARD_THUMBNAIL_DIMENSION, CARD_THUMBNAIL_DIMENSION)
    } else {
        decoded
    };
    let mut encoded = Cursor::new(Vec::new());
    thumbnail
        .write_to(&mut encoded, ImageFormat::Png)
        .map_err(|_| invalid_image())?;
    let encoded = encoded.into_inner();
    if encoded.len() > MAX_ATTACHMENT_BYTES {
        return Err(invalid_image());
    }
    Ok(encoded)
}

pub(crate) fn validate_stored_raster(
    content_type: &str,
    size: i64,
    content_length: i64,
    created_at: &str,
) -> Result<(), PersistenceError> {
    if content_type != "image/png"
        || !(1..=i64::try_from(MAX_ATTACHMENT_BYTES).unwrap_or(i64::MAX)).contains(&size)
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

pub(crate) async fn validate_preserved_safe_raster(
    content_type: &str,
    content: Vec<u8>,
) -> Result<(Vec<u8>, bool), PersistenceError> {
    let (content, safe, _) = prepare_preserved_raster(content_type, content, false).await?;
    Ok((content, safe))
}

pub(crate) async fn prepare_preserved_raster(
    content_type: &str,
    content: Vec<u8>,
    preview: bool,
) -> Result<(Vec<u8>, bool, Option<crate::raster_preview::Preview>), PersistenceError> {
    let Some(format) = safe_image_format(content_type) else {
        return Ok((content, false, None));
    };
    let permit = decode_permit().await?;
    decode_preserved(format, content, preview, permit).await
}

pub(crate) async fn decode_permit()
-> Result<tokio::sync::SemaphorePermit<'static>, PersistenceError> {
    decode_admission()
        .acquire()
        .await
        .map_err(|_| decode_task_failed())
}

pub(crate) async fn lazy_preview<T: Send + 'static>(
    content_type: &str,
    content: Vec<u8>,
    permit: tokio::sync::SemaphorePermit<'static>,
    hold: T,
    generation_guard: tokio::sync::OwnedMutexGuard<()>,
    failure_slot: crate::raster_validation_failure::PreviewFailureSlot,
) -> Result<
    (
        Option<crate::raster_preview::Preview>,
        tokio::sync::OwnedMutexGuard<()>,
    ),
    PersistenceError,
> {
    let content_type = content_type.to_owned();
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let _hold = hold;
        let preview = declared_image_format(&content_type)
            .and_then(|format| validate_preserved_blocking(format, &content, true));
        if let Err(error) = &preview {
            // Record before dropping the guard, even if the caller was cancelled.
            failure_slot.record(error);
        }
        drop(content); // Whole original never waits in the worker result without admission.
        // Cancellation cannot release per-attachment serialization while CPU work continues.
        // Successful work returns the guard for the caller's final store transaction.
        Ok((preview?, generation_guard))
    })
    .await
    .map_err(|_| decode_task_failed())?
}

pub(crate) async fn upload_preview(
    content_type: &str,
    content: Vec<u8>,
    permit: tokio::sync::SemaphorePermit<'static>,
) -> Result<Option<crate::raster_preview::Preview>, PersistenceError> {
    let format = declared_image_format(content_type)?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        validate_preserved_blocking(format, &content, true)
    })
    .await
    .map_err(|_| decode_task_failed())?
}

async fn decode_preserved(
    format: ImageFormat,
    content: Vec<u8>,
    preview: bool,
    permit: tokio::sync::SemaphorePermit<'static>,
) -> Result<(Vec<u8>, bool, Option<crate::raster_preview::Preview>), PersistenceError> {
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let derived = validate_preserved_blocking(format, &content, preview)?;
        Ok((content, true, derived))
    })
    .await
    .map_err(|_| decode_task_failed())?
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
    let reader = bounded_reader(Cursor::new(content))?;
    let detected_format = reader.format().ok_or_else(invalid_image)?;
    if detected_format != declared_format {
        return Err(rejected(
            "attachment_type_mismatch",
            "Raster attachment bytes do not match the declared image type.",
        ));
    }
    let decoded = reader.decode().map_err(|_| invalid_image())?;
    validate_pixels(&decoded)?;
    let mut encoded = Cursor::new(Vec::new());
    decoded
        .write_to(&mut encoded, ImageFormat::Png)
        .map_err(|_| invalid_image())?;
    let content = encoded.into_inner();
    if content.len() > MAX_ATTACHMENT_BYTES {
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

fn validate_preserved_blocking(
    declared_format: ImageFormat,
    content: &[u8],
    preview: bool,
) -> Result<Option<crate::raster_preview::Preview>, PersistenceError> {
    use image::{ImageDecoder, metadata::Orientation};
    let reader = bounded_reader(Cursor::new(content))?;
    let detected_format = reader.format().ok_or_else(invalid_image)?;
    if detected_format != declared_format {
        return Err(rejected(
            "attachment_type_mismatch",
            "Raster attachment bytes do not match the declared image type.",
        ));
    }
    let wants_preview = preview && content.len() > crate::raster_preview::MAX_BYTES;
    let animated = if wants_preview {
        match declared_format {
            ImageFormat::Png => {
                image::codecs::png::PngDecoder::with_limits(Cursor::new(content), decode_limits())
                    .and_then(|d| d.is_apng())
                    .map_err(|_| invalid_image())?
            }
            ImageFormat::WebP => image::codecs::webp::WebPDecoder::new(Cursor::new(content))
                .map_err(|_| invalid_image())?
                .has_animation(),
            ImageFormat::Gif => {
                // The existing validator decodes only the first frame. Inspect
                // metadata with the same GIF library, without decoding later frames.
                let mut options = gif::DecodeOptions::new();
                options.skip_frame_decoding(true);
                let mut decoder = options
                    .read_info(Cursor::new(content))
                    .map_err(|_| invalid_image())?;
                decoder.next_frame_info().map_err(|_| invalid_image())?;
                decoder
                    .next_frame_info()
                    .map_err(|_| invalid_image())?
                    .is_some()
            }
            _ => false,
        }
    } else {
        false
    };
    let mut decoder = reader.into_decoder().map_err(|_| invalid_image())?;
    let mut limits = decode_limits();
    limits
        .reserve(decoder.total_bytes())
        .map_err(|_| invalid_image())?;
    decoder.set_limits(limits).map_err(|_| invalid_image())?;
    let orientation = if wants_preview && !animated {
        decoder
            .exif_metadata()
            .map_err(|_| invalid_image())?
            .as_deref()
            .and_then(Orientation::from_exif_chunk)
            .unwrap_or(Orientation::NoTransforms)
    } else {
        Orientation::NoTransforms
    };
    let mut decoded = DynamicImage::from_decoder(decoder).map_err(|_| invalid_image())?;
    validate_pixels(&decoded)?;
    if !wants_preview || animated {
        return Ok(None);
    }
    decoded.apply_orientation(orientation);
    crate::raster_preview::encode(decoded).map(Some)
}

fn bounded_reader<R: std::io::BufRead + std::io::Seek>(
    source: R,
) -> Result<ImageReader<R>, PersistenceError> {
    let mut reader = ImageReader::new(source)
        .with_guessed_format()
        .map_err(|_| invalid_image())?;
    let limits = decode_limits();
    reader.limits(limits);
    Ok(reader)
}

fn decode_limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_DIMENSION);
    limits.max_image_height = Some(MAX_IMAGE_DIMENSION);
    limits.max_alloc = Some(MAX_RASTER_DECODE_ALLOC_BYTES);
    limits
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
    safe_image_format(content_type).ok_or_else(|| {
        rejected(
            "attachment_type_unsupported",
            "Profile avatars must be PNG, JPEG, GIF, or WebP.",
        )
    })
}

fn safe_image_format(content_type: &str) -> Option<ImageFormat> {
    match content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "image/png" => Some(ImageFormat::Png),
        "image/jpeg" => Some(ImageFormat::Jpeg),
        "image/gif" => Some(ImageFormat::Gif),
        "image/webp" => Some(ImageFormat::WebP),
        _ => None,
    }
}

pub(crate) fn is_safe_raster_content_type(content_type: &str) -> bool {
    safe_image_format(content_type).is_some()
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
    PersistenceError::RuntimeAuthorityTask("raster validation task failed".to_owned())
}

fn rejected(code: &'static str, message: &str) -> PersistenceError {
    PersistenceError::CommandRejected {
        code: code.into(),
        message: message.to_owned(),
    }
}
