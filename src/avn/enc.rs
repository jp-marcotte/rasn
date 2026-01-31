//! Encoding Rust structures into ASN.1 Value Notation (AVN) data.
//!
//! AVN is the native human-readable notation defined in ITU-T X.680 Section 17.

use alloc::string::ToString;

use crate::{
    error::{AvnEncodeErrorKind, EncodeError},
    types::{
        strings::StaticPermittedAlphabet, variants, Constraints, Identifier, IntegerType, RealType,
        Tag,
    },
};

/// Encodes Rust structures into AVN text format.
pub struct Encoder {
    /// Stack of field names for the current encoding context
    stack: alloc::vec::Vec<&'static str>,
    /// Stack of constructed values being built (sequences/sets)
    constructed_stack: alloc::vec::Vec<alloc::vec::Vec<alloc::string::String>>,
    /// The final root value
    root_value: Option<alloc::string::String>,
}

impl Default for Encoder {
    fn default() -> Self {
        Self::new()
    }
}

impl Encoder {
    /// Creates a new default encoder.
    #[must_use]
    pub fn new() -> Self {
        Self {
            stack: alloc::vec![],
            constructed_stack: alloc::vec![],
            root_value: None,
        }
    }

    /// Returns the complete encoded AVN string, consuming the encoder.
    #[allow(clippy::inherent_to_string)]
    #[must_use]
    pub fn to_string(self) -> alloc::string::String {
        self.root_value.unwrap_or_default()
    }

    /// Updates either the root value or adds to the current constructed value.
    fn update_root_or_constructed(
        &mut self,
        value: alloc::string::String,
    ) -> Result<(), EncodeError> {
        match self.stack.pop() {
            Some(field_name) => {
                // Inside a sequence/set - add as "field value" pair
                let field_entry = alloc::format!("{field_name} {value}");
                self.constructed_stack
                    .last_mut()
                    .ok_or(AvnEncodeErrorKind::AvnInternalStackMismatch)?
                    .push(field_entry);
            }
            None => {
                // Root level - set as the final value
                self.root_value = Some(value);
            }
        }
        Ok(())
    }

    /// Escapes a string for AVN format by doubling internal quotes.
    fn escape_string(s: &str) -> alloc::string::String {
        let mut result = alloc::string::String::with_capacity(s.len() + 2);
        result.push('"');
        for c in s.chars() {
            if c == '"' {
                result.push_str("\"\"");
            } else {
                result.push(c);
            }
        }
        result.push('"');
        result
    }

    /// Formats bytes as a hex string: '...'H
    fn format_hex_string(bytes: &[u8]) -> alloc::string::String {
        let hex: alloc::string::String = bytes.iter().map(|b| alloc::format!("{b:02X}")).collect();
        alloc::format!("'{hex}'H")
    }

    /// Formats a bit string as binary: '...'B
    fn format_binary_string(bits: &crate::types::BitStr) -> alloc::string::String {
        let binary: alloc::string::String =
            bits.iter().map(|b| if *b { '1' } else { '0' }).collect();
        alloc::format!("'{binary}'B")
    }

    /// Formats an OID in AVN format: { arc arc arc }
    fn format_oid(arcs: &[u32]) -> alloc::string::String {
        let arc_strings: alloc::vec::Vec<alloc::string::String> =
            arcs.iter().map(|arc| alloc::format!("{arc}")).collect();
        alloc::format!("{{ {} }}", arc_strings.join(" "))
    }
}

impl crate::Encoder<'_> for Encoder {
    type Ok = ();
    type Error = EncodeError;
    type AnyEncoder<'this, const R: usize, const E: usize> = Encoder;

    fn encode_any(
        &mut self,
        t: Tag,
        value: &crate::types::Any,
        _: Identifier,
    ) -> Result<Self::Ok, Self::Error> {
        // Encode ANY as hex-encoded octet string
        self.encode_octet_string(
            t,
            Constraints::default(),
            value.as_bytes(),
            Identifier::EMPTY,
        )
    }

    fn encode_bool(&mut self, _: Tag, value: bool, _: Identifier) -> Result<Self::Ok, Self::Error> {
        let avn_value = if value { "TRUE" } else { "FALSE" };
        self.update_root_or_constructed(avn_value.into())
    }

    fn encode_bit_string(
        &mut self,
        _: Tag,
        _: Constraints,
        value: &crate::types::BitStr,
        _: Identifier,
    ) -> Result<Self::Ok, Self::Error> {
        // X.680 §22.18: hstring can only be used when bit count is multiple of 4
        let avn_value = if value.len().is_multiple_of(4) {
            let mut bitvec = value.to_bitvec();
            bitvec.force_align();
            Self::format_hex_string(&bitvec.into_vec())
        } else {
            Self::format_binary_string(value)
        };
        self.update_root_or_constructed(avn_value)
    }

    fn encode_enumerated<E: crate::types::Enumerated>(
        &mut self,
        _: Tag,
        value: &E,
        _: Identifier,
    ) -> Result<Self::Ok, Self::Error> {
        self.update_root_or_constructed(alloc::string::String::from(value.identifier()))
    }

    fn encode_object_identifier(
        &mut self,
        _: Tag,
        value: &[u32],
        _: Identifier,
    ) -> Result<Self::Ok, Self::Error> {
        // AVN uses { arc arc arc } format (space-separated in braces)
        self.update_root_or_constructed(Self::format_oid(value))
    }

    fn encode_integer<I: IntegerType>(
        &mut self,
        _: Tag,
        _: Constraints,
        value: &I,
        _: Identifier,
    ) -> Result<Self::Ok, Self::Error> {
        if let Some(as_i128) = value.to_i128() {
            self.update_root_or_constructed(alloc::format!("{as_i128}"))
        } else if let Some(bigint) = value.to_bigint() {
            self.update_root_or_constructed(bigint.to_string())
        } else {
            Err(AvnEncodeErrorKind::AvnIntegerEncodingFailed.into())
        }
    }

    fn encode_real<R: RealType>(
        &mut self,
        _: Tag,
        _: Constraints,
        value: &R,
        _: Identifier,
    ) -> Result<Self::Ok, Self::Error> {
        use num_traits::{float::FloatCore, ToPrimitive, Zero};

        let as_float = value
            .try_to_float()
            .ok_or(AvnEncodeErrorKind::AvnRealEncodingFailed)?;

        let avn_value = if as_float.is_infinite() {
            if as_float.is_sign_positive() {
                "PLUS-INFINITY".into()
            } else {
                "MINUS-INFINITY".into()
            }
        } else if as_float.is_nan() {
            // AVN doesn't have a standard NaN representation, use 0
            "0".into()
        } else if as_float.is_zero() {
            "0".into()
        } else if let Some(f64_val) = as_float.to_f64() {
            // AVN REAL format: { mantissa, base, exponent }
            // For practical purposes, we use base 10 representation
            // Example: 3.14 becomes { 314, 10, -2 }
            format_real_as_tuple(f64_val)
        } else {
            return Err(AvnEncodeErrorKind::AvnRealEncodingFailed.into());
        };
        self.update_root_or_constructed(avn_value)
    }

    fn encode_null(&mut self, _: Tag, _: Identifier) -> Result<Self::Ok, Self::Error> {
        self.update_root_or_constructed("NULL".into())
    }

    fn encode_octet_string(
        &mut self,
        _: Tag,
        _: Constraints,
        value: &[u8],
        _: Identifier,
    ) -> Result<Self::Ok, Self::Error> {
        self.update_root_or_constructed(Self::format_hex_string(value))
    }

    fn encode_general_string(
        &mut self,
        _: Tag,
        _: Constraints,
        value: &crate::types::GeneralString,
        _: Identifier,
    ) -> Result<Self::Ok, Self::Error> {
        let s = alloc::string::String::from_utf8(value.to_vec())
            .map_err(|_| AvnEncodeErrorKind::AvnInvalidCharacter)?;
        self.update_root_or_constructed(Self::escape_string(&s))
    }

    fn encode_graphic_string(
        &mut self,
        _: Tag,
        _: Constraints,
        value: &crate::types::GraphicString,
        _: Identifier,
    ) -> Result<Self::Ok, Self::Error> {
        let s = alloc::string::String::from_utf8(value.to_vec())
            .map_err(|_| AvnEncodeErrorKind::AvnInvalidCharacter)?;
        self.update_root_or_constructed(Self::escape_string(&s))
    }

    fn encode_utf8_string(
        &mut self,
        _: Tag,
        _: Constraints,
        value: &str,
        _: Identifier,
    ) -> Result<Self::Ok, Self::Error> {
        self.update_root_or_constructed(Self::escape_string(value))
    }

    fn encode_visible_string(
        &mut self,
        _: Tag,
        _: Constraints,
        value: &crate::types::VisibleString,
        _: Identifier,
    ) -> Result<Self::Ok, Self::Error> {
        let s = alloc::string::String::from_utf8(value.as_iso646_bytes().to_vec())
            .map_err(|_| AvnEncodeErrorKind::AvnInvalidCharacter)?;
        self.update_root_or_constructed(Self::escape_string(&s))
    }

    fn encode_ia5_string(
        &mut self,
        _: Tag,
        _: Constraints,
        value: &crate::types::Ia5String,
        _: Identifier,
    ) -> Result<Self::Ok, Self::Error> {
        let s = alloc::string::String::from_utf8(value.as_iso646_bytes().to_vec())
            .map_err(|_| AvnEncodeErrorKind::AvnInvalidCharacter)?;
        self.update_root_or_constructed(Self::escape_string(&s))
    }

    fn encode_printable_string(
        &mut self,
        _: Tag,
        _: Constraints,
        value: &crate::types::PrintableString,
        _: Identifier,
    ) -> Result<Self::Ok, Self::Error> {
        let s = alloc::string::String::from_utf8(value.as_bytes().to_vec())
            .map_err(|_| AvnEncodeErrorKind::AvnInvalidCharacter)?;
        self.update_root_or_constructed(Self::escape_string(&s))
    }

    fn encode_numeric_string(
        &mut self,
        _: Tag,
        _: Constraints,
        value: &crate::types::NumericString,
        _: Identifier,
    ) -> Result<Self::Ok, Self::Error> {
        let s = alloc::string::String::from_utf8(value.as_bytes().to_vec())
            .map_err(|_| AvnEncodeErrorKind::AvnInvalidCharacter)?;
        self.update_root_or_constructed(Self::escape_string(&s))
    }

    fn encode_teletex_string(
        &mut self,
        _: Tag,
        _: Constraints,
        value: &crate::types::TeletexString,
        _: Identifier,
    ) -> Result<Self::Ok, Self::Error> {
        // TeletexString stores Unicode codepoints as Vec<u32>, convert to UTF-8
        let s: alloc::string::String = value.chars().filter_map(char::from_u32).collect();
        self.update_root_or_constructed(Self::escape_string(&s))
    }

    fn encode_bmp_string(
        &mut self,
        _: Tag,
        _: Constraints,
        value: &crate::types::BmpString,
        _: Identifier,
    ) -> Result<Self::Ok, Self::Error> {
        let s = alloc::string::String::from_utf8(value.to_bytes())
            .map_err(|_| AvnEncodeErrorKind::AvnInvalidCharacter)?;
        self.update_root_or_constructed(Self::escape_string(&s))
    }

    fn encode_generalized_time(
        &mut self,
        _: Tag,
        value: &crate::types::GeneralizedTime,
        _: Identifier,
    ) -> Result<Self::Ok, Self::Error> {
        let s = alloc::string::String::from_utf8(
            crate::ber::enc::Encoder::datetime_to_canonical_generalized_time_bytes(value),
        )
        .map_err(|_| AvnEncodeErrorKind::AvnInvalidCharacter)?;
        self.update_root_or_constructed(Self::escape_string(&s))
    }

    fn encode_utc_time(
        &mut self,
        _: Tag,
        value: &crate::types::UtcTime,
        _: Identifier,
    ) -> Result<Self::Ok, Self::Error> {
        let s = alloc::string::String::from_utf8(
            crate::ber::enc::Encoder::datetime_to_canonical_utc_time_bytes(value),
        )
        .map_err(|_| AvnEncodeErrorKind::AvnInvalidCharacter)?;
        self.update_root_or_constructed(Self::escape_string(&s))
    }

    fn encode_date(
        &mut self,
        _: Tag,
        value: &crate::types::Date,
        _: Identifier,
    ) -> Result<Self::Ok, Self::Error> {
        let s = alloc::string::String::from_utf8(
            crate::ber::enc::Encoder::naivedate_to_date_bytes(value),
        )
        .map_err(|_| AvnEncodeErrorKind::AvnInvalidCharacter)?;
        self.update_root_or_constructed(Self::escape_string(&s))
    }

    fn encode_explicit_prefix<V: crate::Encode>(
        &mut self,
        _: Tag,
        value: &V,
        _: Identifier,
    ) -> Result<Self::Ok, Self::Error> {
        value.encode(self)
    }

    fn encode_sequence<'b, const RC: usize, const EC: usize, C, F>(
        &'b mut self,
        _: Tag,
        encoder_scope: F,
        _: Identifier,
    ) -> Result<Self::Ok, Self::Error>
    where
        C: crate::types::Constructed<RC, EC>,
        F: FnOnce(&mut Self::AnyEncoder<'b, RC, EC>) -> Result<(), Self::Error>,
    {
        // Collect field names for the sequence
        let mut field_names = C::FIELDS
            .iter()
            .map(|f| f.name)
            .collect::<alloc::vec::Vec<&str>>();
        if let Some(extended_fields) = C::EXTENDED_FIELDS {
            field_names.extend(extended_fields.iter().map(|f| f.name));
        }
        field_names.reverse();
        for name in field_names {
            self.stack.push(name);
        }

        // Start new constructed context
        self.constructed_stack.push(alloc::vec![]);

        // Encode the fields
        (encoder_scope)(self)?;

        // Build the AVN sequence: { field1 value1, field2 value2 }
        let fields = self
            .constructed_stack
            .pop()
            .ok_or(AvnEncodeErrorKind::AvnInternalStackMismatch)?;

        let avn_value = if fields.is_empty() {
            "{ }".into()
        } else {
            alloc::format!("{{ {} }}", fields.join(", "))
        };

        self.update_root_or_constructed(avn_value)
    }

    fn encode_sequence_of<E: crate::Encode>(
        &mut self,
        _: Tag,
        value: &[E],
        _: Constraints,
        _: Identifier,
    ) -> Result<Self::Ok, Self::Error> {
        let mut items = alloc::vec::Vec::with_capacity(value.len());
        for item in value {
            let mut item_encoder = Self::new();
            item.encode(&mut item_encoder)?;
            items.push(item_encoder.to_string());
        }

        let avn_value = if items.is_empty() {
            "{ }".into()
        } else {
            alloc::format!("{{ {} }}", items.join(", "))
        };

        self.update_root_or_constructed(avn_value)
    }

    fn encode_set<'b, const RC: usize, const EC: usize, C, F>(
        &'b mut self,
        tag: Tag,
        value: F,
        identifier: Identifier,
    ) -> Result<Self::Ok, Self::Error>
    where
        C: crate::types::Constructed<RC, EC>,
        F: FnOnce(&mut Self::AnyEncoder<'b, RC, EC>) -> Result<(), Self::Error>,
    {
        // SET is encoded the same as SEQUENCE in AVN
        self.encode_sequence::<RC, EC, C, F>(tag, value, identifier)
    }

    fn encode_set_of<E: crate::Encode + Eq + core::hash::Hash>(
        &mut self,
        _: Tag,
        value: &crate::types::SetOf<E>,
        _: Constraints,
        _: Identifier,
    ) -> Result<Self::Ok, Self::Error> {
        let mut items = alloc::vec::Vec::with_capacity(value.len());
        for item in value.to_vec() {
            let mut item_encoder = Self::new();
            item.encode(&mut item_encoder)?;
            items.push(item_encoder.to_string());
        }

        let avn_value = if items.is_empty() {
            "{ }".into()
        } else {
            alloc::format!("{{ {} }}", items.join(", "))
        };

        self.update_root_or_constructed(avn_value)
    }

    fn encode_some<E: crate::Encode>(
        &mut self,
        value: &E,
        _: Identifier,
    ) -> Result<Self::Ok, Self::Error> {
        value.encode(self)
    }

    fn encode_some_with_tag_and_constraints<E: crate::Encode>(
        &mut self,
        _: Tag,
        _: Constraints,
        value: &E,
        _: Identifier,
    ) -> Result<Self::Ok, Self::Error> {
        value.encode(self)
    }

    fn encode_none<E: crate::Encode>(&mut self, _: Identifier) -> Result<Self::Ok, Self::Error> {
        // Pop the field name from stack since we're not encoding anything
        self.stack.pop();
        Ok(())
    }

    fn encode_none_with_tag(&mut self, _: Tag, _: Identifier) -> Result<Self::Ok, Self::Error> {
        self.stack.pop();
        Ok(())
    }

    fn encode_choice<E: crate::Encode + crate::types::Choice>(
        &mut self,
        _: Constraints,
        tag: Tag,
        encode_fn: impl FnOnce(&mut Self) -> Result<Tag, Self::Error>,
        _: Identifier,
    ) -> Result<Self::Ok, Self::Error> {
        let variants = variants::Variants::from_slice(
            &[E::VARIANTS, E::EXTENDED_VARIANTS.unwrap_or(&[])].concat(),
        );

        // Find the identifier for this choice variant
        let identifier = variants
            .iter()
            .enumerate()
            .find_map(|(i, &variant_tag)| {
                (tag == variant_tag)
                    .then_some(E::IDENTIFIERS.get(i))
                    .flatten()
            })
            .ok_or_else(|| crate::error::EncodeError::variant_not_in_choice(self.codec()))?;

        if variants.is_empty() {
            self.update_root_or_constructed("{ }".into())
        } else {
            // Encode the choice value
            let mut value_encoder = Self::new();
            (encode_fn)(&mut value_encoder)?;
            let value_str = value_encoder.to_string();

            // Format as "identifier: value"
            let avn_value = alloc::format!("{identifier}: {value_str}");
            self.update_root_or_constructed(avn_value)
        }
    }

    fn encode_extension_addition<E: crate::Encode>(
        &mut self,
        _: Tag,
        _: Constraints,
        value: E,
        _: Identifier,
    ) -> Result<Self::Ok, Self::Error> {
        value.encode(self)
    }

    fn encode_extension_addition_group<const RC: usize, const EC: usize, E>(
        &mut self,
        value: Option<&E>,
        _: Identifier,
    ) -> Result<Self::Ok, Self::Error>
    where
        E: crate::Encode + crate::types::Constructed<RC, EC>,
    {
        match value {
            Some(v) => v.encode(self),
            None => self.encode_none::<E>(Identifier::EMPTY),
        }
    }

    fn codec(&self) -> crate::Codec {
        crate::Codec::Avn
    }
}

/// Formats a floating-point number in AVN REAL tuple format: { mantissa, base, exponent }
/// Uses base 10 for human readability.
fn format_real_as_tuple(value: f64) -> alloc::string::String {
    // Handle special cases
    if value == 0.0 {
        return "0".into();
    }

    // Convert to string and parse to get mantissa/exponent
    // We use a string representation to avoid floating-point precision issues
    let formatted = alloc::format!("{:E}", value);

    // Parse the scientific notation: e.g., "3.14E2" or "-1.5E-3"
    if let Some(e_pos) = formatted.find(['E', 'e']) {
        let (mantissa_part, exp_part) = formatted.split_at(e_pos);
        let exp_str = &exp_part[1..]; // Skip 'E'

        // Parse the exponent
        let exponent: i32 = exp_str.parse().unwrap_or(0);

        // Remove decimal point from mantissa and adjust exponent
        let mantissa_str = mantissa_part.replace('.', "");
        let decimal_places = if let Some(dot_pos) = mantissa_part.find('.') {
            mantissa_part.len() - dot_pos - 1
        } else {
            0
        };

        // Parse mantissa as integer
        let mantissa: i64 = mantissa_str.parse().unwrap_or(0);

        // Adjust exponent for decimal places
        let adjusted_exponent = exponent - decimal_places as i32;

        alloc::format!("{{ {mantissa}, 10, {adjusted_exponent} }}")
    } else {
        // No exponent, just format as integer
        let mantissa: i64 = value as i64;
        alloc::format!("{{ {mantissa}, 10, 0 }}")
    }
}
