# Transform catalog

A transform chain runs left to right over the **list of values** pulled from
a field map's source fields. Two kinds of transform:

- **Scalar** transforms apply element-wise to each value in the list and
  pass null through untouched (`default` is the one exception — replacing
  null is its purpose).
- **Combining** transforms reduce the whole list to a single value.

A chain must end with at most one value; use a combining transform whenever
more than one source is declared.

Syntax: transforms without arguments are written as a bare name (`- trim`);
transforms with arguments as a single-key map (`- concat: { separator: " " }`).

## String transforms (scalar)

Input must be a string (or null); any other type is an error.

| Transform | Arguments | Behavior |
|---|---|---|
| `trim` | — | Strip leading/trailing whitespace |
| `uppercase` | — | Convert to upper case |
| `lowercase` | — | Convert to lower case |
| `replace` | `from`, `to` | Replace every occurrence of `from` with `to` |
| `split` | `separator`, `index` | Split on `separator`, keep the 0-based `index`-th part; out-of-range is an error |

```yaml
- split: { separator: "@", index: 1 }   # "a@b.com" -> "b.com"
```

## Combining transforms

| Transform | Arguments | Behavior |
|---|---|---|
| `concat` | `separator` (default `""`) | Join all non-null values' text with the separator → one string |
| `coalesce` | — | First non-null value, or null if all are null |
| `constant` | `value` | Discard inputs, produce the literal `value` |

```yaml
- target: full_name
  sources: [first_name, last_name]
  transforms:
    - trim
    - concat: { separator: " " }
```

`constant` is how a target field with no source gets its value (e.g. tagging
every row with a source-system name).

## Null handling

| Transform | Arguments | Behavior |
|---|---|---|
| `default` | `value` | Scalar: replace null with the literal `value`; non-null passes through |

`default` and `coalesce` are the only ways a null becomes something else
mid-chain; every other scalar transform skips nulls.

## Type conversion (scalar)

| Transform | Arguments | Behavior |
|---|---|---|
| `cast` | `to` (a data type) | Explicit type conversion; see the matrix below |
| `parse_date` | `format` | String → date using a [chrono format string](https://docs.rs/chrono/latest/chrono/format/strftime/) (e.g. `"%m/%d/%Y"`) |
| `format_date` | `format` | Date → string using a chrono format string |

`cast` conversion matrix:

| From \ To | `string` | `integer` | `float` | `boolean` | `date` |
|---|---|---|---|---|---|
| `string` | ✓ | parse | parse | parse (`true/false/1/0/yes/no`) | parse ISO |
| `integer` | ✓ | ✓ | ✓ | — | — |
| `float` | ✓ | only if whole (else error: round first) | ✓ | — | — |
| `boolean` | ✓ | `0`/`1` | — | ✓ | — |
| `date` | ISO string | — | — | — | ✓ |

The float→integer rule is deliberate: an implicit truncation would hide data
loss, so the chain must `round` first.

## Numeric transforms (scalar)

| Transform | Arguments | Behavior |
|---|---|---|
| `multiply` | `by` | Multiply; an integer stays an integer when `by` is whole, otherwise the result is a float |
| `round` | `decimals` (default `0`) | Round a float to `decimals` places (result stays a float — `cast` to integer if needed); integers pass through |

The canonical money pattern — dollars to integer cents:

```yaml
- target: balance_cents
  source: balance
  transforms:
    - multiply: { by: 100 }
    - round: {}
    - cast: { to: integer }
```

(`round` matters: `99.95 * 100` is `9994.999…` in floating point.)

## Value mapping (scalar)

| Transform | Arguments | Behavior |
|---|---|---|
| `lookup` | `table`, `default` (optional) | Map a string through a literal table; a key with no entry produces `default`, or an error if no default is given |

```yaml
- target: is_active
  source: status
  transforms:
    - trim
    - lowercase
    - lookup:
        table: { active: true, inactive: false }
        # default: false        # uncomment to accept unknown statuses
```

Omitting `default` is a feature: unknown codes reject the row instead of
being silently swallowed.

## Adding a transform

Add a variant to `Transform` in `src/transform.rs`, implement it in
`apply_scalar` (element-wise) or `apply_one` (combining), and add its name to
`Transform::name()`. Serde derives the mapping-file syntax from the variant
name (snake_case) and its fields — no format changes required. Add a unit
test alongside the existing ones in the same file.
