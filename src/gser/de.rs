//! Decoding Generic String Encoding Rules (GSER) data into Rust structures.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// Intermediate owned representation of a GSER value.
/// Used to avoid lifetime issues when decoding nested structures.
#[derive(Debug, Clone)]
pub enum GserValue {
    /// Boolean: TRUE or FALSE
    Bool(bool),
    /// Integer as string (preserves arbitrary precision)
    Integer(String),
    /// Real number as string
    Real(String),
    /// Null value
    Null,
    /// String value (UTF8, IA5, etc.)
    String(String),
    /// Hex-encoded bytes: '...'H
    Bytes(Vec<u8>),
    /// Bit string with exact bit count
    BitString(crate::types::BitString),
    /// OID as arc vector
    Oid(Vec<u32>),
    /// Enumerated identifier
    Identifier(String),
    /// Sequence/Set: ordered field name -> value pairs
    Constructed(Vec<(String, GserValue)>),
    /// Sequence-of/Set-of: list of values
    Array(Vec<GserValue>),
    /// Choice: (variant_name, inner_value)
    Choice(String, Box<GserValue>),
    /// Absent optional field
    Absent,
}
use nom::{
    branch::alt,
    bytes::complete::{tag, take_while, take_while1},
    character::complete::{char, multispace0, one_of},
    combinator::{map, map_res, opt, recognize, value},
    sequence::{pair, preceded, tuple},
    IResult,
};

use crate::{
    error::{DecodeError, GserDecodeErrorKind},
    types::{
        variants, Any, BitString, BmpString, Constraints, Constructed, Date, DecodeChoice,
        Enumerated, GeneralString, GeneralizedTime, GraphicString, Ia5String, NumericString,
        ObjectIdentifier, Oid, PrintableString, SequenceOf, SetOf, Tag, TeletexString, UtcTime,
        Utf8String, VisibleString,
    },
    Decode,
};

/// Decodes GSER text format into Rust structures.
///
/// Uses an owned value stack approach (like JER) to avoid lifetime issues
/// when decoding nested structures with field reordering.
pub struct Decoder {
    /// Stack of values to decode (like JER's approach)
    stack: Vec<GserValue>,
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
        Ok(Self { stack: vec![value] })
    }

    /// Creates a decoder from an already-parsed value (for nested decoding)
    fn from_value(value: GserValue) -> Self {
        Self { stack: vec![value] }
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

    /// Parses whitespace
    fn ws(input: &str) -> IResult<&str, ()> {
        map(multispace0, |_| ())(input)
    }

    /// Parses a GSER boolean: TRUE or FALSE
    fn parse_boolean(input: &str) -> IResult<&str, bool> {
        alt((value(true, tag("TRUE")), value(false, tag("FALSE"))))(input)
    }

    /// Parses a GSER null: NULL
    fn parse_null(input: &str) -> IResult<&str, ()> {
        value((), tag("NULL"))(input)
    }

    /// Parses a GSER integer (decimal)
    fn parse_integer(input: &str) -> IResult<&str, &str> {
        recognize(pair(
            opt(char('-')),
            take_while1(|c: char| c.is_ascii_digit()),
        ))(input)
    }

    /// Parses an identifier (used for enums, field names, choice variants)
    /// ASN.1 identifiers must start with a letter, then can contain letters, digits, hyphens.
    fn parse_identifier(input: &str) -> IResult<&str, &str> {
        recognize(pair(
            nom::character::complete::satisfy(|c| c.is_ascii_alphabetic() || c == '_'),
            take_while(|c: char| c.is_alphanumeric() || c == '-' || c == '_'),
        ))(input)
    }

    /// Parses a GSER string with doubled-quote escaping: "..." where "" means literal "
    fn parse_string(input: &str) -> IResult<&str, String> {
        let (input, _) = char('"')(input)?;
        let mut result = String::new();
        let mut chars = input.chars().peekable();
        let mut consumed = 0;

        loop {
            match chars.next() {
                Some('"') => {
                    consumed += 1;
                    // Check if next char is also a quote (escaped quote)
                    if chars.peek() == Some(&'"') {
                        result.push('"');
                        chars.next();
                        consumed += 1;
                    } else {
                        // End of string
                        break;
                    }
                }
                Some(c) => {
                    result.push(c);
                    consumed += c.len_utf8();
                }
                None => {
                    return Err(nom::Err::Error(nom::error::Error::new(
                        input,
                        nom::error::ErrorKind::Char,
                    )));
                }
            }
        }

        Ok((&input[consumed..], result))
    }

    /// Parses a hex string: '...'H
    fn parse_hex_string(input: &str) -> IResult<&str, Vec<u8>> {
        let (input, _) = char('\'')(input)?;
        let (input, hex_chars) = take_while(|c: char| c.is_ascii_hexdigit())(input)?;
        let (input, _) = tag("'H")(input)?;

        let bytes = Self::bytes_from_hex(hex_chars).ok_or_else(|| {
            nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Char))
        })?;

        Ok((input, bytes))
    }

    /// Parses a binary string: '...'B (RFC 3641 §3.5)
    /// Returns the BitString with exact bit count preserved.
    fn parse_binary_string(input: &str) -> IResult<&str, BitString> {
        let (input, _) = char('\'')(input)?;
        let (input, binary_chars) = take_while(|c: char| c == '0' || c == '1')(input)?;
        let (input, _) = tag("'B")(input)?;

        let bits: BitString = binary_chars.chars().map(|c| c == '1').collect();
        Ok((input, bits))
    }

    /// Converts a hex string to bytes
    fn bytes_from_hex(hex_string: &str) -> Option<Vec<u8>> {
        if hex_string.is_empty() {
            return Some(Vec::new());
        }
        if !hex_string.len().is_multiple_of(2) {
            return None;
        }

        let mut bytes = Vec::with_capacity(hex_string.len() / 2);
        for i in (0..hex_string.len()).step_by(2) {
            let byte_str = &hex_string[i..i + 2];
            let byte = u8::from_str_radix(byte_str, 16).ok()?;
            bytes.push(byte);
        }
        Some(bytes)
    }

    /// Parses an OID in dotted decimal format: 1.2.840.113549
    /// Requires at least 2 arcs (i.e., at least one dot) to distinguish from integers.
    fn parse_oid(input: &str) -> IResult<&str, Vec<u32>> {
        let (input, first) = map_res(take_while1(|c: char| c.is_ascii_digit()), |s: &str| {
            s.parse::<u32>()
        })(input)?;

        // Require at least one more arc (many1 instead of many0)
        let (input, rest) = nom::multi::many1(preceded(
            char('.'),
            map_res(take_while1(|c: char| c.is_ascii_digit()), |s: &str| {
                s.parse::<u32>()
            }),
        ))(input)?;

        let mut arcs = vec![first];
        arcs.extend(rest);
        Ok((input, arcs))
    }

    /// Parses a GSER real number
    fn parse_real(input: &str) -> IResult<&str, f64> {
        alt((
            value(f64::INFINITY, tag("PLUS-INFINITY")),
            value(f64::NEG_INFINITY, tag("MINUS-INFINITY")),
            map_res(
                recognize(tuple((
                    opt(char('-')),
                    take_while1(|c: char| c.is_ascii_digit()),
                    opt(pair(char('.'), take_while1(|c: char| c.is_ascii_digit()))),
                    opt(tuple((
                        one_of("Ee"),
                        opt(one_of("+-")),
                        take_while1(|c: char| c.is_ascii_digit()),
                    ))),
                ))),
                |s: &str| s.parse::<f64>(),
            ),
        ))(input)
    }

    /// Helper: Find the end of a value (at next comma or closing brace at same nesting level)
    /// Returns the position where the value ends.
    fn find_value_end(input: &str) -> usize {
        let mut depth = 0;
        let mut in_string = false;
        let mut chars = input.char_indices().peekable();

        while let Some((pos, c)) = chars.next() {
            if in_string {
                if c == '"' {
                    // Check for escaped quote
                    if chars.peek().map(|(_, nc)| *nc) == Some('"') {
                        chars.next();
                    } else {
                        in_string = false;
                    }
                }
            } else {
                match c {
                    '"' => in_string = true,
                    '{' => depth += 1,
                    '}' => {
                        if depth == 0 {
                            return pos;
                        }
                        depth -= 1;
                    }
                    ',' if depth == 0 => return pos,
                    _ => {}
                }
            }
        }
        input.len()
    }

    // ============================================================================
    // GserValue parsing functions (owned intermediate representation)
    // ============================================================================

    /// Parses any GSER value into an owned GserValue.
    fn parse_to_value(input: &str) -> IResult<&str, GserValue> {
        let input = input.trim();

        // Try each value type in order - ORDER MATTERS!
        alt((
            // Boolean
            map(Self::parse_boolean, GserValue::Bool),
            // Null
            map(Self::parse_null, |_| GserValue::Null),
            // Hex string as Bytes (octet string) - BEFORE bit string!
            map(Self::parse_hex_string, GserValue::Bytes),
            // Binary string as BitString ('...'B only)
            map(Self::parse_binary_string, GserValue::BitString),
            // Constructed (sequence/set)
            Self::parse_constructed_value,
            // Choice (identifier: value)
            Self::parse_choice_value,
            // OID (dotted decimal) - BEFORE real/integer to avoid ambiguity
            map(Self::parse_oid, GserValue::Oid),
            // Real (PLUS-INFINITY, MINUS-INFINITY, or decimal with exponent)
            Self::parse_real_value,
            // Integer (must be after real to not consume real's integer part)
            map(Self::parse_integer, |s| GserValue::Integer(s.into())),
            // String
            map(Self::parse_string, GserValue::String),
            // Bare identifier (enumerated)
            map(Self::parse_identifier, |s| GserValue::Identifier(s.into())),
        ))(input)
    }

    /// Parses a real number into GserValue::Real
    fn parse_real_value(input: &str) -> IResult<&str, GserValue> {
        // Special values
        if let Ok((rest, _)) = tag::<_, _, nom::error::Error<&str>>("PLUS-INFINITY")(input) {
            return Ok((rest, GserValue::Real("PLUS-INFINITY".into())));
        }
        if let Ok((rest, _)) = tag::<_, _, nom::error::Error<&str>>("MINUS-INFINITY")(input) {
            return Ok((rest, GserValue::Real("MINUS-INFINITY".into())));
        }

        // Numeric with decimal point or exponent
        let result: IResult<&str, &str> = recognize(tuple((
            opt(char('-')),
            take_while1(|c: char| c.is_ascii_digit()),
            alt((
                // Has decimal point
                recognize(pair(
                    pair(char('.'), take_while1(|c: char| c.is_ascii_digit())),
                    opt(tuple((
                        one_of("Ee"),
                        opt(one_of("+-")),
                        take_while1(|c: char| c.is_ascii_digit()),
                    ))),
                )),
                // Has exponent only (no decimal point)
                recognize(tuple((
                    one_of("Ee"),
                    opt(one_of("+-")),
                    take_while1(|c: char| c.is_ascii_digit()),
                ))),
            )),
        )))(input);

        match result {
            Ok((rest, real_str)) => Ok((rest, GserValue::Real(real_str.into()))),
            Err(e) => Err(e),
        }
    }

    /// Parses a constructed value: { field value, ... } or { value, ... }
    fn parse_constructed_value(input: &str) -> IResult<&str, GserValue> {
        let input = input.trim();
        let (input, _) = char('{')(input)?;
        let input = input.trim();

        // Empty sequence
        if let Some(rest) = input.strip_prefix('}') {
            return Ok((rest, GserValue::Constructed(vec![])));
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
        let mut fields = Vec::new();
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
        let mut values = Vec::new();
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
        Ok((rest, GserValue::Choice(identifier.into(), Box::new(value))))
    }
}

impl crate::Decoder for Decoder {
    type Ok = ();
    type Error = DecodeError;
    type AnyDecoder<const R: usize, const E: usize> = Self;

    fn decode_any(&mut self, _: Tag) -> Result<Any, Self::Error> {
        match self.pop()? {
            GserValue::Bytes(bytes) => Ok(Any::new(bytes)),
            other => {
                // For non-bytes, serialize back to string representation
                Ok(Any::new(alloc::format!("{other:?}").into_bytes()))
            }
        }
    }

    fn decode_bit_string(&mut self, _: Tag, _: Constraints) -> Result<BitString, Self::Error> {
        match self.pop()? {
            GserValue::BitString(bits) => Ok(bits),
            GserValue::Bytes(bytes) => Ok(BitString::from_vec(bytes)),
            other => Err(GserDecodeErrorKind::GserTypeMismatch {
                needed: "bit string",
                found: alloc::format!("{other:?}"),
            }
            .into()),
        }
    }

    fn decode_bool(&mut self, _: Tag) -> Result<bool, Self::Error> {
        match self.pop()? {
            GserValue::Bool(b) => Ok(b),
            other => Err(GserDecodeErrorKind::GserTypeMismatch {
                needed: "boolean",
                found: alloc::format!("{other:?}"),
            }
            .into()),
        }
    }

    fn decode_enumerated<E: Enumerated>(&mut self, _: Tag) -> Result<E, Self::Error> {
        let identifier = match self.pop()? {
            GserValue::Identifier(s) => s,
            other => {
                return Err(GserDecodeErrorKind::GserTypeMismatch {
                    needed: "enumerated",
                    found: alloc::format!("{other:?}"),
                }
                .into())
            }
        };

        E::from_identifier(&identifier).ok_or_else(|| {
            GserDecodeErrorKind::GserInvalidEnumDiscriminant {
                discriminant: identifier,
            }
            .into()
        })
    }

    fn decode_integer<I: crate::types::IntegerType>(
        &mut self,
        _: Tag,
        _: Constraints,
    ) -> Result<I, Self::Error> {
        let int_str = match self.pop()? {
            GserValue::Integer(s) => s,
            GserValue::Real(s) => s, // Accept real as integer for compatibility
            other => {
                return Err(GserDecodeErrorKind::GserTypeMismatch {
                    needed: "integer",
                    found: alloc::format!("{other:?}"),
                }
                .into())
            }
        };

        let value = int_str.parse::<i128>().map_err(|_| {
            DecodeError::parser_fail(
                alloc::format!("Failed to parse integer value: {int_str}"),
                self.codec(),
            )
        })?;

        I::try_from(value).map_err(|_| DecodeError::integer_overflow(I::WIDTH, self.codec()))
    }

    fn decode_real<R: crate::types::RealType>(
        &mut self,
        _: Tag,
        _: Constraints,
    ) -> Result<R, Self::Error> {
        let real_str = match self.pop()? {
            GserValue::Real(s) => s,
            GserValue::Integer(s) => s, // Accept integer as real
            other => {
                return Err(GserDecodeErrorKind::GserTypeMismatch {
                    needed: "real",
                    found: alloc::format!("{other:?}"),
                }
                .into())
            }
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
            }
            .into()
        })
    }

    fn decode_null(&mut self, _: Tag) -> Result<(), Self::Error> {
        match self.pop()? {
            GserValue::Null => Ok(()),
            other => Err(GserDecodeErrorKind::GserTypeMismatch {
                needed: "null",
                found: alloc::format!("{other:?}"),
            }
            .into()),
        }
    }

    fn decode_object_identifier(&mut self, _: Tag) -> Result<ObjectIdentifier, Self::Error> {
        let arcs = match self.pop()? {
            GserValue::Oid(arcs) => arcs,
            other => {
                return Err(GserDecodeErrorKind::GserTypeMismatch {
                    needed: "object identifier",
                    found: alloc::format!("{other:?}"),
                }
                .into())
            }
        };

        Oid::new(&arcs).map(ObjectIdentifier::from).ok_or_else(|| {
            GserDecodeErrorKind::InvalidOid {
                value: alloc::format!("{arcs:?}"),
            }
            .into()
        })
    }

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
            GserValue::Absent => {
                return Err(GserDecodeErrorKind::GserTypeMismatch {
                    needed: "sequence",
                    found: "absent".into(),
                }
                .into())
            }
            other => {
                return Err(GserDecodeErrorKind::GserTypeMismatch {
                    needed: "sequence",
                    found: alloc::format!("{other:?}"),
                }
                .into())
            }
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
            let value = field_map.get(name).cloned().unwrap_or(GserValue::Absent);
            self.stack.push(value);
        }

        (decode_fn)(self)
    }

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
            other => {
                return Err(GserDecodeErrorKind::GserTypeMismatch {
                    needed: "sequence of",
                    found: alloc::format!("{other:?}"),
                }
                .into())
            }
        };

        values
            .into_iter()
            .map(|value| {
                let mut decoder = Decoder::from_value(value);
                D::decode(&mut decoder)
            })
            .collect()
    }

    fn decode_set_of<D: Decode + Eq + core::hash::Hash>(
        &mut self,
        _: Tag,
        _: Constraints,
    ) -> Result<SetOf<D>, Self::Error> {
        let values = match self.pop()? {
            GserValue::Array(values) => values,
            GserValue::Constructed(fields) => fields.into_iter().map(|(_, v)| v).collect(),
            other => {
                return Err(GserDecodeErrorKind::GserTypeMismatch {
                    needed: "set of",
                    found: alloc::format!("{other:?}"),
                }
                .into())
            }
        };

        values
            .into_iter()
            .try_fold(SetOf::new(), |mut acc, value| {
                let mut decoder = Decoder::from_value(value);
                acc.insert(D::decode(&mut decoder)?);
                Ok(acc)
            })
    }

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
            }
            .into()),
        }
    }

    fn decode_utf8_string(&mut self, _: Tag, _: Constraints) -> Result<Utf8String, Self::Error> {
        match self.pop()? {
            GserValue::String(s) => Ok(s),
            other => Err(GserDecodeErrorKind::GserTypeMismatch {
                needed: "string",
                found: alloc::format!("{other:?}"),
            }
            .into()),
        }
    }

    fn decode_visible_string(
        &mut self,
        _: Tag,
        _: Constraints,
    ) -> Result<VisibleString, Self::Error> {
        let s = match self.pop()? {
            GserValue::String(s) => s,
            other => {
                return Err(GserDecodeErrorKind::GserTypeMismatch {
                    needed: "string",
                    found: alloc::format!("{other:?}"),
                }
                .into())
            }
        };
        s.try_into().map_err(|e| {
            DecodeError::string_conversion_failed(
                Tag::VISIBLE_STRING,
                alloc::format!("Error transforming VisibleString: {e:?}"),
                self.codec(),
            )
        })
    }

    fn decode_general_string(
        &mut self,
        _: Tag,
        _: Constraints,
    ) -> Result<GeneralString, Self::Error> {
        let s = match self.pop()? {
            GserValue::String(s) => s,
            other => {
                return Err(GserDecodeErrorKind::GserTypeMismatch {
                    needed: "string",
                    found: alloc::format!("{other:?}"),
                }
                .into())
            }
        };
        s.try_into().map_err(|e| {
            DecodeError::string_conversion_failed(
                Tag::GENERAL_STRING,
                alloc::format!("Error transforming GeneralString: {e:?}"),
                self.codec(),
            )
        })
    }

    fn decode_graphic_string(
        &mut self,
        _: Tag,
        _: Constraints,
    ) -> Result<GraphicString, Self::Error> {
        let s = match self.pop()? {
            GserValue::String(s) => s,
            other => {
                return Err(GserDecodeErrorKind::GserTypeMismatch {
                    needed: "string",
                    found: alloc::format!("{other:?}"),
                }
                .into())
            }
        };
        s.try_into().map_err(|e| {
            DecodeError::string_conversion_failed(
                Tag::GRAPHIC_STRING,
                alloc::format!("Error transforming GraphicString: {e:?}"),
                self.codec(),
            )
        })
    }

    fn decode_ia5_string(&mut self, _: Tag, _: Constraints) -> Result<Ia5String, Self::Error> {
        let s = match self.pop()? {
            GserValue::String(s) => s,
            other => {
                return Err(GserDecodeErrorKind::GserTypeMismatch {
                    needed: "string",
                    found: alloc::format!("{other:?}"),
                }
                .into())
            }
        };
        s.try_into().map_err(|e| {
            DecodeError::string_conversion_failed(
                Tag::IA5_STRING,
                alloc::format!("Error transforming Ia5String: {e:?}"),
                self.codec(),
            )
        })
    }

    fn decode_printable_string(
        &mut self,
        _: Tag,
        _: Constraints,
    ) -> Result<PrintableString, Self::Error> {
        let s = match self.pop()? {
            GserValue::String(s) => s,
            other => {
                return Err(GserDecodeErrorKind::GserTypeMismatch {
                    needed: "string",
                    found: alloc::format!("{other:?}"),
                }
                .into())
            }
        };
        s.try_into().map_err(|e| {
            DecodeError::string_conversion_failed(
                Tag::PRINTABLE_STRING,
                alloc::format!("Error transforming PrintableString: {e:?}"),
                self.codec(),
            )
        })
    }

    fn decode_numeric_string(
        &mut self,
        _: Tag,
        _: Constraints,
    ) -> Result<NumericString, Self::Error> {
        let s = match self.pop()? {
            GserValue::String(s) => s,
            other => {
                return Err(GserDecodeErrorKind::GserTypeMismatch {
                    needed: "string",
                    found: alloc::format!("{other:?}"),
                }
                .into())
            }
        };
        s.try_into().map_err(|e| {
            DecodeError::string_conversion_failed(
                Tag::NUMERIC_STRING,
                alloc::format!("Error transforming NumericString: {e:?}"),
                self.codec(),
            )
        })
    }

    fn decode_teletex_string(
        &mut self,
        _: Tag,
        _: Constraints,
    ) -> Result<TeletexString, Self::Error> {
        let s = match self.pop()? {
            GserValue::String(s) => s,
            other => {
                return Err(GserDecodeErrorKind::GserTypeMismatch {
                    needed: "string",
                    found: alloc::format!("{other:?}"),
                }
                .into())
            }
        };
        s.try_into().map_err(|e| {
            DecodeError::string_conversion_failed(
                Tag::TELETEX_STRING,
                alloc::format!("Error transforming TeletexString: {e:?}"),
                self.codec(),
            )
        })
    }

    fn decode_bmp_string(&mut self, _: Tag, _: Constraints) -> Result<BmpString, Self::Error> {
        let s = match self.pop()? {
            GserValue::String(s) => s,
            other => {
                return Err(GserDecodeErrorKind::GserTypeMismatch {
                    needed: "string",
                    found: alloc::format!("{other:?}"),
                }
                .into())
            }
        };
        s.try_into().map_err(|e| {
            DecodeError::string_conversion_failed(
                Tag::BMP_STRING,
                alloc::format!("Error transforming BmpString: {e:?}"),
                self.codec(),
            )
        })
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

    fn decode_utc_time(&mut self, _: Tag) -> Result<UtcTime, Self::Error> {
        let s = match self.pop()? {
            GserValue::String(s) => s,
            other => {
                return Err(GserDecodeErrorKind::GserTypeMismatch {
                    needed: "utc time string",
                    found: alloc::format!("{other:?}"),
                }
                .into())
            }
        };
        crate::ber::de::Decoder::parse_any_utc_time_string(s)
    }

    fn decode_generalized_time(&mut self, _: Tag) -> Result<GeneralizedTime, Self::Error> {
        let s = match self.pop()? {
            GserValue::String(s) => s,
            other => {
                return Err(GserDecodeErrorKind::GserTypeMismatch {
                    needed: "generalized time string",
                    found: alloc::format!("{other:?}"),
                }
                .into())
            }
        };
        crate::ber::de::Decoder::parse_any_generalized_time_string(s)
    }

    fn decode_date(&mut self, _: Tag) -> Result<Date, Self::Error> {
        let s = match self.pop()? {
            GserValue::String(s) => s,
            other => {
                return Err(GserDecodeErrorKind::GserTypeMismatch {
                    needed: "date string",
                    found: alloc::format!("{other:?}"),
                }
                .into())
            }
        };
        crate::ber::de::Decoder::parse_date_string(&s)
    }

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
            other => {
                return Err(GserDecodeErrorKind::GserTypeMismatch {
                    needed: "set",
                    found: alloc::format!("{other:?}"),
                }
                .into())
            }
        };

        let field_map: alloc::collections::BTreeMap<_, _> = fields.into_iter().collect();

        // Get field info from the type and decode in tag order
        let mut field_indices: Vec<_> = SET::FIELDS.iter().enumerate().collect();
        field_indices
            .sort_by(|(_, a), (_, b)| a.tag_tree.smallest_tag().cmp(&b.tag_tree.smallest_tag()));

        let mut decoded_fields = Vec::new();
        for (index, field) in field_indices.into_iter() {
            let value = field_map.get(field.name).cloned().unwrap_or(GserValue::Absent);
            self.stack.push(value);
            decoded_fields.push((decode_fn)(self, index, field.tag)?);
        }

        // Handle extension fields if present
        for (index, field) in SET::EXTENDED_FIELDS
            .iter()
            .flat_map(|fields| fields.iter())
            .enumerate()
        {
            let value = field_map.get(field.name).cloned().unwrap_or(GserValue::Absent);
            self.stack.push(value);
            decoded_fields.push((decode_fn)(self, index + SET::FIELDS.len(), field.tag)?);
        }

        (field_fn)(decoded_fields)
    }

    fn decode_choice<D>(&mut self, _: Constraints) -> Result<D, Self::Error>
    where
        D: DecodeChoice,
    {
        let (identifier, inner_value) = match self.pop()? {
            GserValue::Choice(id, value) => (id, *value),
            other => {
                return Err(GserDecodeErrorKind::GserTypeMismatch {
                    needed: "choice",
                    found: alloc::format!("{other:?}"),
                }
                .into())
            }
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

    fn codec(&self) -> crate::Codec {
        crate::Codec::Gser
    }
}
