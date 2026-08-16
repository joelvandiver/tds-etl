# How the platform works

The prototype is a **metadata-driven ETL worker**. The engine (Rust) contains
no pipeline-specific logic. Everything that describes a particular pipeline —
what the source looks like, what the target looks like, and how one becomes
the other — lives in data files that the engine interprets at runtime.

## The core idea: three data artifacts

Every pipeline is described by three declarative artifacts plus a spec that
ties them together:

```
            left model                map                 right model
          (customer_raw)     (customer_raw -> customer)    (customer)
               |                       |                      |
   input  ─────┴──> [extract] ─> [validate_left] ─> [transform] ─> [validate_right] ──> output
                                                                          |
                                                                       rejects
```

- The **left model** declares the shape of the raw source: field names, types,
  and which fields are required.
- The **right model** declares the shape the target expects, in the same
  format. Left and right are the *same kind of thing* — a `Model` — used on
  opposite sides of the map.
- The **mapping** declares, for each target field, which left-model field(s)
  feed it and a chain of transforms that shape the value. Transforms are data
  too: an enum deserialized from the mapping file, never code written per
  pipeline.
- The **pipeline spec** binds the three artifacts to concrete input/output
  endpoints (CSV/JSON/JSONL files) and an optional rejects file.

Because all four are data, a new pipeline is authored by writing YAML (or
JSON), not by recompiling the engine. See [spec-reference.md](spec-reference.md)
for the exact file formats and [transforms.md](transforms.md) for every
transform.

## Life of a row

`etl run pipeline.yaml` loads the spec, statically validates the mapping
against both models (before touching any data), then streams every input row
through four stages. A failure at any stage **rejects the row** — recording
the row number, the stage, the error messages, and a snapshot of the record —
and the run continues. A run never aborts because of one bad row.

1. **`extract`** — the row is read and each cell is coerced to the type the
   left model declares for that column. CSV cells are text, so `balance:
   float` means the string `"1250.50"` becomes the float `1250.5` here — and
   `"not-a-number"` rejects the row at this stage. Columns the left model
   does not declare are carried along as strings (they simply won't be
   mapped). Empty cells become null.

2. **`validate_left`** — the typed record is checked against the left model:
   every `required: true` field must be present and non-null. (Type
   correctness is largely guaranteed by extract; this stage is what catches
   missing data.)

3. **`transform`** — for each target field in the mapping, the engine pulls
   the declared source values out of the record (missing fields become null)
   and runs the transform chain left to right. Each failing target field
   contributes one error message; if any fail, the row is rejected with all
   of them.

4. **`validate_right`** — the mapped record must conform to the right model:
   required fields present, every value matching its declared type. This is
   the engine's guarantee to the consumer of the output: nothing that reaches
   the output file violates the right model.

Rows that survive all four stages are written to the output endpoint. Output
column order and selection follow the right model, so the output shape is
also governed by data.

## How transforms execute

A transform chain operates on a *list* of values — one per declared source
field. Two kinds of transform work on that list differently:

- **Scalar transforms** (`trim`, `cast`, `parse_date`, …) apply element-wise
  to each value in the list. They pass null through untouched (except
  `default`, whose whole job is replacing null), so optional source fields
  flow through chains without special-casing.
- **Combining transforms** (`concat`, `coalesce`, `constant`) reduce the list
  to a single value.

At the end of the chain the list must contain exactly one value (or zero, in
which case the target is null). If a map declares two sources and never
combines them, that is a per-row transform error telling the author to add
`concat` or `coalesce` — the engine never guesses.

## Design decisions

**Strict typing, explicit conversions.** A value only changes type when the
mapping says so. `cast: { to: integer }` on a float with a fractional part is
an error until the chain rounds first; a string is never silently treated as
a number. The consequence: a mapping file is a complete, honest record of
what happens to the data — exactly what you want the reviewable artifact in
an ETL platform to be.

**Validation is layered, and static checks come first.** `etl validate` (and
the first step of every run) proves the mapping is consistent with both
models — no unknown source/target fields, no duplicate targets, every
required right field mapped — without reading a single row. Data-dependent
problems then surface per row, per stage, with the stage name in the reject.

**Rejects are data too.** The rejects file is JSON with the same vocabulary
the engine uses internally (`row`, `stage`, `errors`, `record`), so a
downstream process — or a human — can triage failures programmatically.

**Order is preserved everywhere.** Records are ordered maps
(`IndexMap`), model fields are ordered lists, and output columns follow the
right model's declaration order. Deterministic output makes diffs meaningful.

## Engine anatomy

```
src/
  value.rs      # Value: the dynamic scalar (null/bool/integer/float/string/date)
  model.rs      # Model, Field, DataType; parse/coerce/check + record validation
  record.rs     # Record = IndexMap<String, Value> (insertion-ordered)
  transform.rs  # the Transform enum (pure data) and its interpreter
  mapping.rs    # FieldMap/Mapping: static validation + per-record apply
  io.rs         # endpoints: read (typed by left model) / write (shaped by right model)
  pipeline.rs   # spec loading, path resolution, the four-stage run loop
  main.rs       # CLI: `etl run`, `etl validate`
```

The dependency direction is strictly downward: `pipeline` orchestrates
`io` + `mapping` + `model`; `mapping` interprets `transform`; everything
speaks `Value`/`Record`. The crate builds as a library with a thin CLI binary
on top, so the worker can later be embedded in a service without touching the
core.

One implementation note: spec files are parsed by funneling both YAML and
JSON through `serde_json::Value` first (`load_data` in `src/pipeline.rs`).
This gives enums the same single-key-map shape in both formats — mappings
write `- concat: { separator: " " }` rather than serde_yaml's tag syntax
(`!concat`).

## Extending the engine

The intended growth path keeps the "capability in code, configuration in
data" split:

- **New transform** — add a variant to `Transform` in `src/transform.rs`,
  implement it in the interpreter (`apply_one` for combining, `apply_scalar`
  for element-wise), and add its name to `Transform::name()`. Mapping files
  can use it immediately; no format changes needed.
- **New data type** — extend `DataType` and `Value`, plus the
  parse/coerce/check methods in `src/model.rs`.
- **New endpoint** — extend `Format` (or generalize `Endpoint`) in
  `src/io.rs`; the pipeline loop is agnostic to where rows come from or go.
