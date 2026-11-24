//! The person's signature: drawn once with a finger, kept with the settings,
//! offered on every field that asks for one, and drawn on the filled form.
//!
//! It is kept as pen strokes, not a picture, so it stays sharp at any size on
//! screen and in the PDF. Each point is `[x, y]` in units of the pad's width,
//! from its top-left corner.

use crate::api0::Field;
use dioxus::prelude::*;

/// Drawing on the pad, in the page: each stroke as it is drawn, then all the
/// strokes back to Rust when the finger lifts. Points closer than a couple of
/// pixels are dropped, and kept to four decimals, so the settings stay small.
/// The script may run before the pad is on the page: it waits for it.
const PAD_JS: &str = r#"
const start = (tries) => {
  const pad = document.getElementById("sig-pad");
  if (!pad || !pad.getBoundingClientRect().width) {
    if (tries > 0) requestAnimationFrame(() => start(tries - 1));
    return;
  }
  const ctx = pad.getContext("2d");
  const size = () => pad.getBoundingClientRect();
  const fit = () => {
    const r = size(), d = window.devicePixelRatio || 1;
    pad.width = r.width * d; pad.height = r.height * d;
    ctx.setTransform(d, 0, 0, d, 0, 0);
    ctx.lineWidth = Math.max(2.5, r.width * 0.009); ctx.lineCap = "round"; ctx.lineJoin = "round";
    ctx.strokeStyle = getComputedStyle(pad).color;
  };
  fit();
  let strokes = [], cur = null;
  const at = (e) => { const r = size(); return [(e.clientX - r.left) / r.width, (e.clientY - r.top) / r.width]; };
  const line = (a, b) => {
    const w = size().width;
    ctx.beginPath(); ctx.moveTo(a[0] * w, a[1] * w); ctx.lineTo(b[0] * w, b[1] * w); ctx.stroke();
  };
  pad.onpointerdown = (e) => {
    e.preventDefault();
    pad.setPointerCapture(e.pointerId);
    cur = [at(e)]; strokes.push(cur);
    line(cur[0], [cur[0][0] + 0.0005, cur[0][1]]);
  };
  pad.onpointermove = (e) => {
    if (!cur) return;
    const p = at(e), q = cur[cur.length - 1];
    if (Math.hypot(p[0] - q[0], p[1] - q[1]) * size().width < 2) return;
    cur.push(p); line(q, p);
  };
  const end = () => {
    if (!cur) return;
    cur = null;
    dioxus.send(strokes.map((s) => s.map((p) => [+p[0].toFixed(4), +p[1].toFixed(4)])));
  };
  pad.onpointerup = end; pad.onpointercancel = end;
  window.__scan2filClearPad = () => {
    strokes = []; cur = null;
    ctx.clearRect(0, 0, pad.width, pad.height);
    dioxus.send([]);
  };
};
start(120);
"#;

pub type Strokes = Vec<Vec<[f32; 2]>>;

/// The answer a signature field gets once it is signed: what Copy answers
/// shows, and what tells the filled form to draw the signature there.
pub const SIGNED: &str = "(signed)";

/// The words a signature field's label is likely to hold, for documents
/// where api0 did not say so. Not "firma": in German that is the company.
const WORDS: &[&str] = &[
    "signature",
    "sign here",
    "signed",
    "signé",
    "unterschrift",
    "handtekening",
    "assinatura",
    "podpis",
];

/// Whether the reader signs in `f` rather than writing in it.
pub fn is_signature(f: &Field) -> bool {
    f.signature || {
        let label = f.label.to_lowercase();
        WORDS.iter().any(|w| label.contains(w))
    }
}

/// The smallest box around every point: left, top, width, height.
pub fn bounds(strokes: &Strokes) -> Option<(f32, f32, f32, f32)> {
    let points = strokes.iter().flatten();
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    let mut any = false;
    for [x, y] in points {
        any = true;
        x0 = x0.min(*x);
        y0 = y0.min(*y);
        x1 = x1.max(*x);
        y1 = y1.max(*y);
    }
    // A dot or a straight line still has some size.
    any.then(|| (x0, y0, (x1 - x0).max(0.01), (y1 - y0).max(0.01)))
}

/// The strokes as an SVG path's `d`, moved by `(dx, dy)` and scaled by `k`.
pub fn path(strokes: &Strokes, dx: f32, dy: f32, k: f32) -> String {
    let mut d = String::new();
    for s in strokes {
        for (i, [x, y]) in s.iter().enumerate() {
            let (px, py) = (dx + x * k, dy + y * k);
            d.push_str(&format!(
                "{}{px:.2} {py:.2} ",
                if i == 0 { "M" } else { "L" }
            ));
        }
        // A tap is a dot: a line of no length, drawn with round caps.
        if s.len() == 1 {
            let [x, y] = s[0];
            d.push_str(&format!("L{:.2} {:.2} ", dx + x * k + 0.01, dy + y * k));
        }
    }
    d
}

/// The pen's width for strokes drawn `k` units per pad width: about what a
/// finger draws on the pad.
pub fn pen(k: f32) -> f32 {
    k * 0.009
}

/// The signature alone, as an SVG that fills its container's width.
pub fn preview(strokes: &Strokes) -> String {
    let Some((x, y, w, h)) = bounds(strokes) else {
        return String::new();
    };
    let k = 100.0 / w;
    let (vw, vh) = (100.0 + pen(k) * 2.0, h * k + pen(k) * 2.0);
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {vw:.2} {vh:.2}"><path d="{}" fill="none" stroke="currentColor" stroke-width="{:.2}" stroke-linecap="round" stroke-linejoin="round"/></svg>"#,
        path(strokes, -x * k + pen(k), -y * k + pen(k), k),
        pen(k)
    )
}

/// A pad to sign on with a finger, and its Clear button. `on_change` gets
/// the strokes after each one is drawn, and none after Clear.
#[component]
pub fn Pad(on_change: EventHandler<Strokes>) -> Element {
    // Not `onmounted`: on Android that event fails to cross into Rust, and
    // the page stops updating.
    use_future(move || async move {
        let mut pad = document::eval(PAD_JS);
        while let Ok(strokes) = pad.recv::<Strokes>().await {
            on_change.call(strokes);
        }
    });
    rsx! {
        div { class: "sig-pad-wrap",
            canvas {
                id: "sig-pad",
                class: "sig-pad",
            }
            span { class: "sig-pad-line" }
            button { class: "sig-pad-clear",
                onclick: move |_| { document::eval("window.__scan2filClearPad && window.__scan2filClearPad()"); },
                {crate::i18n::tr("Clear", "Effacer")}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_fields_are_found_by_flag_or_label() {
        let flagged = Field {
            label: "Lieu et date".into(),
            signature: true,
            ..Default::default()
        };
        let by_label = Field {
            label: "Unterschrift des Antragstellers".into(),
            ..Default::default()
        };
        let company = Field {
            label: "Firma".into(),
            ..Default::default()
        };
        assert!(is_signature(&flagged));
        assert!(is_signature(&by_label));
        assert!(!is_signature(&company), "Firma is the company in German");
    }

    #[test]
    fn bounds_cover_every_stroke() {
        let s: Strokes = vec![vec![[0.1, 0.2], [0.5, 0.25]], vec![[0.3, 0.05]]];
        let (x, y, w, h) = bounds(&s).unwrap();
        assert!((x - 0.1).abs() < 1e-6 && (y - 0.05).abs() < 1e-6);
        assert!((w - 0.4).abs() < 1e-6 && (h - 0.2).abs() < 1e-6);
        assert_eq!(bounds(&Vec::new()), None);
    }

    #[test]
    fn the_preview_draws_each_stroke() {
        let s: Strokes = vec![vec![[0.1, 0.2], [0.5, 0.25]], vec![[0.3, 0.05]]];
        let svg = preview(&s);
        assert_eq!(svg.matches('M').count(), 2);
        assert!(svg.starts_with("<svg"));
        assert_eq!(preview(&Vec::new()), "");
    }
}
