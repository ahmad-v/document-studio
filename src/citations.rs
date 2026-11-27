//! Bounded, offline interchange for CSL JSON, RIS and BibTeX references.
//! This deliberately exchanges records; it does not claim to render a citation style.
use localwright_core::{Error, Job, Result};
use serde_json::{Map, Value};

const MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_RECORDS: usize = 10_000;

fn invalid(message: impl Into<String>) -> Error {
    Error::input(message.into())
}

fn text(value: &Value) -> Option<&str> {
    value.as_str()
}

fn records(value: Value) -> Result<Vec<Value>> {
    let list = match value {
        Value::Array(values) => values,
        Value::Object(_) => vec![value],
        _ => {
            return Err(invalid(
                "CSL JSON must contain an object or an array of objects",
            ))
        }
    };
    if list.is_empty() || list.len() > MAX_RECORDS {
        return Err(invalid("Reference count must be between 1 and 10,000"));
    }
    for record in &list {
        let object = record
            .as_object()
            .ok_or_else(|| invalid("Each CSL reference must be an object"))?;
        if object
            .get("type")
            .and_then(text)
            .is_none_or(|s| s.trim().is_empty())
        {
            return Err(invalid("Each CSL reference needs a non-empty type"));
        }
        if let Some(id) = object.get("id") {
            if !id.is_string() || id.as_str().is_some_and(|s| s.is_empty() || s.len() > 512) {
                return Err(invalid(
                    "CSL reference IDs must be non-empty strings up to 512 bytes",
                ));
            }
        }
        if let Some(title) = object.get("title") {
            if !title.is_string() {
                return Err(invalid("CSL title must be a string"));
            }
        }
    }
    Ok(list)
}

fn csl_from_ris(input: &str, job: &Job) -> Result<Vec<Value>> {
    let mut all = Vec::new();
    let mut current: Map<String, Value> = Map::new();
    let mut authors: Vec<Value> = Vec::new();
    let mut keywords: Vec<String> = Vec::new();
    let mut saw_record = false;
    for raw in input.lines() {
        job.check()?;
        let line = raw.trim_end_matches('\r');
        if line.len() > 64 * 1024 {
            return Err(invalid("RIS line exceeds 64 KiB"));
        }
        if line.trim().is_empty() {
            continue;
        }
        let tagged = if line == "ER  -" {
            Some(("ER", ""))
        } else {
            line.split_once("  - ")
        };
        if let Some((tag, value)) = tagged {
            if tag.len() != 2 || !tag.bytes().all(|b| b.is_ascii_uppercase()) {
                return Err(invalid("Malformed RIS tag"));
            }
            let value = value.trim();
            match tag {
                "TY" => {
                    if saw_record {
                        return Err(invalid("RIS record is missing ER terminator"));
                    }
                    saw_record = true;
                    current.insert(
                        "type".into(),
                        Value::String(
                            match value {
                                "JOUR" | "JFULL" => "article-journal",
                                "BOOK" | "EBOOK" => "book",
                                "CHAP" => "chapter",
                                "THES" => "thesis",
                                "RPRT" => "report",
                                "CONF" => "paper-conference",
                                _ => "article",
                            }
                            .into(),
                        ),
                    );
                }
                "ER" => {
                    if !saw_record {
                        return Err(invalid("RIS end marker without a record"));
                    }
                    if !authors.is_empty() {
                        current.insert("author".into(), Value::Array(std::mem::take(&mut authors)));
                    }
                    if !keywords.is_empty() {
                        current.insert(
                            "keyword".into(),
                            Value::Array(keywords.drain(..).map(Value::String).collect()),
                        );
                    }
                    if !current.contains_key("id") {
                        current
                            .insert("id".into(), Value::String(format!("ris-{}", all.len() + 1)));
                    }
                    all.push(Value::Object(std::mem::take(&mut current)));
                    saw_record = false;
                }
                "ID" => {
                    current.insert("id".into(), Value::String(value.to_owned()));
                }
                "AU" | "A1" => {
                    let (family, given) =
                        value.split_once(", ").map_or((value, ""), |(a, b)| (a, b));
                    let mut name = Map::new();
                    name.insert("family".into(), Value::String(family.trim().into()));
                    if !given.trim().is_empty() {
                        name.insert("given".into(), Value::String(given.trim().into()));
                    }
                    authors.push(Value::Object(name));
                }
                "TI" | "T1" => {
                    current.insert("title".into(), Value::String(value.into()));
                }
                "T2" | "JO" | "JF" => {
                    current
                        .entry("container-title")
                        .or_insert_with(|| Value::String(value.into()));
                }
                "PY" | "Y1" | "DA" => {
                    current.insert("issued".into(), issued(value));
                }
                "VL" => {
                    current.insert("volume".into(), Value::String(value.into()));
                }
                "IS" => {
                    current.insert("issue".into(), Value::String(value.into()));
                }
                "SP" => {
                    current.insert("page".into(), Value::String(value.into()));
                }
                "EP" => {
                    if let Some(Value::String(start)) = current.get_mut("page") {
                        start.push('-');
                        start.push_str(value);
                    } else {
                        current.insert("page".into(), Value::String(value.into()));
                    }
                }
                "DO" => {
                    current.insert("DOI".into(), Value::String(value.into()));
                }
                "UR" => {
                    current.insert("URL".into(), Value::String(value.into()));
                }
                "PB" => {
                    current.insert("publisher".into(), Value::String(value.into()));
                }
                "CY" => {
                    current.insert("publisher-place".into(), Value::String(value.into()));
                }
                "AB" => {
                    current.insert("abstract".into(), Value::String(value.into()));
                }
                "KW" => keywords.push(value.into()),
                _ => {}
            }
        } else {
            return Err(invalid(
                "RIS continuation lines are unsupported; provide complete tagged fields",
            ));
        }
    }
    if saw_record {
        return Err(invalid("RIS record is missing ER terminator"));
    }
    if all.is_empty() || all.len() > MAX_RECORDS {
        return Err(invalid("RIS must contain 1 to 10,000 records"));
    }
    records(Value::Array(all))
}

fn issued(value: &str) -> Value {
    let year = value
        .get(..4)
        .filter(|s| s.bytes().all(|b| b.is_ascii_digit()));
    match year {
        Some(y) => serde_json::json!({"date-parts":[[y.parse::<i32>().unwrap_or(0)]]}),
        None => Value::String(value.into()),
    }
}

fn ris(records: &[Value], job: &Job) -> Result<String> {
    let mut out = String::new();
    for (index, record) in records.iter().enumerate() {
        job.check()?;
        let object = record.as_object().expect("validated CSL record");
        let ty = match object.get("type").and_then(text).unwrap_or("") {
            "article-journal" => "JOUR",
            "book" => "BOOK",
            "chapter" => "CHAP",
            "thesis" => "THES",
            "report" => "RPRT",
            "paper-conference" => "CONF",
            _ => "GEN",
        };
        put(&mut out, "TY", ty)?;
        put(
            &mut out,
            "ID",
            object
                .get("id")
                .and_then(text)
                .unwrap_or(&format!("csl-{}", index + 1)),
        )?;
        if let Some(title) = object.get("title").and_then(text) {
            put(&mut out, "TI", title)?;
        }
        if let Some(author_list) = object.get("author").and_then(Value::as_array) {
            for author in author_list {
                let family = author.get("family").and_then(text).unwrap_or("");
                let given = author.get("given").and_then(text).unwrap_or("");
                if !family.is_empty() {
                    put(
                        &mut out,
                        "AU",
                        if given.is_empty() {
                            family.to_owned()
                        } else {
                            format!("{family}, {given}")
                        }
                        .as_str(),
                    )?;
                }
            }
        }
        for (key, tag) in [
            ("container-title", "JO"),
            ("volume", "VL"),
            ("issue", "IS"),
            ("page", "SP"),
            ("DOI", "DO"),
            ("URL", "UR"),
            ("publisher", "PB"),
            ("publisher-place", "CY"),
            ("abstract", "AB"),
        ] {
            if let Some(v) = object.get(key).and_then(text) {
                put(&mut out, tag, v)?;
            }
        }
        if let Some(date) = object
            .get("issued")
            .and_then(|v| v.get("date-parts"))
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .and_then(Value::as_array)
        {
            if let Some(year) = date.first().and_then(Value::as_i64) {
                put(&mut out, "PY", &year.to_string())?;
            }
        }
        out.push_str("ER  -\n");
    }
    Ok(out)
}

fn put(out: &mut String, tag: &str, value: &str) -> Result<()> {
    if value.contains(['\n', '\r', '\0']) || value.len() > 64 * 1024 {
        return Err(invalid(
            "Citation field contains a line break or exceeds 64 KiB",
        ));
    }
    out.push_str(tag);
    out.push_str("  - ");
    out.push_str(value);
    out.push('\n');
    Ok(())
}

fn bibtex(records: &[Value], job: &Job) -> Result<String> {
    let mut out = String::new();
    for (index, record) in records.iter().enumerate() {
        job.check()?;
        let object = record.as_object().expect("validated CSL record");
        let kind = match object.get("type").and_then(text).unwrap_or("") {
            "article-journal" => "article",
            "book" => "book",
            "chapter" => "incollection",
            "thesis" => "thesis",
            "report" => "techreport",
            "paper-conference" => "inproceedings",
            _ => "misc",
        };
        let id = object
            .get("id")
            .and_then(text)
            .filter(|s| valid_bib_key(s))
            .map(str::to_owned)
            .unwrap_or_else(|| format!("ref{}", index + 1));
        out.push_str(&format!("@{kind}{{{id},\n"));
        let mut fields: Vec<(&str, String)> = Vec::new();
        if let Some(title) = object.get("title").and_then(text) {
            fields.push(("title", title.into()));
        }
        if let Some(authors) = object.get("author").and_then(Value::as_array) {
            let names = authors
                .iter()
                .filter_map(|a| {
                    let family = a.get("family").and_then(text).unwrap_or("");
                    let given = a.get("given").and_then(text).unwrap_or("");
                    (!family.is_empty()).then(|| {
                        if given.is_empty() {
                            family.to_owned()
                        } else {
                            format!("{given} {family}")
                        }
                    })
                })
                .collect::<Vec<_>>()
                .join(" and ");
            if !names.is_empty() {
                fields.push(("author", names));
            }
        }
        for (source, target) in [
            ("container-title", "journal"),
            ("volume", "volume"),
            ("issue", "number"),
            ("page", "pages"),
            ("DOI", "doi"),
            ("URL", "url"),
            ("publisher", "publisher"),
            ("publisher-place", "address"),
            ("abstract", "abstract"),
        ] {
            if let Some(value) = object.get(source).and_then(text) {
                fields.push((target, value.into()));
            }
        }
        if let Some(year) = object
            .get("issued")
            .and_then(|v| v.get("date-parts"))
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .and_then(Value::as_i64)
        {
            fields.push(("year", year.to_string()));
        }
        for (n, (key, value)) in fields.iter().enumerate() {
            let comma = if n + 1 == fields.len() { "" } else { "," };
            out.push_str(&format!("  {key} = {{{}}}{comma}\n", escape_bib(value)?));
        }
        out.push_str("}\n\n");
    }
    Ok(out)
}

fn valid_bib_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 128
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_:.".contains(&b))
}
fn escape_bib(value: &str) -> Result<String> {
    if value.contains(['\n', '\r', '\0']) || value.len() > 64 * 1024 {
        return Err(invalid(
            "BibTeX field contains a line break or exceeds 64 KiB",
        ));
    }
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        escaped.push_str(match ch {
            '\\' => "\\textbackslash{}",
            '{' => "\\{",
            '}' => "\\}",
            '&' => "\\&",
            '%' => "\\%",
            '$' => "\\$",
            '#' => "\\#",
            '_' => "\\_",
            '~' => "\\textasciitilde{}",
            '^' => "\\textasciicircum{}",
            other => {
                escaped.push(other);
                continue;
            }
        });
    }
    Ok(escaped)
}

pub fn convert(input: &[u8], from: &str, to: &str, job: &Job) -> Result<Vec<u8>> {
    if input.len() > MAX_BYTES {
        return Err(invalid("Citation input exceeds 8 MiB"));
    }
    let source = std::str::from_utf8(input).map_err(|_| invalid("Citation input must be UTF-8"))?;
    let list = match from {
        "csl-json" => records(serde_json::from_str(source).map_err(|e| invalid(e.to_string()))?)?,
        "ris" => csl_from_ris(source, job)?,
        _ => return Err(invalid("Supported source formats are csl-json and ris")),
    };
    let output = match to {
        "csl-json" => serde_json::to_vec_pretty(&list)?,
        "ris" => ris(&list, job)?.into_bytes(),
        "bibtex" => bibtex(&list, job)?.into_bytes(),
        _ => {
            return Err(invalid(
                "Supported output formats are csl-json, ris and bibtex",
            ))
        }
    };
    if output.len() > MAX_BYTES {
        return Err(invalid("Converted bibliography exceeds 8 MiB"));
    }
    Ok(output)
}

pub fn validate(input: &[u8]) -> Result<usize> {
    if input.len() > MAX_BYTES {
        return Err(invalid("Citation input exceeds 8 MiB"));
    }
    let source = std::str::from_utf8(input).map_err(|_| invalid("CSL JSON must be UTF-8"))?;
    let parsed = serde_json::from_str(source).map_err(|e| invalid(e.to_string()))?;
    Ok(records(parsed)?.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    const FIXTURE: &str = r#"[{"id":"smith-2024","type":"article-journal","title":"Research & Rust","author":[{"family":"Smith","given":"Ada"}],"container-title":"Offline Systems","volume":"2","issue":"1","page":"4-9","DOI":"10.1234/example","issued":{"date-parts":[[2024]]}}]"#;
    #[test]
    fn csl_ris_csl_round_trip_preserves_supported_fields() {
        let ris = convert(FIXTURE.as_bytes(), "csl-json", "ris", &Job::new()).unwrap();
        let csl = convert(&ris, "ris", "csl-json", &Job::new()).unwrap();
        let value: Value = serde_json::from_slice(&csl).unwrap();
        let record = &value[0];
        assert_eq!(record["title"], "Research & Rust");
        assert_eq!(record["author"][0]["family"], "Smith");
        assert_eq!(record["issued"]["date-parts"][0][0], 2024);
        assert_eq!(record["DOI"], "10.1234/example");
    }
    #[test]
    fn bibtex_escapes_braces_and_preserves_unicode() {
        let input = r#"{"id":"key","type":"book","title":"Café {science} & systems_"}"#;
        let bib = convert(input.as_bytes(), "csl-json", "bibtex", &Job::new()).unwrap();
        let bib = String::from_utf8(bib).unwrap();
        assert!(bib.contains("Café "));
        assert!(bib.contains(r"\{science\}"));
        assert!(bib.contains(r"\& systems\_"));
    }
    #[test]
    fn rejects_malformed_or_unbounded_records() {
        assert!(validate(br#"{"title":"missing type"}"#).is_err());
        assert!(convert(
            b"TY  - JOUR\nTI  - unterminated",
            "ris",
            "csl-json",
            &Job::new()
        )
        .is_err());
        assert!(convert(b"not-json", "csl-json", "ris", &Job::new()).is_err());
    }
}
