use crate::io::{self, Endpoint};
use crate::mapping::Mapping;
use crate::model::Model;
use crate::record;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// The top-level worker spec: everything a run needs, all of it data.
/// Relative paths are resolved against the directory the spec file lives in.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineSpec {
    pub left_model: PathBuf,
    pub right_model: PathBuf,
    pub mapping: PathBuf,
    pub input: Endpoint,
    pub output: Endpoint,
    /// Optional JSON file collecting rejected rows with their errors.
    #[serde(default)]
    pub rejects: Option<PathBuf>,
}

impl PipelineSpec {
    pub fn load(path: &Path) -> Result<Self> {
        let mut spec: PipelineSpec = load_data(path)?;
        let base = path.parent().unwrap_or(Path::new("."));
        for p in [&mut spec.left_model, &mut spec.right_model, &mut spec.mapping] {
            *p = resolve(base, p);
        }
        spec.input.path = resolve(base, &spec.input.path);
        spec.output.path = resolve(base, &spec.output.path);
        if let Some(r) = &spec.rejects {
            spec.rejects = Some(resolve(base, r));
        }
        Ok(spec)
    }
}

fn resolve(base: &Path, p: &Path) -> PathBuf {
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        base.join(p)
    }
}

/// Load a YAML or JSON spec file (models, mappings, pipeline specs).
///
/// Both formats are funneled through `serde_json::Value` so enums use the
/// same single-key-map shape everywhere (e.g. `- concat: {separator: " "}`),
/// instead of serde_yaml's YAML-tag representation.
pub fn load_data<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("cannot read {}", path.display()))?;
    let is_json = path.extension().is_some_and(|e| e == "json");
    let parsed = if is_json {
        serde_json::from_str::<serde_json::Value>(&text).map_err(anyhow::Error::from)
    } else {
        serde_yaml::from_str::<serde_json::Value>(&text).map_err(anyhow::Error::from)
    };
    parsed
        .and_then(|v| T::deserialize(v).map_err(anyhow::Error::from))
        .with_context(|| format!("cannot parse {}", path.display()))
}

/// Parse a YAML string the same way `load_data` parses files.
pub fn from_yaml_str<T: serde::de::DeserializeOwned>(text: &str) -> Result<T> {
    let value: serde_json::Value = serde_yaml::from_str(text)?;
    Ok(T::deserialize(value)?)
}

/// A fully loaded pipeline: spec plus the three data artifacts it references.
pub struct Pipeline {
    pub spec: PipelineSpec,
    pub left: Model,
    pub right: Model,
    pub mapping: Mapping,
}

impl Pipeline {
    pub fn load(spec_path: &Path) -> Result<Self> {
        let spec = PipelineSpec::load(spec_path)?;
        let left: Model = load_data(&spec.left_model)?;
        let right: Model = load_data(&spec.right_model)?;
        let mapping: Mapping = load_data(&spec.mapping)?;
        Ok(Pipeline { spec, left, right, mapping })
    }

    /// Static checks only — no data touched.
    pub fn validate(&self) -> Vec<String> {
        self.mapping.validate(&self.left, &self.right)
    }

    pub fn run(&self) -> Result<RunReport> {
        let static_errors = self.validate();
        if !static_errors.is_empty() {
            bail!(
                "mapping is inconsistent with the models:\n  - {}",
                static_errors.join("\n  - ")
            );
        }

        let rows = io::read(&self.spec.input, &self.left)?;
        let mut report = RunReport {
            rows_read: rows.len(),
            ..RunReport::default()
        };
        let mut output = Vec::new();

        for (i, row) in rows.into_iter().enumerate() {
            let row_number = i + 1;
            let rec = match row {
                Ok(rec) => rec,
                Err(errors) => {
                    report.reject(row_number, Stage::Extract, errors, None);
                    continue;
                }
            };
            let left_errors = self.left.validate(&rec);
            if !left_errors.is_empty() {
                report.reject(row_number, Stage::ValidateLeft, left_errors, Some(&rec));
                continue;
            }
            let mapped = match self.mapping.apply(&rec) {
                Ok(m) => m,
                Err(errors) => {
                    report.reject(row_number, Stage::Transform, errors, Some(&rec));
                    continue;
                }
            };
            let right_errors = self.right.validate(&mapped);
            if !right_errors.is_empty() {
                report.reject(row_number, Stage::ValidateRight, right_errors, Some(&mapped));
                continue;
            }
            output.push(mapped);
        }

        io::write(&self.spec.output, &output, &self.right)?;
        report.rows_written = output.len();

        if let Some(path) = &self.spec.rejects {
            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() {
                    fs::create_dir_all(parent)?;
                }
            }
            fs::write(path, serde_json::to_string_pretty(&report.rejects)?)?;
        }
        Ok(report)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Extract,
    ValidateLeft,
    Transform,
    ValidateRight,
}

#[derive(Debug, Serialize)]
pub struct Reject {
    pub row: usize,
    pub stage: Stage,
    pub errors: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record: Option<serde_json::Value>,
}

#[derive(Debug, Default)]
pub struct RunReport {
    pub rows_read: usize,
    pub rows_written: usize,
    pub rejects: Vec<Reject>,
}

impl RunReport {
    fn reject(
        &mut self,
        row: usize,
        stage: Stage,
        errors: Vec<String>,
        rec: Option<&record::Record>,
    ) {
        self.rejects.push(Reject {
            row,
            stage,
            errors,
            record: rec.map(record::to_json),
        });
    }
}
