# Spec reference

All spec files may be written in YAML or JSON; a `.json` extension selects
the JSON parser, anything else is parsed as YAML. Both are deserialized into
the same structures, and enums (transforms, formats, types) use the same
single-key-map / plain-string shape in both formats.

## Pipeline spec

The entry point for `etl run` and `etl validate`. Relative paths are resolved
**against the directory the spec file lives in**, so a pipeline directory is
relocatable.

```yaml
left_model: left.yaml            # path to the left (source) model
right_model: right.yaml          # path to the right (target) model
mapping: map.yaml                # path to the mapping
input:  { format: csv,  path: input.csv }
output: { format: json, path: ../out/customers.json }
rejects: ../out/customers.rejects.json   # optional
```

| Field | Required | Meaning |
|---|---|---|
| `left_model` | yes | Model file describing the raw input shape |
| `right_model` | yes | Model file describing the output shape |
| `mapping` | yes | Mapping file between the two models |
| `input` | yes | Endpoint: `format` + `path` |
| `output` | yes | Endpoint: `format` + `path` (parent directories are created) |
| `rejects` | no | JSON file that receives rejected rows with their errors |

## Models

A model is a named, ordered list of typed fields. The same format describes
both sides of a mapping.

```yaml
name: customer_raw
fields:
  - { name: first_name, type: string, required: true }
  - { name: dob,        type: string }
  - { name: balance,    type: float }
```

| Field property | Required | Meaning |
|---|---|---|
| `name` | yes | Field name (matches CSV header / JSON key) |
| `type` | yes | One of the data types below |
| `required` | no (default `false`) | Row is rejected if the field is missing or null |

Field order matters on the right model: it defines output column order.
Input fields *not* declared in the left model are carried through as strings
and ignored by validation; they are available to the mapping but won't be
type-coerced.

### Data types

| Type | In-memory value | Accepted on input |
|---|---|---|
| `string` | UTF-8 string | any text / JSON string |
| `integer` | 64-bit signed | text parseable as `i64`; JSON integer |
| `float` | 64-bit float | text parseable as `f64`; any JSON number |
| `boolean` | bool | `true/false`, `1/0`, `yes/no` (case-insensitive); JSON boolean |
| `date` | calendar date | ISO `YYYY-MM-DD` text (other formats: keep the field a `string` and use the `parse_date` transform) |

Empty CSV cells and JSON `null` become **null** regardless of declared type;
whether that rejects the row depends on `required`.

## Mapping

```yaml
left: customer_raw     # must match the left model's `name`
right: customer        # must match the right model's `name`
fields:
  - target: full_name
    sources: [first_name, last_name]
    transforms:
      - trim
      - concat: { separator: " " }

  - target: birth_date
    source: dob                      # single-source shorthand
    transforms:
      - parse_date: { format: "%m/%d/%Y" }

  - target: source_system            # no source: value comes from a constant
    transforms:
      - constant: { value: legacy_crm }
```

| FieldMap property | Required | Meaning |
|---|---|---|
| `target` | yes | Right-model field to produce |
| `source` | no | Single left-model source field |
| `sources` | no | List of left-model source fields |
| `transforms` | no (default `[]`) | Chain applied left to right; see [transforms.md](transforms.md) |

Rules enforced by static validation (`etl validate`, and before every run):

- `left`/`right` must match the model names they are paired with.
- Each `target` must exist in the right model, and appear at most once.
- Each source must exist in the left model.
- `source` and `sources` are mutually exclusive on one field map.
- Every `required` right-model field must have a mapping.

At run time, a field map with no combining transform must end with at most
one value; mapping two sources without `concat`/`coalesce` is a per-row
error. With no sources and no transforms, the target is null.

## Endpoints and formats

| Format | Input behavior | Output behavior |
|---|---|---|
| `csv` | First row is the header; cells coerced per the left model; empty cell → null | Header + one row per record; columns in right-model order; null → empty cell |
| `json` | Top-level array of objects | Pretty-printed array of objects; dates as ISO strings |
| `jsonl` | One JSON object per line (blank lines skipped) | One compact JSON object per line |

## Rejects file

A JSON array; one entry per rejected row:

```json
{
  "row": 4,
  "stage": "validate_left",
  "errors": ["required field 'first_name' is missing"],
  "record": { "first_name": null, "last_name": "Unknown", "...": "..." }
}
```

`row` is the 1-based data row number (excluding the CSV header). `stage` is
one of `extract`, `validate_left`, `transform`, `validate_right` — see the
[row lifecycle](how-it-works.md#life-of-a-row). `record` is the row as it
looked entering the failing stage (typed input for the first three stages,
mapped output for `validate_right`); it is omitted when the row could not be
read at all.

## CLI

```sh
etl validate <spec>   # static checks only; exit 1 if the mapping is inconsistent
etl run <spec>        # full run; prints a summary and per-row reject reasons
```

`etl run` exits successfully as long as the run itself completes — rejected
rows are reported (and written to the rejects file), not treated as a process
failure. Hard errors (unreadable spec, missing input file, inconsistent
mapping) exit non-zero.
