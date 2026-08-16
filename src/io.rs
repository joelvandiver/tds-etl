use crate::model::Model;
use crate::record::{self, Record};
use crate::value::Value;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Csv,
    Json,
    Jsonl,
}

/// Where records come from or go to.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Endpoint {
    pub format: Format,
    pub path: PathBuf,
}

/// One input row: either a typed record, or the reasons it could not even be
/// read (e.g. a CSV cell that fails type coercion against the left model).
pub type RowResult = Result<Record, Vec<String>>;

/// Read all rows from an endpoint, coercing each cell to the type declared in
/// the (left) model. Undeclared columns are carried along as strings.
pub fn read(endpoint: &Endpoint, model: &Model) -> Result<Vec<RowResult>> {
    match endpoint.format {
        Format::Csv => read_csv(endpoint, model),
        Format::Json => read_json(endpoint, model),
        Format::Jsonl => read_jsonl(endpoint, model),
    }
}

fn read_csv(endpoint: &Endpoint, model: &Model) -> Result<Vec<RowResult>> {
    let mut reader = csv::Reader::from_path(&endpoint.path)
        .with_context(|| format!("cannot open input {}", endpoint.path.display()))?;
    let headers = reader.headers()?.clone();
    let mut rows = Vec::new();
    for row in reader.records() {
        let row = row?;
        let mut rec = Record::new();
        let mut errors = Vec::new();
        for (header, cell) in headers.iter().zip(row.iter()) {
            let value = match model.field(header) {
                Some(field) => match field.dtype.parse_str(cell) {
                    Ok(v) => v,
                    Err(e) => {
                        errors.push(format!("field '{header}': {e}"));
                        continue;
                    }
                },
                None if cell.is_empty() => Value::Null,
                None => Value::String(cell.to_string()),
            };
            rec.insert(header.to_string(), value);
        }
        rows.push(if errors.is_empty() { Ok(rec) } else { Err(errors) });
    }
    Ok(rows)
}

fn read_json(endpoint: &Endpoint, model: &Model) -> Result<Vec<RowResult>> {
    let text = fs::read_to_string(&endpoint.path)
        .with_context(|| format!("cannot open input {}", endpoint.path.display()))?;
    let parsed: serde_json::Value = serde_json::from_str(&text)?;
    let Some(items) = parsed.as_array() else {
        bail!("JSON input must be a top-level array of objects");
    };
    Ok(items.iter().map(|item| json_row(item, model)).collect())
}

fn read_jsonl(endpoint: &Endpoint, model: &Model) -> Result<Vec<RowResult>> {
    let text = fs::read_to_string(&endpoint.path)
        .with_context(|| format!("cannot open input {}", endpoint.path.display()))?;
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let parsed: serde_json::Value = serde_json::from_str(line)?;
            Ok(json_row(&parsed, model))
        })
        .collect()
}

fn json_row(item: &serde_json::Value, model: &Model) -> RowResult {
    let Some(obj) = item.as_object() else {
        return Err(vec![format!("expected a JSON object, got {item}")]);
    };
    let mut rec = Record::new();
    let mut errors = Vec::new();
    for (key, raw) in obj {
        let value = match model.field(key) {
            Some(field) => match field.dtype.coerce_json(raw) {
                Ok(v) => v,
                Err(e) => {
                    errors.push(format!("field '{key}': {e}"));
                    continue;
                }
            },
            None => match raw {
                serde_json::Value::Null => Value::Null,
                serde_json::Value::String(s) => Value::String(s.clone()),
                other => Value::String(other.to_string()),
            },
        };
        rec.insert(key.clone(), value);
    }
    if errors.is_empty() {
        Ok(rec)
    } else {
        Err(errors)
    }
}

/// Write records to an endpoint. Column order and selection follow the
/// (right) model, so output shape is governed by data, not by code.
pub fn write(endpoint: &Endpoint, records: &[Record], model: &Model) -> Result<()> {
    if let Some(parent) = endpoint.path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    match endpoint.format {
        Format::Csv => {
            let mut writer = csv::Writer::from_path(&endpoint.path)?;
            writer.write_record(model.fields.iter().map(|f| f.name.as_str()))?;
            for rec in records {
                writer.write_record(model.fields.iter().map(|f| {
                    rec.get(&f.name).map(|v| v.to_string()).unwrap_or_default()
                }))?;
            }
            writer.flush()?;
        }
        Format::Json => {
            let items: Vec<_> = records.iter().map(record::to_json).collect();
            fs::write(&endpoint.path, serde_json::to_string_pretty(&items)?)?;
        }
        Format::Jsonl => {
            let mut out = String::new();
            for rec in records {
                out.push_str(&serde_json::to_string(&record::to_json(rec))?);
                out.push('\n');
            }
            fs::write(&endpoint.path, out)?;
        }
    }
    Ok(())
}
