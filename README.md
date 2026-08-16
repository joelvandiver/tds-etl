# TDS ETL

This repository serves as the learning space for the technical design spec (TDS) for the ETL (extract-transform-load) platform.

## Prototype: metadata-driven ETL worker

A Rust CLI (`etl`) that processes a file according to an ETL data model where
**the left model, the right model, and the map between them are all data**.
The engine contains no pipeline-specific logic; it interprets four spec files:

| Artifact | Example | What it declares |
|---|---|---|
| Left model | `examples/customers/left.yaml` | Shape of the raw source (fields, types, required) |
| Right model | `examples/customers/right.yaml` | Shape of the target |
| Mapping | `examples/customers/map.yaml` | Per target field: source field(s) + a transform chain |
| Pipeline spec | `examples/customers/pipeline.yaml` | The three files above plus input/output/rejects endpoints |

Specs may be YAML or JSON. Input/output formats: `csv`, `json` (array of
objects), `jsonl`.

### Documentation

- [How the platform works](docs/how-it-works.md) — architecture, life of a row, design decisions, extension points
- [Spec reference](docs/spec-reference.md) — pipeline spec, models, mappings, data types, endpoints, rejects, CLI
- [Transform catalog](docs/transforms.md) — every transform with arguments, semantics, and examples

### Run it

```sh
cargo run -- validate examples/customers/pipeline.yaml   # static checks only
cargo run -- run examples/customers/pipeline.yaml        # full run
cargo test                                               # unit + end-to-end tests
```

### Row lifecycle

Every row passes through four stages, and a failure at any stage rejects the
row (with its errors and a snapshot) rather than aborting the run:

1. **extract** — read the row, coercing each cell to the left model's declared type
2. **validate_left** — required/type checks against the left model
3. **transform** — apply each target field's transform chain
4. **validate_right** — the mapped record must conform to the right model

Rejects are reported in the run summary and optionally written to a JSON file
(`rejects:` in the pipeline spec).

### Transforms are data

Each target field maps from zero or more source fields through a chain of
declarative transforms, applied left to right. Scalar transforms apply
element-wise and pass nulls through; combining transforms reduce multiple
source values to one.

```yaml
- target: full_name
  sources: [first_name, last_name]
  transforms:
    - trim
    - concat: { separator: " " }
```

Available: `trim`, `uppercase`, `lowercase`, `replace`, `split`, `concat`,
`coalesce`, `constant`, `default`, `lookup`, `cast`, `parse_date`,
`format_date`, `multiply`, `round`. Adding one means adding a variant to
`src/transform.rs` — the mapping format picks it up automatically.

Typing is strict by design: the mapping must make conversions explicit
(e.g. `cast: { to: integer }` fails on a fractional float until you `round`),
so the map file is a complete, honest record of what happens to the data.

### Crate layout

```
src/
  value.rs      # dynamic scalar Value (null/bool/integer/float/string/date)
  model.rs      # Model, Field, DataType + record validation
  record.rs     # Record = ordered map of field -> Value
  transform.rs  # the Transform enum (data) and its interpreter
  mapping.rs    # FieldMap/Mapping: static validation + per-record apply
  io.rs         # csv/json/jsonl endpoints, typed by the models
  pipeline.rs   # spec loading and the run loop (extract -> validate -> map -> validate -> load)
  main.rs       # CLI: `etl run`, `etl validate`
```
