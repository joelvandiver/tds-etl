use crate::model::DataType;
use crate::value::Value;
use chrono::NaiveDate;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// A transform is pure data, deserialized from the mapping file. Each target
/// field declares a chain of these, applied left to right to the list of
/// values pulled from the source fields.
///
/// Scalar transforms (trim, cast, ...) apply element-wise and pass null
/// through untouched; combining transforms (concat, coalesce, constant)
/// reduce the list to a single value.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Transform {
    Trim,
    Uppercase,
    Lowercase,
    Replace {
        from: String,
        to: String,
    },
    Split {
        separator: String,
        index: usize,
    },
    Concat {
        #[serde(default)]
        separator: String,
    },
    Coalesce,
    Constant {
        value: Value,
    },
    Default {
        value: Value,
    },
    Lookup {
        table: IndexMap<String, Value>,
        #[serde(default)]
        default: Option<Value>,
    },
    Cast {
        to: DataType,
    },
    ParseDate {
        format: String,
    },
    FormatDate {
        format: String,
    },
    Multiply {
        by: f64,
    },
    Round {
        #[serde(default)]
        decimals: u32,
    },
}

/// Apply a chain of transforms to the values pulled from the source fields.
pub fn apply_chain(transforms: &[Transform], mut values: Vec<Value>) -> Result<Vec<Value>, String> {
    for t in transforms {
        values = apply_one(t, values)?;
    }
    Ok(values)
}

fn apply_one(t: &Transform, values: Vec<Value>) -> Result<Vec<Value>, String> {
    match t {
        Transform::Concat { separator } => {
            let joined = values
                .iter()
                .filter(|v| !v.is_null())
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(separator);
            Ok(vec![Value::String(joined)])
        }
        Transform::Coalesce => Ok(vec![values
            .into_iter()
            .find(|v| !v.is_null())
            .unwrap_or(Value::Null)]),
        Transform::Constant { value } => Ok(vec![value.clone()]),
        _ => values.into_iter().map(|v| apply_scalar(t, v)).collect(),
    }
}

fn apply_scalar(t: &Transform, v: Value) -> Result<Value, String> {
    // Null passes through every scalar transform except `default`.
    if v.is_null() {
        return Ok(match t {
            Transform::Default { value } => value.clone(),
            _ => Value::Null,
        });
    }
    match t {
        Transform::Trim => on_string(t, v, |s| Ok(Value::String(s.trim().to_string()))),
        Transform::Uppercase => on_string(t, v, |s| Ok(Value::String(s.to_uppercase()))),
        Transform::Lowercase => on_string(t, v, |s| Ok(Value::String(s.to_lowercase()))),
        Transform::Replace { from, to } => {
            on_string(t, v, |s| Ok(Value::String(s.replace(from, to))))
        }
        Transform::Split { separator, index } => on_string(t, v, |s| {
            s.split(separator.as_str())
                .nth(*index)
                .map(|part| Value::String(part.to_string()))
                .ok_or_else(|| format!("split index {index} out of range for '{s}'"))
        }),
        Transform::Default { .. } => Ok(v),
        Transform::Lookup { table, default } => on_string(t, v, |s| match table.get(s) {
            Some(mapped) => Ok(mapped.clone()),
            None => match default {
                Some(d) => Ok(d.clone()),
                None => Err(format!("lookup: no entry for '{s}' and no default given")),
            },
        }),
        Transform::Cast { to } => cast(v, *to),
        Transform::ParseDate { format } => on_string(t, v, |s| {
            NaiveDate::parse_from_str(s, format)
                .map(Value::Date)
                .map_err(|e| format!("parse_date: '{s}' does not match '{format}': {e}"))
        }),
        Transform::FormatDate { format } => match v {
            Value::Date(d) => Ok(Value::String(d.format(format).to_string())),
            other => Err(type_error(t, &other)),
        },
        Transform::Multiply { by } => match v {
            Value::Integer(i) if by.fract() == 0.0 => Ok(Value::Integer(i * *by as i64)),
            Value::Integer(i) => Ok(Value::Float(i as f64 * by)),
            Value::Float(f) => Ok(Value::Float(f * by)),
            other => Err(type_error(t, &other)),
        },
        Transform::Round { decimals } => match v {
            Value::Float(f) => {
                let factor = 10f64.powi(*decimals as i32);
                Ok(Value::Float((f * factor).round() / factor))
            }
            Value::Integer(i) => Ok(Value::Integer(i)),
            other => Err(type_error(t, &other)),
        },
        // Combining transforms are handled in apply_one.
        Transform::Concat { .. } | Transform::Coalesce | Transform::Constant { .. } => {
            unreachable!()
        }
    }
}

fn cast(v: Value, to: DataType) -> Result<Value, String> {
    match (v, to) {
        (v, DataType::String) => Ok(Value::String(v.to_string())),
        (Value::String(s), to) => to.parse_str(&s),
        (Value::Integer(i), DataType::Integer) => Ok(Value::Integer(i)),
        (Value::Integer(i), DataType::Float) => Ok(Value::Float(i as f64)),
        (Value::Float(f), DataType::Integer) if f.fract() == 0.0 => Ok(Value::Integer(f as i64)),
        (Value::Float(f), DataType::Integer) => Err(format!(
            "cast: {f} has a fractional part; round before casting to integer"
        )),
        (Value::Float(f), DataType::Float) => Ok(Value::Float(f)),
        (Value::Bool(b), DataType::Integer) => Ok(Value::Integer(b as i64)),
        (Value::Bool(b), DataType::Boolean) => Ok(Value::Bool(b)),
        (Value::Date(d), DataType::Date) => Ok(Value::Date(d)),
        (v, to) => Err(format!("cast: cannot cast {} to {}", v.type_name(), to.name())),
    }
}

fn on_string(
    t: &Transform,
    v: Value,
    f: impl FnOnce(&str) -> Result<Value, String>,
) -> Result<Value, String> {
    match v {
        Value::String(s) => f(&s),
        other => Err(type_error(t, &other)),
    }
}

fn type_error(t: &Transform, v: &Value) -> String {
    format!("{}: cannot apply to {} value ({v})", t.name(), v.type_name())
}

impl Transform {
    pub fn name(&self) -> &'static str {
        match self {
            Transform::Trim => "trim",
            Transform::Uppercase => "uppercase",
            Transform::Lowercase => "lowercase",
            Transform::Replace { .. } => "replace",
            Transform::Split { .. } => "split",
            Transform::Concat { .. } => "concat",
            Transform::Coalesce => "coalesce",
            Transform::Constant { .. } => "constant",
            Transform::Default { .. } => "default",
            Transform::Lookup { .. } => "lookup",
            Transform::Cast { .. } => "cast",
            Transform::ParseDate { .. } => "parse_date",
            Transform::FormatDate { .. } => "format_date",
            Transform::Multiply { .. } => "multiply",
            Transform::Round { .. } => "round",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chain(yaml: &str) -> Vec<Transform> {
        crate::pipeline::from_yaml_str(yaml).unwrap()
    }

    #[test]
    fn trim_and_concat() {
        let t = chain("[trim, {concat: {separator: ' '}}]");
        let out = apply_chain(
            &t,
            vec![
                Value::String("  Ada ".into()),
                Value::String("Lovelace".into()),
            ],
        )
        .unwrap();
        assert_eq!(out, vec![Value::String("Ada Lovelace".into())]);
    }

    #[test]
    fn parse_date_passes_null_through() {
        let t = chain("[{parse_date: {format: '%m/%d/%Y'}}]");
        assert_eq!(apply_chain(&t, vec![Value::Null]).unwrap(), vec![Value::Null]);
        let out = apply_chain(&t, vec![Value::String("12/10/1815".into())]).unwrap();
        assert_eq!(
            out,
            vec![Value::Date(NaiveDate::from_ymd_opt(1815, 12, 10).unwrap())]
        );
    }

    #[test]
    fn lookup_with_and_without_default() {
        let t = chain("[{lookup: {table: {active: true, inactive: false}}}]");
        let out = apply_chain(&t, vec![Value::String("active".into())]).unwrap();
        assert_eq!(out, vec![Value::Bool(true)]);
        let err = apply_chain(&t, vec![Value::String("closed".into())]).unwrap_err();
        assert!(err.contains("no entry for 'closed'"));
    }

    #[test]
    fn multiply_round_cast_to_integer() {
        let t = chain("[{multiply: {by: 100}}, {round: {}}, {cast: {to: integer}}]");
        let out = apply_chain(&t, vec![Value::Float(99.95)]).unwrap();
        assert_eq!(out, vec![Value::Integer(9995)]);
    }

    #[test]
    fn cast_fractional_float_to_integer_fails() {
        let t = chain("[{cast: {to: integer}}]");
        let err = apply_chain(&t, vec![Value::Float(1.5)]).unwrap_err();
        assert!(err.contains("round before casting"));
    }
}
