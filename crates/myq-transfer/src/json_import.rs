//! JSON as an import source. A JSON file is turned into a CSV file once, at the boundary, and the whole CSV pipeline (preview, column
//! mapping, schema inference, insert / ignore / replace / update, rollback) runs on that CSV unchanged.
//!
//! Accepted: an array of objects (`[{...}, {...}]`), JSON Lines / NDJSON (one object per line), or a single object (one row).
//! Columns are the union of the keys in first-seen order; a key a row lacks and a JSON `null` both become `\N` (the NULL text the import
//! dialog then uses); strings are kept as they are, numbers and booleans become their JSON text, nested arrays and objects become compact
//! JSON text. An array is read into memory, so it is refused above [`MAX_ARRAY_BYTES`]; JSON Lines is streamed.

use crate::{Error, Result};
use serde_json::Value;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

/// The text that stands for NULL in the CSV that is written.
pub const NULL_TEXT: &str = "\\N";

/// Largest JSON array that is read into memory (a larger one should be JSON Lines).
pub const MAX_ARRAY_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct JsonConversion {
    pub rows: u64,
    pub columns: Vec<String>,
}

fn cell(v: &Value) -> String {
    match v {
        Value::Null => NULL_TEXT.to_string(),
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

fn objects(path: &Path) -> Result<Box<dyn Iterator<Item = Result<serde_json::Map<String, Value>>>>> {
    let mut head = [0u8; 4096];
    let n = File::open(path)?.read(&mut head)?;
    let first = String::from_utf8_lossy(&head[..n]).trim_start_matches('\u{feff}').trim_start().chars().next();
    let size = std::fs::metadata(path)?.len();
    let obj = |v: Value, at: String| -> Result<serde_json::Map<String, Value>> {
        match v {
            Value::Object(m) => Ok(m),
            other => Err(Error::Invalid(format!("{at} is {}, not an object: every row of a JSON import must be an object like {{\"column\": value}}", match other { Value::Array(_) => "an array", Value::String(_) => "a string", Value::Number(_) => "a number", Value::Bool(_) => "true or false", _ => "null", }))),
        }
    };
    if first == Some('[') {
        if size > MAX_ARRAY_BYTES {
            return Err(Error::Invalid(format!("this JSON array is {} MB: arrays are read into memory up to {} MB. Convert it to JSON Lines (one object per line) to import it.", size / 1_048_576, MAX_ARRAY_BYTES / 1_048_576)));
        }
        let text = std::fs::read_to_string(path)?;
        let all: Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| Error::Invalid(format!("not valid JSON: {e}")))?;
        let Value::Array(items) = all else { return Err(Error::Invalid("expected a JSON array".into())) };
        Ok(Box::new(items.into_iter().enumerate().map(move |(i, v)| obj(v, format!("item {}", i + 1)))))
    } else {
        // one object, or one object per line
        let reader = BufReader::new(File::open(path)?);
        let mut n = 0usize;
        Ok(Box::new(reader.lines().filter_map(move |l| match l {
            Err(e) => Some(Err(Error::Io(e))),
            Ok(l) => {
                let t = l.trim_start_matches('\u{feff}').trim().to_string();
                if t.is_empty() { return None; }
                n += 1;
                Some(serde_json::from_str::<Value>(&t).map_err(|e| Error::Invalid(format!("line {n} is not valid JSON: {e}"))).and_then(|v| obj(v, format!("line {n}"))))
            }
        })))
    }
}

/// Write `src` as CSV to `dest` (header row, `\N` for NULL). Returns the number of rows and the columns.
pub fn json_to_csv(src: &Path, dest: &Path) -> Result<JsonConversion> {
    let mut columns: Vec<String> = Vec::new();
    for o in objects(src)? {
        for k in o?.keys() {
            if !columns.contains(k) {
                columns.push(k.clone());
            }
        }
    }
    if columns.is_empty() {
        return Err(Error::Invalid("no rows found: the file holds no JSON objects".into()));
    }
    let mut w = csv::WriterBuilder::new().from_writer(File::create(dest)?);
    w.write_record(&columns)?;
    let mut rows = 0u64;
    for o in objects(src)? {
        let o = o?;
        w.write_record(columns.iter().map(|c| o.get(c).map(cell).unwrap_or_else(|| NULL_TEXT.to_string())))?;
        rows += 1;
    }
    w.flush()?;
    Ok(JsonConversion { rows, columns })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str, content: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("mylodb_json_{}_{}", std::process::id(), name));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("in.json");
        std::fs::write(&p, content).unwrap();
        p
    }
    fn convert(name: &str, content: &str) -> Result<(JsonConversion, String)> {
        let src = tmp(name, content);
        let dest = src.with_file_name("out.csv");
        let r = json_to_csv(&src, &dest)?;
        Ok((r, std::fs::read_to_string(dest).unwrap()))
    }

    #[test]
    fn an_array_of_objects_becomes_csv_with_the_union_of_keys() {
        let (r, csv) = convert("array", r#"[{"id":1,"name":"Ann","vip":true},{"id":2,"name":"O'Neil, \"Bo\"","extra":null,"nested":{"a":[1,2]}},{"id":3}]"#).unwrap();
        assert_eq!((r.rows, r.columns.clone()), (3, vec!["id".to_string(), "name".into(), "vip".into(), "extra".into(), "nested".into()]));
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines[0], "id,name,vip,extra,nested");
        assert_eq!(lines[1], "1,Ann,true,\\N,\\N", "a key the row lacks is NULL");
        assert_eq!(lines[2], r#"2,"O'Neil, ""Bo""",\N,\N,"{""a"":[1,2]}""#, "quotes and commas survive, JSON null is NULL, nested JSON is kept as text");
        assert_eq!(lines[3], "3,\\N,\\N,\\N,\\N");
    }

    #[test]
    fn json_lines_and_a_single_object_work_and_blank_lines_are_skipped() {
        let (r, csv) = convert("lines", "{\"a\":1}\n\n{\"a\":2,\"b\":\"x\"}\n").unwrap();
        assert_eq!((r.rows, r.columns), (2, vec!["a".to_string(), "b".into()]));
        assert_eq!(csv.lines().collect::<Vec<_>>(), ["a,b", "1,\\N", "2,x"]);
        let (one, _) = convert("one", "{\"only\": \"row\"}").unwrap();
        assert_eq!(one.rows, 1);
    }

    #[test]
    fn numbers_keep_their_text_and_a_byte_order_mark_is_ignored() {
        let (_, csv) = convert("num", "\u{feff}[{\"n\":12345678901234567890,\"f\":1.50,\"neg\":-3}]").unwrap();
        assert_eq!(csv.lines().nth(1).unwrap(), "12345678901234567890,1.5,-3", "a number is not rounded through a float (1.50 prints as 1.5, as JSON numbers do)");
    }

    #[test]
    fn mistakes_are_explained() {
        assert!(convert("bad1", "[1, 2]").unwrap_err().to_string().contains("item 1 is a number, not an object"));
        assert!(convert("bad2", "[{\"a\":1},").unwrap_err().to_string().contains("not valid JSON"));
        assert!(convert("bad3", "{\"a\":1}\nnot json\n").unwrap_err().to_string().contains("line 2 is not valid JSON"));
        assert!(convert("bad4", "[]").unwrap_err().to_string().contains("no rows found"));
        assert!(convert("bad5", "").unwrap_err().to_string().contains("no rows found"));
    }
}
