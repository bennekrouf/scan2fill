//! Where answers go, from the text printed on the page.
//!
//! The model names each field's printed label ("Nom et prénom") and each
//! option's text ("12 mois (CHF 200.–)"), but its guesses of where they are
//! on the photo are rough. The phone's own text reader (Apple's Vision) gives
//! every printed word with its exact box, so each label is looked up among
//! those words: the answer goes right after it on the same line, and an
//! option's tick goes just before its text. A field whose label is not found
//! keeps the model's guess.

use crate::api0::{Area, Field};
use std::collections::HashSet;

/// A word read on the page, with its box (fractions of the page, top-left).
#[derive(Clone, Debug, PartialEq)]
pub struct Word {
    pub text: String,
    pub area: Area,
}

/// A line of text read on the page, its words left to right.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Line {
    pub words: Vec<Word>,
}

/// `fields`, with each answer's and each tick's place taken from the printed
/// text where its label is found there.
pub fn snap(fields: &[Field], lines: &[Line]) -> Vec<Field> {
    let mut lines: Vec<&Line> = lines.iter().filter(|l| !l.words.is_empty()).collect();
    lines.sort_by(|a, b| top(a).total_cmp(&top(b)));
    let mut used = HashSet::new();
    fields
        .iter()
        .map(|f| {
            let mut f = f.clone();
            for c in f.choices.iter_mut() {
                if let Some(found) = find(&lines, &c.label, &mut used) {
                    let o = union(&found.words);
                    // The tick box sits just left of the option's text.
                    c.area = Some(Area { x: (o.x - o.h * 1.4).max(0.0), y: o.y, w: o.h, h: o.h });
                }
            }
            if let Some(found) = find(&lines, &f.label, &mut used) {
                f.area = Some(after(&found));
            }
            f
        })
        .collect()
}

fn top(l: &Line) -> f32 {
    l.words.iter().map(|w| w.area.y).fold(f32::MAX, f32::min)
}

struct Found<'a> {
    line: &'a Line,
    words: Vec<Word>,
    /// Index of the first word after the label (its colon included).
    next: usize,
}

/// The first unused run of words in `lines` that spells `label`.
fn find<'a>(lines: &[&'a Line], label: &str, used: &mut HashSet<(usize, usize)>) -> Option<Found<'a>> {
    let wanted: Vec<String> = label.split_whitespace().map(fold).filter(|t| !t.is_empty()).collect();
    if wanted.is_empty() {
        return None;
    }
    // The model sometimes writes a longer label than is printed: the start of
    // it, of two words or more, is enough.
    let shortest = if wanted.len() == 1 { 1 } else { 2.max(wanted.len() / 2) };
    for len in (shortest..=wanted.len()).rev() {
        let want = &wanted[..len];
        for (li, line) in lines.iter().enumerate() {
            let tokens: Vec<(usize, String)> = line
                .words
                .iter()
                .enumerate()
                .map(|(i, w)| (i, fold(&w.text)))
                .filter(|(_, t)| !t.is_empty())
                .collect();
            'start: for s in 0..tokens.len() {
                if s + want.len() > tokens.len() {
                    break;
                }
                for (k, w) in want.iter().enumerate() {
                    let (wi, t) = &tokens[s + k];
                    let same = t == w || (k == 0 && t.ends_with(w.as_str()) && t.len() <= w.len() + 1);
                    if !same || used.contains(&(li, *wi)) {
                        continue 'start;
                    }
                }
                let first = tokens[s].0;
                let last = tokens[s + want.len() - 1].0;
                // The colon or dots right after the label belong to it.
                let mut next = last + 1;
                while next < line.words.len() && fold(&line.words[next].text).is_empty() && !is_blank(&line.words[next].text) {
                    next += 1;
                }
                for i in first..=last {
                    used.insert((li, i));
                }
                return Some(Found { line, words: line.words[first..next].to_vec(), next });
            }
        }
    }
    None
}

/// Underscores or dots drawn as the line to write on.
fn is_blank(t: &str) -> bool {
    t.chars().filter(|c| matches!(c, '_' | '.' | '…' | '-')).count() >= 3
}

fn union(words: &[Word]) -> Area {
    let x0 = words.iter().map(|w| w.area.x).fold(f32::MAX, f32::min);
    let y0 = words.iter().map(|w| w.area.y).fold(f32::MAX, f32::min);
    let x1 = words.iter().map(|w| w.area.x + w.area.w).fold(f32::MIN, f32::max);
    let y1 = words.iter().map(|w| w.area.y + w.area.h).fold(f32::MIN, f32::max);
    Area { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
}

/// The space for the answer: after the label, up to the next printed word on
/// its line (the next field's label), or under it when the line is full.
fn after(found: &Found) -> Area {
    let l = union(&found.words);
    let start = l.x + l.w + 0.008;
    let rest = &found.line.words[found.next..];
    let end = rest.iter().find(|w| !fold(&w.text).is_empty()).map(|w| w.area.x - 0.012).unwrap_or(0.96);
    // Written on the same line when the form leaves room there: a drawn
    // blank, or a colon after the label. A bare heading has its box below.
    let inline = rest.iter().any(|w| is_blank(&w.text)) || found.words.iter().any(|w| w.text.trim_end().ends_with(':'));
    if inline && end - start >= 0.05 {
        // As high as the whole printed line, not just the label's words (an
        // accent or a "p" makes those taller or shorter), so every answer on
        // the page is written the same size.
        let row = union(&found.line.words);
        Area { x: start, y: row.y - row.h * 0.1, w: end - start, h: row.h * 1.2 }
    } else {
        Area { x: l.x, y: l.y + l.h * 1.3, w: (0.96 - l.x).min(0.6), h: l.h * 1.2 }
    }
}

/// A word reduced to what matters for matching: lower case, no accents, only
/// letters and digits.
fn fold(t: &str) -> String {
    t.chars()
        .flat_map(char::to_lowercase)
        .filter_map(|c| {
            let c = match c {
                'à' | 'á' | 'â' | 'ä' | 'ã' | 'å' => 'a',
                'ç' => 'c',
                'è' | 'é' | 'ê' | 'ë' => 'e',
                'ì' | 'í' | 'î' | 'ï' => 'i',
                'ñ' => 'n',
                'ò' | 'ó' | 'ô' | 'ö' | 'õ' => 'o',
                'ù' | 'ú' | 'û' | 'ü' => 'u',
                'ÿ' | 'ý' => 'y',
                'ß' => 's',
                c => c,
            };
            c.is_alphanumeric().then_some(c)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api0::Choice;

    /// A line of words laid out left to right at `y`, each 0.01 high and as
    /// wide as its text (0.008 a letter, 0.006 between words).
    fn line(y: f32, x: f32, text: &str) -> Line {
        let mut x = x;
        let words = text
            .split(' ')
            .map(|t| {
                let w = 0.008 * t.chars().count() as f32;
                let word = Word { text: t.into(), area: Area { x, y, w, h: 0.01 } };
                x += w + 0.006;
                word
            })
            .collect();
        Line { words }
    }

    fn page() -> Vec<Line> {
        vec![
            line(0.30, 0.07, "Nom et prénom : ______________________"),
            line(0.40, 0.07, "NPA / Localité : __________ Téléphone : __________"),
            line(0.60, 0.07, "☐6 mois (CHF 120.–) ☐12 mois (CHF 200.–)"),
            line(0.80, 0.07, "Remarques"),
        ]
    }

    #[test]
    fn the_answer_goes_after_its_label_up_to_the_next_one() {
        let fields = vec![
            Field { label: "Nom et prénom".into(), ..Default::default() },
            Field { label: "NPA / Localité".into(), ..Default::default() },
            Field { label: "Téléphone".into(), ..Default::default() },
        ];
        let out = snap(&fields, &page());
        let name = out[0].area.unwrap();
        assert!((name.y - 0.299).abs() < 0.002, "on the label's line: {name:?}");
        // "Nom et prénom :" ends at 0.07 + 3 words and a colon.
        assert!(name.x > 0.19 && name.x < 0.22, "just after the colon: {name:?}");
        let npa = out[1].area.unwrap();
        let phone = out[2].area.unwrap();
        assert!(npa.x + npa.w < phone.x, "NPA stops before Téléphone: {npa:?} {phone:?}");
        assert!((phone.y - npa.y).abs() < 1e-6);
    }

    #[test]
    fn ticks_go_left_of_their_option_even_with_the_box_read_into_the_word() {
        let fields = vec![Field {
            label: "Durée".into(),
            choices: vec![
                Choice { label: "6 mois (CHF 120.–)".into(), area: None },
                Choice { label: "12 mois (CHF 200.–)".into(), area: None },
            ],
            ..Default::default()
        }];
        let out = snap(&fields, &page());
        let six = out[0].choices[0].area.unwrap();
        let twelve = out[0].choices[1].area.unwrap();
        assert!(six.x < twelve.x);
        assert!((twelve.y - 0.60).abs() < 1e-6);
        assert!(out[0].area.is_none(), "no printed 'Durée' on this page");
    }

    #[test]
    fn a_label_not_on_the_page_keeps_the_models_guess() {
        let guess = Some(Area { x: 0.5, y: 0.5, w: 0.2, h: 0.02 });
        let fields = vec![Field { label: "Numéro AVS".into(), area: guess, ..Default::default() }];
        assert_eq!(snap(&fields, &page())[0].area, guess);
    }

    #[test]
    fn a_label_alone_on_its_line_is_answered_under_it() {
        let fields = vec![Field { label: "Remarques".into(), ..Default::default() }];
        let r = snap(&fields, &page())[0].area.unwrap();
        assert!(r.y > 0.81, "under the label: {r:?}");
    }

    #[test]
    fn a_repeated_label_is_matched_in_order() {
        let lines = vec![line(0.2, 0.07, "Date : ________"), line(0.7, 0.07, "Date : ________")];
        let fields = vec![
            Field { label: "Date".into(), ..Default::default() },
            Field { label: "Date".into(), ..Default::default() },
        ];
        let out = snap(&fields, &lines);
        assert!(out[0].area.unwrap().y < 0.3 && out[1].area.unwrap().y > 0.6);
    }
}
