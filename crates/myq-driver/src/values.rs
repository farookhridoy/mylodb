//! Turning server values into display text. One owner for every result producer (editor and data grid).
//!
//! Binary-protocol results carry typed values (dates, ints) while text-protocol results carry bytes;
//! both are normalised here using the column's declared type.

use mysql_async::{consts::ColumnType as T, Column, Row, Value};

/// True for BINARY/VARBINARY/BLOB/GEOMETRY columns: their bytes are shown as hex, never as text.
pub fn is_binary(c: &Column) -> bool {
    c.character_set() == 63
        && matches!(
            c.column_type(),
            T::MYSQL_TYPE_TINY_BLOB
                | T::MYSQL_TYPE_MEDIUM_BLOB
                | T::MYSQL_TYPE_LONG_BLOB
                | T::MYSQL_TYPE_BLOB
                | T::MYSQL_TYPE_VAR_STRING
                | T::MYSQL_TYPE_STRING
                | T::MYSQL_TYPE_VARCHAR
                | T::MYSQL_TYPE_GEOMETRY
        )
}

pub fn is_numeric(c: &Column) -> bool {
    matches!(
        c.column_type(),
        T::MYSQL_TYPE_TINY
            | T::MYSQL_TYPE_SHORT
            | T::MYSQL_TYPE_LONG
            | T::MYSQL_TYPE_LONGLONG
            | T::MYSQL_TYPE_INT24
            | T::MYSQL_TYPE_FLOAT
            | T::MYSQL_TYPE_DOUBLE
            | T::MYSQL_TYPE_DECIMAL
            | T::MYSQL_TYPE_NEWDECIMAL
            | T::MYSQL_TYPE_YEAR
    )
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

/// Largest prefix of `b` (at most `max` bytes) that is valid UTF-8.
fn utf8_prefix(b: &[u8], max: usize) -> &str {
    let cut = &b[..b.len().min(max)];
    match std::str::from_utf8(cut) {
        Ok(s) => s,
        Err(e) => std::str::from_utf8(&cut[..e.valid_up_to()]).unwrap_or(""),
    }
}

/// Display text for one cell. `max` caps the output size (bytes of the source value);
/// the returned flag says the cell was cut. `None` is SQL NULL.
pub fn cell_text(v: Value, col: &Column, max: Option<usize>) -> (Option<String>, bool) {
    let cap = max.unwrap_or(usize::MAX);
    match v {
        Value::NULL => (None, false),
        Value::Bytes(b) => {
            if col.column_type() == T::MYSQL_TYPE_BIT {
                let n = b.iter().fold(0u64, |acc, x| (acc << 8) | *x as u64);
                return (Some(n.to_string()), false);
            }
            let cut = b.len() > cap;
            if is_binary(col) {
                return (Some(format!("0x{}", hex(&b[..b.len().min(cap)]))), cut);
            }
            match std::str::from_utf8(&b) {
                Ok(_) => (Some(utf8_prefix(&b, cap).to_string()), cut),
                Err(_) => (Some(format!("0x{}", hex(&b[..b.len().min(cap)]))), cut),
            }
        }
        Value::Int(i) => (Some(i.to_string()), false),
        Value::UInt(u) => (Some(u.to_string()), false),
        Value::Float(f) => (Some(f.to_string()), false),
        Value::Double(d) => (Some(d.to_string()), false),
        Value::Date(y, mo, d, h, mi, s, us) => {
            // Fractional digits follow the column's declared precision; stray microseconds show all 6.
            let digits = match col.decimals() as usize {
                0 if us > 0 => 6,
                n => n.min(6),
            };
            let frac = if digits > 0 { format!(".{us:06}")[..=digits].to_string() } else { String::new() };
            let text = if col.column_type() == T::MYSQL_TYPE_DATE {
                format!("{y:04}-{mo:02}-{d:02}")
            } else {
                format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}{frac}")
            };
            (Some(text), false)
        }
        Value::Time(neg, days, h, mi, s, us) => {
            let hours = days * 24 + h as u32;
            let frac = if us > 0 { format!(".{us:06}") } else { String::new() };
            (Some(format!("{}{hours:02}:{mi:02}:{s:02}{frac}", if neg { "-" } else { "" })), false)
        }
    }
}

/// Convert a whole row. Returns the cells and the indexes of the cells that were truncated.
pub fn row_cells(row: Row, max: Option<usize>) -> (Vec<Option<String>>, Vec<usize>) {
    let cols = row.columns();
    let mut cut = Vec::new();
    let cells = row
        .unwrap()
        .into_iter()
        .zip(cols.iter())
        .enumerate()
        .map(|(i, (v, c))| {
            let (t, was_cut) = cell_text(v, c, max);
            if was_cut {
                cut.push(i);
            }
            t
        })
        .collect();
    (cells, cut)
}
