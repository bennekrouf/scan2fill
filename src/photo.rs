//! Getting a phone photo small enough to send.
//!
//! A phone photo is 3–12 MB; base64 adds a third, and api0 takes 8 MiB. Text
//! on a page stays legible at 1600 px on the long side, so the photo is turned
//! upright (EXIF orientation), the page in it straightened when it was not
//! scanned (`flatten`), shrunk to that, turned upright (`upright`) and
//! re-encoded as JPEG.

use image::{codecs::jpeg::JpegEncoder, DynamicImage, ImageDecoder, ImageReader};
use std::io::Cursor;

const LONG_SIDE: u32 = 1600;
/// The page is looked for and straightened at this size, before the last shrink.
const FIND_SIDE: u32 = 2400;
const QUALITY: u8 = 82;

/// Where the photo comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// The system's document scanner: flat and cropped already.
    Scanner,
    /// A photo, which may show the page at an angle on a table.
    Photo,
}

/// The page as sent to api0: the boxes it answers with are on this image.
#[derive(Clone, Debug, PartialEq)]
pub struct Page {
    pub jpeg: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// The text printed on it, read on the phone (empty where there is no
    /// reader, or it failed).
    pub lines: Vec<crate::layout::Line>,
    /// Why the phone couldn't read the text, when it couldn't.
    pub text_problem: Option<String>,
}

pub fn prepare(bytes: &[u8], source: Source) -> Result<Page, String> {
    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| format!("could not read the photo: {e}"))?;
    let mut decoder = reader
        .into_decoder()
        .map_err(|_| crate::i18n::msg("This photo's format is not supported. Use a JPEG or PNG.", "Le format de cette photo n'est pas pris en charge. Utilisez un JPEG ou un PNG."))?;
    let orientation = decoder.orientation().ok();
    let mut img = DynamicImage::from_decoder(decoder)
        .map_err(|e| format!("could not read the photo: {e}"))?;
    if let Some(o) = orientation {
        img.apply_orientation(o);
    }
    if source == Source::Photo {
        if img.width().max(img.height()) > FIND_SIDE {
            img = img.resize(FIND_SIDE, FIND_SIDE, image::imageops::FilterType::Triangle);
        }
        if let Some(corners) = find_page(&img) {
            img = crate::flatten::flatten(&img, corners);
        }
    }
    if img.width().max(img.height()) > LONG_SIDE {
        img = img.resize(LONG_SIDE, LONG_SIDE, image::imageops::FilterType::Triangle);
    }
    // The photo is upright now, but the page in it may not be.
    img = match crate::upright::turns(&img) {
        1 => img.rotate90(),
        2 => img.rotate180(),
        3 => img.rotate270(),
        _ => img,
    };
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, QUALITY)
        .encode_image(&img.to_rgb8())
        .map_err(|e| format!("could not prepare the photo: {e}"))?;
    Ok(Page {
        jpeg: out,
        width: img.width(),
        height: img.height(),
        lines: Vec::new(),
        text_problem: None,
    })
}

/// The page's corners on `img`, where the phone (or, failing it, `detect`)
/// can find them and they look like a whole page.
fn find_page(img: &DynamicImage) -> Option<crate::flatten::Corners> {
    let mut jpeg = Vec::new();
    let by_phone = JpegEncoder::new_with_quality(&mut jpeg, 90)
        .encode_image(&img.to_rgb8())
        .ok()
        .and_then(|()| crate::sheet::find_page(&jpeg))
        .filter(crate::flatten::Corners::plausible);
    by_phone.or_else(|| crate::flatten::detect(img).filter(crate::flatten::Corners::plausible))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shrinks_a_large_image() {
        let big = DynamicImage::new_rgb8(4000, 3000);
        let mut png = Vec::new();
        big.write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let page = prepare(&png, Source::Scanner).unwrap();
        let back = image::load_from_memory(&page.jpeg).unwrap();
        assert_eq!((back.width(), back.height()), (1600, 1200));
        assert_eq!((page.width, page.height), (1600, 1200));
    }
}
