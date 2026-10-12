use crate::{raster_assets::prepare_preserved_raster, raster_preview::*};
use image::{ImageEncoder, Rgba, RgbaImage};

type TestResult = Result<(), Box<dyn std::error::Error>>;

pub(crate) fn large_png(alpha: bool, oriented: bool) -> Result<Vec<u8>, image::ImageError> {
    let image = RgbaImage::from_fn(1600, 800, |x, y| {
        let noise = x
            .wrapping_mul(1664525)
            .wrapping_add(y.wrapping_mul(1013904223));
        Rgba([
            if x < 800 { 240 } else { 10 },
            (noise >> 8) as u8,
            noise as u8,
            if alpha { 128 } else { 255 },
        ])
    });
    let mut bytes = Vec::new();
    let mut encoder = image::codecs::png::PngEncoder::new(&mut bytes);
    if oriented {
        // Little-endian TIFF, one SHORT orientation entry: rotate 90 clockwise.
        encoder
            .set_exif_metadata(vec![
                73, 73, 42, 0, 8, 0, 0, 0, 1, 0, 18, 1, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0, 0,
            ])
            .map_err(image::ImageError::Unsupported)?;
    }
    encoder.write_image(
        &image,
        image.width(),
        image.height(),
        image::ExtendedColorType::Rgba8,
    )?;
    Ok(bytes)
}

#[tokio::test]
async fn preview_bounds_alpha_orientation_and_original_passthrough() -> TestResult {
    for alpha in [false, true] {
        let original = large_png(alpha, true)?;
        assert!(original.len() > MAX_BYTES);
        let (preserved, safe, preview) =
            prepare_preserved_raster("image/png", original.clone(), true).await?;
        assert_eq!(preserved, original);
        assert!(safe);
        let preview = preview.ok_or("missing preview")?;
        assert!(preview.content.len() <= MAX_BYTES);
        let decoded = image::load_from_memory(&preview.content)?.to_rgba8();
        assert!(decoded.width().max(decoded.height()) <= MAX_DIMENSION);
        assert!(decoded.height().abs_diff(decoded.width() * 2) <= 1);
        // Rotation changes a left/right color split into a top/bottom split.
        let top = decoded.get_pixel(decoded.width() / 2, decoded.height() / 4);
        let bottom = decoded.get_pixel(decoded.width() / 2, 3 * decoded.height() / 4);
        assert!(top[0] > 220 && bottom[0] < 30);
        assert_eq!(top[3], if alpha { 128 } else { 255 });
        eprintln!(
            "preview alpha={alpha}: {} bytes, {}x{}",
            preview.content.len(),
            decoded.width(),
            decoded.height()
        );
    }
    let mut small = Vec::new();
    image::DynamicImage::new_rgba8(10, 10).write_to(
        &mut std::io::Cursor::new(&mut small),
        image::ImageFormat::Png,
    )?;
    let (content, safe, preview) =
        prepare_preserved_raster("image/png", small.clone(), true).await?;
    assert!(safe && preview.is_none());
    assert_eq!(content, small);

    let image = image::load_from_memory(&large_png(false, false)?)?.to_rgba8();
    let mut animation = Vec::new();
    {
        let mut encoder = image::codecs::gif::GifEncoder::new(&mut animation);
        encoder.encode_frame(image::Frame::new(image.clone()))?;
        // The first-frame validator already accepts this animation. Later
        // frames must not consume a second full-canvas decoder allocation.
        encoder.encode_frame(image::Frame::new(RgbaImage::from_pixel(
            4096,
            4096,
            Rgba([10, 20, 30, 255]),
        )))?;
    }
    assert!(animation.len() > MAX_BYTES);
    let (content, safe, preview) =
        prepare_preserved_raster("image/gif", animation.clone(), true).await?;
    assert!(safe && preview.is_none());
    assert_eq!(content, animation);
    Ok(())
}
