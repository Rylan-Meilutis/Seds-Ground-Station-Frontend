//! The report studio's Excel report, written as the station's worker writes
//! it with openpyxl (`export_xlsx` in GroundStation26
//! `backend/src/recording_report.py`): a "Regression metadata" sheet, then
//! one "Channel N" sheet per channel with its samples, a frozen header row and
//! a scatter chart of value against received time. Same sheets, cells, cell
//! types and charts; the parts are openpyxl's, without its default theme.
use super::py::Py;
use super::report::Stream;

const NS_MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const NS_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const NS_PKG_REL: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const CT_WORKSHEET: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml";

pub(super) fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

/// C's `%.16g`, which openpyxl writes numbers with.
pub(super) fn g16(x: f64) -> String {
    if !x.is_finite() {
        return if x.is_nan() { "nan".into() } else if x > 0.0 { "inf".into() } else { "-inf".into() };
    }
    if x == 0.0 {
        return if x.is_sign_negative() { "-0".into() } else { "0".into() };
    }
    let sci = format!("{x:.15e}");
    let (mantissa, exponent) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    let trim = |s: &str| -> String {
        if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_owned() } else { s.to_owned() }
    };
    if (-4..16).contains(&exponent) {
        let decimals = (15 - exponent).max(0) as usize;
        trim(&format!("{x:.decimals$}"))
    } else {
        format!("{}e{}{:02}", trim(mantissa), if exponent < 0 { '-' } else { '+' }, exponent.abs())
    }
}

fn column(index: usize) -> char {
    (b'A' + index as u8) as char
}

/// One cell as openpyxl writes it; None when the value is empty.
fn cell(reference: &str, value: &Py) -> Option<String> {
    Some(match value {
        Py::None => return None,
        Py::Bool(b) => format!("<c r=\"{reference}\" t=\"b\"><v>{}</v></c>", u8::from(*b)),
        Py::Int(i) => format!("<c r=\"{reference}\" t=\"n\"><v>{}</v></c>", if i.unsigned_abs() < 10u64.pow(16) { i.to_string() } else { g16(*i as f64) }),
        Py::Float(f) => format!("<c r=\"{reference}\" t=\"n\"><v>{}</v></c>", g16(*f)),
        Py::Str(s) => {
            let space = if s.trim() != s { " xml:space=\"preserve\"" } else { "" };
            format!("<c r=\"{reference}\" t=\"inlineStr\"><is><t{space}>{}</t></is></c>", escape(s))
        }
        other => return cell(reference, &Py::Str(other.dumps(false, false))),
    })
}

fn sheet(rows: &[Vec<Py>], frozen: bool, drawing: bool) -> String {
    let width = rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
    let mut out = format!(
        "<worksheet xmlns=\"{NS_MAIN}\"><sheetPr><outlinePr summaryBelow=\"1\" summaryRight=\"1\" /><pageSetUpPr /></sheetPr><dimension ref=\"A1:{}{}\" /><sheetViews><sheetView workbookViewId=\"0\">",
        column(width - 1),
        rows.len().max(1)
    );
    out.push_str(if frozen {
        "<pane ySplit=\"1\" topLeftCell=\"A2\" activePane=\"bottomLeft\" state=\"frozen\" /><selection pane=\"bottomLeft\" activeCell=\"A1\" sqref=\"A1\" />"
    } else {
        "<selection activeCell=\"A1\" sqref=\"A1\" />"
    });
    out.push_str("</sheetView></sheetViews><sheetFormatPr baseColWidth=\"8\" defaultRowHeight=\"15\" /><sheetData>");
    for (r, row) in rows.iter().enumerate() {
        out.push_str(&format!("<row r=\"{}\">", r + 1));
        for (c, value) in row.iter().enumerate() {
            if let Some(text) = cell(&format!("{}{}", column(c), r + 1), value) {
                out.push_str(&text);
            }
        }
        out.push_str("</row>");
    }
    out.push_str("</sheetData><pageMargins left=\"0.75\" right=\"0.75\" top=\"1\" bottom=\"1\" header=\"0.5\" footer=\"0.5\" />");
    if drawing {
        out.push_str(&format!("<drawing xmlns:r=\"{NS_REL}\" r:id=\"rId1\" />"));
    }
    out.push_str("</worksheet>");
    out
}

fn rich(text: &str) -> String {
    format!("<tx><rich><a:bodyPr /><a:p><a:pPr><a:defRPr /></a:pPr><a:r><a:t>{}</a:t></a:r></a:p></rich></tx>", escape(text))
}

fn chart(title: &str, sheet_name: &str, last_row: usize) -> String {
    let axis = |id: u32, cross: u32, title: &str| {
        format!(
            "<valAx><axId val=\"{id}\" /><scaling><orientation val=\"minMax\" /></scaling><axPos val=\"l\" /><majorGridlines /><title>{}</title><majorTickMark val=\"none\" /><minorTickMark val=\"none\" /><crossAx val=\"{cross}\" /></valAx>",
            rich(title)
        )
    };
    // openpyxl writes a one-cell range as that cell.
    let reference = |col: char| escape(&if last_row == 2 { format!("'{sheet_name}'!${col}$2") } else { format!("'{sheet_name}'!${col}$2:${col}${last_row}") });
    format!(
        "<chartSpace xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns=\"http://schemas.openxmlformats.org/drawingml/2006/chart\"><chart><title>{}</title><plotArea><scatterChart><ser><idx val=\"0\" /><order val=\"0\" /><spPr><a:ln><a:prstDash val=\"solid\" /></a:ln></spPr><marker><symbol val=\"none\" /><spPr><a:ln><a:prstDash val=\"solid\" /></a:ln></spPr></marker><xVal><numRef><f>{}</f></numRef></xVal><yVal><numRef><f>{}</f></numRef></yVal></ser><axId val=\"10\" /><axId val=\"20\" /></scatterChart>{}{}</plotArea><legend><legendPos val=\"r\" /></legend><plotVisOnly val=\"1\" /><dispBlanksAs val=\"gap\" /></chart></chartSpace>",
        rich(title),
        reference('A'),
        reference('B'),
        axis(10, 20, "Received timestamp (ms UTC)"),
        axis(20, 10, "Recorded value"),
    )
}

fn drawing() -> &'static str {
    "<wsDr xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns=\"http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing\"><oneCellAnchor><from><col>7</col><colOff>0</colOff><row>1</row><rowOff>0</rowOff></from><ext cx=\"9000000\" cy=\"4320000\" /><graphicFrame><nvGraphicFramePr><cNvPr id=\"1\" name=\"Chart 1\" /><cNvGraphicFramePr /></nvGraphicFramePr><xfrm /><a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/chart\"><c:chart r:id=\"rId1\" /></a:graphicData></a:graphic></graphicFrame><clientData /></oneCellAnchor></wsDr>"
}

fn relationships(items: &[(&str, String)]) -> String {
    let mut out = format!("<Relationships xmlns=\"{NS_PKG_REL}\">");
    for (i, (kind, target)) in items.iter().enumerate() {
        out.push_str(&format!("<Relationship Type=\"{kind}\" Target=\"{target}\" Id=\"rId{}\" />", i + 1));
    }
    out.push_str("</Relationships>");
    out
}

/// `YYYY-MM-DDTHH:MM:SSZ` of a Unix time in milliseconds.
pub(super) fn utc(ms: i64) -> (i64, u32, u32, u32, u32, u32) {
    let seconds = ms.div_euclid(1000);
    let (days, rest) = (seconds.div_euclid(86_400), seconds.rem_euclid(86_400));
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day, (rest / 3600) as u32, (rest % 3600 / 60) as u32, (rest % 60) as u32)
}

/// The workbook of the chosen channels with their regression metadata.
pub(super) fn workbook(streams: &[&Stream], metadata: &[Py], generated_ms: i64) -> Vec<u8> {
    let mut meta = vec![vec![Py::str("Channel"), Py::str("Regression / provenance")]];
    for (stream, info) in streams.iter().zip(metadata) {
        let text: Vec<char> = info.dumps(false, false).chars().collect();
        for chunk in text.chunks(30_000) {
            meta.push(vec![Py::str(&stream.key), Py::Str(chunk.iter().collect())]);
        }
    }
    let (y, mo, d, h, mi, s) = utc(generated_ms);
    let stamp = format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z");
    let mut parts: Vec<(String, Vec<u8>)> = Vec::new();
    let mut overrides = format!(
        "<Override PartName=\"/xl/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml\" /><Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\" /><Override PartName=\"/docProps/app.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.extended-properties+xml\" /><Override PartName=\"/xl/worksheets/sheet1.xml\" ContentType=\"{CT_WORKSHEET}\" />"
    );
    parts.push(("docProps/app.xml".into(), b"<Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\"><Application>Microsoft Excel Compatible / SEDS ground station (offline report)</Application><AppVersion>3.1</AppVersion></Properties>".to_vec()));
    parts.push((
        "docProps/core.xml".into(),
        format!("<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"><dc:creator>openpyxl</dc:creator><dcterms:created xsi:type=\"dcterms:W3CDTF\">{stamp}</dcterms:created><dcterms:modified xsi:type=\"dcterms:W3CDTF\">{stamp}</dcterms:modified></cp:coreProperties>").into_bytes(),
    ));
    parts.push(("xl/worksheets/sheet1.xml".into(), sheet(&meta, false, false).into_bytes()));
    let mut charts = String::new();
    let mut sheets = String::from("<sheet name=\"Regression metadata\" sheetId=\"1\" state=\"visible\" r:id=\"rId1\" />");
    let mut workbook_rels: Vec<(&str, String)> = vec![("http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet", "/xl/worksheets/sheet1.xml".into())];
    for (n, stream) in streams.iter().enumerate() {
        let number = n + 1;
        let name = format!("Channel {number}");
        let mut rows = vec![["Received ms", "Value", "Source ms", "Recording", "Row ID", "Channel"].map(Py::str).to_vec()];
        for s in &stream.samples {
            rows.push(vec![Py::Int(s.t), s.value.clone().unwrap_or(Py::None), s.source.map_or(Py::None, Py::Int), Py::str(s.recording), Py::Int(s.row_id), Py::str(&stream.key)]);
        }
        let sheet_part = format!("xl/worksheets/sheet{}.xml", number + 1);
        parts.push((sheet_part.clone(), sheet(&rows, true, true).into_bytes()));
        parts.push((format!("xl/drawings/drawing{number}.xml"), drawing().as_bytes().to_vec()));
        parts.push((format!("xl/drawings/_rels/drawing{number}.xml.rels"), relationships(&[("http://schemas.openxmlformats.org/officeDocument/2006/relationships/chart", format!("/xl/charts/chart{number}.xml"))]).into_bytes()));
        parts.push((format!("xl/worksheets/_rels/sheet{}.xml.rels", number + 1), relationships(&[("http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing", format!("/xl/drawings/drawing{number}.xml"))]).into_bytes()));
        overrides.push_str(&format!("<Override PartName=\"/{sheet_part}\" ContentType=\"{CT_WORKSHEET}\" /><Override PartName=\"/xl/drawings/drawing{number}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.drawing+xml\" />"));
        charts.push_str(&format!("<Override PartName=\"/xl/charts/chart{number}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.drawingml.chart+xml\" />"));
        parts.push((format!("xl/charts/chart{number}.xml"), chart(&stream.key, &name, rows.len()).into_bytes()));
        sheets.push_str(&format!("<sheet name=\"{name}\" sheetId=\"{}\" state=\"visible\" r:id=\"rId{}\" />", number + 1, number + 1));
        workbook_rels.push(("http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet", format!("/{sheet_part}")));
    }
    workbook_rels.push(("http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles", "styles.xml".into()));
    parts.push(("xl/styles.xml".into(), format!("<styleSheet xmlns=\"{NS_MAIN}\"><numFmts count=\"0\" /><fonts count=\"1\"><font><name val=\"Calibri\" /><family val=\"2\" /><sz val=\"11\" /><scheme val=\"minor\" /></font></fonts><fills count=\"2\"><fill><patternFill /></fill><fill><patternFill patternType=\"gray125\" /></fill></fills><borders count=\"1\"><border><left /><right /><top /><bottom /><diagonal /></border></borders><cellStyleXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\" /></cellStyleXfs><cellXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\" pivotButton=\"0\" quotePrefix=\"0\" xfId=\"0\" /></cellXfs><cellStyles count=\"1\"><cellStyle name=\"Normal\" xfId=\"0\" builtinId=\"0\" hidden=\"0\" /></cellStyles><tableStyles count=\"0\" defaultTableStyle=\"TableStyleMedium9\" defaultPivotStyle=\"PivotStyleLight16\" /></styleSheet>").into_bytes()));
    parts.push(("_rels/.rels".into(), relationships(&[
        ("http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument", "xl/workbook.xml".into()),
        ("http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties", "docProps/core.xml".into()),
        ("http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties", "docProps/app.xml".into()),
    ]).into_bytes()));
    parts.push(("xl/workbook.xml".into(), format!("<workbook xmlns:r=\"{NS_REL}\" xmlns=\"{NS_MAIN}\"><workbookPr /><workbookProtection /><bookViews><workbookView visibility=\"visible\" minimized=\"0\" showHorizontalScroll=\"1\" showVerticalScroll=\"1\" showSheetTabs=\"1\" tabRatio=\"600\" firstSheet=\"0\" activeTab=\"0\" autoFilterDateGrouping=\"1\" /></bookViews><sheets>{sheets}</sheets><definedNames /><calcPr calcId=\"124519\" fullCalcOnLoad=\"1\" /></workbook>").into_bytes()));
    parts.push(("xl/_rels/workbook.xml.rels".into(), relationships(&workbook_rels).into_bytes()));
    parts.push((
        "[Content_Types].xml".into(),
        format!("<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\" /><Default Extension=\"xml\" ContentType=\"application/xml\" />{overrides}{charts}<Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\" /></Types>").into_bytes(),
    ));
    zip(&parts, (y, mo, d, h, mi, s))
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// A ZIP archive of deflated entries stamped with `time` (UTC fields).
fn zip(parts: &[(String, Vec<u8>)], time: (i64, u32, u32, u32, u32, u32)) -> Vec<u8> {
    let (y, mo, d, h, mi, s) = time;
    let dos_time = ((h << 11) | (mi << 5) | (s / 2)) as u16;
    let dos_date = ((((y - 1980).clamp(0, 127) as u32) << 9) | (mo << 5) | d) as u16;
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in parts {
        let packed = miniz_oxide::deflate::compress_to_vec(data, 6);
        let crc = crc32(data);
        let offset = out.len() as u32;
        let header = |signature: u32, central: bool| {
            let mut h = Vec::new();
            h.extend_from_slice(&signature.to_le_bytes());
            if central {
                h.extend_from_slice(&20u16.to_le_bytes());
            }
            for v in [20u16, 0, 8, dos_time, dos_date] {
                h.extend_from_slice(&v.to_le_bytes());
            }
            for v in [crc, packed.len() as u32, data.len() as u32] {
                h.extend_from_slice(&v.to_le_bytes());
            }
            h.extend_from_slice(&(name.len() as u16).to_le_bytes());
            h.extend_from_slice(&0u16.to_le_bytes());
            if central {
                for v in [0u16, 0, 0] {
                    h.extend_from_slice(&v.to_le_bytes());
                }
                h.extend_from_slice(&0u32.to_le_bytes());
                h.extend_from_slice(&offset.to_le_bytes());
            }
            h.extend_from_slice(name.as_bytes());
            h
        };
        out.extend_from_slice(&header(0x0403_4b50, false));
        out.extend_from_slice(&packed);
        central.extend_from_slice(&header(0x0201_4b50, true));
    }
    let start = out.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    for v in [0u16, 0, parts.len() as u16, parts.len() as u16] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&start.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_print_as_percent_16g() {
        for (x, text) in [(700.0, "700"), (698.5, "698.5"), (0.1 + 0.2, "0.3"), (1e16, "1e+16"), (1e-7, "1e-07"), (0.0001, "0.0001"), (-2.5, "-2.5"), (1767225600000.0, "1767225600000"), (123456789.123456789, "123456789.1234568")] {
            assert_eq!(g16(x), text);
        }
        assert_eq!(utc(1_767_225_600_000), (2026, 1, 1, 0, 0, 0));
        assert_eq!(utc(951_825_599_000), (2000, 2, 29, 11, 59, 59));
    }
}
