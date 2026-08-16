use crate::value::Value;
use indexmap::IndexMap;

/// A single row moving through the pipeline. Insertion order is preserved so
/// output column order is stable.
pub type Record = IndexMap<String, Value>;

pub fn to_json(record: &Record) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for (k, v) in record {
        map.insert(k.clone(), v.to_json());
    }
    serde_json::Value::Object(map)
}
