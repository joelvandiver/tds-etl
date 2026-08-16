use crate::model::Model;
use crate::record::Record;
use crate::transform::{apply_chain, Transform};
use crate::value::Value;
use serde::{Deserialize, Serialize};

/// How one target (right-model) field is produced: which left-model fields
/// feed it and which transform chain shapes the value. All data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldMap {
    pub target: String,
    /// Convenience for the common single-source case.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<String>,
    #[serde(default)]
    pub transforms: Vec<Transform>,
}

impl FieldMap {
    pub fn source_names(&self) -> Vec<&str> {
        match &self.source {
            Some(s) => vec![s.as_str()],
            None => self.sources.iter().map(String::as_str).collect(),
        }
    }
}

/// The map between the left and right models.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mapping {
    pub left: String,
    pub right: String,
    pub fields: Vec<FieldMap>,
}

impl Mapping {
    /// Static consistency checks against the two models. Run before any data
    /// is touched; returns one message per problem.
    pub fn validate(&self, left: &Model, right: &Model) -> Vec<String> {
        let mut errors = Vec::new();
        if self.left != left.name {
            errors.push(format!(
                "mapping declares left model '{}' but was given '{}'",
                self.left, left.name
            ));
        }
        if self.right != right.name {
            errors.push(format!(
                "mapping declares right model '{}' but was given '{}'",
                self.right, right.name
            ));
        }
        let mut seen = std::collections::HashSet::new();
        for fm in &self.fields {
            if !seen.insert(&fm.target) {
                errors.push(format!("target field '{}' is mapped more than once", fm.target));
            }
            if right.field(&fm.target).is_none() {
                errors.push(format!(
                    "target field '{}' does not exist in right model '{}'",
                    fm.target, right.name
                ));
            }
            if fm.source.is_some() && !fm.sources.is_empty() {
                errors.push(format!(
                    "target field '{}' declares both 'source' and 'sources'",
                    fm.target
                ));
            }
            for s in fm.source_names() {
                if left.field(s).is_none() {
                    errors.push(format!(
                        "source field '{s}' (for target '{}') does not exist in left model '{}'",
                        fm.target, left.name
                    ));
                }
            }
        }
        for field in &right.fields {
            if field.required && !self.fields.iter().any(|fm| fm.target == field.name) {
                errors.push(format!(
                    "required right field '{}' has no mapping",
                    field.name
                ));
            }
        }
        errors
    }

    /// Apply the mapping to one left-model record, producing a right-model
    /// record. Returns one message per failed target field.
    pub fn apply(&self, record: &Record) -> Result<Record, Vec<String>> {
        let mut out = Record::new();
        let mut errors = Vec::new();
        for fm in &self.fields {
            let inputs: Vec<Value> = fm
                .source_names()
                .iter()
                .map(|s| record.get(*s).cloned().unwrap_or(Value::Null))
                .collect();
            match apply_chain(&fm.transforms, inputs) {
                Ok(values) => match values.len() {
                    0 => {
                        out.insert(fm.target.clone(), Value::Null);
                    }
                    1 => {
                        out.insert(fm.target.clone(), values.into_iter().next().unwrap());
                    }
                    n => errors.push(format!(
                        "target '{}': {n} values remain after transforms; \
                         add a combining transform such as concat or coalesce",
                        fm.target
                    )),
                },
                Err(e) => errors.push(format!("target '{}': {e}", fm.target)),
            }
        }
        if errors.is_empty() {
            Ok(out)
        } else {
            Err(errors)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn models() -> (Model, Model) {
        let left = serde_yaml::from_str(
            "{name: l, fields: [{name: a, type: string}, {name: b, type: string}]}",
        )
        .unwrap();
        let right = serde_yaml::from_str(
            "{name: r, fields: [{name: ab, type: string, required: true}]}",
        )
        .unwrap();
        (left, right)
    }

    #[test]
    fn validate_catches_unknown_fields_and_unmapped_required() {
        let (left, right) = models();
        let mapping: Mapping = crate::pipeline::from_yaml_str(
            "{left: l, right: r, fields: [{target: nope, source: missing}]}",
        )
        .unwrap();
        let errors = mapping.validate(&left, &right);
        assert_eq!(errors.len(), 3);
    }

    #[test]
    fn apply_maps_two_sources_to_one_target() {
        let (_, _) = models();
        let mapping: Mapping = crate::pipeline::from_yaml_str(
            "{left: l, right: r, fields: [{target: ab, sources: [a, b], transforms: [{concat: {separator: '-'}}]}]}",
        )
        .unwrap();
        let mut rec = Record::new();
        rec.insert("a".into(), Value::String("x".into()));
        rec.insert("b".into(), Value::String("y".into()));
        let out = mapping.apply(&rec).unwrap();
        assert_eq!(out.get("ab"), Some(&Value::String("x-y".into())));
    }

    #[test]
    fn apply_reports_uncombined_sources() {
        let mapping: Mapping = crate::pipeline::from_yaml_str(
            "{left: l, right: r, fields: [{target: ab, sources: [a, b]}]}",
        )
        .unwrap();
        let mut rec = Record::new();
        rec.insert("a".into(), Value::String("x".into()));
        rec.insert("b".into(), Value::String("y".into()));
        let errors = mapping.apply(&rec).unwrap_err();
        assert!(errors[0].contains("combining transform"));
    }
}
