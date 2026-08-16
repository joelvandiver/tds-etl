use std::path::Path;
use tds_etl::pipeline::{Pipeline, Stage};
use tds_etl::value::Value;

fn example(path: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
}

#[test]
fn customers_example_runs_end_to_end() {
    let pipeline = Pipeline::load(&example("examples/customers/pipeline.yaml")).unwrap();
    let report = pipeline.run().unwrap();

    assert_eq!(report.rows_read, 6);
    assert_eq!(report.rows_written, 3);
    assert_eq!(report.rejects.len(), 3);

    // Row 4: missing required first_name -> left validation.
    // Row 5: balance not a float -> extract (coercion against left model).
    // Row 6: status 'pending' not in lookup table -> transform.
    let stages: Vec<(usize, Stage)> = report.rejects.iter().map(|r| (r.row, r.stage)).collect();
    assert!(stages.contains(&(4, Stage::ValidateLeft)));
    assert!(stages.contains(&(5, Stage::Extract)));
    assert!(stages.contains(&(6, Stage::Transform)));

    let output: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(example("examples/out/customers.json")).unwrap(),
    )
    .unwrap();
    let rows = output.as_array().unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0]["full_name"], "Ada Lovelace");
    assert_eq!(rows[0]["birth_date"], "1815-12-10");
    assert_eq!(rows[0]["balance_cents"], 125050);
    assert_eq!(rows[0]["is_active"], true);
    assert_eq!(rows[1]["balance_cents"], 9995); // 99.95 * 100 rounded exactly
    assert_eq!(rows[2]["full_name"], "Alan Turing");
    assert_eq!(rows[2]["birth_date"], serde_json::Value::Null); // empty dob passes through
    assert_eq!(rows[2]["is_active"], false);
    for row in rows {
        assert_eq!(row["source_system"], "legacy_crm");
    }
}

#[test]
fn mapped_record_stays_typed_in_memory() {
    let pipeline = Pipeline::load(&example("examples/customers/pipeline.yaml")).unwrap();
    let mut rec = tds_etl::record::Record::new();
    rec.insert("first_name".into(), Value::String("Ada".into()));
    rec.insert("last_name".into(), Value::String("Lovelace".into()));
    rec.insert("balance".into(), Value::Float(1.5));
    rec.insert("status".into(), Value::String("active".into()));
    let out = pipeline.mapping.apply(&rec).unwrap();
    assert_eq!(out.get("balance_cents"), Some(&Value::Integer(150)));
    assert_eq!(out.get("is_active"), Some(&Value::Bool(true)));
    assert!(pipeline.right.validate(&out).is_empty());
}
