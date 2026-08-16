use crate::record::Record;
use crate::value::Value;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

/// Declared type of a model field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataType {
    String,
    Integer,
    Float,
    Boolean,
    Date,
}

impl DataType {
    pub fn name(&self) -> &'static str {
        match self {
            DataType::String => "string",
            DataType::Integer => "integer",
            DataType::Float => "float",
            DataType::Boolean => "boolean",
            DataType::Date => "date",
        }
    }

    /// Parse a raw text cell (e.g. from CSV) into a typed value.
    /// Empty text is treated as null.
    pub fn parse_str(&self, raw: &str) -> Result<Value, String> {
        if raw.is_empty() {
            return Ok(Value::Null);
        }
        match self {
            DataType::String => Ok(Value::String(raw.to_string())),
            DataType::Integer => raw
                .parse::<i64>()
                .map(Value::Integer)
                .map_err(|_| format!("'{raw}' is not a valid integer")),
            DataType::Float => raw
                .parse::<f64>()
                .map(Value::Float)
                .map_err(|_| format!("'{raw}' is not a valid float")),
            DataType::Boolean => match raw.to_ascii_lowercase().as_str() {
                "true" | "1" | "yes" => Ok(Value::Bool(true)),
                "false" | "0" | "no" => Ok(Value::Bool(false)),
                _ => Err(format!("'{raw}' is not a valid boolean")),
            },
            DataType::Date => NaiveDate::parse_from_str(raw, "%Y-%m-%d")
                .map(Value::Date)
                .map_err(|_| format!("'{raw}' is not a valid ISO date (YYYY-MM-DD)")),
        }
    }

    /// Coerce a JSON value into this type.
    pub fn coerce_json(&self, v: &serde_json::Value) -> Result<Value, String> {
        use serde_json::Value as J;
        match (self, v) {
            (_, J::Null) => Ok(Value::Null),
            (DataType::String, J::String(s)) => Ok(Value::String(s.clone())),
            (DataType::Integer, J::Number(n)) if n.is_i64() => {
                Ok(Value::Integer(n.as_i64().unwrap()))
            }
            (DataType::Float, J::Number(n)) => Ok(Value::Float(n.as_f64().unwrap())),
            (DataType::Boolean, J::Bool(b)) => Ok(Value::Bool(*b)),
            // Dates and stringly-typed cells arrive as JSON strings.
            (_, J::String(s)) => self.parse_str(s),
            _ => Err(format!("JSON value {v} is not a valid {}", self.name())),
        }
    }

    /// Does an in-memory value satisfy this declared type?
    pub fn check(&self, v: &Value) -> bool {
        matches!(
            (self, v),
            (_, Value::Null)
                | (DataType::String, Value::String(_))
                | (DataType::Integer, Value::Integer(_))
                | (DataType::Float, Value::Float(_))
                | (DataType::Boolean, Value::Bool(_))
                | (DataType::Date, Value::Date(_))
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Field {
    pub name: String,
    #[serde(rename = "type")]
    pub dtype: DataType,
    #[serde(default)]
    pub required: bool,
}

/// A data model: an ordered set of typed fields. Both the left (source) and
/// right (target) side of a mapping are described by one of these.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Model {
    pub name: String,
    pub fields: Vec<Field>,
}

impl Model {
    pub fn field(&self, name: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.name == name)
    }

    /// Validate a record against this model. Returns one message per problem;
    /// empty means the record conforms. Fields not declared in the model are
    /// ignored (they simply won't be mapped).
    pub fn validate(&self, record: &Record) -> Vec<String> {
        let mut errors = Vec::new();
        for field in &self.fields {
            match record.get(&field.name) {
                None | Some(Value::Null) => {
                    if field.required {
                        errors.push(format!("required field '{}' is missing", field.name));
                    }
                }
                Some(v) => {
                    if !field.dtype.check(v) {
                        errors.push(format!(
                            "field '{}' expected {} but got {} ({v})",
                            field.name,
                            field.dtype.name(),
                            v.type_name()
                        ));
                    }
                }
            }
        }
        errors
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::Record;

    fn model() -> Model {
        serde_yaml::from_str(
            r#"
            name: t
            fields:
              - { name: id, type: integer, required: true }
              - { name: note, type: string }
            "#,
        )
        .unwrap()
    }

    #[test]
    fn validate_passes_conforming_record() {
        let mut r = Record::new();
        r.insert("id".into(), Value::Integer(1));
        assert!(model().validate(&r).is_empty());
    }

    #[test]
    fn validate_flags_missing_required_and_bad_type() {
        let mut r = Record::new();
        r.insert("note".into(), Value::Integer(7));
        let errors = model().validate(&r);
        assert_eq!(errors.len(), 2);
        assert!(errors[0].contains("required field 'id'"));
        assert!(errors[1].contains("expected string"));
    }

    #[test]
    fn parse_str_empty_is_null() {
        assert_eq!(DataType::Integer.parse_str("").unwrap(), Value::Null);
    }
}
