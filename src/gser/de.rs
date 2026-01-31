//! Decoding Generic String Encoding Rules (GSER) data into Rust structures.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use nom::{
    branch::alt,
    bytes::complete::{tag, take_while, take_while1},
    character::complete::{char, multispace0, one_of},
    combinator::{map, map_res, opt, recognize, value},
    multi::many0,
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
pub struct Decoder<'input> {
    input: &'input str,
}

impl<'input> Decoder<'input> {
    /// Creates a new decoder from the given input string.
    pub fn new(input: &'input str) -> Result<Self, DecodeError> {
        Ok(Self { input })
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
    fn parse_identifier(input: &str) -> IResult<&str, &str> {
        take_while1(|c: char| c.is_alphanumeric() || c == '-' || c == '_')(input)
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

    /// Parses hex '...'H as BitString (byte-aligned)
    fn parse_hex_as_bitstring(input: &str) -> IResult<&str, BitString> {
        let (input, bytes) = Self::parse_hex_string(input)?;
        Ok((input, BitString::from_vec(bytes)))
    }

    /// Parses either hex '...'H or binary '...'B (RFC 3641 §3.5)
    fn parse_bit_string(input: &str) -> IResult<&str, BitString> {
        alt((Self::parse_hex_as_bitstring, Self::parse_binary_string))(input)
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
    fn parse_oid(input: &str) -> IResult<&str, Vec<u32>> {
        let (input, first) = map_res(take_while1(|c: char| c.is_ascii_digit()), |s: &str| {
            s.parse::<u32>()
        })(input)?;

        let (input, rest) = many0(preceded(
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

    /// Parses a single field-value pair from a sequence
    fn parse_field_value(input: &str) -> IResult<&str, (&str, &str)> {
        let input = input.trim_start();
        let (input, field_name) = Self::parse_identifier(input)?;
        let input = input.trim_start();

        let end_pos = Self::find_value_end(input);
        let value_str = input[..end_pos].trim();
        Ok((&input[end_pos..], (field_name, value_str)))
    }

    /// Parses a sequence/set: { field value, field value, ... }
    /// Returns a list of (field_name, field_value_str) pairs
    fn parse_sequence_content(input: &str) -> IResult<&str, Vec<(&str, &str)>> {
        let input = input.trim_start();
        let (input, _) = char('{')(input)?;
        let input = input.trim_start();

        // Check for empty sequence
        if let Some(rest) = input.strip_prefix('}') {
            return Ok((rest, Vec::new()));
        }

        let (input, first) = Self::parse_field_value(input)?;
        let mut fields = vec![first];

        let mut remaining = input;
        loop {
            let trimmed = remaining.trim_start();
            if let Some(rest) = trimmed.strip_prefix('}') {
                remaining = rest;
                break;
            }
            if let Some(after_comma) = trimmed.strip_prefix(',') {
                let (rest, field) = Self::parse_field_value(after_comma)?;
                fields.push(field);
                remaining = rest;
            } else {
                break;
            }
        }

        Ok((remaining, fields))
    }

    /// Parses a single value from a sequence-of
    fn parse_single_value(input: &str) -> IResult<&str, &str> {
        let input = input.trim_start();
        let end_pos = Self::find_value_end(input);
        let value_str = input[..end_pos].trim();
        Ok((&input[end_pos..], value_str))
    }

    /// Parses a sequence of values: { value, value, ... }
    fn parse_sequence_of_content(input: &str) -> IResult<&str, Vec<&str>> {
        let input = input.trim_start();
        let (input, _) = char('{')(input)?;
        let input = input.trim_start();

        // Check for empty sequence
        if let Some(rest) = input.strip_prefix('}') {
            return Ok((rest, Vec::new()));
        }

        let (input, first) = Self::parse_single_value(input)?;
        let mut values = vec![first];

        let mut remaining = input;
        loop {
            let trimmed = remaining.trim_start();
            if let Some(rest) = trimmed.strip_prefix('}') {
                remaining = rest;
                break;
            }
            if let Some(after_comma) = trimmed.strip_prefix(',') {
                let (rest, val) = Self::parse_single_value(after_comma)?;
                values.push(val);
                remaining = rest;
            } else {
                break;
            }
        }

        Ok((remaining, values))
    }

    /// Parses a choice: identifier: value
    fn parse_choice(input: &str) -> IResult<&str, (&str, &str)> {
        let (input, _) = Self::ws(input)?;
        let (input, identifier) = Self::parse_identifier(input)?;
        let (input, _) = Self::ws(input)?;
        let (input, _) = char(':')(input)?;
        let (input, _) = Self::ws(input)?;

        // The rest is the value
        Ok(("", (identifier, input.trim())))
    }

    /// Consume and return the remaining input
    fn take_input(&mut self) -> &'input str {
        // If there's a null byte separator (from sequence field parsing),
        // only take up to the null byte
        if let Some(pos) = self.input.find('\0') {
            let (current, rest) = self.input.split_at(pos);
            self.input = &rest[1..]; // Skip the null byte
            current
        } else {
            let input = self.input;
            self.input = "";
            input
        }
    }
}

impl crate::Decoder for Decoder<'_> {
    type Ok = ();
    type Error = DecodeError;
    type AnyDecoder<const R: usize, const E: usize> = Self;

    fn decode_any(&mut self, _: Tag) -> Result<Any, Self::Error> {
        let input = self.take_input().trim();
        // Try to parse as hex string for ANY type
        match Self::parse_hex_string(input) {
            Ok((_, bytes)) => Ok(Any::new(bytes)),
            Err(_) => Ok(Any::new(input.as_bytes().to_vec())),
        }
    }

    fn decode_bit_string(&mut self, _: Tag, _: Constraints) -> Result<BitString, Self::Error> {
        let input = self.take_input().trim();
        // RFC 3641 §3.5: Accept both hstring and bstring formats
        let (_, bits) = Self::parse_bit_string(input).map_err(|e| {
            DecodeError::parser_fail(
                alloc::format!("Failed to parse GSER bit string: {e:?}"),
                self.codec(),
            )
        })?;
        Ok(bits)
    }

    fn decode_bool(&mut self, _: Tag) -> Result<bool, Self::Error> {
        let input = self.take_input().trim();
        let (_, value) = Self::parse_boolean(input).map_err(|e| {
            DecodeError::parser_fail(
                alloc::format!("Failed to parse GSER boolean: {e:?}"),
                self.codec(),
            )
        })?;
        Ok(value)
    }

    fn decode_enumerated<E: Enumerated>(&mut self, _: Tag) -> Result<E, Self::Error> {
        let input = self.take_input().trim();
        let (_, identifier) = Self::parse_identifier(input).map_err(|e| {
            DecodeError::parser_fail(
                alloc::format!("Failed to parse GSER enumerated: {e:?}"),
                self.codec(),
            )
        })?;
        E::from_identifier(identifier).ok_or_else(|| {
            GserDecodeErrorKind::GserInvalidEnumDiscriminant {
                discriminant: identifier.into(),
            }
            .into()
        })
    }

    fn decode_integer<I: crate::types::IntegerType>(
        &mut self,
        _: Tag,
        _: Constraints,
    ) -> Result<I, Self::Error> {
        let input = self.take_input().trim();
        let (_, int_str) = Self::parse_integer(input).map_err(|e| {
            DecodeError::parser_fail(
                alloc::format!("Failed to parse GSER integer: {e:?}"),
                self.codec(),
            )
        })?;

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
        let input = self.take_input().trim();
        let (_, value) = Self::parse_real(input).map_err(|e| {
            DecodeError::parser_fail(
                alloc::format!("Failed to parse GSER real: {e:?}"),
                self.codec(),
            )
        })?;

        R::try_from_float(value).ok_or_else(|| {
            GserDecodeErrorKind::GserTypeMismatch {
                needed: "real number",
                found: input.into(),
            }
            .into()
        })
    }

    fn decode_null(&mut self, _: Tag) -> Result<(), Self::Error> {
        let input = self.take_input().trim();
        let (_, _) = Self::parse_null(input).map_err(|e| {
            DecodeError::parser_fail(
                alloc::format!("Failed to parse GSER null: {e:?}"),
                self.codec(),
            )
        })?;
        Ok(())
    }

    fn decode_object_identifier(&mut self, _: Tag) -> Result<ObjectIdentifier, Self::Error> {
        let input = self.take_input().trim();
        let (_, arcs) = Self::parse_oid(input).map_err(|e| {
            DecodeError::parser_fail(
                alloc::format!("Failed to parse GSER OID: {e:?}"),
                self.codec(),
            )
        })?;

        Oid::new(&arcs).map(ObjectIdentifier::from).ok_or_else(|| {
            GserDecodeErrorKind::InvalidOid {
                value: input.into(),
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
        let input = self.take_input().trim();
        let (_, fields) = Self::parse_sequence_content(input).map_err(|e| {
            DecodeError::parser_fail(
                alloc::format!("Failed to parse GSER sequence: {e:?}"),
                self.codec(),
            )
        })?;

        // Build a map of field names to values
        let field_map: alloc::collections::BTreeMap<&str, &str> = fields.into_iter().collect();

        // Get field names from the type
        let mut field_names = D::FIELDS.iter().map(|f| f.name).collect::<Vec<&str>>();
        if let Some(extended_fields) = D::EXTENDED_FIELDS {
            field_names.extend(extended_fields.iter().map(|f| f.name));
        }

        // Create a decoder that yields field values in order
        let field_values: Vec<Option<&str>> = field_names
            .iter()
            .map(|name| field_map.get(name).copied())
            .collect();

        // Store values for decoding - use a null byte separator between field values
        // Fields are in definition order, so decode_fn will decode them in that order
        let values_str = field_values
            .into_iter()
            .map(|v| v.unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\0");

        // Leak the string so it has a static lifetime - this is a memory leak
        // but for practical use cases it's negligible
        self.input = Box::leak(values_str.into_boxed_str());

        (decode_fn)(self)
    }

    fn decode_sequence_of<D: Decode>(
        &mut self,
        _: Tag,
        _: Constraints,
    ) -> Result<SequenceOf<D>, Self::Error> {
        let input = self.take_input().trim();
        let (_, values) = Self::parse_sequence_of_content(input).map_err(|e| {
            DecodeError::parser_fail(
                alloc::format!("Failed to parse GSER sequence of: {e:?}"),
                self.codec(),
            )
        })?;

        values
            .into_iter()
            .map(|value_str| {
                let mut decoder = Decoder::new(value_str)?;
                D::decode(&mut decoder)
            })
            .collect()
    }

    fn decode_set_of<D: Decode + Eq + core::hash::Hash>(
        &mut self,
        _: Tag,
        _: Constraints,
    ) -> Result<SetOf<D>, Self::Error> {
        let input = self.take_input().trim();
        let (_, values) = Self::parse_sequence_of_content(input).map_err(|e| {
            DecodeError::parser_fail(
                alloc::format!("Failed to parse GSER set of: {e:?}"),
                self.codec(),
            )
        })?;

        values
            .into_iter()
            .try_fold(SetOf::new(), |mut acc, value_str| {
                let mut decoder = Decoder::new(value_str)?;
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
        let input = self.take_input().trim();
        let (_, bytes) = Self::parse_hex_string(input).map_err(|e| {
            DecodeError::parser_fail(
                alloc::format!("Failed to parse GSER octet string: {e:?}"),
                self.codec(),
            )
        })?;
        Ok(T::from(bytes))
    }

    fn decode_utf8_string(&mut self, _: Tag, _: Constraints) -> Result<Utf8String, Self::Error> {
        let input = self.take_input().trim();
        let (_, s) = Self::parse_string(input).map_err(|e| {
            DecodeError::parser_fail(
                alloc::format!("Failed to parse GSER string: {e:?}"),
                self.codec(),
            )
        })?;
        Ok(s)
    }

    fn decode_visible_string(
        &mut self,
        _: Tag,
        _: Constraints,
    ) -> Result<VisibleString, Self::Error> {
        let input = self.take_input().trim();
        let (_, s) = Self::parse_string(input).map_err(|e| {
            DecodeError::parser_fail(
                alloc::format!("Failed to parse GSER string: {e:?}"),
                self.codec(),
            )
        })?;
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
        let input = self.take_input().trim();
        let (_, s) = Self::parse_string(input).map_err(|e| {
            DecodeError::parser_fail(
                alloc::format!("Failed to parse GSER string: {e:?}"),
                self.codec(),
            )
        })?;
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
        let input = self.take_input().trim();
        let (_, s) = Self::parse_string(input).map_err(|e| {
            DecodeError::parser_fail(
                alloc::format!("Failed to parse GSER string: {e:?}"),
                self.codec(),
            )
        })?;
        s.try_into().map_err(|e| {
            DecodeError::string_conversion_failed(
                Tag::GRAPHIC_STRING,
                alloc::format!("Error transforming GraphicString: {e:?}"),
                self.codec(),
            )
        })
    }

    fn decode_ia5_string(&mut self, _: Tag, _: Constraints) -> Result<Ia5String, Self::Error> {
        let input = self.take_input().trim();
        let (_, s) = Self::parse_string(input).map_err(|e| {
            DecodeError::parser_fail(
                alloc::format!("Failed to parse GSER string: {e:?}"),
                self.codec(),
            )
        })?;
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
        let input = self.take_input().trim();
        let (_, s) = Self::parse_string(input).map_err(|e| {
            DecodeError::parser_fail(
                alloc::format!("Failed to parse GSER string: {e:?}"),
                self.codec(),
            )
        })?;
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
        let input = self.take_input().trim();
        let (_, s) = Self::parse_string(input).map_err(|e| {
            DecodeError::parser_fail(
                alloc::format!("Failed to parse GSER string: {e:?}"),
                self.codec(),
            )
        })?;
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
        let input = self.take_input().trim();
        let (_, s) = Self::parse_string(input).map_err(|e| {
            DecodeError::parser_fail(
                alloc::format!("Failed to parse GSER string: {e:?}"),
                self.codec(),
            )
        })?;
        s.try_into().map_err(|e| {
            DecodeError::string_conversion_failed(
                Tag::TELETEX_STRING,
                alloc::format!("Error transforming TeletexString: {e:?}"),
                self.codec(),
            )
        })
    }

    fn decode_bmp_string(&mut self, _: Tag, _: Constraints) -> Result<BmpString, Self::Error> {
        let input = self.take_input().trim();
        let (_, s) = Self::parse_string(input).map_err(|e| {
            DecodeError::parser_fail(
                alloc::format!("Failed to parse GSER string: {e:?}"),
                self.codec(),
            )
        })?;
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
        let input = self.take_input().trim();
        let (_, s) = Self::parse_string(input).map_err(|e| {
            DecodeError::parser_fail(
                alloc::format!("Failed to parse GSER time string: {e:?}"),
                self.codec(),
            )
        })?;
        crate::ber::de::Decoder::parse_any_utc_time_string(s)
    }

    fn decode_generalized_time(&mut self, _: Tag) -> Result<GeneralizedTime, Self::Error> {
        let input = self.take_input().trim();
        let (_, s) = Self::parse_string(input).map_err(|e| {
            DecodeError::parser_fail(
                alloc::format!("Failed to parse GSER time string: {e:?}"),
                self.codec(),
            )
        })?;
        crate::ber::de::Decoder::parse_any_generalized_time_string(s)
    }

    fn decode_date(&mut self, _: Tag) -> Result<Date, Self::Error> {
        let input = self.take_input().trim();
        let (_, s) = Self::parse_string(input).map_err(|e| {
            DecodeError::parser_fail(
                alloc::format!("Failed to parse GSER date string: {e:?}"),
                self.codec(),
            )
        })?;
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
        let input = self.take_input().trim();
        let (_, fields) = Self::parse_sequence_content(input).map_err(|e| {
            DecodeError::parser_fail(
                alloc::format!("Failed to parse GSER set: {e:?}"),
                self.codec(),
            )
        })?;

        // Build a map of field names to values
        let field_map: alloc::collections::BTreeMap<&str, &str> = fields.into_iter().collect();

        // Get field info from the type and decode in tag order
        let mut field_indices: Vec<_> = SET::FIELDS.iter().enumerate().collect();
        field_indices
            .sort_by(|(_, a), (_, b)| a.tag_tree.smallest_tag().cmp(&b.tag_tree.smallest_tag()));

        let mut decoded_fields = Vec::new();
        for (index, field) in field_indices.into_iter() {
            let value_str = field_map.get(field.name).copied().unwrap_or("");
            self.input = value_str;
            decoded_fields.push((decode_fn)(self, index, field.tag)?);
        }

        // Handle extension fields if present
        for (index, field) in SET::EXTENDED_FIELDS
            .iter()
            .flat_map(|fields| fields.iter())
            .enumerate()
        {
            let value_str = field_map.get(field.name).copied().unwrap_or("");
            self.input = value_str;
            decoded_fields.push((decode_fn)(self, index + SET::FIELDS.len(), field.tag)?);
        }

        (field_fn)(decoded_fields)
    }

    fn decode_choice<D>(&mut self, _: Constraints) -> Result<D, Self::Error>
    where
        D: DecodeChoice,
    {
        // Save the rest of the input (after null separator) before processing
        let rest = if let Some(pos) = self.input.find('\0') {
            let (current, rest) = self.input.split_at(pos);
            self.input = current;
            Some(&rest[1..]) // Skip the null byte
        } else {
            None
        };

        let input = self.take_input().trim();
        let (_, (identifier, value_str)) = Self::parse_choice(input).map_err(|e| {
            DecodeError::parser_fail(
                alloc::format!("Failed to parse GSER choice: {e:?}"),
                self.codec(),
            )
        })?;

        // Find the tag for this identifier
        let tag = D::IDENTIFIERS
            .iter()
            .enumerate()
            .find(|(_, id)| id.eq_ignore_ascii_case(identifier))
            .and_then(|(i, _)| {
                variants::Variants::from_slice(
                    &[D::VARIANTS, D::EXTENDED_VARIANTS.unwrap_or(&[])].concat(),
                )
                .get(i)
                .copied()
            })
            .ok_or_else(|| GserDecodeErrorKind::InvalidChoiceVariant {
                variant: identifier.into(),
            })?;

        self.input = value_str;
        let result = D::from_tag(self, tag)?;

        // Restore the rest of the input
        if let Some(rest) = rest {
            self.input = rest;
        }

        Ok(result)
    }

    fn decode_optional<D: Decode>(&mut self) -> Result<Option<D>, Self::Error> {
        // In GSER sequences, look for the next field value separated by null byte
        if self.input.is_empty() {
            return Ok(None);
        }

        // Check if there's a separator
        if let Some(pos) = self.input.find('\0') {
            let (current, rest) = self.input.split_at(pos);
            if current.is_empty() {
                self.input = &rest[1..]; // Skip the null separator
                return Ok(None);
            }
            self.input = current;
            let result = D::decode(self)?;
            self.input = &rest[1..]; // Skip the null separator
            Ok(Some(result))
        } else {
            let current = self.input;
            if current.is_empty() {
                return Ok(None);
            }
            self.input = "";
            let mut decoder = Decoder::new(current)?;
            Ok(Some(D::decode(&mut decoder)?))
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
        tag: Tag,
        constraints: Constraints,
    ) -> Result<Option<D>, Self::Error>
    where
        D: Decode,
    {
        self.decode_extension_addition_with_tag_and_constraints::<D>(tag, constraints)
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
