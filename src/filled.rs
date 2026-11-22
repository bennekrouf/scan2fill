//! The form filled in: the answers written on the page photo where api0 said
//! each blank is, shown on screen (SVG) and kept as a one-page PDF.
//!
//! Both are drawn from the same marks, so what is seen is what is shared. A
//! field api0 gave no place for is left off the page; the screen says how many.

use crate::api0::{Area, Field};
use crate::photo::Page;
use base64::Engine;

/// Answer ink: a dark blue, like a pen.
const INK: (f32, f32, f32) = (0.10, 0.20, 0.62);
const INK_HEX: &str = "#1a3399";
/// A4 width in points: the PDF page is that wide, as tall as the photo needs.
const PDF_WIDTH: f32 = 595.0;

/// One thing to write on the page.
#[derive(Clone, Debug, PartialEq)]
pub struct Mark {
    pub area: Area,
    pub text: String,
    /// A tick in an option's box rather than written text.
    pub tick: bool,
}

/// What goes on the page, and how many answers had nowhere to go.
pub fn marks(fields: &[Field], answers: &[String]) -> (Vec<Mark>, usize) {
    let mut out = Vec::new();
    let mut unplaced = 0;
    for (i, f) in fields.iter().enumerate() {
        let answer = answers.get(i).map(|a| a.trim()).unwrap_or("");
        if answer.is_empty() {
            continue;
        }
        if let Some(area) = chosen(f, answer) {
            out.push(Mark { area, text: "X".into(), tick: true });
        } else if let Some(area) = f.area {
            out.push(Mark { area, text: answer.to_string(), tick: false });
        } else {
            unplaced += 1;
        }
    }
    (out, unplaced)
}

/// The tick box of the option `answer` names, if it names one.
fn chosen(f: &Field, answer: &str) -> Option<Area> {
    let a = answer.to_lowercase();
    f.choices
        .iter()
        .find(|c| !c.label.trim().is_empty() && c.label.trim().to_lowercase() == a)
        .or_else(|| f.choices.iter().find(|c| !c.label.trim().is_empty() && a.contains(&c.label.trim().to_lowercase())))
        .and_then(|c| c.area)
}

/// Where and how big to write `m` on a page `w` by `h` (any unit): the left
/// of the text, its baseline from the top, and its size.
fn place(m: &Mark, w: f32, h: f32) -> (f32, f32, f32) {
    let (bx, by, bw, bh) = (m.area.x * w, m.area.y * h, m.area.w * w, m.area.h * h);
    if m.tick {
        let size = (bh.min(bw) * 1.1).clamp(w * 0.012, w * 0.04);
        return (bx + (bw - size * 0.6) / 2.0, by + (bh + size * 0.7) / 2.0, size);
    }
    // As tall as the blank allows, within a readable range, then narrowed
    // until the text fits its width (Helvetica averages about half an em).
    let chars = m.text.chars().count().max(1) as f32;
    let size = (bh * 0.8)
        .clamp(w * 0.014, w * 0.026)
        .min((bw - w * 0.004).max(w * 0.01) / (0.52 * chars))
        .max(w * 0.009);
    (bx + w * 0.004, by + bh - (bh - size * 0.75).max(0.0) * 0.35 - size * 0.12, size)
}

/// The filled page as an SVG for the screen.
pub fn svg(page: &Page, marks: &[Mark]) -> String {
    let (w, h) = (page.width as f32, page.height as f32);
    let img = base64::engine::general_purpose::STANDARD.encode(&page.jpeg);
    let mut s = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" width="100%"><image href="data:image/jpeg;base64,{img}" x="0" y="0" width="{w}" height="{h}"/>"#
    );
    for m in marks {
        let (x, y, size) = place(m, w, h);
        let weight = if m.tick { "700" } else { "400" };
        s.push_str(&format!(
            r#"<text x="{x:.1}" y="{y:.1}" font-family="Helvetica, Arial, sans-serif" font-size="{size:.1}" font-weight="{weight}" fill="{INK_HEX}">{}</text>"#,
            xml_escape(&m.text)
        ));
    }
    s.push_str("</svg>");
    s
}

fn xml_escape(t: &str) -> String {
    t.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The filled page as a one-page PDF: the photo, with the answers as text
/// over it (Helvetica, which every PDF reader has, so nothing is embedded).
pub fn pdf(page: &Page, marks: &[Mark]) -> Vec<u8> {
    let pw = PDF_WIDTH;
    let ph = PDF_WIDTH * page.height as f32 / page.width.max(1) as f32;

    let mut content = format!("q {pw:.2} 0 0 {ph:.2} 0 0 cm /Im0 Do Q\n").into_bytes();
    let (r, g, b) = INK;
    for m in marks {
        let (x, top, size) = place(m, pw, ph);
        let font = if m.tick { "F2" } else { "F1" };
        content.extend_from_slice(format!("BT /{font} {size:.2} Tf {r} {g} {b} rg {x:.2} {:.2} Td (", ph - top).as_bytes());
        content.extend_from_slice(&pdf_text(&m.text));
        content.extend_from_slice(b") Tj ET\n");
    }

    let mut out: Vec<u8> = b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n".to_vec();
    let mut offsets = Vec::new();
    let mut object = |out: &mut Vec<u8>, body: &[u8]| {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", offsets.len()).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    };
    object(&mut out, b"<< /Type /Catalog /Pages 2 0 R >>");
    object(&mut out, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
    object(
        &mut out,
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {pw:.2} {ph:.2}] /Resources << /XObject << /Im0 4 0 R >> /Font << /F1 5 0 R /F2 6 0 R >> >> /Contents 7 0 R >>"
        )
        .as_bytes(),
    );
    let mut image = format!(
        "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n",
        page.width,
        page.height,
        page.jpeg.len()
    )
    .into_bytes();
    image.extend_from_slice(&page.jpeg);
    image.extend_from_slice(b"\nendstream");
    object(&mut out, &image);
    object(&mut out, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>");
    object(&mut out, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>");
    let mut stream = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
    stream.extend_from_slice(&content);
    stream.extend_from_slice(b"\nendstream");
    object(&mut out, &stream);

    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1).as_bytes());
    for o in &offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", offsets.len() + 1).as_bytes());
    out
}

/// `t` in WinAnsi, escaped for a PDF string: Latin letters and accents
/// (French, German, Italian…) come through; what WinAnsi lacks becomes "?".
fn pdf_text(t: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for c in t.chars() {
        let b = match c {
            '€' => 0x80,
            '‘' | '’' => b'\'',
            '“' | '”' => b'"',
            '–' => 0x96,
            '—' => 0x97,
            '…' => 0x85,
            '\u{a0}' => b' ',
            c if (' '..='~').contains(&c) || ('\u{a1}'..='\u{ff}').contains(&c) => c as u32 as u8,
            _ => b'?',
        };
        if matches!(b, b'(' | b')' | b'\\') {
            out.push(b'\\');
        }
        out.push(b);
    }
    out
}

/// A file name for the PDF, from the document's title.
pub fn file_name(title: &str) -> String {
    let base: String = title
        .chars()
        .map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let base: String = base.chars().take(60).collect();
    if base.is_empty() { "Filled form.pdf".into() } else { format!("{base} - filled.pdf") }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api0::Choice;

    fn area(x: f32, y: f32, w: f32, h: f32) -> Option<Area> {
        Some(Area { x, y, w, h })
    }

    #[test]
    fn answers_go_in_their_box_ticks_in_the_chosen_option() {
        let fields = vec![
            Field { label: "Nom".into(), area: area(0.2, 0.1, 0.5, 0.03), ..Default::default() },
            Field {
                label: "Véhicule".into(),
                choices: vec![
                    Choice { label: "à mon nom".into(), area: area(0.1, 0.5, 0.02, 0.015) },
                    Choice { label: "en leasing".into(), area: area(0.5, 0.5, 0.02, 0.015) },
                ],
                ..Default::default()
            },
            Field { label: "Téléphone".into(), area: None, ..Default::default() },
            Field { label: "E-mail".into(), area: area(0.2, 0.3, 0.5, 0.03), ..Default::default() },
        ];
        let answers = vec!["Dupont Jean".into(), "En leasing".into(), "079 000 00 00".into(), "  ".into()];
        let (m, unplaced) = marks(&fields, &answers);
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].text, "Dupont Jean");
        assert!(m[1].tick && (m[1].area.x - 0.5).abs() < 1e-6);
        assert_eq!(unplaced, 1, "the phone has no box; the empty e-mail is not counted");
    }

    #[test]
    fn the_pdf_is_well_formed() {
        let img = image::DynamicImage::new_rgb8(40, 60);
        let mut jpeg = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut jpeg), image::ImageFormat::Jpeg).unwrap();
        let page = Page { jpeg, width: 40, height: 60, lines: Vec::new() };
        let marks = vec![Mark { area: Area { x: 0.1, y: 0.1, w: 0.8, h: 0.05 }, text: "Zoë (né) à Genève".into(), tick: false }];
        let pdf = pdf(&page, &marks);
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.starts_with("%PDF-1.4"));
        assert!(text.trim_end().ends_with("%%EOF"));
        // The xref points at each object.
        let start: usize = text.rsplit("startxref\n").next().unwrap().lines().next().unwrap().parse().unwrap();
        assert!(pdf[start..].starts_with(b"xref"));
        for n in 1..=7 {
            assert!(text.contains(&format!("\n{n} 0 obj\n")));
        }
        let latin: &[u8] = b"Zo\xeb \\(n\xe9\\) \xe0 G";
        assert!(pdf.windows(latin.len()).any(|w| w == latin), "accents in WinAnsi, brackets escaped");
    }

    #[test]
    fn file_names_are_tidy() {
        assert_eq!(file_name("Demande de carte / résident"), "Demande de carte résident - filled.pdf");
        assert_eq!(file_name("  "), "Filled form.pdf");
    }
}
