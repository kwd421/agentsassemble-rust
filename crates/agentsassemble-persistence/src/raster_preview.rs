//! Derived conversation pixels; originals keep their exact bytes and metadata.
use crate::PersistenceError;
use image::{DynamicImage, GenericImageView, ImageEncoder, imageops::FilterType};

pub(crate) const MAX_BYTES: usize = agentsassemble_domain::MAX_MESSAGE_ATTACHMENT_PREVIEW_BYTES;
pub(crate) const MAX_DIMENSION: u32 = 1100;

pub(crate) struct Preview {
    pub(crate) content_type: &'static str,
    pub(crate) content: Vec<u8>,
}

pub(crate) fn encode(image: &DynamicImage) -> Result<Preview, PersistenceError> {
    let transparent =
        image.color().has_alpha() && image.pixels().any(|(_, _, pixel)| pixel[3] != 255);
    let mut side = MAX_DIMENSION.min(image.width().max(image.height()));
    loop {
        let scaled = image.resize(side, side, FilterType::Triangle).to_rgba8();
        let mut content = Vec::new();
        let content_type = if transparent {
            image::codecs::webp::WebPEncoder::new_lossless(&mut content).write_image(
                &scaled,
                scaled.width(),
                scaled.height(),
                image::ExtendedColorType::Rgba8,
            )
        } else {
            let rgb = DynamicImage::ImageRgba8(scaled).to_rgb8();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut content, 80).write_image(
                &rgb,
                rgb.width(),
                rgb.height(),
                image::ExtendedColorType::Rgb8,
            )
        }
        .map_err(|_| PersistenceError::RuntimeAuthorityTask("preview encoding failed".into()));
        content_type?;
        if content.len() <= MAX_BYTES {
            return Ok(Preview {
                content_type: if transparent {
                    "image/webp"
                } else {
                    "image/jpeg"
                },
                content,
            });
        }
        // Byte bound owns output dimensions too, including incompressible alpha.
        side = (side * 3 / 4).max(1);
    }
}
