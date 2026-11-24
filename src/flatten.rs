//! Straightening a page photographed at an angle.
//!
//! The phone finds the page's four corners on the photo (Apple's Vision on
//! iOS), or `detect` does where it can't: the page is the light shape on a
//! darker table. The page between them is drawn onto a flat rectangle,
//! cropping away the table around it. A page from the system's document
//! scanner arrives flat already.

use image::{DynamicImage, Rgb, RgbImage};

/// The page's corners on the photo, in fractions of its width and height from
/// the top-left corner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Corners {
    pub top_left: (f32, f32),
    pub top_right: (f32, f32),
    pub bottom_right: (f32, f32),
    pub bottom_left: (f32, f32),
}

/// The flat page is at most this long, in pixels.
const MAX_SIDE: f64 = 3000.0;

impl Corners {
    /// Whether these corners are worth straightening: a page that fills
    /// enough of the photo, its corners in order. A small or twisted shape
    /// is more likely a box printed on the page than the page itself.
    pub fn plausible(&self) -> bool {
        let p = [self.top_left, self.top_right, self.bottom_right, self.bottom_left];
        let inside = p.iter().all(|&(x, y)| (-0.02..=1.02).contains(&x) && (-0.02..=1.02).contains(&y));
        // Twice the area by the shoelace formula: positive when the corners
        // go clockwise on the photo, as listed.
        let area2: f32 = (0..4)
            .map(|i| {
                let (a, b) = (p[i], p[(i + 1) % 4]);
                a.0 * b.1 - b.0 * a.1
            })
            .sum();
        inside && area2 / 2.0 > 0.2
    }
}

/// The page's corners on `img`, found as the largest light shape on it: a
/// sheet of paper on a darker table. `None` when nothing stands out that
/// way, or the shape is not four-sided (a hand over the page, a light table).
pub fn detect(img: &DynamicImage) -> Option<Corners> {
    // Small and slightly blurred: the page's shape matters, not its print.
    let small = img.thumbnail(SEARCH_SIDE, SEARCH_SIDE).blur(1.5).to_luma8();
    let (w, h) = (small.width() as usize, small.height() as usize);
    let light = otsu(small.as_raw());
    let mask: Vec<bool> = small.as_raw().iter().map(|&v| v > light).collect();
    // The print is darker than the paper: close its gaps so the page is one
    // shape, by taking each pixel light when most around it are.
    let mask = majority(&mask, w, h, 3);

    let (shape, area) = largest_shape(&mask, w, h)?;
    if (area as f32) < 0.2 * (w * h) as f32 {
        return None;
    }
    let (mut tl, mut tr, mut br, mut bl) = ((0, 0), (0, 0), (0, 0), (0, 0));
    let (mut tl_v, mut tr_v, mut br_v, mut bl_v) = (i64::MAX, i64::MIN, i64::MIN, i64::MAX);
    for (i, _) in shape.iter().enumerate().filter(|(_, &on)| on) {
        let (x, y) = ((i % w) as i64, (i / w) as i64);
        if x + y < tl_v {
            (tl_v, tl) = (x + y, (x, y));
        }
        if x + y > br_v {
            (br_v, br) = (x + y, (x, y));
        }
        if x - y > tr_v {
            (tr_v, tr) = (x - y, (x, y));
        }
        if x - y < bl_v {
            (bl_v, bl) = (x - y, (x, y));
        }
    }
    let at = |(x, y): (i64, i64)| ((x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32);
    let corners = Corners {
        top_left: at(tl),
        top_right: at(tr),
        bottom_right: at(br),
        bottom_left: at(bl),
    };
    // Four-sided: the shape fills the shape its corners make.
    let p = [corners.top_left, corners.top_right, corners.bottom_right, corners.bottom_left];
    let quad: f32 = (0..4)
        .map(|i| {
            let (a, b) = (p[i], p[(i + 1) % 4]);
            a.0 * b.1 - b.0 * a.1
        })
        .sum::<f32>()
        / 2.0;
    let filled = area as f32 / (w * h) as f32;
    (quad > 0.0 && (filled / quad - 1.0).abs() < 0.12).then_some(corners)
}

/// The page is looked for on a photo this long.
const SEARCH_SIDE: u32 = 320;

/// The grey level that best splits `pixels` into dark and light (Otsu).
fn otsu(pixels: &[u8]) -> u8 {
    let mut hist = [0u64; 256];
    for &p in pixels {
        hist[p as usize] += 1;
    }
    let total = pixels.len() as f64;
    let sum: f64 = hist.iter().enumerate().map(|(i, &c)| i as f64 * c as f64).sum();
    let (mut dark_n, mut dark_sum, mut best, mut at) = (0.0, 0.0, 0.0, 128u8);
    for (t, &c) in hist.iter().enumerate() {
        dark_n += c as f64;
        dark_sum += t as f64 * c as f64;
        let light_n = total - dark_n;
        if dark_n == 0.0 || light_n == 0.0 {
            continue;
        }
        let gap = dark_sum / dark_n - (sum - dark_sum) / light_n;
        let between = dark_n * light_n * gap * gap;
        if between > best {
            (best, at) = (between, t as u8);
        }
    }
    at
}

/// Each pixel set when most of the square of radius `r` around it is.
fn majority(mask: &[bool], w: usize, h: usize, r: usize) -> Vec<bool> {
    // Sums over rectangles from a table of running sums.
    let mut sums = vec![0u32; (w + 1) * (h + 1)];
    for y in 0..h {
        for x in 0..w {
            sums[(y + 1) * (w + 1) + x + 1] = u32::from(mask[y * w + x])
                + sums[y * (w + 1) + x + 1]
                + sums[(y + 1) * (w + 1) + x]
                - sums[y * (w + 1) + x];
        }
    }
    let mut out = vec![false; w * h];
    for y in 0..h {
        for x in 0..w {
            let (x0, y0) = (x.saturating_sub(r), y.saturating_sub(r));
            let (x1, y1) = ((x + r + 1).min(w), (y + r + 1).min(h));
            let on = sums[y1 * (w + 1) + x1] + sums[y0 * (w + 1) + x0]
                - sums[y0 * (w + 1) + x1]
                - sums[y1 * (w + 1) + x0];
            out[y * w + x] = on * 2 > ((x1 - x0) * (y1 - y0)) as u32;
        }
    }
    out
}

/// The largest connected set shape in `mask`, and how many pixels it has.
fn largest_shape(mask: &[bool], w: usize, h: usize) -> Option<(Vec<bool>, usize)> {
    let mut label = vec![0u32; w * h];
    let (mut best, mut best_size, mut next) = (0u32, 0usize, 0u32);
    let mut stack = Vec::new();
    for start in 0..w * h {
        if !mask[start] || label[start] != 0 {
            continue;
        }
        next += 1;
        label[start] = next;
        stack.push(start);
        let mut size = 0;
        while let Some(i) = stack.pop() {
            size += 1;
            let (x, y) = (i % w, i / w);
            let around = [
                (x > 0).then(|| i - 1),
                (x + 1 < w).then(|| i + 1),
                (y > 0).then(|| i - w),
                (y + 1 < h).then(|| i + w),
            ];
            for j in around.into_iter().flatten() {
                if mask[j] && label[j] == 0 {
                    label[j] = next;
                    stack.push(j);
                }
            }
        }
        if size > best_size {
            (best, best_size) = (next, size);
        }
    }
    (best_size > 0).then(|| (label.iter().map(|&l| l == best).collect(), best_size))
}

/// The page between `corners`, drawn flat.
pub fn flatten(img: &DynamicImage, corners: Corners) -> DynamicImage {
    let src = img.to_rgb8();
    let (w, h) = (f64::from(src.width()), f64::from(src.height()));
    let px = |(x, y): (f32, f32)| (f64::from(x) * w, f64::from(y) * h);
    let (tl, tr, br, bl) = (
        px(corners.top_left),
        px(corners.top_right),
        px(corners.bottom_right),
        px(corners.bottom_left),
    );
    let dist = |a: (f64, f64), b: (f64, f64)| ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt();
    let mut out_w = dist(tl, tr).max(dist(bl, br));
    let mut out_h = dist(tl, bl).max(dist(tr, br));
    let scale = (MAX_SIDE / out_w.max(out_h)).min(1.0);
    out_w = (out_w * scale).round().max(1.0);
    out_h = (out_h * scale).round().max(1.0);

    let Some(to_photo) = homography(
        [(0.0, 0.0), (out_w, 0.0), (out_w, out_h), (0.0, out_h)],
        [tl, tr, br, bl],
    ) else {
        return img.clone();
    };
    let mut out = RgbImage::new(out_w as u32, out_h as u32);
    for (x, y, pixel) in out.enumerate_pixels_mut() {
        let (u, v) = apply(&to_photo, f64::from(x) + 0.5, f64::from(y) + 0.5);
        *pixel = sample(&src, u - 0.5, v - 0.5);
    }
    DynamicImage::ImageRgb8(out)
}

/// The 3×3 matrix (its last entry 1) that takes each of `from` to `to`.
fn homography(from: [(f64, f64); 4], to: [(f64, f64); 4]) -> Option<[f64; 9]> {
    // Eight equations in the eight unknowns h0..h7:
    //   u = (h0 x + h1 y + h2) / (h6 x + h7 y + 1)
    //   v = (h3 x + h4 y + h5) / (h6 x + h7 y + 1)
    let mut m = [[0f64; 9]; 8];
    for i in 0..4 {
        let ((x, y), (u, v)) = (from[i], to[i]);
        m[2 * i] = [x, y, 1.0, 0.0, 0.0, 0.0, -u * x, -u * y, u];
        m[2 * i + 1] = [0.0, 0.0, 0.0, x, y, 1.0, -v * x, -v * y, v];
    }
    // Gaussian elimination with partial pivoting.
    for col in 0..8 {
        let pivot = (col..8).max_by(|&a, &b| m[a][col].abs().total_cmp(&m[b][col].abs()))?;
        if m[pivot][col].abs() < 1e-12 {
            return None;
        }
        m.swap(col, pivot);
        let pivot_row = m[col];
        for (row, r) in m.iter_mut().enumerate() {
            if row != col {
                let f = r[col] / pivot_row[col];
                for (cell, p) in r.iter_mut().zip(pivot_row).skip(col) {
                    *cell -= f * p;
                }
            }
        }
    }
    let mut h = [0f64; 9];
    for i in 0..8 {
        h[i] = m[i][8] / m[i][i];
    }
    h[8] = 1.0;
    Some(h)
}

fn apply(h: &[f64; 9], x: f64, y: f64) -> (f64, f64) {
    let d = h[6] * x + h[7] * y + h[8];
    ((h[0] * x + h[1] * y + h[2]) / d, (h[3] * x + h[4] * y + h[5]) / d)
}

/// The colour at (x, y) between pixel centres; white outside the photo.
fn sample(img: &RgbImage, x: f64, y: f64) -> Rgb<u8> {
    let (w, h) = (i64::from(img.width()), i64::from(img.height()));
    let (x0, y0) = (x.floor() as i64, y.floor() as i64);
    let (fx, fy) = (x - x0 as f64, y - y0 as f64);
    let at = |px: i64, py: i64| -> [f64; 3] {
        if px < 0 || py < 0 || px >= w || py >= h {
            return [255.0; 3];
        }
        let p = img.get_pixel(px as u32, py as u32).0;
        [f64::from(p[0]), f64::from(p[1]), f64::from(p[2])]
    };
    let (a, b, c, d) = (at(x0, y0), at(x0 + 1, y0), at(x0, y0 + 1), at(x0 + 1, y0 + 1));
    let mut out = [0u8; 3];
    for i in 0..3 {
        let top = a[i] + (b[i] - a[i]) * fx;
        let bottom = c[i] + (d[i] - c[i]) * fx;
        out[i] = (top + (bottom - top) * fy).round().clamp(0.0, 255.0) as u8;
    }
    Rgb(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A grey photo with a white page on it, its corners where `corners` says,
    /// and a black square printed in the page's top-left quarter.
    fn photo(corners: Corners) -> DynamicImage {
        let (w, h) = (800u32, 600u32);
        let page = |x: f64, y: f64| -> Option<(f64, f64)> {
            // Where (x, y) is on the page, from 0 to 1 each way, if on it.
            let to_page = homography(
                [
                    (f64::from(corners.top_left.0) * 800.0, f64::from(corners.top_left.1) * 600.0),
                    (f64::from(corners.top_right.0) * 800.0, f64::from(corners.top_right.1) * 600.0),
                    (f64::from(corners.bottom_right.0) * 800.0, f64::from(corners.bottom_right.1) * 600.0),
                    (f64::from(corners.bottom_left.0) * 800.0, f64::from(corners.bottom_left.1) * 600.0),
                ],
                [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)],
            )?;
            let (u, v) = apply(&to_page, x, y);
            ((0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v)).then_some((u, v))
        };
        let img = RgbImage::from_fn(w, h, |x, y| match page(f64::from(x) + 0.5, f64::from(y) + 0.5) {
            Some((u, v)) if (0.1..0.4).contains(&u) && (0.1..0.4).contains(&v) => Rgb([0, 0, 0]),
            Some(_) => Rgb([250, 250, 250]),
            None => Rgb([90, 90, 90]),
        });
        DynamicImage::ImageRgb8(img)
    }

    fn askew() -> Corners {
        Corners {
            top_left: (0.20, 0.10),
            top_right: (0.85, 0.15),
            bottom_right: (0.80, 0.90),
            bottom_left: (0.10, 0.85),
        }
    }

    #[test]
    fn a_page_at_an_angle_comes_out_flat_without_the_table() {
        let flat = flatten(&photo(askew()), askew()).to_rgb8();
        let (w, h) = (flat.width(), flat.height());
        let at = |u: f32, v: f32| flat.get_pixel((u * w as f32) as u32, (v * h as f32) as u32).0[0];
        // The printed square is where it is on the page, and nothing of the
        // grey table is left near the edges.
        assert!(at(0.25, 0.25) < 40);
        assert!(at(0.6, 0.6) > 230);
        assert!(at(0.02, 0.5) > 230 && at(0.98, 0.5) > 230 && at(0.5, 0.02) > 230 && at(0.5, 0.98) > 230);
        assert!(at(0.45, 0.25) > 230 && at(0.25, 0.45) > 230);
    }

    #[test]
    fn a_light_page_on_a_dark_table_is_found() {
        let found = detect(&photo(askew())).unwrap();
        let near = |a: (f32, f32), b: (f32, f32)| (a.0 - b.0).abs() < 0.02 && (a.1 - b.1).abs() < 0.02;
        assert!(near(found.top_left, askew().top_left), "{found:?}");
        assert!(near(found.top_right, askew().top_right), "{found:?}");
        assert!(near(found.bottom_right, askew().bottom_right), "{found:?}");
        assert!(near(found.bottom_left, askew().bottom_left), "{found:?}");
    }

    #[test]
    fn nothing_is_found_on_a_photo_without_a_page() {
        let plain = DynamicImage::ImageRgb8(RgbImage::from_pixel(400, 300, Rgb([120, 120, 120])));
        assert_eq!(detect(&plain), None);
        // A light round shape is not a page.
        let disc = DynamicImage::ImageRgb8(RgbImage::from_fn(400, 300, |x, y| {
            let (dx, dy) = (x as f32 - 200.0, y as f32 - 150.0);
            if dx * dx + dy * dy < 120.0 * 120.0 { Rgb([250, 250, 250]) } else { Rgb([60, 60, 60]) }
        }));
        assert_eq!(detect(&disc), None);
    }

    #[test]
    fn small_or_twisted_shapes_are_not_taken_for_the_page() {
        assert!(askew().plausible());
        let small = Corners {
            top_left: (0.4, 0.4),
            top_right: (0.6, 0.4),
            bottom_right: (0.6, 0.6),
            bottom_left: (0.4, 0.6),
        };
        assert!(!small.plausible());
        let crossed = Corners {
            top_right: askew().bottom_left,
            bottom_left: askew().top_right,
            ..askew()
        };
        assert!(!crossed.plausible());
    }
}
