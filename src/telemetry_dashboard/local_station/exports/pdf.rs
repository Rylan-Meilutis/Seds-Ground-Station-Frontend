//! The report studio's PDF report, drawn as the station's worker draws it
//! with matplotlib (`export_pdf` in GroundStation26
//! `backend/src/recording_report.py`, PdfPages with the PDF base fonts). For
//! each channel: a page with the value plotted against seconds after the
//! first sample, the regression/provenance JSON wrapped at 110 columns (48
//! lines a page), and the samples as tables of 35 rows. Same pages, text
//! (titles, axis labels, tick labels from matplotlib's tick locator and
//! formatter, tables) and plotted points; positions follow matplotlib's
//! layout closely but are not pixel-identical.
use super::py::Py;
use super::report::{Failure, Stream};
use super::xlsx::utc;

const PAGE_W: f64 = 842.4;
const PAGE_H: f64 = 597.6;
/// The axes box of an 11.7 x 8.3 in figure with matplotlib's subplot margins.
const AX_X: f64 = 105.3;
const AX_Y: f64 = 65.736;
const AX_W: f64 = 652.86;
const AX_H: f64 = 460.152;
const LINE_RGB: &str = "0.1215686275 0.4666666667 0.7058823529";

/// Helvetica advance widths (1/1000 em) for Windows-1252 codes 32..=255.
const HELVETICA: [u16; 224] = [
    278, 278, 355, 556, 556, 889, 667, 222, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584,
    556, 1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 278, 278,
    278, 469, 556, 222, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500,
    334, 260, 334, 584, 0, 0, 0, 0, 0, 0, 1000, 556, 556, 0, 1000, 667, 333, 1000, 0, 611, 0, 0, 222, 222, 333, 333, 350, 556, 1000, 0, 1000, 500, 333,
    944, 0, 500, 667, 0, 333, 556, 556, 167, 556, 556, 556, 556, 191, 333, 556, 333, 333, 500, 500, 400, 556, 556, 556, 278, 556, 537, 350, 222, 333,
    333, 556, 1000, 1000, 0, 611, 667, 333, 333, 333, 333, 333, 333, 333, 333, 667, 333, 333, 278, 333, 333, 333, 1000, 722, 778, 778, 778, 778, 778,
    584, 778, 722, 722, 722, 722, 667, 667, 611, 556, 1000, 556, 370, 556, 556, 889, 500, 556, 778, 1000, 365, 278, 278, 278, 278, 556, 889, 556, 556,
    556, 278, 556, 584, 222, 611, 944, 611, 556, 500, 556, 500,
];

/// Text as Windows-1252 bytes (`?` for characters it lacks), as matplotlib
/// encodes text for the PDF base fonts.
pub(super) fn cp1252(text: &str) -> Vec<u8> {
    text.chars()
        .map(|c| match c as u32 {
            0..=0x7f | 0xa0..=0xff => c as u8,
            _ => CP1252_HIGH.iter().find(|(h, _)| *h == c).map_or(b'?', |(_, b)| *b),
        })
        .collect()
}

/// Windows-1252's characters at 0x80..=0x9f.
pub(super) const CP1252_HIGH: [(char, u8); 27] = [
        ('€', 0x80), ('‚', 0x82), ('ƒ', 0x83), ('„', 0x84), ('…', 0x85), ('†', 0x86), ('‡', 0x87), ('ˆ', 0x88), ('‰', 0x89), ('Š', 0x8a), ('‹', 0x8b),
        ('Œ', 0x8c), ('Ž', 0x8e), ('‘', 0x91), ('’', 0x92), ('“', 0x93), ('”', 0x94), ('•', 0x95), ('–', 0x96), ('—', 0x97), ('˜', 0x98), ('™', 0x99),
        ('š', 0x9a), ('›', 0x9b), ('œ', 0x9c), ('ž', 0x9e), ('Ÿ', 0x9f),
];

#[derive(Clone, Copy, PartialEq)]
enum Font {
    Helvetica,
    Courier,
}

fn width(text: &[u8], font: Font, size: f64) -> f64 {
    let units: u32 = text
        .iter()
        .map(|&b| match font {
            Font::Courier => 600,
            Font::Helvetica => u32::from(if b >= 32 { HELVETICA[usize::from(b - 32)] } else { 0 }),
        })
        .sum();
    f64::from(units) * size / 1000.0
}

fn num(x: f64) -> String {
    let text = format!("{x:.6}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" { "0".into() } else { text.into() }
}

#[derive(Clone, Copy)]
enum Align {
    Left,
    Center,
    Right,
}

#[derive(Default)]
struct Page {
    ops: String,
}

impl Page {
    fn new() -> Self {
        let mut page = Page::default();
        page.ops.push_str(&format!("1 g 0 0 {} {} re f\n", num(PAGE_W), num(PAGE_H)));
        page
    }
    fn text(&mut self, x: f64, y: f64, size: f64, font: Font, text: &str, align: Align) {
        if text.is_empty() {
            return;
        }
        let bytes = cp1252(text);
        let w = width(&bytes, font, size);
        let x = match align {
            Align::Left => x,
            Align::Center => x - w / 2.0,
            Align::Right => x - w,
        };
        self.ops.push_str(&format!("0 g BT /{} {} Tf {} {} Td ", if font == Font::Courier { "F2" } else { "F1" }, num(size), num(x), num(y)));
        self.string(&bytes);
        self.ops.push_str(" Tj ET\n");
    }
    fn rotated_text(&mut self, x: f64, y_center: f64, size: f64, text: &str) {
        let bytes = cp1252(text);
        let y = y_center - width(&bytes, Font::Helvetica, size) / 2.0;
        self.ops.push_str(&format!("0 g BT /F1 {} Tf 0 1 -1 0 {} {} Tm ", num(size), num(x), num(y)));
        self.string(&bytes);
        self.ops.push_str(" Tj ET\n");
    }
    fn string(&mut self, bytes: &[u8]) {
        self.ops.push('(');
        for &b in bytes {
            match b {
                b'(' | b')' | b'\\' => {
                    self.ops.push('\\');
                    self.ops.push(b as char);
                }
                0x20..=0x7e => self.ops.push(b as char),
                _ => self.ops.push_str(&format!("\\{b:03o}")),
            }
        }
        self.ops.push(')');
    }
    fn line(&mut self, x0: f64, y0: f64, x1: f64, y1: f64) {
        self.ops.push_str(&format!("{} {} m {} {} l S\n", num(x0), num(y0), num(x1), num(y1)));
    }
}

/// Python's float `divmod` (also numpy's for float64).
fn divmod(x: f64, y: f64) -> (f64, f64) {
    let mut m = x % y;
    let mut div = (x - m) / y;
    if m != 0.0 {
        if (y < 0.0) != (m < 0.0) {
            m += y;
            div -= 1.0;
        }
    } else {
        m = 0.0f64.copysign(y);
    }
    let floordiv = if div != 0.0 {
        let f = div.floor();
        if div - f > 0.5 { f + 1.0 } else { f }
    } else {
        0.0f64.copysign(x / y)
    };
    (floordiv, m)
}

fn floordiv(x: f64, y: f64) -> f64 {
    divmod(x, y).0
}

/// `matplotlib.transforms._nonsingular`.
fn nonsingular(vmin: f64, vmax: f64, expander: f64, tiny: f64) -> (f64, f64) {
    if !vmin.is_finite() || !vmax.is_finite() {
        return (-expander, expander);
    }
    let (mut vmin, mut vmax) = if vmax < vmin { (vmax, vmin) } else { (vmin, vmax) };
    let maxabs = vmin.abs().max(vmax.abs());
    if maxabs < (1e6 / tiny) * f64::MIN_POSITIVE {
        vmin = -expander;
        vmax = expander;
    } else if vmax - vmin <= maxabs * tiny {
        if vmax == 0.0 && vmin == 0.0 {
            vmin = -expander;
            vmax = expander;
        } else {
            vmin -= expander * vmin.abs();
            vmax += expander * vmax.abs();
        }
    }
    (vmin, vmax)
}

/// The axis view limits matplotlib's autoscaling gives data in `[lo, hi]`
/// (NaN bounds: no data): 5 % margins, singular ranges widened.
fn view_limits(lo: f64, hi: f64) -> (f64, f64) {
    let (x0, x1) = nonsingular(lo, hi, 0.05, 1e-15);
    let delta = (x1 - x0) * 0.05;
    let delta = if delta.is_finite() { delta } else { 0.0 };
    nonsingular(x0 - delta, x1 + delta, 1e-12, 1e-13)
}

/// `MaxNLocator(nbins="auto", steps=[1, 2, 2.5, 5, 10])` as the auto
/// locator places ticks on these axes (nine bins on both).
fn tick_values(vmin: f64, vmax: f64) -> Vec<f64> {
    let (vmin, vmax) = nonsingular(vmin, vmax, 1e-13, 1e-14);
    let nbins = 9.0;
    let dv = (vmax - vmin).abs();
    let meanv = (vmax + vmin) / 2.0;
    let offset = if meanv.abs() / dv < 100.0 { 0.0 } else { 10f64.powf(floordiv(meanv.abs().log10(), 1.0)).copysign(meanv) };
    let scale = 10f64.powf(floordiv((dv / nbins).log10(), 1.0));
    let (lo, hi) = (vmin - offset, vmax - offset);
    let steps: Vec<f64> = [0.1, 0.2, 0.25, 0.5, 1.0, 2.0, 2.5, 5.0, 10.0, 20.0].iter().map(|s| s * scale).collect();
    let raw_step = (hi - lo) / nbins;
    let istep = steps.iter().position(|&s| s >= raw_step).unwrap_or(steps.len() - 1);
    let closeto = |ms: f64, edge: f64, step: f64| {
        let tol = if offset.abs() > 0.0 { 10f64.powf((offset.abs() / step).log10() - 12.0).clamp(1e-10, 0.4999) } else { 1e-10 };
        (ms - edge).abs() < tol
    };
    let mut ticks = Vec::new();
    for &step in steps[..=istep].iter().rev() {
        let best = floordiv(lo, step) * step;
        let (d, m) = divmod(lo - best, step);
        let low = if closeto(m / step, 1.0, step) { d + 1.0 } else { d };
        let (d, m) = divmod(hi - best, step);
        let high = if closeto(m / step, 0.0, step) { d } else { d + 1.0 };
        ticks = (0..).map(|i| low + f64::from(i)).take_while(|n| *n < high + 1.0).map(|n| n * step + best).collect();
        if ticks.iter().filter(|&&t| t <= hi && t >= lo).count() >= 2 {
            break;
        }
    }
    ticks.into_iter().map(|t| t + offset).collect()
}

/// `ScalarFormatter`'s labels for `locs` on an axis viewing `[vmin, vmax]`,
/// and its offset text ("1e-7", "+3.044e3", ...).
fn tick_labels(locs: &[f64], vmin: f64, vmax: f64) -> (Vec<String>, String) {
    if locs.is_empty() {
        return (Vec::new(), String::new());
    }
    let visible: Vec<f64> = locs.iter().copied().filter(|l| vmin <= *l && *l <= vmax).collect();
    // _compute_offset (offset threshold 4)
    let mut offset = 0.0;
    if let (Some(lmin), Some(lmax)) = (visible.iter().copied().reduce(f64::min), visible.iter().copied().reduce(f64::max))
        && lmin != lmax
        && !(lmin <= 0.0 && 0.0 <= lmax)
    {
        let (abs_min, abs_max) = if lmin.abs() <= lmax.abs() { (lmin.abs(), lmax.abs()) } else { (lmax.abs(), lmin.abs()) };
        let sign = 1f64.copysign(lmin);
        let oom_max = abs_max.log10().ceil();
        let p = |o: f64| 10f64.powf(o);
        let mut oom = oom_max;
        while floordiv(abs_min, p(oom)) == floordiv(abs_max, p(oom)) {
            oom -= 1.0;
        }
        oom += 1.0;
        if (abs_max - abs_min) / p(oom) <= 1e-2 {
            let mut o = oom_max;
            while floordiv(abs_max, p(o)) - floordiv(abs_min, p(o)) <= 1.0 {
                o -= 1.0;
            }
            oom = o + 1.0;
        }
        offset = if floordiv(abs_max, p(oom)) >= 1000.0 { sign * floordiv(abs_max, p(oom)) * p(oom) } else { 0.0 };
    }
    // _set_order_of_magnitude (power limits -5, 6)
    let mut order = 0i32;
    if !visible.is_empty() {
        let oom = if offset != 0.0 {
            (vmax - vmin).log10().floor() as i32
        } else {
            let val = visible.iter().map(|l| l.abs()).fold(0.0, f64::max);
            if val == 0.0 { 0 } else { val.log10().floor() as i32 }
        };
        if oom <= -5 || oom >= 6 {
            order = oom;
        }
    }
    // _set_format
    let scaled = |v: f64| (v - offset) / 10f64.powi(order);
    let mut points: Vec<f64> = locs.to_vec();
    if locs.len() < 2 {
        points.extend([vmin, vmax]);
    }
    let points: Vec<f64> = points.into_iter().map(scaled).collect();
    let mut range = points.iter().copied().fold(f64::NEG_INFINITY, f64::max) - points.iter().copied().fold(f64::INFINITY, f64::min);
    if range == 0.0 {
        range = points.iter().map(|p| p.abs()).fold(0.0, f64::max);
    }
    if range == 0.0 {
        range = 1.0;
    }
    let points = if locs.len() < 2 { &points[..points.len() - 2] } else { &points[..] };
    let range_oom = range.log10().floor() as i32;
    let mut sigfigs = (3 - range_oom).max(0);
    let thresh = 1e-3 * 10f64.powi(range_oom);
    while sigfigs >= 0 {
        let factor = 10f64.powi(sigfigs);
        let worst = points.iter().map(|p| (p - (p * factor).round_ties_even() / factor).abs()).fold(0.0, f64::max);
        if worst < thresh { sigfigs -= 1 } else { break }
    }
    let decimals = (sigfigs + 1) as usize;
    let labels = locs
        .iter()
        .map(|&l| {
            let xp = scaled(l);
            let xp = if xp.abs() < 1e-8 { 0.0 } else { xp };
            format!("{xp:.decimals$}")
        })
        .collect();
    let mut text = String::new();
    if order != 0 || offset != 0.0 {
        if order != 0 {
            text.push_str(&format!("1e{order}"));
        }
        if offset != 0.0 {
            let e = offset.abs().log10().floor() as i32;
            let s = (offset / 10f64.powi(e) * 1e10).round() / 1e10;
            let significand = if s % 1.0 == 0.0 { format!("{}", s as i64) } else { format!("{}", g10(s)) };
            let data = if e == 0 { significand } else { format!("{significand}e{e}") };
            text.push_str(&if offset > 0.0 { format!("+{data}") } else { data });
        }
    }
    (labels, text)
}

/// `%1.10g`.
fn g10(x: f64) -> String {
    let text = format!("{x:.9e}");
    let (mantissa, exponent) = text.split_once('e').unwrap_or((&text, "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    if (-4..10).contains(&exponent) {
        let decimals = (9 - exponent).max(0) as usize;
        let fixed = format!("{x:.decimals$}");
        if fixed.contains('.') { fixed.trim_end_matches('0').trim_end_matches('.').to_owned() } else { fixed }
    } else {
        let m = mantissa.trim_end_matches('0').trim_end_matches('.');
        format!("{m}e{}{:02}", if exponent < 0 { '-' } else { '+' }, exponent.abs())
    }
}

/// Visible ticks of an axis and their labels, with the offset text.
fn axis(vmin: f64, vmax: f64) -> (Vec<(f64, String)>, String) {
    let locs = tick_values(vmin, vmax);
    let (labels, offset) = tick_labels(&locs, vmin, vmax);
    let tol = (vmax - vmin) * 1e-10;
    let ticks = locs.into_iter().zip(labels).filter(|(l, _)| vmin - tol <= *l && *l <= vmax + tol).collect();
    (ticks, offset)
}

fn plot_page(key: &str, stream: &Stream) -> Page {
    let start = stream.samples[0].t;
    let points: Vec<(f64, Option<f64>)> = stream.samples.iter().map(|s| ((s.t - start) as f64 / 1000.0, s.value.as_ref().and_then(Py::number))).collect();
    let finite: Vec<(f64, f64)> = points.iter().filter_map(|(x, y)| y.map(|y| (*x, y))).collect();
    let bounds = |f: fn(&(f64, f64)) -> f64| {
        if finite.is_empty() {
            (f64::NEG_INFINITY, f64::INFINITY)
        } else {
            (finite.iter().map(f).fold(f64::INFINITY, f64::min), finite.iter().map(f).fold(f64::NEG_INFINITY, f64::max))
        }
    };
    let (xb, yb) = (bounds(|p| p.0), bounds(|p| p.1));
    let (x0, x1) = view_limits(xb.0, xb.1);
    let (y0, y1) = view_limits(yb.0, yb.1);
    let px = |x: f64| AX_X + (x - x0) / (x1 - x0) * AX_W;
    let py = |y: f64| AX_Y + (y - y0) / (y1 - y0) * AX_H;
    let mut page = Page::new();
    let clip = format!("q {} {} {} {} re W n ", num(AX_X), num(AX_Y), num(AX_W), num(AX_H));
    let (xticks, xoffset) = axis(x0, x1);
    for (x, label) in &xticks {
        page.ops.push_str(&format!("{clip}/A2 gs 0.8 w 0.6901960784 0.6901960784 0.6901960784 RG\n"));
        page.line(px(*x), AX_Y, px(*x), AX_Y + AX_H);
        page.ops.push_str("Q q 0.8 w 0 G\n");
        page.line(px(*x), AX_Y, px(*x), AX_Y - 3.5);
        page.ops.push_str("Q\n");
        page.text(px(*x), AX_Y - 14.18, 10.0, Font::Helvetica, label, Align::Center);
    }
    page.text(AX_X + AX_W / 2.0, AX_Y - 27.53, 10.0, Font::Helvetica, &format!("Seconds after receive UTC epoch {start} ms"), Align::Center);
    page.text(AX_X + AX_W, AX_Y - 27.53, 10.0, Font::Helvetica, &xoffset, Align::Right);
    let (yticks, yoffset) = axis(y0, y1);
    let mut left = AX_X - 7.0;
    for (y, label) in &yticks {
        page.ops.push_str(&format!("{clip}/A2 gs 0.8 w 0.6901960784 0.6901960784 0.6901960784 RG\n"));
        page.line(AX_X, py(*y), AX_X + AX_W, py(*y));
        page.ops.push_str("Q q 0.8 w 0 G\n");
        page.line(AX_X, py(*y), AX_X - 3.5, py(*y));
        page.ops.push_str("Q\n");
        page.text(AX_X - 7.0, py(*y) - 3.59, 10.0, Font::Helvetica, label, Align::Right);
        left = left.min(AX_X - 7.0 - width(&cp1252(label), Font::Helvetica, 10.0));
    }
    page.rotated_text(left - 6.07, AX_Y + AX_H / 2.0, 10.0, "Recorded value");
    page.text(AX_X, AX_Y + AX_H + 3.0, 10.0, Font::Helvetica, &yoffset, Align::Left);
    // The recorded values; a missing value breaks the line.
    page.ops.push_str(&format!("{clip}2 J 1 j 0.7 w {LINE_RGB} RG\n"));
    let mut pen_down = false;
    for (x, y) in &points {
        match y {
            Some(y) => {
                page.ops.push_str(&format!("{} {} {}\n", num(px(*x)), num(py(*y)), if pen_down { "l" } else { "m" }));
                pen_down = true;
            }
            None => pen_down = false,
        }
    }
    page.ops.push_str("S Q\nq 0.8 w 0 G 0 J\n");
    page.ops.push_str(&format!("{} {} {} {} re S Q\n", num(AX_X), num(AX_Y), num(AX_W), num(AX_H)));
    page.text(AX_X + AX_W / 2.0, AX_Y + AX_H + 6.0, 12.0, Font::Helvetica, key, Align::Center);
    page
}

fn is_ws(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ')
}
fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}
fn is_letter(c: char) -> bool {
    is_word(c) && !c.is_numeric()
}
fn is_word_punct(c: char) -> bool {
    is_word(c) || "!\"'&.,?".contains(c)
}

/// `textwrap.TextWrapper._split`: whitespace runs and words, hyphenated
/// words split after their hyphens (Python's `wordsep_re`).
fn chunks(text: &str) -> Vec<String> {
    let t: Vec<char> = text.chars().map(|c| if is_ws(c) { ' ' } else { c }).collect();
    let at = |i: isize| if i < 0 { None } else { t.get(i as usize).copied() };
    let mut out = Vec::new();
    let mut p = 0usize;
    while p < t.len() {
        let end = if is_ws(t[p]) {
            let mut e = p;
            while e < t.len() && is_ws(t[e]) {
                e += 1;
            }
            e
        } else if p > 0 && is_word_punct(t[p - 1]) && t[p] == '-' && {
            let mut e = p;
            while e < t.len() && t[e] == '-' {
                e += 1;
            }
            e - p >= 2 && t.get(e).is_some_and(|c| is_word(*c))
        } {
            let mut e = p;
            while e < t.len() && t[e] == '-' {
                e += 1;
            }
            e
        } else {
            let mut found = t.len();
            let mut k = p + 1;
            while k <= t.len() {
                if is_ws(t[k - 1]) {
                    found = k - 1;
                    break;
                }
                let q = k as isize;
                let letter = |i: isize| at(i).is_some_and(is_letter);
                // hyphenated word: a hyphen after two letters or after letter-hyphen-letter, before letter[-]letter
                if at(q) == Some('-')
                    && ((letter(q - 2) && letter(q - 1)) || (letter(q - 3) && at(q - 2) == Some('-') && letter(q - 1)))
                    && letter(q + 1)
                    && (letter(q + 2) || (at(q + 2) == Some('-') && letter(q + 3)))
                {
                    found = k + 1;
                    break;
                }
                // end of word
                if k == t.len() || is_ws(t[k]) {
                    found = k;
                    break;
                }
                // em-dash
                if is_word_punct(t[k - 1]) && {
                    let mut e = k;
                    while e < t.len() && t[e] == '-' {
                        e += 1;
                    }
                    e - k >= 2 && t.get(e).is_some_and(|c| is_word(*c))
                } {
                    found = k;
                    break;
                }
                k += 1;
            }
            found
        };
        out.push(t[p..end].iter().collect());
        p = end;
    }
    out
}

/// `textwrap.wrap(text, width)`.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut chunks: Vec<Vec<char>> = chunks(text).into_iter().map(|c| c.chars().collect()).collect();
    chunks.reverse();
    let blank = |c: &Vec<char>| c.iter().all(|&ch| ch == ' ');
    let mut lines: Vec<String> = Vec::new();
    while !chunks.is_empty() {
        let mut line: Vec<Vec<char>> = Vec::new();
        let mut len = 0;
        if chunks.last().is_some_and(blank) && !lines.is_empty() {
            chunks.pop();
        }
        while let Some(chunk) = chunks.last() {
            if len + chunk.len() <= width {
                len += chunk.len();
                line.push(chunks.pop().unwrap_or_default());
            } else {
                break;
            }
        }
        if let Some(chunk) = chunks.last()
            && chunk.len() > width
        {
            let space_left = if width < 1 { 1 } else { width - len };
            if space_left > 0 {
                let chunk = chunks.pop().unwrap_or_default();
                let mut end = space_left;
                if chunk.len() > space_left
                    && let Some(hyphen) = chunk[..space_left].iter().rposition(|&c| c == '-')
                    && hyphen > 0
                    && chunk[..hyphen].iter().any(|&c| c != '-')
                {
                    end = hyphen + 1;
                }
                line.push(chunk[..end].to_vec());
                chunks.push(chunk[end..].to_vec());
            } else if line.is_empty() {
                line.push(chunks.pop().unwrap_or_default());
            }
        }
        if line.last().is_some_and(blank) {
            line.pop();
        }
        if !line.is_empty() {
            lines.push(line.iter().flatten().collect());
        }
    }
    lines
}

fn regression_pages(key: &str, metadata: &Py, out: &mut Vec<Page>) {
    let lines = wrap(&metadata.dumps(false, false), 110);
    for group in lines.chunks(48) {
        let mut page = Page::new();
        for (i, line) in group.iter().enumerate() {
            page.text(AX_X, AX_Y + AX_H - 7.7488 - 9.6336 * i as f64, 8.0, Font::Courier, line, Align::Left);
        }
        page.text(AX_X + AX_W / 2.0, AX_Y + AX_H + 6.0, 9.0, Font::Helvetica, &format!("{key} — regression / provenance"), Align::Center);
        out.push(page);
    }
}

fn table_pages(key: &str, stream: &Stream, out: &mut Vec<Page>) {
    const HEADER: [&str; 5] = ["Received ms", "Value", "Source ms", "Recording", "Row ID"];
    let (cell_w, cell_h) = (AX_W / 5.0, 16.2);
    for (n, group) in stream.samples.chunks(35).enumerate() {
        let mut page = Page::new();
        let rows = group.len() + 1;
        let top = AX_Y + AX_H / 2.0 + rows as f64 * cell_h / 2.0;
        for r in 0..rows {
            let bottom = top - (r + 1) as f64 * cell_h;
            let cells: [String; 5] = if r == 0 {
                HEADER.map(str::to_owned)
            } else {
                let s = &group[r - 1];
                [s.t.to_string(), s.value.as_ref().map(Py::py_str).unwrap_or_default(), s.source.map(|v| v.to_string()).unwrap_or_default(), s.recording.to_owned(), s.row_id.to_string()]
            };
            for (c, text) in cells.iter().enumerate() {
                let left = AX_X + c as f64 * cell_w;
                page.ops.push_str(&format!("1 g 1 w 0 G {} {} {} {} re B\n", num(left), num(bottom), num(cell_w), num(cell_h)));
                if r == 0 {
                    page.text(left + cell_w / 2.0, bottom + 6.8225, 5.0, Font::Helvetica, text, Align::Center);
                } else {
                    page.text(left + cell_w * 0.9, bottom + 6.8225, 5.0, Font::Helvetica, text, Align::Right);
                }
            }
        }
        let first = n * 35 + 1;
        page.text(AX_X + AX_W / 2.0, AX_Y + AX_H + 6.0, 9.0, Font::Helvetica, &format!("{key} — recorded data {first}–{}", first + group.len() - 1), Align::Center);
        out.push(page);
    }
}

/// The PDF report of the chosen channels.
pub(super) fn document(streams: &[&Stream], metadata: &[Py], generated_ms: i64) -> Result<Vec<u8>, Failure> {
    if streams.iter().map(|s| s.samples.len()).sum::<usize>() > 20_000 {
        return Err((422, "PDF raw-data appendix exceeds 20,000 samples. Narrow the selection or use CSV/Excel.".into()));
    }
    let mut pages = Vec::new();
    for (stream, info) in streams.iter().zip(metadata) {
        pages.push(plot_page(&stream.key, stream));
        regression_pages(&stream.key, info, &mut pages);
        table_pages(&stream.key, stream, &mut pages);
    }
    let (y, mo, d, h, mi, s) = utc(generated_ms);
    let mut objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        Vec::new(),
        b"<< /F1 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >> /F2 << /Type /Font /Subtype /Type1 /BaseFont /Courier /Encoding /WinAnsiEncoding >> >>".to_vec(),
        b"<< /A2 << /Type /ExtGState /CA 0.3 /ca 1 >> >>".to_vec(),
        format!("<< /Creator (SEDS ground station UI, offline report) /Producer (SEDS ground station UI) /CreationDate (D:{y:04}{mo:02}{d:02}{h:02}{mi:02}{s:02}Z) >>").into_bytes(),
    ];
    let mut kids = Vec::new();
    for page in pages {
        let packed = miniz_oxide::deflate::compress_to_vec_zlib(page.ops.as_bytes(), 6);
        let mut stream = format!("<< /Length {} /Filter /FlateDecode >>\nstream\n", packed.len()).into_bytes();
        stream.extend_from_slice(&packed);
        stream.extend_from_slice(b"\nendstream");
        objects.push(stream);
        let contents = objects.len();
        objects.push(format!("<< /Type /Page /Parent 2 0 R /Resources << /Font 3 0 R /ExtGState 4 0 R /ProcSet [ /PDF /Text ] >> /MediaBox [ 0 0 {} {} ] /Contents {contents} 0 R >>", num(PAGE_W), num(PAGE_H)).into_bytes());
        kids.push(format!("{} 0 R", objects.len()));
    }
    objects[1] = format!("<< /Type /Pages /Kids [ {} ] /Count {} >>", kids.join(" "), kids.len()).into_bytes();
    let mut out = b"%PDF-1.4\n%\xac\xdc \xab\xba\n".to_vec();
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(object);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R /Info 5 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).as_bytes());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_wraps_as_python_textwrap() {
        let text = r#"{"status": "unavailable", "reason": "No historical regression recorded for this derived channel.", "x": "abc-def well-known --flag a--b"}"#;
        assert_eq!(wrap(text, 40), [
            r#"{"status": "unavailable", "reason": "No"#,
            r#"historical regression recorded for this"#,
            r#"derived channel.", "x": "abc-def well-"#,
            r#"known --flag a--b"}"#,
        ]);
        assert_eq!(wrap(&"x".repeat(25), 10), ["xxxxxxxxxx", "xxxxxxxxxx", "xxxxx"]);
    }

    #[test]
    fn ticks_follow_matplotlib() {
        // Data range, view limits, tick labels and offset text from matplotlib 3.11
        // (an 11.7 x 8.3 in figure, `ax.plot`, `fig.canvas.draw()`).
        let cases: &[(f64, f64, f64, f64, &[&str], &str)] = &[
            (0.0_f64, 204.0_f64, -10.200000000000001_f64, 214.2_f64, &["0", "25", "50", "75", "100", "125", "150", "175", "200"], ""),
            (0.0_f64, 236.0_f64, -11.8_f64, 247.8_f64, &["0", "50", "100", "150", "200"], ""),
            (31.7580169_f64, 31.7621_f64, 31.757812745000003_f64, 31.762304155_f64, &["31.7580", "31.7585", "31.7590", "31.7595", "31.7600", "31.7605", "31.7610", "31.7615", "31.7620"], ""),
            (91077.1_f64, 91090.3_f64, 91076.44_f64, 91090.96_f64, &["91078", "91080", "91082", "91084", "91086", "91088", "91090"], ""),
            (1e-07_f64, 1e-07_f64, 9.449999999999999e-08_f64, 1.055e-07_f64, &["0.96", "0.98", "1.00", "1.02", "1.04"], "1e-7"),
            (0.0_f64, 0.0_f64, -0.05500000000000001_f64, 0.05500000000000001_f64, &["-0.04", "-0.02", "0.00", "0.02", "0.04"], ""),
            (745.0_f64, 745.0_f64, 704.025_f64, 785.975_f64, &["710", "720", "730", "740", "750", "760", "770", "780"], ""),
            (-0.021_f64, 3044.557_f64, -152.2499_f64, 3196.7859_f64, &["0", "500", "1000", "1500", "2000", "2500", "3000"], ""),
            (7.9_f64, 7.899764_f64, 7.8997522_f64, 7.900011800000001_f64, &["-0.00020", "-0.00015", "-0.00010", "-0.00005", "0.00000"], "+7.9"),
            (0.25_f64, 1.125_f64, 0.20625_f64, 1.16875_f64, &["0.4", "0.6", "0.8", "1.0"], ""),
            (1767225600000.0_f64, 1767225836000.0_f64, 1767225588200.0_f64, 1767225847800.0_f64, &["600000", "650000", "700000", "750000", "800000"], "+1.767225e12"),
            (-5e-06_f64, 3e-06_f64, -5.400000000000001e-06_f64, 3.4e-06_f64, &["-5", "-4", "-3", "-2", "-1", "0", "1", "2", "3"], "1e-6"),
            (12.0_f64, 13.75_f64, 11.9125_f64, 13.8375_f64, &["12.00", "12.25", "12.50", "12.75", "13.00", "13.25", "13.50", "13.75"], ""),
            (0.0_f64, 1.0_f64, -0.05_f64, 1.05_f64, &["0.0", "0.2", "0.4", "0.6", "0.8", "1.0"], ""),
            (1.0_f64, 1.0_f64, 0.945_f64, 1.0550000000000002_f64, &["0.96", "0.98", "1.00", "1.02", "1.04"], ""),
            (-640.0_f64, -639.5_f64, -640.025_f64, -639.475_f64, &["-640.0", "-639.9", "-639.8", "-639.7", "-639.6", "-639.5"], ""),
            (1000000000.0_f64, 1000000003.0_f64, 999999999.85_f64, 1000000003.15_f64, &["0.0", "0.5", "1.0", "1.5", "2.0", "2.5", "3.0"], "+1e9"),
        ];
        for &(lo, hi, v0, v1, labels, offset) in cases {
            let (x0, x1) = view_limits(lo, hi);
            assert!((x0 - v0).abs() <= v0.abs() * 1e-12 && (x1 - v1).abs() <= v1.abs() * 1e-12, "{lo}..{hi}: {x0}..{x1}");
            let (ticks, text) = axis(x0, x1);
            assert_eq!(ticks.iter().map(|t| t.1.as_str()).collect::<Vec<_>>(), labels, "{lo}..{hi}");
            assert_eq!(text, offset, "{lo}..{hi}");
        }
    }
}
