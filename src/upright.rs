//! Turning a page that lies sideways or upside down in the photo.
//!
//! A sheet photographed flat on a table often comes out sideways: the phone
//! can't tell which way is up when it looks straight down, so the photo's own
//! orientation says nothing about the page's. The answers are written across
//! the photo, so a sideways page gets them at right angles to its text.
//!
//! The page's text tells which way it is turned:
//! - its lines run along the rows of ink, so their direction says whether the
//!   page is sideways;
//! - in Latin script more letters reach up (b d f h k l t, capitals) than down
//!   (g j p q y), so a line of text has more ink above its middle band than
//!   below it, and the other way round when the page is upside down.

use image::DynamicImage;

/// The quarter turns clockwise that put the page upright: 0 to 3.
pub fn turns(img: &DynamicImage) -> u8 {
    let ink = Ink::of(img);
    let along_rows = ink.lineness();
    let turned = ink.turned_clockwise();
    let along_columns = turned.lineness();
    // Turn sideways only when the lines clearly run down the photo: a page
    // with little text, or a photo of something else, stays as it is.
    if along_columns > along_rows * SIDEWAYS {
        // Sideways, it is one way or the other: the likelier one.
        if turned.downness() > 0.0 {
            3
        } else {
            1
        }
    } else if ink.upside_down() {
        2
    } else {
        0
    }
}

/// How much more the text must line up down the photo than across it.
const SIDEWAYS: f64 = 1.2;
/// How much more ink must sit below the lines' middle than above them for an
/// upright-looking page to be turned over: printed text gives 0.6, a phone
/// photo of a letter 0.1 to 0.2.
const UPSIDE_DOWN: f64 = 0.1;
/// Columns of the photo looked at apart, so that a page photographed a little
/// askew still shows its lines.
const STRIPS: usize = 8;

/// Dark against what surrounds it, so shadows across the page don't count.
struct Ink {
    w: usize,
    h: usize,
    dark: Vec<bool>,
}

impl Ink {
    fn of(img: &DynamicImage) -> Ink {
        let gray = img.to_luma8();
        let (w, h) = (gray.width() as usize, gray.height() as usize);
        // Sums of every rectangle from the top-left corner, for the mean
        // brightness around each pixel.
        let mut sum = vec![0u64; (w + 1) * (h + 1)];
        for y in 0..h {
            let mut row = 0u64;
            for x in 0..w {
                row += u64::from(gray.as_raw()[y * w + x]);
                sum[(y + 1) * (w + 1) + x + 1] = sum[y * (w + 1) + x + 1] + row;
            }
        }
        let r = (w.max(h) / 60).max(4);
        let mut dark = vec![false; w * h];
        for y in 0..h {
            let (y0, y1) = (y.saturating_sub(r), (y + r + 1).min(h));
            for x in 0..w {
                let (x0, x1) = (x.saturating_sub(r), (x + r + 1).min(w));
                let total = sum[y1 * (w + 1) + x1] + sum[y0 * (w + 1) + x0]
                    - sum[y0 * (w + 1) + x1]
                    - sum[y1 * (w + 1) + x0];
                let mean = total as f64 / ((x1 - x0) * (y1 - y0)) as f64;
                let p = f64::from(gray.as_raw()[y * w + x]);
                dark[y * w + x] = p < mean * 0.8 && mean - p > 18.0;
            }
        }
        Ink { w, h, dark }
    }

    fn turned_clockwise(&self) -> Ink {
        let (w, h) = (self.h, self.w);
        let mut dark = vec![false; w * h];
        for y in 0..h {
            for x in 0..w {
                dark[y * w + x] = self.dark[(self.h - 1 - x) * self.w + y];
            }
        }
        Ink { w, h, dark }
    }

    /// The ink in each row of each strip of columns.
    fn profiles(&self) -> Vec<Vec<u32>> {
        let width = (self.w / STRIPS).max(1);
        (0..self.w / width)
            .map(|s| {
                (0..self.h)
                    .map(|y| {
                        let row = &self.dark[y * self.w + s * width..y * self.w + (s + 1) * width];
                        row.iter().filter(|d| **d).count() as u32
                    })
                    .collect()
            })
            .collect()
    }

    /// How much of the text is gaps between lines running across the photo:
    /// in a strip of columns, the empty rows between the first and the last
    /// with ink. Text running down the photo leaves few, as its lines fill in
    /// each other's gaps between letters.
    fn lineness(&self) -> f64 {
        let (mut gaps, mut rows) = (0usize, 0usize);
        for p in self.profiles() {
            let peak = *p.iter().max().unwrap_or(&0);
            let inked = |n: &u32| *n * 20 > peak;
            let (Some(first), Some(last)) = (p.iter().position(inked), p.iter().rposition(inked)) else {
                continue;
            };
            gaps += p[first..=last].iter().filter(|n| !inked(n)).count();
            rows += last + 1 - first;
        }
        if rows == 0 {
            0.0
        } else {
            gaps as f64 / rows as f64
        }
    }

    /// Whether the lines of text have clearly more ink below their middle
    /// band than above it.
    fn upside_down(&self) -> bool {
        self.downness() > UPSIDE_DOWN
    }

    /// From -1 (all the ink beside the lines' middle is above it) to 1 (all below).
    fn downness(&self) -> f64 {
        let (mut above, mut below) = (0u64, 0u64);
        for p in self.profiles() {
            for (start, end) in lines(&p) {
                let line = &p[start..end];
                let peak = *line.iter().max().unwrap_or(&0);
                let middle = |n: &u32| *n * 5 >= peak * 2;
                let (Some(top), Some(bottom)) =
                    (line.iter().position(middle), line.iter().rposition(middle))
                else {
                    continue;
                };
                above += line[..top].iter().map(|&n| u64::from(n)).sum::<u64>();
                below += line[bottom + 1..].iter().map(|&n| u64::from(n)).sum::<u64>();
            }
        }
        let all = (above + below) as f64;
        if all > 0.0 {
            (below as f64 - above as f64) / all
        } else {
            0.0
        }
    }
}

/// The runs of rows with ink in them that are the height of a line of text.
fn lines(profile: &[u32]) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    let mut start = None;
    for (y, &n) in profile.iter().chain(std::iter::once(&0)).enumerate() {
        match (start, n > 1) {
            (None, true) => start = Some(y),
            (Some(s), false) => {
                if (5..=80).contains(&(y - s)) {
                    found.push((s, y));
                }
                start = None;
            }
            _ => {}
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A page of text: lines of letters with ascenders ("l" and "b" reaching
    /// up from a round middle) and the odd descender ("p" reaching down).
    fn page() -> DynamicImage {
        let (w, h) = (600u32, 800u32);
        let mut img = image::GrayImage::from_pixel(w, h, image::Luma([235]));
        let mut ink = |x: u32, y: u32| {
            if x < w && y < h {
                img.put_pixel(x, y, image::Luma([30]));
            }
        };
        for line in 0..24 {
            let base = 60 + line * 28; // the line's baseline
            let length = if line % 5 == 4 { 260 } else { 480 };
            let mut x = 50;
            let mut letter = 0;
            while x < 50 + length {
                // The middle band, 8 px high, of every letter.
                for dx in 0..6 {
                    for dy in 0..8 {
                        ink(x + dx, base - dy);
                    }
                }
                // Every other letter reaches 6 px up; one in seven 6 px down.
                if letter % 2 == 0 {
                    for dy in 8..14 {
                        ink(x, base - dy);
                        ink(x + 1, base - dy);
                    }
                }
                if letter % 7 == 3 {
                    for dy in 1..7 {
                        ink(x + 4, base + dy);
                        ink(x + 5, base + dy);
                    }
                }
                letter += 1;
                x += if letter % 6 == 0 { 14 } else { 9 };
            }
        }
        DynamicImage::ImageLuma8(img)
    }

    #[test]
    fn an_upright_page_stays() {
        assert_eq!(turns(&page()), 0);
    }

    #[test]
    fn a_sideways_page_is_turned_back() {
        // Turned a quarter clockwise, it needs three more to be upright.
        assert_eq!(turns(&page().rotate90()), 3);
        assert_eq!(turns(&page().rotate270()), 1);
    }

    #[test]
    fn an_upside_down_page_is_turned_over() {
        assert_eq!(turns(&page().rotate180()), 2);
    }

    #[test]
    fn a_blank_photo_stays() {
        let blank = DynamicImage::ImageLuma8(image::GrayImage::from_pixel(400, 300, image::Luma([200])));
        assert_eq!(turns(&blank), 0);
    }

    /// Real photos, when SCAN2FIL_UPRIGHT_SAMPLES names a folder of
    /// `<turns>_<name>.jpg|png`: the turns each one needs.
    #[test]
    fn sample_photos_are_turned_as_named() {
        let Some(dir) = std::env::var_os("SCAN2FIL_UPRIGHT_SAMPLES") else {
            return;
        };
        let mut wrong = Vec::new();
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(expected) = name.split('_').next().and_then(|t| t.parse::<u8>().ok()) else {
                continue;
            };
            let img = image::open(entry.path()).unwrap();
            let got = turns(&img);
            println!("{name}: {got}");
            if got != expected {
                wrong.push(format!("{name}: {got}, expected {expected}"));
            }
        }
        assert!(wrong.is_empty(), "{wrong:?}");
    }
}
