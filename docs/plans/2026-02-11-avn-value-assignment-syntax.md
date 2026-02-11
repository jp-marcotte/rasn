# AVN Value Assignment Syntax Support

**Branch:** `feat/avn-value-assignment-syntax`
**Base:** `fix-jer-null-optional`

## Problem

The AVN decoder (`src/avn/de.rs`) currently parses bare AVN values:

```
header : { major-version 2, minor-version 3, iccid '89001234...'H }
```

Real-world TCA profile files (TS.48 SAIP test profiles, ITU-T X.680 examples) use X.680 **value assignment** syntax with a name and type prefix:

```
value1 ProfileElement ::= header : {
  major-version 2,
  minor-version 3,
  iccid '89001234...'H
}
value2 ProfileElement ::= mf : {
  mf-header { mandated NULL, identification 4 },
  ...
}
```

Key differences from what the decoder handles today:
1. Each value has a `name Type ::=` prefix before the actual value
2. Multiple values are back-to-back (no blank-line separator)
3. The prefix is not part of the ASN.1 value — it's metadata the decoder should strip

## Scope

Update `rasn::avn::decode` to accept **both** formats:
- Bare values (current behavior, no change)
- Value assignments (`name Type ::= value` — strip prefix, decode value)

Add a new public function for multi-value decoding:
- `rasn::avn::decode_values` — splits concatenated value assignments, returns `Vec<T>`

The encoder (`src/avn/enc.rs`) is **not** changed — it continues to emit bare values.

## Implementation Plan

### 1. Update `Decoder::new()` to strip value assignment prefix

**File:** `src/avn/de.rs`, method `Decoder::new()`

Currently:
```rust
pub fn new(input: &str) -> Result<Self, DecodeError> {
    let (_, value) = Self::parse_to_value(input.trim()).map_err(|e| { ... })?;
    Ok(Self { stack: vec![value] })
}
```

Add a preprocessing step before `parse_to_value`:
```rust
pub fn new(input: &str) -> Result<Self, DecodeError> {
    let input = Self::strip_value_assignment(input.trim());
    let (_, value) = Self::parse_to_value(&input).map_err(|e| { ... })?;
    Ok(Self { stack: vec![value] })
}
```

New helper:
```rust
/// Strip an optional X.680 value assignment prefix: `name Type ::= `
/// Pattern: identifier, whitespace, Type (uppercase start), whitespace, "::=", whitespace
/// If the pattern matches, return everything after "::=" (trimmed).
/// If not, return the input unchanged.
fn strip_value_assignment(input: &str) -> &str
```

The pattern to match:
- `[a-z][a-zA-Z0-9-_]*` — value name (starts lowercase per X.680)
- whitespace
- `[A-Z][a-zA-Z0-9-_]*` — type reference (starts uppercase per X.680)
- whitespace
- `::=`
- rest is the actual value

This is safe because no bare AVN value starts with `lowercase-word Uppercase-word ::=`.

### 2. Add `decode_values()` for multi-value input

**File:** `src/avn/mod.rs`

New public function:
```rust
/// Decode multiple X.680 value assignments from a single string.
///
/// Input format:
/// ```text
/// name1 Type ::= value1
/// name2 Type ::= value2
/// ```
///
/// Values are split at the boundary where one assignment ends and the next
/// begins (detected by brace depth returning to 0 followed by a new
/// value assignment pattern).
///
/// Also accepts blank-line-separated bare values.
pub fn decode_values<T: crate::Decode>(input: &str) -> Result<Vec<T>, crate::error::DecodeError>
```

Splitting logic:
1. Scan input for lines matching the value assignment pattern (`name Type ::=`) at brace depth 0
2. Each such line starts a new value assignment
3. Collect the text from after `::=` up to (but not including) the next assignment start
4. Decode each chunk individually using `decode()`

Fallback: if no value assignment pattern is found, split on blank lines instead (for bare-value multi-PE format).

### 3. Add test fixtures

**Directory:** `tests/fixtures/saip/`

Already copied:
- `saip23_nobertlv_norfm.asn` — 26 PEs in value-assignment AVN format (89 KB)
- `saip23_nobertlv_norfm.der` — matching DER binary (12 KB)

### 4. Add tests

**File:** `src/avn/mod.rs` (in `#[cfg(test)] mod tests`)

Tests to add:

**a) Single value assignment stripping:**
```rust
#[test]
fn strip_value_assignment_prefix() {
    // value1 ProfileElement ::= header : { ... } should decode the same as header : { ... }
    let bare = "Test1: 3";
    let assigned = "myValue SimpleChoice ::= Test1: 3";
    let bare_decoded: SimpleChoice = crate::avn::decode(bare).unwrap();
    let assigned_decoded: SimpleChoice = crate::avn::decode(assigned).unwrap();
    assert_eq!(bare_decoded, assigned_decoded);
}
```

**b) Value assignment prefix is optional (bare values still work):**
```rust
#[test]
fn bare_values_still_decode() {
    // All existing tests continue to pass — this is just a sanity check
    let decoded: bool = crate::avn::decode("TRUE").unwrap();
    assert!(decoded);
}
```

**c) Multi-value splitting with `decode_values`:**
```rust
#[test]
fn decode_multiple_value_assignments() {
    let input = r#"val1 SimpleChoice ::= Test1: 3
val2 SimpleChoice ::= Test2: "hi""#;
    let values: Vec<SimpleChoice> = crate::avn::decode_values(input).unwrap();
    assert_eq!(values.len(), 2);
    assert_eq!(values[0], SimpleChoice::Test1(3));
    assert_eq!(values[1], SimpleChoice::Test2("hi".into()));
}
```

**d) SAIP fixture round-trip (integration test):**

This test requires `rssng-asn1` types, so it should live in the **rss-ng** workspace (`tca-convert` integration tests), NOT in rasn itself. The rasn tests should use the local test types defined in the test module.

However, you can add a basic file-loading sanity test that just verifies `decode_values` can split the file into the expected number of chunks without a type-specific decode.

### 5. Edge cases to handle

- **Nested braces:** The splitter must track brace depth. A new value assignment is only recognized at depth 0.
- **Strings containing `::=`:** The splitter must not be fooled by `::=` inside quoted strings (e.g., `"some ::= text"`). Track string quoting state.
- **Trailing whitespace/newlines:** Trim each chunk before decoding.
- **Mixed format:** If the first line is a value assignment, assume the whole input uses that format. If not, fall back to blank-line splitting. Don't try to mix.
- **Empty input:** Return empty vec from `decode_values`, error from `decode`.

## Files Changed

| File | Change |
|------|--------|
| `src/avn/de.rs` | Add `strip_value_assignment()`, update `Decoder::new()` |
| `src/avn/mod.rs` | Add `pub fn decode_values<T>()`, add tests |
| `tests/fixtures/saip/` | Test fixture files (already added) |

## Verification

1. `cargo test` — all existing AVN tests still pass (bare value format unchanged)
2. New tests pass for value-assignment stripping and multi-value splitting
3. No changes to encoder — `rasn::avn::encode` output is unchanged
