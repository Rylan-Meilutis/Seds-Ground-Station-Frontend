//! The Excel and PDF reports built offline against the station's
//! (GroundStation26 `contract_fixtures`: `replay-bundle/exports/report-documents.json`
//! lists each request and the station's file). Both files are read back:
//! XLSX sheets, cells, cell types and charts; PDF pages, the text on each
//! page and the plotted points. A bundle made with `?reports=1` hands out the
//! station's standard reports byte for byte.
use super::super::bundle;
use super::{ExportInputs, answer};
use serde_json::Value;
use std::collections::BTreeMap;

const BUNDLE: &[u8] = include_bytes!("../../../../tests/fixtures/backend-contract/replay-bundle/flight.gsreplay");

fn fixture(name: &str) -> Vec<u8> {
    let path = crate::backend_contract::dir().join("replay-bundle").join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn flight_files() -> BTreeMap<String, Vec<u8>> {
    bundle::unzip(BUNDLE).unwrap()
}

fn inputs(case: &str, with_reports: bool) -> ExportInputs {
    let mut files = flight_files();
    let (recording, csv) = if case == "flight" {
        ("groundstation_recording_2026-01-01_00-00-00_000.db", files["data/telemetry.csv"].clone())
    } else {
        files.retain(|path, _| path == "config/report_context.json" || path == "manifest.json");
        files.insert("derived/calibration_history.json".into(), fixture("exports/cases-calibration-history.json"));
        ("groundstation_recording_2026-01-02_00-00-00_000.db", fixture("exports/cases-recording.csv"))
    };
    if with_reports {
        files.insert("exports/reports.json".into(), fixture("exports/reports.json"));
        files.insert("exports/report.xlsx".into(), fixture("exports/flight-report.xlsx"));
        files.insert("exports/report.pdf".into(), fixture("exports/flight-report.pdf"));
    }
    ExportInputs::from_bundle(recording, std::rc::Rc::new(csv), &files)
}

fn unescape(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let end = rest[i..].find(';').map_or(rest.len(), |e| i + e);
        let entity = &rest[i + 1..end];
        out.push_str(&match entity {
            "amp" => "&".into(),
            "lt" => "<".into(),
            "gt" => ">".into(),
            "quot" => "\"".into(),
            "apos" => "'".into(),
            e if e.starts_with("#x") => char::from_u32(u32::from_str_radix(&e[2..], 16).unwrap()).unwrap().to_string(),
            e if e.starts_with('#') => char::from_u32(e[1..].parse().unwrap()).unwrap().to_string(),
            e => format!("&{e};"),
        });
        rest = &rest[(end + 1).min(rest.len())..];
    }
    out.push_str(rest);
    out
}

/// Every `<tag ...>inner</tag>` (or self-closed) element's attributes and inner text.
fn elements<'a>(xml: &'a str, tag: &str) -> Vec<(&'a str, &'a str)> {
    let mut out = Vec::new();
    let open = format!("<{tag}");
    let mut at = 0;
    while let Some(i) = xml[at..].find(&open) {
        let start = at + i;
        let after = start + open.len();
        if !xml[after..].starts_with([' ', '>', '/']) {
            at = after;
            continue;
        }
        let head_end = start + xml[start..].find('>').unwrap();
        let attributes = &xml[after..head_end];
        if attributes.ends_with('/') {
            out.push((attributes, ""));
            at = head_end + 1;
            continue;
        }
        let close = format!("</{tag}>");
        let end = head_end + xml[head_end..].find(&close).unwrap();
        out.push((attributes, &xml[head_end + 1..end]));
        at = end + close.len();
    }
    out
}

fn attribute(attributes: &str, name: &str) -> Option<String> {
    let key = format!(" {name}=\"");
    let i = attributes.find(&key)? + key.len();
    Some(unescape(&attributes[i..i + attributes[i..].find('"')?]))
}

/// A workbook's content: per sheet (in order) its name, frozen header, every
/// cell as (reference, type, value), and its chart's titles and series.
fn workbook(bytes: &[u8]) -> Vec<String> {
    let files = bundle::unzip(bytes).unwrap();
    let text = |path: &str| String::from_utf8(files.get(path.trim_start_matches('/')).unwrap_or_else(|| panic!("no {path}")).clone()).unwrap();
    let rels = |path: &str| -> BTreeMap<String, String> {
        let (dir, file) = path.rsplit_once('/').unwrap();
        elements(&text(&format!("{dir}/_rels/{file}.rels")), "Relationship")
            .into_iter()
            .map(|(a, _)| {
                let target = attribute(a, "Target").unwrap();
                let target = if target.starts_with('/') { target } else { format!("/{dir}/{target}") };
                (attribute(a, "Id").unwrap(), target)
            })
            .collect()
    };
    let mut out = Vec::new();
    let core = text("docProps/core.xml");
    for tag in ["dcterms:created", "dcterms:modified"] {
        out.push(format!("{tag} {}", elements(&core, tag)[0].1));
    }
    let workbook_rels = rels("xl/workbook.xml");
    for (a, _) in elements(&text("xl/workbook.xml"), "sheet") {
        let path = &workbook_rels[&attribute(a, "r:id").unwrap()];
        let xml = text(path);
        out.push(format!("sheet {:?} frozen={}", attribute(a, "name").unwrap(), xml.contains("state=\"frozen\"")));
        for (attributes, inner) in elements(&xml, "c") {
            let kind = attribute(attributes, "t").unwrap_or_else(|| "n".into());
            let value = match kind.as_str() {
                "inlineStr" => unescape(elements(inner, "t")[0].1),
                "n" => format!("{:?}", elements(inner, "v")[0].1.parse::<f64>().unwrap()),
                _ => elements(inner, "v")[0].1.to_owned(),
            };
            out.push(format!("{} {kind} {value}", attribute(attributes, "r").unwrap()));
        }
        if let Some((a, _)) = elements(&xml, "drawing").first() {
            let drawing = &rels(path.trim_start_matches('/'))[&attribute(a, "r:id").unwrap()];
            let anchor = text(drawing);
            let from = elements(&anchor, "from")[0].1;
            out.push(format!("chart at col {} row {}", elements(from, "col")[0].1, elements(from, "row")[0].1));
            let chart_rel = attribute(elements(&anchor, "c:chart")[0].0, "r:id").unwrap();
            let chart = text(&rels(drawing.trim_start_matches('/'))[&chart_rel]);
            for (_, title) in elements(&chart, "a:t") {
                out.push(format!("chart text {}", unescape(title)));
            }
            for (_, formula) in elements(&chart, "f") {
                out.push(format!("chart series {}", unescape(formula)));
            }
        }
    }
    out
}

fn pdf_objects(bytes: &[u8]) -> BTreeMap<u32, Vec<u8>> {
    let mut objects = BTreeMap::new();
    let mut at = 0;
    while let Some(i) = find(&bytes[at..], b" 0 obj") {
        let head = at + i;
        let start = bytes[..head].iter().rposition(|b| !b.is_ascii_digit()).map_or(0, |p| p + 1);
        let number: u32 = std::str::from_utf8(&bytes[start..head]).unwrap().parse().unwrap();
        let body_start = head + 6;
        let end = body_start + find(&bytes[body_start..], b"endobj").unwrap();
        objects.insert(number, bytes[body_start..end].to_vec());
        at = end;
    }
    objects
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn reference(dict: &[u8], key: &str) -> u32 {
    let i = find(dict, key.as_bytes()).unwrap_or_else(|| panic!("no {key}")) + key.len();
    let text = String::from_utf8_lossy(&dict[i..]);
    text.split_whitespace().next().unwrap().parse().unwrap()
}

fn stream(object: &[u8]) -> Vec<u8> {
    let start = find(object, b"stream").unwrap() + 6;
    let start = start + if object[start] == b'\r' { 2 } else { 1 };
    let end = find(object, b"endstream").unwrap();
    // The end-of-line before `endstream` is not part of the data, but a
    // deflate stream may end in those bytes too; zlib ignores what follows it.
    let data = &object[start..end];
    if find(object, b"FlateDecode").is_some() { miniz_oxide::inflate::decompress_to_vec_zlib(data).unwrap() } else { data.to_vec() }
}

fn from_cp1252(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| match b {
            0x80..=0x9f => super::pdf::CP1252_HIGH.iter().find(|(_, code)| *code == b).map_or('?', |(c, _)| *c),
            _ => char::from(b),
        })
        .collect()
}

/// A PDF's content: per page, its text strings (sorted) and the plotted
/// line's points scaled to their bounding box (subpaths kept).
fn document(bytes: &[u8]) -> Vec<(Vec<String>, Vec<Vec<(f64, f64)>>)> {
    let objects = pdf_objects(bytes);
    let catalog = objects.values().find(|o| find(o, b"/Type /Catalog").is_some()).unwrap();
    let pages = &objects[&reference(catalog, "/Pages")];
    let kids_start = find(pages, b"/Kids").unwrap();
    let kids_end = kids_start + find(&pages[kids_start..], b"]").unwrap();
    let kids: Vec<u32> = String::from_utf8_lossy(&pages[kids_start + 5..kids_end]).replace('[', " ").split(" R").filter_map(|r| r.split_whitespace().next().and_then(|n| n.parse().ok())).collect();
    let mut out = Vec::new();
    for kid in kids {
        let content = stream(&objects[&reference(&objects[&kid], "/Contents")]);
        let mut texts = Vec::new();
        let mut operands: Vec<String> = Vec::new();
        let mut strings: Vec<Vec<u8>> = Vec::new();
        let mut color = String::new();
        let mut saved = Vec::new();
        let mut path: Vec<Vec<(f64, f64)>> = Vec::new();
        let mut line: Vec<Vec<(f64, f64)>> = Vec::new();
        let mut i = 0;
        while i < content.len() {
            let c = content[i];
            if c.is_ascii_whitespace() {
                i += 1;
            } else if c == b'(' {
                let mut depth = 1;
                let mut s = Vec::new();
                i += 1;
                while depth > 0 {
                    match content[i] {
                        b'\\' => {
                            i += 1;
                            match content[i] {
                                b'n' => s.push(b'\n'),
                                b'r' => s.push(b'\r'),
                                b't' => s.push(b'\t'),
                                d @ b'0'..=b'7' => {
                                    let mut v = u32::from(d - b'0');
                                    for _ in 0..2 {
                                        if matches!(content[i + 1], b'0'..=b'7') {
                                            i += 1;
                                            v = v * 8 + u32::from(content[i] - b'0');
                                        }
                                    }
                                    s.push(v as u8);
                                }
                                other => s.push(other),
                            }
                        }
                        b'(' => {
                            depth += 1;
                            s.push(b'(');
                        }
                        b')' => {
                            depth -= 1;
                            if depth > 0 {
                                s.push(b')');
                            }
                        }
                        other => s.push(other),
                    }
                    i += 1;
                }
                strings.push(s);
            } else {
                let start = i;
                while i < content.len() && !content[i].is_ascii_whitespace() && content[i] != b'(' {
                    i += 1;
                }
                let token = String::from_utf8_lossy(&content[start..i]).into_owned();
                let number = |k: usize| operands[operands.len() - k].parse::<f64>().unwrap();
                match token.as_str() {
                    "Tj" => texts.push(from_cp1252(&strings.pop().unwrap())),
                    "RG" => color = operands[operands.len() - 3..].iter().map(|n| format!("{:.4}", n.parse::<f64>().unwrap())).collect::<Vec<_>>().join(" "),
                    "q" => saved.push(color.clone()),
                    "Q" => color = saved.pop().unwrap_or_default(),
                    "m" => path.push(vec![(number(2), number(1))]),
                    "l" => path.last_mut().unwrap().push((number(2), number(1))),
                    "S" => {
                        if color == "0.1216 0.4667 0.7059" {
                            line.append(&mut path);
                        }
                        path.clear();
                    }
                    "f" | "B" | "n" | "re" => {
                        if token != "re" {
                            path.clear();
                        }
                    }
                    _ => {}
                }
                if token.starts_with(|c: char| c.is_ascii_digit() || c == '-' || c == '.') {
                    operands.push(token);
                } else {
                    operands.clear();
                    strings.clear();
                }
            }
        }
        texts.sort();
        let points: Vec<&(f64, f64)> = line.iter().flatten().collect();
        let bound = |f: fn(&(f64, f64)) -> f64| (points.iter().map(|p| f(p)).fold(f64::INFINITY, f64::min), points.iter().map(|p| f(p)).fold(f64::NEG_INFINITY, f64::max));
        let ((x0, x1), (y0, y1)) = (bound(|p| p.0), bound(|p| p.1));
        let scale = |v: f64, lo: f64, hi: f64| if hi > lo { (v - lo) / (hi - lo) } else { 0.0 };
        let line = line.into_iter().map(|sub| sub.into_iter().map(|(x, y)| (scale(x, x0, x1), scale(y, y0, y1))).collect()).collect();
        out.push((texts, line));
    }
    out
}

fn assert_same_document(path: &str, ours: &[u8], station: &[u8]) {
    let (ours, station) = (document(ours), document(station));
    assert_eq!(ours.len(), station.len(), "{path}: page count");
    for (n, (a, b)) in ours.iter().zip(&station).enumerate() {
        assert_eq!(a.0, b.0, "{path}: text on page {}", n + 1);
        assert_eq!(a.1.len(), b.1.len(), "{path}: line pieces on page {}", n + 1);
        for (pa, pb) in a.1.iter().zip(&b.1) {
            assert_eq!(pa.len(), pb.len(), "{path}: points on page {}", n + 1);
            for (p, q) in pa.iter().zip(pb) {
                assert!((p.0 - q.0).abs() < 1e-4 && (p.1 - q.1).abs() < 1e-4, "{path}: page {} point {p:?} vs {q:?}", n + 1);
            }
        }
    }
}

#[test]
fn excel_and_pdf_reports_match_the_station() {
    let listing: Value = serde_json::from_slice(&fixture("exports/report-documents.json")).unwrap();
    let mut compared = 0;
    for entry in listing.as_array().unwrap() {
        let path = entry["path"].as_str().unwrap();
        let station = fixture(entry["file"].as_str().unwrap());
        let built = answer(&inputs(entry["case"].as_str().unwrap(), false), path).unwrap().unwrap_or_else(|e| panic!("{path}: {e:?}"));
        // REPORT_DUMP_DIR=<dir> keeps the offline files for inspection.
        if let Some(dir) = std::env::var_os("REPORT_DUMP_DIR") {
            let name = std::path::Path::new(entry["file"].as_str().unwrap()).file_name().unwrap().to_owned();
            std::fs::write(std::path::Path::new(&dir).join(name), &built.bytes).unwrap();
        }
        if entry["file"].as_str().unwrap().ends_with(".xlsx") {
            assert_eq!(built.content_type, "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet");
            assert_eq!(workbook(&built.bytes), workbook(&station), "{path}");
        } else {
            assert_eq!(built.content_type, "application/pdf");
            assert_same_document(path, &built.bytes, &station);
        }
        // A bundle with the standard reports hands out the station's own file.
        if entry["case"] == "flight" && entry["standard"] == true {
            let bundled = answer(&inputs("flight", true), path).unwrap().unwrap();
            assert!(bundled.bytes == station, "{path}: the bundled report is the station's");
        }
        compared += 1;
    }
    assert_eq!(compared, 8);
}

#[test]
fn the_bundled_reports_answer_every_selection_of_everything() {
    let with = inputs("flight", true);
    let id = "groundstation_recording_2026-01-01_00-00-00_000.db";
    let ask = |request: Value| {
        let encoded: String = request.to_string().bytes().map(|b| format!("%{b:02X}")).collect();
        answer(&with, &format!("/api/recordings/report?request={encoded}")).unwrap().unwrap().bytes
    };
    let bundled = fixture("exports/flight-report.pdf");
    // The report studio's whole loaded window and every channel in preview order.
    let preview: Value = serde_json::from_slice(&fixture("exports/flight-report-preview.json")).unwrap();
    let all: Vec<Value> = preview["channels"].as_array().unwrap().iter().map(|c| c["key"].clone()).collect();
    assert!(ask(serde_json::json!({"recording": id, "format": "pdf", "channels": all, "start_ms": preview["start_ms"], "end_ms": preview["end_ms"]})) == bundled);
    assert!(ask(serde_json::json!({"recording": "", "format": "pdf", "channels": null})) == bundled);
    // Fewer channels or another order are built here instead.
    let mut reversed = all.clone();
    reversed.reverse();
    assert!(ask(serde_json::json!({"recording": id, "format": "pdf", "channels": reversed})) != bundled);
    assert!(ask(serde_json::json!({"recording": id, "format": "pdf", "channels": null, "start_ms": preview["start_ms"].as_i64().unwrap() + 1})) != bundled);
}
