//! Images for notes, made ready to keep: checked before they are decoded,
//! scaled down to a size that stays sharp on HiDPI screens, and written again
//! as PNG or JPEG, which leaves their metadata (EXIF, GPS) behind. See
//! `docs/design/note-images.md`.

use std::io::Cursor;

use image::{DynamicImage, GenericImageView, ImageFormat, ImageReader, imageops::FilterType};

/// More pixels than this is refused before decoding: a small file can claim a
/// huge image (a decompression bomb).
pub const MAX_PIXELS: u64 = 50_000_000;
/// Longer edges are scaled down to this.
pub const LONG_EDGE: u32 = 2560;
/// An image, once prepared, holds at most this many bytes.
pub const MAX_BYTES: usize = 2 * 1024 * 1024;
/// The quality photos are written at.
const JPEG_QUALITY: u8 = 85;

/// An image made ready to keep.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prepared {
    pub bytes: Vec<u8>,
    /// `image/png` or `image/jpeg`.
    pub mime: &'static str,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, PartialEq, Eq)]
pub enum PrepareError {
    /// Not an image this can read (PNG, JPEG, GIF, WebP, BMP, TIFF).
    Unreadable(String),
    /// More pixels than [`MAX_PIXELS`].
    TooManyPixels { width: u32, height: u32 },
    /// Still larger than [`MAX_BYTES`] once prepared.
    TooLarge(usize),
}

impl std::fmt::Display for PrepareError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PrepareError::Unreadable(why) => write!(f, "not an image that can be read: {why}"),
            PrepareError::TooManyPixels { width, height } => {
                write!(f, "too large an image: {width} × {height}")
            }
            PrepareError::TooLarge(bytes) => write!(
                f,
                "too large once prepared: {} KB, of {} KB at most",
                bytes / 1024,
                MAX_BYTES / 1024
            ),
        }
    }
}

impl std::error::Error for PrepareError {}

/// The formats read: what screenshots and photos come as.
fn readable(format: ImageFormat) -> bool {
    matches!(
        format,
        ImageFormat::Png
            | ImageFormat::Jpeg
            | ImageFormat::Gif
            | ImageFormat::WebP
            | ImageFormat::Bmp
            | ImageFormat::Tiff
    )
}

/// Makes `input` (an image file's bytes) ready to keep: see the module.
pub fn prepare(input: &[u8]) -> Result<Prepared, PrepareError> {
    let unreadable = |e: &dyn std::fmt::Display| PrepareError::Unreadable(e.to_string());
    let reader = ImageReader::new(Cursor::new(input))
        .with_guessed_format()
        .map_err(|e| unreadable(&e))?;
    let format = reader
        .format()
        .filter(|f| readable(*f))
        .ok_or_else(|| PrepareError::Unreadable("an unknown format".into()))?;
    // The size the header claims, before anything is decoded.
    let (width, height) = reader.into_dimensions().map_err(|e| unreadable(&e))?;
    if u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(PrepareError::TooManyPixels { width, height });
    }
    let mut reader = ImageReader::with_format(Cursor::new(input), format);
    // The decoder's own guard, should the data disagree with the header.
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(width);
    limits.max_image_height = Some(height);
    reader.limits(limits);
    // A GIF gives its first frame.
    let image = reader.decode().map_err(|e| unreadable(&e))?;
    let image = scaled(image);
    let (width, height) = image.dimensions();

    let png = encode_png(&image)?;
    let prepared = if uses_alpha(&image) {
        Prepared {
            bytes: png,
            mime: "image/png",
            width,
            height,
        }
    } else {
        let jpeg = encode_jpeg(&image)?;
        // Screenshots and diagrams (flat colors, text) are smaller, and stay
        // crisp, as PNG; photos are many times smaller as JPEG. PNG is kept
        // unless it is more than twice the JPEG.
        if png.len() <= jpeg.len() * 2 {
            Prepared {
                bytes: png,
                mime: "image/png",
                width,
                height,
            }
        } else {
            Prepared {
                bytes: jpeg,
                mime: "image/jpeg",
                width,
                height,
            }
        }
    };
    if prepared.bytes.len() > MAX_BYTES {
        return Err(PrepareError::TooLarge(prepared.bytes.len()));
    }
    Ok(prepared)
}

/// `image` with its long edge at [`LONG_EDGE`] at most.
fn scaled(image: DynamicImage) -> DynamicImage {
    let (w, h) = image.dimensions();
    if w.max(h) <= LONG_EDGE {
        return image;
    }
    // `resize` keeps the aspect ratio within the box.
    image.resize(LONG_EDGE, LONG_EDGE, FilterType::Lanczos3)
}

/// Whether any pixel is not fully opaque.
fn uses_alpha(image: &DynamicImage) -> bool {
    image.color().has_alpha() && image.to_rgba8().pixels().any(|p| p.0[3] < 255)
}

fn encode_png(image: &DynamicImage) -> Result<Vec<u8>, PrepareError> {
    let mut out = Cursor::new(Vec::new());
    let image = if uses_alpha(image) {
        DynamicImage::ImageRgba8(image.to_rgba8())
    } else {
        DynamicImage::ImageRgb8(image.to_rgb8())
    };
    image
        .write_to(&mut out, ImageFormat::Png)
        .map_err(|e| PrepareError::Unreadable(e.to_string()))?;
    Ok(out.into_inner())
}

fn encode_jpeg(image: &DynamicImage) -> Result<Vec<u8>, PrepareError> {
    let mut out = Vec::new();
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY);
    image
        .to_rgb8()
        .write_with_encoder(encoder)
        .map_err(|e| PrepareError::Unreadable(e.to_string()))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage, Rgba, RgbaImage};

    fn png_of(image: &DynamicImage) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        image.write_to(&mut out, ImageFormat::Png).expect("png");
        out.into_inner()
    }

    /// A screenshot: flat background, a few colored bars, sharp edges.
    fn screenshot(w: u32, h: u32) -> DynamicImage {
        let img = RgbImage::from_fn(w, h, |x, y| {
            if y % 40 < 12 && x % 300 < 220 {
                Rgb([30, 30, 30])
            } else if x < 200 {
                Rgb([240, 242, 245])
            } else {
                Rgb([255, 255, 255])
            }
        });
        DynamicImage::ImageRgb8(img)
    }

    /// A photo: smooth gradients with noise everywhere.
    fn photo(w: u32, h: u32) -> DynamicImage {
        let mut seed = 0x2545_f491_u32;
        let img = RgbImage::from_fn(w, h, |x, y| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let n = (seed % 48) as u8;
            Rgb([
                ((x * 255 / w) as u8).saturating_add(n),
                ((y * 255 / h) as u8).saturating_add(n / 2),
                (((x + y) * 255 / (w + h)) as u8).saturating_add(n / 3),
            ])
        });
        DynamicImage::ImageRgb8(img)
    }

    #[test]
    fn a_screenshot_stays_png() {
        let p = prepare(&png_of(&screenshot(1200, 800))).expect("prepare");
        assert_eq!((p.mime, p.width, p.height), ("image/png", 1200, 800));
    }

    #[test]
    fn a_photo_becomes_jpeg() {
        let p = prepare(&png_of(&photo(1200, 800))).expect("prepare");
        assert_eq!(p.mime, "image/jpeg");
        assert!(p.bytes.len() < 400 * 1024, "{} bytes", p.bytes.len());
    }

    #[test]
    fn transparency_stays_png() {
        let img = RgbaImage::from_fn(300, 200, |x, _| Rgba([200, 30, 30, (x % 256) as u8]));
        let p = prepare(&png_of(&DynamicImage::ImageRgba8(img))).expect("prepare");
        assert_eq!(p.mime, "image/png");
        let back = image::load_from_memory(&p.bytes).expect("decode");
        assert!(back.color().has_alpha());
    }

    #[test]
    fn large_images_are_scaled_to_the_long_edge() {
        let p = prepare(&png_of(&screenshot(5120, 2880))).expect("prepare");
        assert_eq!((p.width, p.height), (2560, 1440));
        let p = prepare(&png_of(&screenshot(1000, 4000))).expect("prepare");
        assert_eq!((p.width, p.height), (640, 2560));
    }

    #[test]
    fn exif_does_not_survive() {
        // A JPEG with an EXIF segment (APP1) right after its start.
        let mut jpeg = Vec::new();
        photo(400, 300)
            .to_rgb8()
            .write_with_encoder(image::codecs::jpeg::JpegEncoder::new(&mut jpeg))
            .expect("jpeg");
        let payload = b"Exif\0\0GPS 21.0285N 105.8542E";
        let len = (payload.len() + 2) as u16;
        let mut segment = vec![0xFF, 0xE1];
        segment.extend_from_slice(&len.to_be_bytes());
        segment.extend_from_slice(payload);
        jpeg.splice(2..2, segment);
        assert!(jpeg.windows(4).any(|w| w == b"Exif"));
        let p = prepare(&jpeg).expect("prepare");
        assert!(!p.bytes.windows(4).any(|w| w == b"Exif"));
        assert!(!p.bytes.windows(3).any(|w| w == b"GPS"));
    }

    #[test]
    fn a_bomb_is_refused_before_decoding() {
        // A PNG whose header claims 100,000 × 100,000 pixels and holds none.
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut ihdr = b"IHDR".to_vec();
        ihdr.extend_from_slice(&100_000u32.to_be_bytes());
        ihdr.extend_from_slice(&100_000u32.to_be_bytes());
        ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
        let mut chunk = |kind_and_data: &[u8]| {
            png.extend_from_slice(&((kind_and_data.len() - 4) as u32).to_be_bytes());
            png.extend_from_slice(kind_and_data);
            png.extend_from_slice(&crc32(kind_and_data).to_be_bytes());
        };
        chunk(&ihdr);
        // A little compressed data, as a bomb would start; the rest never comes.
        chunk(b"IDAT\x78\x9c\x63\x00\x00");
        chunk(b"IEND");
        assert_eq!(
            prepare(&png),
            Err(PrepareError::TooManyPixels {
                width: 100_000,
                height: 100_000
            })
        );
    }

    #[test]
    fn what_is_not_an_image_is_refused() {
        assert!(matches!(
            prepare(b"#!/bin/sh\necho not an image\n"),
            Err(PrepareError::Unreadable(_))
        ));
    }

    /// CRC-32 (IEEE), as PNG chunks carry it.
    fn crc32(data: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for &b in data {
            crc ^= u32::from(b);
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }
}
