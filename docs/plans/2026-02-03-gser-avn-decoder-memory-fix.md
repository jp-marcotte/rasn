# GSER/AVN Decoder Memory Leak Fix

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Eliminate memory leaks in GSER and AVN decoders by replacing `Box::leak` with an owned value stack approach.

**Architecture:** Refactor both decoders to use a `Vec<GserValue>` / `Vec<AvnValue>` stack (similar to JER's `Vec<serde_json::Value>`). Parse input into owned enum variants, then pop values during decoding. This eliminates lifetime issues without leaking memory.

**Tech Stack:** Rust, nom parser combinators (already in use), no new dependencies.

---

## Background

The current GSER/AVN decoders store `&'input str` and use `Box::leak` when decoding sequences because field values must be reordered. This leaks memory on every SEQUENCE decode.

JER solves this by parsing into `serde_json::Value` first, then using a stack. We'll adopt the same pattern with a custom enum for GSER/AVN values.

---

### Task 1: Define GserValue Enum

**Files:**
- Modify: `src/gser/de.rs:1-30`

**Step 1: Add the GserValue enum after imports**

```rust
/// Intermediate owned representation of a GSER value.
/// Used to avoid lifetime issues when decoding nested structures.
#[derive(Debug, Clone)]
pub enum GserValue {
    /// Boolean: TRUE or FALSE
    Bool(bool),
    /// Integer as string (preserves arbitrary precision)
    Integer(alloc::string::String),
    /// Real number as string
    Real(alloc::string::String),
    /// Null value
    Null,
    /// String value (UTF8, IA5, etc.)
    String(alloc::string::String),
    /// Hex-encoded bytes: '...'H
    Bytes(alloc::vec::Vec<u8>),
    /// Bit string with exact bit count
    BitString(crate::types::BitString),
    /// OID as arc vector
    Oid(alloc::vec::Vec<u32>),
    /// Enumerated identifier
    Identifier(alloc::string::String),
    /// Sequence/Set: ordered field name -> value pairs
    Constructed(alloc::vec::Vec<(alloc::string::String, GserValue)>),
    /// Sequence-of/Set-of: list of values
    Array(alloc::vec::Vec<GserValue>),
    /// Choice: (variant_name, inner_value)
    Choice(alloc::string::String, alloc::boxed::Box<GserValue>),
    /// Absent optional field
    Absent,
}
```

**Step 2: Run cargo check**

Run: `cargo check -p rasn 2>&1 | head -20`
Expected: No errors (enum is just defined, not used yet)

**Step 3: Commit**

```bash
git add src/gser/de.rs
git commit -m "feat(gser): add GserValue enum for owned intermediate representation"
```

---

### Task 2: Add GserValue Parser Functions

**Files:**
- Modify: `src/gser/de.rs` (add after existing parse_* functions, around line 315)

**Step 1: Add parse_to_value function**

```rust
impl<'input> Decoder<'input> {
    // ... existing parse functions ...

    /// Parses any GSER value into an owned GserValue.
    fn parse_to_value(input: &str) -> IResult<&str, GserValue> {
        let input = input.trim();

        // Try each value type in order
        alt((
            // Boolean
            map(Self::parse_boolean, GserValue::Bool),
            // Null
            map(Self::parse_null, |_| GserValue::Null),
            // Bit string (before hex to catch 'B suffix)
            map(Self::parse_bit_string, GserValue::BitString),
            // Hex string (octet string)
            map(Self::parse_hex_string, GserValue::Bytes),
            // Constructed (sequence/set)
            Self::parse_constructed_value,
            // Choice (identifier: value)
            Self::parse_choice_value,
            // Real (PLUS-INFINITY, MINUS-INFINITY, or decimal with E)
            Self::parse_real_value,
            // Integer (must be after real to not consume real's integer part)
            map(Self::parse_integer, |s| GserValue::Integer(s.into())),
            // String
            map(Self::parse_string, GserValue::String),
            // OID (dotted decimal)
            map(Self::parse_oid, GserValue::Oid),
            // Bare identifier (enumerated)
            map(Self::parse_identifier, |s| GserValue::Identifier(s.into())),
        ))(input)
    }

    /// Parses a real number into GserValue::Real
    fn parse_real_value(input: &str) -> IResult<&str, GserValue> {
        let (rest, _) = Self::parse_real(input)?;
        // Calculate how much was consumed
        let consumed = input.len() - rest.len();
        Ok((rest, GserValue::Real(input[..consumed].into())))
    }

    /// Parses a constructed value: { field value, ... } or { value, ... }
    fn parse_constructed_value(input: &str) -> IResult<&str, GserValue> {
        let input = input.trim();
        let (input, _) = char('{')(input)?;
        let input = input.trim();

        // Empty sequence
        if let Some(rest) = input.strip_prefix('}') {
            return Ok((rest, GserValue::Constructed(alloc::vec![])));
        }

        // Peek to determine if this is named fields or array
        // Named fields have: identifier value
        // Arrays have: value, value (no field names)
        let is_named = Self::peek_is_named_field(input);

        if is_named {
            Self::parse_named_fields(input)
        } else {
            Self::parse_array_values(input)
        }
    }

    /// Peek ahead to determine if input starts with "identifier value" pattern
    fn peek_is_named_field(input: &str) -> bool {
        // Try parsing identifier followed by non-colon (choice has "id: value")
        if let Ok((rest, _)) = Self::parse_identifier(input) {
            let rest = rest.trim_start();
            // If next char is ':', it's a choice, not a named field
            // If next char is something else, it's a named field
            !rest.starts_with(':') && !rest.is_empty()
        } else {
            false
        }
    }

    /// Parse named fields: { field1 value1, field2 value2 }
    fn parse_named_fields(input: &str) -> IResult<&str, GserValue> {
        let mut fields = alloc::vec::Vec::new();
        let mut remaining = input;

        loop {
            let trimmed = remaining.trim();
            if let Some(rest) = trimmed.strip_prefix('}') {
                return Ok((rest, GserValue::Constructed(fields)));
            }

            // Parse field name
            let (rest, name) = Self::parse_identifier(trimmed)?;
            let rest = rest.trim_start();

            // Parse field value
            let end_pos = Self::find_value_end(rest);
            let value_str = &rest[..end_pos];
            let (_, value) = Self::parse_to_value(value_str)?;

            fields.push((name.into(), value));
            remaining = &rest[end_pos..];

            // Skip comma if present
            let trimmed = remaining.trim_start();
            if let Some(after_comma) = trimmed.strip_prefix(',') {
                remaining = after_comma;
            } else {
                remaining = trimmed;
            }
        }
    }

    /// Parse array values: { value1, value2, ... }
    fn parse_array_values(input: &str) -> IResult<&str, GserValue> {
        let mut values = alloc::vec::Vec::new();
        let mut remaining = input;

        loop {
            let trimmed = remaining.trim();
            if let Some(rest) = trimmed.strip_prefix('}') {
                return Ok((rest, GserValue::Array(values)));
            }

            // Parse value
            let end_pos = Self::find_value_end(trimmed);
            let value_str = &trimmed[..end_pos];
            let (_, value) = Self::parse_to_value(value_str)?;

            values.push(value);
            remaining = &trimmed[end_pos..];

            // Skip comma if present
            let trimmed = remaining.trim_start();
            if let Some(after_comma) = trimmed.strip_prefix(',') {
                remaining = after_comma;
            } else {
                remaining = trimmed;
            }
        }
    }

    /// Parse choice value: identifier: value
    fn parse_choice_value(input: &str) -> IResult<&str, GserValue> {
        let (input, _) = Self::ws(input)?;
        let (input, identifier) = Self::parse_identifier(input)?;
        let (input, _) = Self::ws(input)?;
        let (input, _) = char(':')(input)?;
        let (input, _) = Self::ws(input)?;

        let (rest, value) = Self::parse_to_value(input)?;
        Ok((rest, GserValue::Choice(identifier.into(), alloc::boxed::Box::new(value))))
    }
}
```

**Step 2: Add nom imports if missing**

Ensure these are in the imports at the top:
```rust
use nom::branch::alt;
use nom::combinator::map;
```

**Step 3: Run cargo check**

Run: `cargo check -p rasn 2>&1 | head -30`
Expected: No errors

**Step 4: Commit**

```bash
git add src/gser/de.rs
git commit -m "feat(gser): add parse_to_value for owned value parsing"
```

---

### Task 3: Refactor Decoder Struct to Use Stack

**Files:**
- Modify: `src/gser/de.rs` (Decoder struct and new() method)

**Step 1: Change Decoder struct to use value stack**

Replace the existing Decoder struct:

```rust
/// Decodes GSER text format into Rust structures.
pub struct Decoder {
    /// Stack of values to decode (like JER's approach)
    stack: alloc::vec::Vec<GserValue>,
}

impl Decoder {
    /// Creates a new decoder from the given input string.
    pub fn new(input: &str) -> Result<Self, DecodeError> {
        let (_, value) = Self::parse_to_value(input.trim()).map_err(|e| {
            DecodeError::parser_fail(
                alloc::format!("Failed to parse GSER: {e:?}"),
                crate::Codec::Gser,
            )
        })?;
        Ok(Self {
            stack: alloc::vec![value],
        })
    }

    /// Creates a decoder from an already-parsed value (for nested decoding)
    fn from_value(value: GserValue) -> Self {
        Self {
            stack: alloc::vec![value],
        }
    }

    /// Pops the next value from the stack
    fn pop(&mut self) -> Result<GserValue, DecodeError> {
        self.stack.pop().ok_or_else(|| {
            DecodeError::from(GserDecodeErrorKind::GserEndOfInput {})
        })
    }

    /// Peeks at the next value without removing it
    fn peek(&self) -> Option<&GserValue> {
        self.stack.last()
    }
}
```

**Step 2: Remove the old 'input lifetime from impl blocks**

The `Decoder<'input>` becomes just `Decoder`. Update all `impl<'input> Decoder<'input>` to `impl Decoder`.

**Step 3: Run cargo check**

Run: `cargo check -p rasn 2>&1 | head -50`
Expected: Many errors about missing methods - we'll fix these next

**Step 4: Commit (WIP)**

```bash
git add src/gser/de.rs
git commit -m "wip(gser): refactor Decoder to use value stack"
```

---

### Task 4: Update Primitive Decode Methods

**Files:**
- Modify: `src/gser/de.rs` (decode_bool, decode_integer, decode_null, etc.)

**Step 1: Update decode_bool**

```rust
fn decode_bool(&mut self, _: Tag) -> Result<bool, Self::Error> {
    match self.pop()? {
        GserValue::Bool(b) => Ok(b),
        other => Err(GserDecodeErrorKind::GserTypeMismatch {
            needed: "boolean",
            found: alloc::format!("{other:?}"),
        }.into()),
    }
}
```

**Step 2: Update decode_integer**

```rust
fn decode_integer<I: crate::types::IntegerType>(
    &mut self,
    _: Tag,
    _: Constraints,
) -> Result<I, Self::Error> {
    let int_str = match self.pop()? {
        GserValue::Integer(s) => s,
        GserValue::Real(s) => s, // Accept real as integer for compatibility
        other => return Err(GserDecodeErrorKind::GserTypeMismatch {
            needed: "integer",
            found: alloc::format!("{other:?}"),
        }.into()),
    };

    let value = int_str.parse::<i128>().map_err(|_| {
        DecodeError::parser_fail(
            alloc::format!("Failed to parse integer value: {int_str}"),
            self.codec(),
        )
    })?;

    I::try_from(value).map_err(|_| DecodeError::integer_overflow(I::WIDTH, self.codec()))
}
```

**Step 3: Update decode_null**

```rust
fn decode_null(&mut self, _: Tag) -> Result<(), Self::Error> {
    match self.pop()? {
        GserValue::Null => Ok(()),
        other => Err(GserDecodeErrorKind::GserTypeMismatch {
            needed: "null",
            found: alloc::format!("{other:?}"),
        }.into()),
    }
}
```

**Step 4: Update decode_octet_string**

```rust
fn decode_octet_string<'buf, T>(
    &'buf mut self,
    _: Tag,
    _: Constraints,
) -> Result<T, Self::Error>
where
    T: From<&'buf [u8]> + From<Vec<u8>>,
{
    match self.pop()? {
        GserValue::Bytes(bytes) => Ok(T::from(bytes)),
        other => Err(GserDecodeErrorKind::GserTypeMismatch {
            needed: "octet string",
            found: alloc::format!("{other:?}"),
        }.into()),
    }
}
```

**Step 5: Update decode_bit_string**

```rust
fn decode_bit_string(&mut self, _: Tag, _: Constraints) -> Result<BitString, Self::Error> {
    match self.pop()? {
        GserValue::BitString(bits) => Ok(bits),
        GserValue::Bytes(bytes) => Ok(BitString::from_vec(bytes)),
        other => Err(GserDecodeErrorKind::GserTypeMismatch {
            needed: "bit string",
            found: alloc::format!("{other:?}"),
        }.into()),
    }
}
```

**Step 6: Update decode_object_identifier**

```rust
fn decode_object_identifier(&mut self, _: Tag) -> Result<ObjectIdentifier, Self::Error> {
    let arcs = match self.pop()? {
        GserValue::Oid(arcs) => arcs,
        other => return Err(GserDecodeErrorKind::GserTypeMismatch {
            needed: "object identifier",
            found: alloc::format!("{other:?}"),
        }.into()),
    };

    Oid::new(&arcs).map(ObjectIdentifier::from).ok_or_else(|| {
        GserDecodeErrorKind::InvalidOid {
            value: alloc::format!("{arcs:?}"),
        }.into()
    })
}
```

**Step 7: Update decode_enumerated**

```rust
fn decode_enumerated<E: Enumerated>(&mut self, _: Tag) -> Result<E, Self::Error> {
    let identifier = match self.pop()? {
        GserValue::Identifier(s) => s,
        other => return Err(GserDecodeErrorKind::GserTypeMismatch {
            needed: "enumerated",
            found: alloc::format!("{other:?}"),
        }.into()),
    };

    E::from_identifier(&identifier).ok_or_else(|| {
        GserDecodeErrorKind::GserInvalidEnumDiscriminant {
            discriminant: identifier,
        }.into()
    })
}
```

**Step 8: Update string decode methods (decode_utf8_string, etc.)**

All string methods follow the same pattern:

```rust
fn decode_utf8_string(&mut self, _: Tag, _: Constraints) -> Result<Utf8String, Self::Error> {
    match self.pop()? {
        GserValue::String(s) => Ok(s),
        other => Err(GserDecodeErrorKind::GserTypeMismatch {
            needed: "string",
            found: alloc::format!("{other:?}"),
        }.into()),
    }
}
```

Apply similar pattern to: `decode_visible_string`, `decode_general_string`, `decode_ia5_string`, `decode_printable_string`, `decode_numeric_string`, `decode_teletex_string`, `decode_bmp_string`, `decode_graphic_string`.

Each needs `.try_into()` for the specific string type.

**Step 9: Update decode_real**

```rust
fn decode_real<R: crate::types::RealType>(
    &mut self,
    _: Tag,
    _: Constraints,
) -> Result<R, Self::Error> {
    let real_str = match self.pop()? {
        GserValue::Real(s) => s,
        GserValue::Integer(s) => s, // Accept integer as real
        other => return Err(GserDecodeErrorKind::GserTypeMismatch {
            needed: "real",
            found: alloc::format!("{other:?}"),
        }.into()),
    };

    // Parse using existing logic
    let (_, value) = Self::parse_real(&real_str).map_err(|e| {
        DecodeError::parser_fail(
            alloc::format!("Failed to parse GSER real: {e:?}"),
            self.codec(),
        )
    })?;

    R::try_from_float(value).ok_or_else(|| {
        GserDecodeErrorKind::GserTypeMismatch {
            needed: "real number",
            found: real_str,
        }.into()
    })
}
```

**Step 10: Run cargo check**

Run: `cargo check -p rasn 2>&1 | head -50`
Expected: Fewer errors, mainly about sequence/choice/optional methods

**Step 11: Commit**

```bash
git add src/gser/de.rs
git commit -m "feat(gser): update primitive decode methods for stack-based approach"
```

---

### Task 5: Update Structured Decode Methods

**Files:**
- Modify: `src/gser/de.rs` (decode_sequence, decode_sequence_of, decode_choice, etc.)

**Step 1: Update decode_sequence**

```rust
fn decode_sequence<const RC: usize, const EC: usize, D, DF, F>(
    &mut self,
    _: Tag,
    _: Option<DF>,
    decode_fn: F,
) -> Result<D, Self::Error>
where
    D: Constructed<RC, EC>,
    DF: FnOnce() -> D,
    F: FnOnce(&mut Self::AnyDecoder<RC, EC>) -> Result<D, Self::Error>,
{
    let fields = match self.pop()? {
        GserValue::Constructed(fields) => fields,
        GserValue::Absent => return Err(GserDecodeErrorKind::GserTypeMismatch {
            needed: "sequence",
            found: "absent".into(),
        }.into()),
        other => return Err(GserDecodeErrorKind::GserTypeMismatch {
            needed: "sequence",
            found: alloc::format!("{other:?}"),
        }.into()),
    };

    // Build a map of field names to values
    let field_map: alloc::collections::BTreeMap<_, _> = fields.into_iter().collect();

    // Get field names from the type
    let mut field_names = D::FIELDS.iter().map(|f| f.name).collect::<Vec<&str>>();
    if let Some(extended_fields) = D::EXTENDED_FIELDS {
        field_names.extend(extended_fields.iter().map(|f| f.name));
    }

    // Push field values onto stack in reverse order (so first field is on top)
    for name in field_names.into_iter().rev() {
        let value = field_map
            .get(name)
            .cloned()
            .unwrap_or(GserValue::Absent);
        self.stack.push(value);
    }

    (decode_fn)(self)
}
```

**Step 2: Update decode_sequence_of**

```rust
fn decode_sequence_of<D: Decode>(
    &mut self,
    _: Tag,
    _: Constraints,
) -> Result<SequenceOf<D>, Self::Error> {
    let values = match self.pop()? {
        GserValue::Array(values) => values,
        GserValue::Constructed(fields) => {
            // Accept constructed as array (for compatibility)
            fields.into_iter().map(|(_, v)| v).collect()
        }
        other => return Err(GserDecodeErrorKind::GserTypeMismatch {
            needed: "sequence of",
            found: alloc::format!("{other:?}"),
        }.into()),
    };

    values
        .into_iter()
        .map(|value| {
            let mut decoder = Decoder::from_value(value);
            D::decode(&mut decoder)
        })
        .collect()
}
```

**Step 3: Update decode_set_of**

```rust
fn decode_set_of<D: Decode + Eq + core::hash::Hash>(
    &mut self,
    _: Tag,
    _: Constraints,
) -> Result<SetOf<D>, Self::Error> {
    let values = match self.pop()? {
        GserValue::Array(values) => values,
        GserValue::Constructed(fields) => {
            fields.into_iter().map(|(_, v)| v).collect()
        }
        other => return Err(GserDecodeErrorKind::GserTypeMismatch {
            needed: "set of",
            found: alloc::format!("{other:?}"),
        }.into()),
    };

    values
        .into_iter()
        .try_fold(SetOf::new(), |mut acc, value| {
            let mut decoder = Decoder::from_value(value);
            acc.insert(D::decode(&mut decoder)?);
            Ok(acc)
        })
}
```

**Step 4: Update decode_choice**

```rust
fn decode_choice<D>(&mut self, _: Constraints) -> Result<D, Self::Error>
where
    D: DecodeChoice,
{
    let (identifier, inner_value) = match self.pop()? {
        GserValue::Choice(id, value) => (id, *value),
        other => return Err(GserDecodeErrorKind::GserTypeMismatch {
            needed: "choice",
            found: alloc::format!("{other:?}"),
        }.into()),
    };

    // Find the tag for this identifier
    let tag = D::IDENTIFIERS
        .iter()
        .enumerate()
        .find(|(_, id)| id.eq_ignore_ascii_case(&identifier))
        .and_then(|(i, _)| {
            variants::Variants::from_slice(
                &[D::VARIANTS, D::EXTENDED_VARIANTS.unwrap_or(&[])].concat(),
            )
            .get(i)
            .copied()
        })
        .ok_or_else(|| GserDecodeErrorKind::InvalidChoiceVariant {
            variant: identifier,
        })?;

    // Push the inner value and decode
    self.stack.push(inner_value);
    D::from_tag(self, tag)
}
```

**Step 5: Update decode_optional**

```rust
fn decode_optional<D: Decode>(&mut self) -> Result<Option<D>, Self::Error> {
    match self.peek() {
        Some(GserValue::Absent) | None => {
            self.stack.pop(); // Remove the Absent marker
            Ok(None)
        }
        Some(_) => {
            // Value present, decode it
            Ok(Some(D::decode(self)?))
        }
    }
}
```

**Step 6: Update other optional variants**

```rust
fn decode_optional_with_tag<D: Decode>(&mut self, _: Tag) -> Result<Option<D>, Self::Error> {
    self.decode_optional()
}

fn decode_optional_with_constraints<D: Decode>(
    &mut self,
    _: Constraints,
) -> Result<Option<D>, Self::Error> {
    self.decode_optional()
}

fn decode_optional_with_tag_and_constraints<D: Decode>(
    &mut self,
    _: Tag,
    _: Constraints,
) -> Result<Option<D>, Self::Error> {
    self.decode_optional()
}

fn decode_optional_with_explicit_prefix<D: Decode>(
    &mut self,
    _: Tag,
) -> Result<Option<D>, Self::Error> {
    self.decode_optional()
}

fn decode_explicit_prefix<D: Decode>(&mut self, _: Tag) -> Result<D, Self::Error> {
    D::decode(self)
}
```

**Step 7: Update extension addition methods**

```rust
fn decode_extension_addition_with_explicit_tag_and_constraints<D>(
    &mut self,
    _: Tag,
    _: Constraints,
) -> Result<Option<D>, Self::Error>
where
    D: Decode,
{
    self.decode_optional()
}

fn decode_extension_addition_with_tag_and_constraints<D>(
    &mut self,
    _: Tag,
    _: Constraints,
) -> Result<Option<D>, Self::Error>
where
    D: Decode,
{
    self.decode_optional()
}

fn decode_extension_addition_group<
    const RC: usize,
    const EC: usize,
    D: Decode + Constructed<RC, EC>,
>(
    &mut self,
) -> Result<Option<D>, Self::Error> {
    self.decode_optional()
}
```

**Step 8: Update decode_any**

```rust
fn decode_any(&mut self, _: Tag) -> Result<Any, Self::Error> {
    match self.pop()? {
        GserValue::Bytes(bytes) => Ok(Any::new(bytes)),
        other => {
            // For non-bytes, serialize back to string representation
            // This is a best-effort for ANY type
            Ok(Any::new(alloc::format!("{other:?}").into_bytes()))
        }
    }
}
```

**Step 9: Update decode_set**

```rust
fn decode_set<const RC: usize, const EC: usize, FIELDS, SET, D, F>(
    &mut self,
    _: Tag,
    decode_fn: D,
    field_fn: F,
) -> Result<SET, Self::Error>
where
    SET: Decode + Constructed<RC, EC>,
    FIELDS: Decode,
    D: Fn(&mut Self::AnyDecoder<RC, EC>, usize, Tag) -> Result<FIELDS, Self::Error>,
    F: FnOnce(Vec<FIELDS>) -> Result<SET, Self::Error>,
{
    let fields = match self.pop()? {
        GserValue::Constructed(fields) => fields,
        other => return Err(GserDecodeErrorKind::GserTypeMismatch {
            needed: "set",
            found: alloc::format!("{other:?}"),
        }.into()),
    };

    let field_map: alloc::collections::BTreeMap<_, _> = fields.into_iter().collect();

    // Get field info from the type and decode in tag order
    let mut field_indices: Vec<_> = SET::FIELDS.iter().enumerate().collect();
    field_indices
        .sort_by(|(_, a), (_, b)| a.tag_tree.smallest_tag().cmp(&b.tag_tree.smallest_tag()));

    let mut decoded_fields = Vec::new();
    for (index, field) in field_indices.into_iter() {
        let value = field_map
            .get(field.name)
            .cloned()
            .unwrap_or(GserValue::Absent);
        self.stack.push(value);
        decoded_fields.push((decode_fn)(self, index, field.tag)?);
    }

    // Handle extension fields if present
    for (index, field) in SET::EXTENDED_FIELDS
        .iter()
        .flat_map(|fields| fields.iter())
        .enumerate()
    {
        let value = field_map
            .get(field.name)
            .cloned()
            .unwrap_or(GserValue::Absent);
        self.stack.push(value);
        decoded_fields.push((decode_fn)(self, index + SET::FIELDS.len(), field.tag)?);
    }

    (field_fn)(decoded_fields)
}
```

**Step 10: Update time decode methods**

```rust
fn decode_utc_time(&mut self, _: Tag) -> Result<UtcTime, Self::Error> {
    let s = match self.pop()? {
        GserValue::String(s) => s,
        other => return Err(GserDecodeErrorKind::GserTypeMismatch {
            needed: "utc time string",
            found: alloc::format!("{other:?}"),
        }.into()),
    };
    crate::ber::de::Decoder::parse_any_utc_time_string(s)
}

fn decode_generalized_time(&mut self, _: Tag) -> Result<GeneralizedTime, Self::Error> {
    let s = match self.pop()? {
        GserValue::String(s) => s,
        other => return Err(GserDecodeErrorKind::GserTypeMismatch {
            needed: "generalized time string",
            found: alloc::format!("{other:?}"),
        }.into()),
    };
    crate::ber::de::Decoder::parse_any_generalized_time_string(s)
}

fn decode_date(&mut self, _: Tag) -> Result<Date, Self::Error> {
    let s = match self.pop()? {
        GserValue::String(s) => s,
        other => return Err(GserDecodeErrorKind::GserTypeMismatch {
            needed: "date string",
            found: alloc::format!("{other:?}"),
        }.into()),
    };
    crate::ber::de::Decoder::parse_date_string(&s)
}
```

**Step 11: Run cargo check**

Run: `cargo check -p rasn 2>&1 | head -30`
Expected: Should compile cleanly

**Step 12: Commit**

```bash
git add src/gser/de.rs
git commit -m "feat(gser): update structured decode methods for stack-based approach"
```

---

### Task 6: Remove Old Parser Functions and Clean Up

**Files:**
- Modify: `src/gser/de.rs`

**Step 1: Remove take_input method**

Delete the `take_input` method as it's no longer needed.

**Step 2: Remove the old parse_field_value, parse_sequence_content, parse_single_value, parse_sequence_of_content methods**

These are replaced by the new `parse_to_value` based approach.

**Step 3: Keep the primitive parsers**

Keep: `parse_boolean`, `parse_null`, `parse_integer`, `parse_identifier`, `parse_string`, `parse_hex_string`, `parse_binary_string`, `parse_bit_string`, `parse_oid`, `parse_real`, `bytes_from_hex`, `ws`, `find_value_end`

These are still used by `parse_to_value`.

**Step 4: Update the AnyDecoder type**

```rust
type AnyDecoder<const R: usize, const E: usize> = Self;
```

This stays the same since we're using the same Decoder type.

**Step 5: Run cargo check**

Run: `cargo check -p rasn 2>&1`
Expected: Clean compilation

**Step 6: Commit**

```bash
git add src/gser/de.rs
git commit -m "refactor(gser): remove obsolete parser methods"
```

---

### Task 7: Run GSER Tests

**Files:**
- Test: `src/gser/mod.rs`

**Step 1: Run all GSER tests**

Run: `cargo test -p rasn --lib -- gser:: 2>&1 | tail -60`
Expected: All 49 tests pass

**Step 2: Fix any failing tests**

If tests fail, debug and fix. Common issues:
- Parser order in `alt()` matters
- Type matching for edge cases

**Step 3: Commit if fixes were needed**

```bash
git add src/gser/
git commit -m "fix(gser): test fixes after decoder refactor"
```

---

### Task 8: Apply Same Changes to AVN Decoder

**Files:**
- Modify: `src/avn/de.rs`

**Step 1: Copy the GserValue enum as AvnValue**

Same structure, different name for clarity.

**Step 2: Apply all the same refactoring steps**

Follow Tasks 2-6 but for AVN:
- Add `AvnValue` enum
- Add `parse_to_value` and helper functions
- Refactor `Decoder` to use stack
- Update all decode methods
- Remove old methods

**Key AVN differences:**
- OID format: `{ arc arc arc }` instead of `1.2.3`
- REAL format: `{ mantissa, base, exponent }` tuple

**Step 3: Run AVN tests**

Run: `cargo test -p rasn --lib -- avn:: 2>&1 | tail -60`
Expected: All 55 tests pass

**Step 4: Commit**

```bash
git add src/avn/de.rs
git commit -m "feat(avn): refactor decoder to use stack-based approach (no memory leak)"
```

---

### Task 9: Final Integration Test

**Files:**
- Test: Full test suite

**Step 1: Run all rasn tests**

Run: `cargo test -p rasn 2>&1 | tail -30`
Expected: All tests pass

**Step 2: Run clippy**

Run: `cargo clippy -p rasn 2>&1 | head -30`
Expected: No new warnings

**Step 3: Final commit**

```bash
git add -A
git commit -m "feat: eliminate memory leaks in GSER/AVN decoders

Refactored both decoders to use an owned value stack approach
(like JER) instead of borrowed string slices with Box::leak.

- Added GserValue/AvnValue enums as intermediate representations
- Parse input into owned values once, then pop from stack
- No more memory leaks on repeated decode operations
- All existing tests pass"
```

---

## Summary

This plan converts the GSER/AVN decoders from a borrowed-slice approach (with memory leaks) to an owned-value-stack approach (like JER). The key insight is that parsing into an intermediate owned representation eliminates lifetime issues entirely.

**Total commits:** 9-10 (incremental, testable)

**Risk areas:**
- Parser order in `alt()` - may need adjustment based on test results
- Edge cases in type matching - covered by existing tests
