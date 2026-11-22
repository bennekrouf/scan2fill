//! Getting a phone photo small enough to send.
//!
//! A phone photo is 3–12 MB; base64 adds a third, and api0 takes 8 MiB. Text
//! on a page stays legible at 1600 px on the long side, so the photo is turned
//! upright (EXIF orientation), shrunk to that and re-encoded as JPEG.

use image::{codecs::jpeg::JpegEncoder, DynamicImage, ImageDecoder, ImageReader};
use std::io::Cursor;

const LONG_SIDE: u32 = 1600;
const QUALITY: u8 = 82;

/// The page as sent to api0: the boxes it answers with are on this image.
#[derive(Clone, Debug, PartialEq)]
pub struct Page {
    pub jpeg: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// The text printed on it, read on the phone (empty where there is no
    /// reader, or it failed).
    pub lines: Vec<crate::layout::Line>,
}

pub fn prepare(bytes: &[u8]) -> Result<Page, String> {
    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| format!("could not read the photo: {e}"))?;
    let mut decoder = reader
        .into_decoder()
        .map_err(|_| "This photo's format is not supported. Use a JPEG or PNG.".to_string())?;
    let orientation = decoder.orientation().ok();
    let mut img = DynamicImage::from_decoder(decoder).map_err(|e| format!("could not read the photo: {e}"))?;
    if let Some(o) = orientation {
        img.apply_orientation(o);
    }
    if img.width().max(img.height()) > LONG_SIDE {
        img = img.resize(LONG_SIDE, LONG_SIDE, image::imageops::FilterType::Triangle);
    }
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, QUALITY)
        .encode_image(&img.to_rgb8())
        .map_err(|e| format!("could not prepare the photo: {e}"))?;
    Ok(Page { jpeg: out, width: img.width(), height: img.height(), lines: Vec::new() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shrinks_a_large_image() {
        let big = DynamicImage::new_rgb8(4000, 3000);
        let mut png = Vec::new();
        big.write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let page = prepare(&png).unwrap();
        let back = image::load_from_memory(&page.jpeg).unwrap();
        assert_eq!((back.width(), back.height()), (1600, 1200));
        assert_eq!((page.width, page.height), (1600, 1200));
    }
}
