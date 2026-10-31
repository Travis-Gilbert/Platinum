//! Read a file into passages the CRM can highlight.
//!
//! This follows Glim's readers: a JSON value becomes a tree and each field is
//! a passage, a CSV record becomes a row, and a SQLite database is copied
//! aside and queried read-only so the original file is left untouched. Glim's
//! service crate is not linked. It pins rusqlite 0.37, Servo pins 0.38, and
//! both want the one sqlite library.

use std::fs::{self, File};
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};
use serde_json::Value;

use super::model::MaterialKind;

/// One line of the reading. A passage index is a field or a row that can be tagged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Row {
    pub depth: u32,
    pub label: String,
    pub passage: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Reading {
    pub kind: MaterialKind,
    pub title: String,
    pub passages: Vec<String>,
    pub rows: Vec<Row>,
}

const TABLE_ROWS: usize = 40;
const DATABASE_TABLES: usize = 8;
const REFUSED: &str = "Attach a text file, a JSON object, a CSV table, or a SQLite database.";

pub(crate) fn open(path: &Path) -> Result<Reading, String> {
    let title = file_title(path);
    let extension = file_extension(path);
    if refused(&extension) {
        return Err(REFUSED.into());
    }
    if is_sqlite(path) {
        return open_database(path, title);
    }
    if json_extension(&extension) {
        return open_json(path, title, &extension);
    }
    if table_extension(&extension) {
        return open_table(path, title, &extension);
    }
    open_text(path, title)
}

fn file_title(path: &Path) -> String {
    path.file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("File")
        .to_owned()
}

fn file_extension(path: &Path) -> String {
    path.extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn refused(extension: &str) -> bool {
    matches!(
        extension,
        "pdf" | "png" | "jpg" | "jpeg" | "gif" | "webp" | "zip" | "docx" | "xlsx"
    )
}

fn json_extension(extension: &str) -> bool {
    matches!(extension, "json" | "jsonc" | "jsonl" | "ndjson")
}

fn table_extension(extension: &str) -> bool {
    matches!(extension, "csv" | "tsv")
}

fn open_database(path: &Path, title: String) -> Result<Reading, String> {
    if sqlite_header(path) {
        database(path, title)
    } else {
        Err("This database could not be opened.".into())
    }
}

fn open_json(path: &Path, title: String, extension: &str) -> Result<Reading, String> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(_) => return Err("This file could not be read.".into()),
    };
    let value = match parse_json(extension, &text) {
        Ok(value) => value,
        Err(error) => return Err(error.to_string()),
    };
    json_reading(title, &value)
}

fn parse_json(extension: &str, text: &str) -> Result<Value, serde_json::Error> {
    match extension {
        "jsonc" => serde_json::from_str(&strip_jsonc(text)),
        "jsonl" | "ndjson" => json_lines(text),
        _ => serde_json::from_str(text),
    }
}

fn open_table(path: &Path, title: String, extension: &str) -> Result<Reading, String> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(_) => return Err("This file could not be read.".into()),
    };
    let delimiter = if extension == "tsv" { b'\t' } else { b',' };
    table(title, &read_records(file, delimiter)?)
}

fn read_records(file: File, delimiter: u8) -> Result<Vec<Vec<String>>, String> {
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(false)
        .flexible(true)
        .from_reader(file);
    let mut records = Vec::new();
    for record in reader.records() {
        let record = match record {
            Ok(record) => record,
            Err(error) => return Err(error.to_string()),
        };
        records.push(record.iter().map(str::to_owned).collect());
        if records.len() == TABLE_ROWS {
            break;
        }
    }
    Ok(records)
}

fn open_text(path: &Path, title: String) -> Result<Reading, String> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(_) => return Err("This file could not be read.".into()),
    };
    if bytes.contains(&0) {
        return Err(REFUSED.into());
    }
    match String::from_utf8(bytes) {
        Ok(text) => text_reading(title, &text),
        Err(_) => Err("This file is not UTF-8.".into()),
    }
}

fn json_reading(title: String, value: &Value) -> Result<Reading, String> {
    let mut passages = Vec::new();
    let mut rows = Vec::new();
    let mut next = 0usize;
    walk("$", "$", value, 0, &mut next, &mut passages, &mut rows);
    if passages.is_empty() {
        return Err("This JSON object has no fields to highlight.".into());
    }
    Ok(Reading {
        kind: MaterialKind::Json,
        title,
        passages,
        rows,
    })
}

fn walk(
    key: &str,
    path: &str,
    value: &Value,
    depth: u32,
    next: &mut usize,
    passages: &mut Vec<String>,
    rows: &mut Vec<Row>,
) {
    match value {
        Value::Object(fields) => {
            walk_object(key, path, fields, depth, next, passages, rows);
        }
        Value::Array(items) => walk_array(key, path, items, depth, next, passages, rows),
        other => walk_scalar(key, path, other, depth, next, passages, rows),
    }
}

fn walk_object(
    key: &str,
    path: &str,
    fields: &serde_json::Map<String, Value>,
    depth: u32,
    next: &mut usize,
    passages: &mut Vec<String>,
    rows: &mut Vec<Row>,
) {
    rows.push(Row {
        depth,
        label: format!("{key}  {{ {} keys }}", fields.len()),
        passage: None,
    });
    for (child_key, child) in fields {
        let child_path = if path == "$" {
            format!("$.{child_key}")
        } else {
            format!("{path}.{child_key}")
        };
        walk(
            child_key,
            &child_path,
            child,
            depth + 1,
            next,
            passages,
            rows,
        );
    }
}

fn walk_array(
    key: &str,
    path: &str,
    items: &[Value],
    depth: u32,
    next: &mut usize,
    passages: &mut Vec<String>,
    rows: &mut Vec<Row>,
) {
    rows.push(Row {
        depth,
        label: format!("{key}  [ {} items ]", items.len()),
        passage: None,
    });
    for (index, child) in items.iter().enumerate() {
        let child_key = format!("[{index}]");
        walk(
            &child_key,
            &format!("{path}{child_key}"),
            child,
            depth + 1,
            next,
            passages,
            rows,
        );
    }
}

fn walk_scalar(
    key: &str,
    path: &str,
    value: &Value,
    depth: u32,
    next: &mut usize,
    passages: &mut Vec<String>,
    rows: &mut Vec<Row>,
) {
    let Some(text) = scalar_text(value) else {
        return;
    };
    let passage = *next;
    passages.push(format!("{path}: {text}"));
    *next += 1;
    rows.push(Row {
        depth,
        label: format!("{key}  {text}"),
        passage: Some(passage),
    });
}

fn scalar_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        Value::Null => Some("null".to_owned()),
        Value::Object(_) | Value::Array(_) => None,
    }
}

/// Drop JSONC comments and trailing commas. Offsets inside strings stay put,
/// which is the approach Glim uses before parsing.
fn strip_jsonc(text: &str) -> String {
    let mut bytes = text.as_bytes().to_vec();
    blank_comments(&mut bytes);
    blank_trailing_commas(&mut bytes);
    String::from_utf8(bytes).unwrap_or_else(|_| text.to_owned())
}

fn blank_comments(bytes: &mut [u8]) {
    let mut index = 0;
    let mut string = false;
    while index < bytes.len() {
        index = comment_step(bytes, index, &mut string);
    }
}

fn comment_step(bytes: &mut [u8], index: usize, string: &mut bool) -> usize {
    if *string {
        return string_step(bytes, index, string);
    }
    if bytes[index] == b'"' {
        *string = true;
        return index + 1;
    }
    if bytes.get(index..index + 2) == Some(b"//") {
        return blank_until_newline(bytes, index);
    }
    if bytes.get(index..index + 2) == Some(b"/*") {
        return blank_block(bytes, index);
    }
    index + 1
}

fn string_step(bytes: &mut [u8], index: usize, string: &mut bool) -> usize {
    if bytes[index] == b'\\' {
        return index + 2;
    }
    if bytes[index] == b'"' {
        *string = false;
    }
    index + 1
}

fn blank_until_newline(bytes: &mut [u8], mut index: usize) -> usize {
    while index < bytes.len() && bytes[index] != b'\n' {
        bytes[index] = b' ';
        index += 1;
    }
    index
}

fn blank_block(bytes: &mut [u8], index: usize) -> usize {
    bytes[index] = b' ';
    if index + 1 < bytes.len() {
        bytes[index + 1] = b' ';
    }
    let mut cursor = index + 2;
    while cursor + 1 < bytes.len() && bytes.get(cursor..cursor + 2) != Some(b"*/") {
        if !matches!(bytes[cursor], b'\n' | b'\r') {
            bytes[cursor] = b' ';
        }
        cursor += 1;
    }
    if cursor + 1 < bytes.len() {
        bytes[cursor] = b' ';
        bytes[cursor + 1] = b' ';
        cursor += 2;
    }
    cursor
}

fn blank_trailing_commas(bytes: &mut [u8]) {
    let mut index = 0;
    let mut string = false;
    while index < bytes.len() {
        index = comma_step(bytes, index, &mut string);
    }
}

fn comma_step(bytes: &mut [u8], index: usize, string: &mut bool) -> usize {
    if *string && bytes[index] == b'\\' {
        return index + 2;
    }
    if bytes[index] == b'"' {
        *string = !*string;
    }
    if !*string && bytes[index] == b',' && next_is_closer(bytes, index) {
        bytes[index] = b' ';
    }
    index + 1
}

fn next_is_closer(bytes: &[u8], index: usize) -> bool {
    let mut next = index + 1;
    while next < bytes.len() && bytes[next].is_ascii_whitespace() {
        next += 1;
    }
    bytes
        .get(next)
        .is_some_and(|byte| matches!(byte, b'}' | b']'))
}

fn json_lines(text: &str) -> Result<Value, serde_json::Error> {
    let values = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(serde_json::from_str)
        .collect::<Result<Vec<Value>, _>>()?;
    Ok(Value::Array(values))
}

fn table(title: String, records: &[Vec<String>]) -> Result<Reading, String> {
    let passages: Vec<String> = records
        .iter()
        .take(TABLE_ROWS)
        .map(|row| row.join(" · "))
        .filter(|row| !row.is_empty())
        .collect();
    if passages.is_empty() {
        return Err("This table has no rows to highlight.".into());
    }
    Ok(Reading {
        kind: MaterialKind::Table,
        title,
        rows: passage_rows(&passages),
        passages,
    })
}

fn passage_rows(passages: &[String]) -> Vec<Row> {
    passages
        .iter()
        .enumerate()
        .map(|(index, label)| Row {
            depth: 0,
            label: label.clone(),
            passage: Some(index),
        })
        .collect()
}

fn is_sqlite(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str(),
        "db" | "db3" | "sqlite" | "sqlite3" | "s3db"
    )
}

fn sqlite_header(path: &Path) -> bool {
    let mut header = [0; 16];
    fs::File::open(path)
        .and_then(|mut file| std::io::Read::read_exact(&mut file, &mut header))
        .is_ok()
        && &header == b"SQLite format 3\0"
}

fn database(path: &Path, title: String) -> Result<Reading, String> {
    let scratch = scratch_dir(path);
    let _ = fs::remove_dir_all(&scratch);
    if fs::create_dir_all(&scratch).is_err() {
        return Err("This database could not be opened.".into());
    }
    let copy = scratch.join("snapshot.sqlite");
    let read = read_snapshot(path, &copy, &scratch, title);
    let _ = fs::remove_dir_all(&scratch);
    read
}

fn scratch_dir(path: &Path) -> PathBuf {
    std::env::temp_dir().join(format!(
        "frame-crm-db-{}-{}",
        std::process::id(),
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("db")
    ))
}

fn read_snapshot(
    path: &Path,
    copy: &Path,
    scratch: &Path,
    title: String,
) -> Result<Reading, String> {
    if fs::copy(path, copy).is_err() {
        return Err("This database could not be opened.".into());
    }
    copy_sidecar(path, scratch, "-wal");
    copy_sidecar(path, scratch, "-journal");
    let connection = match Connection::open_with_flags(copy, OpenFlags::SQLITE_OPEN_READ_ONLY) {
        Ok(connection) => connection,
        Err(error) => return Err(error.to_string()),
    };
    if let Err(error) = connection.execute_batch("PRAGMA query_only=ON;") {
        return Err(error.to_string());
    }
    let names = schema_names(&connection)?;
    tables_reading(&connection, &names, title)
}

fn schema_names(connection: &Connection) -> Result<Vec<String>, String> {
    let mut statement = connection
        .prepare(
            "SELECT name FROM sqlite_schema WHERE type IN ('table','view') AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], |row| row.get(0))
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

fn tables_reading(
    connection: &Connection,
    names: &[String],
    title: String,
) -> Result<Reading, String> {
    let mut passages = Vec::new();
    let mut rows = Vec::new();
    let mut tables = 0usize;
    for name in names {
        if tables == DATABASE_TABLES {
            rows.push(Row {
                depth: 0,
                label: "Further tables stay in the file.".into(),
                passage: None,
            });
            break;
        }
        if append_table(connection, name, &mut passages, &mut rows)? {
            tables += 1;
        }
    }
    if passages.is_empty() {
        return Err("This database has no rows to highlight.".into());
    }
    Ok(Reading {
        kind: MaterialKind::Database,
        title,
        passages,
        rows,
    })
}

fn append_table(
    connection: &Connection,
    name: &str,
    passages: &mut Vec<String>,
    rows: &mut Vec<Row>,
) -> Result<bool, String> {
    let columns = table_columns(connection, name)?;
    if columns.is_empty() {
        return Ok(false);
    }
    let records = table_records(connection, name, &columns)?;
    if records.is_empty() {
        return Ok(false);
    }
    rows.push(Row {
        depth: 0,
        label: name.to_owned(),
        passage: None,
    });
    for record in records {
        let passage = passages.len();
        let label = format!("{name}: {}", record.join(" · "));
        passages.push(label.clone());
        rows.push(Row {
            depth: 1,
            label,
            passage: Some(passage),
        });
    }
    Ok(true)
}

fn table_records(
    connection: &Connection,
    name: &str,
    columns: &[String],
) -> Result<Vec<Vec<String>>, String> {
    let projection = columns
        .iter()
        .map(|column| format!("ifnull(CAST({} AS TEXT), 'NULL')", quote(column)))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT {projection} FROM {} LIMIT {TABLE_ROWS}",
        quote(name)
    );
    let mut statement = connection
        .prepare(&sql)
        .map_err(|error| error.to_string())?;
    let width = statement.column_count();
    let rows = statement
        .query_map([], |row| row_cells(row, width))
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

fn row_cells(row: &rusqlite::Row, width: usize) -> rusqlite::Result<Vec<String>> {
    let mut cells = Vec::with_capacity(width);
    for index in 0..width {
        cells.push(row.get(index)?);
    }
    Ok(cells)
}

fn copy_sidecar(source: &Path, scratch: &Path, suffix: &str) {
    let mut name = source.as_os_str().to_owned();
    name.push(suffix);
    let sidecar = PathBuf::from(name);
    if sidecar.is_file() {
        let _ = fs::copy(&sidecar, scratch.join(format!("snapshot.sqlite{suffix}")));
    }
}

fn table_columns(connection: &Connection, table: &str) -> Result<Vec<String>, String> {
    connection
        .prepare("SELECT name FROM pragma_table_info(?) ORDER BY cid")
        .map_err(|error| error.to_string())?
        .query_map([table], |row| row.get(0))
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<String>, _>>()
        .map_err(|error| error.to_string())
}

fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn text_reading(title: String, body: &str) -> Result<Reading, String> {
    let passages = text_passages(body);
    if passages.is_empty() {
        return Err("This file has no text to highlight.".into());
    }
    Ok(Reading {
        kind: MaterialKind::Text,
        title,
        rows: passage_rows(&passages),
        passages,
    })
}

fn text_passages(body: &str) -> Vec<String> {
    let normalized = body.replace("\r\n", "\n");
    if normalized.contains("\n\n") {
        return normalized
            .split("\n\n")
            .map(|block| block.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|block| !block.is_empty())
            .collect();
    }
    normalized
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "frame-crm-material-{}-{}",
            name,
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_json_object_becomes_one_passage_per_field() {
        let dir = scratch("json");
        let path = dir.join("acme.json");
        fs::write(
            &path,
            r#"{"buyer":"Ada Chen","plan":{"when":"Q2"},"seats":3,"ready":true,"note":null}"#,
        )
        .unwrap();
        let reading = open(&path).unwrap();
        assert_eq!(reading.kind, MaterialKind::Json);
        assert_eq!(
            reading.passages,
            vec![
                "$.buyer: Ada Chen".to_owned(),
                "$.plan.when: Q2".to_owned(),
                "$.seats: 3".to_owned(),
                "$.ready: true".to_owned(),
                "$.note: null".to_owned(),
            ]
        );
        assert_eq!(open(&path).unwrap().passages, reading.passages);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_jsonc_comment_and_a_trailing_comma_still_parse() {
        let dir = scratch("jsonc");
        let path = dir.join("acme.jsonc");
        fs::write(
            &path,
            "{\n  // buyer\n  \"buyer\": \"Ada Chen\", /* keep */\n  \"url\": \"http://x\",\n}\n",
        )
        .unwrap();
        let reading = open(&path).unwrap();
        assert_eq!(reading.passages[0], "$.buyer: Ada Chen");
        assert!(
            reading
                .passages
                .iter()
                .any(|line| line.contains("http://x"))
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_csv_row_becomes_a_passage() {
        let dir = scratch("csv");
        let path = dir.join("people.csv");
        fs::write(&path, "name,role\nAda Chen,Buyer\n").unwrap();
        let reading = open(&path).unwrap();
        assert_eq!(reading.kind, MaterialKind::Table);
        assert_eq!(reading.passages[1], "Ada Chen · Buyer");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_sqlite_row_becomes_a_passage() {
        let dir = scratch("db");
        let path = dir.join("acme.sqlite");
        let status = std::process::Command::new("python3")
            .arg("-c")
            .arg("import sqlite3,sys; c=sqlite3.connect(sys.argv[1]); c.execute('create table people(name)'); c.execute('insert into people values (?)', ('Ada Chen',)); c.commit()")
            .arg(&path)
            .status()
            .unwrap();
        assert!(status.success());
        let reading = open(&path).unwrap();
        assert_eq!(reading.kind, MaterialKind::Database);
        assert!(reading.passages.iter().any(|row| row.contains("Ada Chen")));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_that_is_not_a_database_is_refused() {
        let dir = scratch("bad-db");
        let path = dir.join("notes.sqlite");
        fs::write(&path, "not a database").unwrap();
        let error = open(&path).unwrap_err();
        assert!(error.contains("could not be opened"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_pdf_is_refused() {
        let dir = scratch("pdf");
        let path = dir.join("brief.pdf");
        fs::write(&path, "not a pdf").unwrap();
        let error = open(&path).unwrap_err();
        assert!(error.contains("JSON object"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn blank_lines_split_text_into_passages() {
        let dir = scratch("text");
        let path = dir.join("notes.txt");
        fs::write(&path, "Ada walked the floor.\n\nWe need a pilot in Q2.\n").unwrap();
        let reading = open(&path).unwrap();
        assert_eq!(reading.kind, MaterialKind::Text);
        assert_eq!(reading.passages.len(), 2);
        assert_eq!(reading.passages[1], "We need a pilot in Q2.");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_json_line_and_an_array_item_become_passages() {
        let dir = scratch("lines-json");
        let lines = dir.join("acme.jsonl");
        fs::write(&lines, "{\"buyer\":\"Ada Chen\"}\n{\"when\":\"Q2\"}\n").unwrap();
        let reading = open(&lines).unwrap();
        assert!(
            reading
                .passages
                .iter()
                .any(|line| line.contains("Ada Chen"))
        );
        let items = dir.join("items.json");
        fs::write(&items, "{\"items\":[\"Ada\",\"Q2\"]}").unwrap();
        let reading = open(&items).unwrap();
        assert!(reading.passages.iter().any(|line| line.contains("[0]")));
        assert!(reading.passages.iter().any(|line| line.contains("Q2")));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_of_single_lines_keeps_each_line() {
        let dir = scratch("lines");
        let path = dir.join("notes.txt");
        fs::write(&path, "Ada walked the floor.\nWe need a pilot in Q2.\n").unwrap();
        let reading = open(&path).unwrap();
        assert_eq!(reading.passages.len(), 2);
        assert_eq!(reading.passages[0], "Ada walked the floor.");
        let _ = fs::remove_dir_all(&dir);
    }
}
