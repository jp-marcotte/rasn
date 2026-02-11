//! ASN.1 Value Notation (AVN) - ITU-T X.680
//!
//! AVN is the native human-readable notation for ASN.1 values as defined
//! in ITU-T X.680 Section 17. It provides a standardized text representation
//! for ASN.1 data structures.
//!
//! ## Format Summary
//!
//! | ASN.1 Type | AVN Format | Example |
//! |------------|------------|---------|
//! | BOOLEAN | `TRUE` / `FALSE` | `TRUE` |
//! | INTEGER | Decimal number | `42` |
//! | ENUMERATED | identifier | `active` |
//! | NULL | `NULL` | `NULL` |
//! | BIT STRING | `'...'H` (hex) or `'...'B` (binary) | `'1010'B`, `'AB'H` |
//! | OCTET STRING | `'...'H` | `'48656C6C6F'H` |
//! | OBJECT IDENTIFIER | `{ arc arc ... }` | `{ 1 2 840 113549 }` |
//! | REAL | `0`, `PLUS-INFINITY`, `MINUS-INFINITY`, `{ m, b, e }` | `{ 314, 10, -2 }` |
//! | Strings | `"..."` (doubled quotes for escaping) | `"Hello ""World"""` |
//! | SEQUENCE/SET | `{ field value, field value }` | `{ name "John", age 30 }` |
//! | SEQUENCE OF/SET OF | `{ value, value }` | `{ 1, 2, 3 }` |
//! | CHOICE | `identifier: value` | `request: { ... }` |
//!
//! ## Key Differences from GSER (RFC 3641)
//!
//! | Aspect | AVN (X.680) | GSER (RFC 3641) |
//! |--------|-------------|-----------------|
//! | OID format | `{ 1 2 840 }` (space-separated in braces) | `1.2.840` (dotted decimal) |
//! | BIT STRING | Both `'1010'B` and `'AB'H` | Only `'AB'H` |
//! | REAL | `{ mantissa, base, exponent }` tuple | `3.14E0` decimal |

pub mod de;
pub mod enc;

/// Attempts to decode `T` from `input` using AVN.
///
/// # Errors
/// Returns error specific to AVN decoder if decoding is not possible.
pub fn decode<T: crate::Decode>(input: &str) -> Result<T, crate::error::DecodeError> {
    T::decode(&mut de::Decoder::new(input)?)
}

/// Attempts to encode `value` to AVN.
///
/// # Errors
/// Returns error specific to AVN encoder if encoding is not possible.
pub fn encode<T: crate::Encode>(
    value: &T,
) -> Result<alloc::string::String, crate::error::EncodeError> {
    let mut encoder = enc::Encoder::new();
    value.encode(&mut encoder)?;
    Ok(encoder.to_string())
}

#[cfg(test)]
mod tests {
    macro_rules! round_trip_avn {
        ($typ:ty, $value:expr, $expected:expr) => {{
            let value: $typ = $value;
            pretty_assertions::assert_eq!(value, round_trip_value!($typ, $value, $expected));
        }};
    }

    macro_rules! round_trip_value {
        ($typ:ty, $value:expr, $expected:expr) => {{
            let value: $typ = $value;
            let expected: &'static str = $expected;
            let actual_encoding = crate::avn::encode(&value).unwrap();

            pretty_assertions::assert_eq!(expected, &*actual_encoding);

            let decoded_value: $typ = crate::avn::decode(&actual_encoding).unwrap();
            decoded_value
        }};
    }

    macro_rules! round_trip_string_type {
        ($typ:ty) => {{
            let string = alloc::string::String::from(" 1234567890");
            let expected: &'static str = "\" 1234567890\"";
            let value: $typ = string.try_into().unwrap();
            let actual_encoding = crate::avn::encode(&value).unwrap();

            pretty_assertions::assert_eq!(expected, &actual_encoding);

            let decoded_value: $typ = crate::avn::decode(&actual_encoding).unwrap();

            pretty_assertions::assert_eq!(value, decoded_value);
        }};
    }

    use crate::prelude::*;

    #[derive(AsnType, Decode, Encode, Debug, PartialEq)]
    #[rasn(automatic_tags)]
    #[rasn(crate_root = "crate")]
    #[non_exhaustive]
    struct TestTypeA {
        #[rasn(value("0..3", extensible))]
        juice: Integer,
        wine: Inner,
        #[rasn(extension_addition)]
        grappa: BitString,
    }

    #[derive(AsnType, Decode, Encode, Debug, PartialEq)]
    #[rasn(choice, automatic_tags)]
    #[rasn(crate_root = "crate")]
    enum Inner {
        #[rasn(value("0..3"))]
        Wine(u8),
    }

    #[derive(AsnType, Decode, Encode, Debug, Clone, Copy, PartialEq)]
    #[rasn(automatic_tags, enumerated)]
    #[rasn(crate_root = "crate")]
    enum SimpleEnum {
        Test1 = 5,
        Test2 = 2,
    }

    #[derive(AsnType, Decode, Encode, Debug, Clone, Copy, PartialEq)]
    #[rasn(automatic_tags, enumerated)]
    #[rasn(crate_root = "crate")]
    #[non_exhaustive]
    enum ExtEnum {
        Test1 = 5,
        Test2 = 2,
        #[rasn(extension_addition)]
        Test3 = -1,
    }

    #[derive(AsnType, Decode, Encode, Debug, Clone, PartialEq, Ord, Eq, PartialOrd, Hash)]
    #[rasn(automatic_tags, choice)]
    #[rasn(crate_root = "crate")]
    enum SimpleChoice {
        Test1(u8),
        #[rasn(size("0..3"))]
        Test2(Utf8String),
    }

    #[derive(AsnType, Decode, Encode, Debug, Clone, PartialEq)]
    #[rasn(automatic_tags, choice)]
    #[rasn(crate_root = "crate")]
    #[non_exhaustive]
    enum ExtChoice {
        Test1(u8),
        #[rasn(size("0..3"))]
        Test2(Utf8String),
        #[rasn(extension_addition)]
        Test3(bool),
    }

    #[derive(AsnType, Decode, Encode, Debug, PartialEq)]
    #[rasn(automatic_tags)]
    #[rasn(crate_root = "crate")]
    #[non_exhaustive]
    struct Very {
        #[rasn(extension_addition)]
        a: Option<Nested>,
    }

    #[derive(AsnType, Decode, Encode, Debug, PartialEq)]
    #[rasn(automatic_tags)]
    #[rasn(crate_root = "crate")]
    struct Nested {
        very: Option<Struct>,
        nested: Option<bool>,
    }

    #[derive(AsnType, Decode, Encode, Debug, PartialEq)]
    #[rasn(automatic_tags)]
    #[rasn(crate_root = "crate")]
    struct Struct {
        strct: Option<u8>,
    }

    #[derive(AsnType, Decode, Encode, Debug, PartialEq)]
    #[rasn(crate_root = "crate", delegate, size("3", extensible))]
    struct ConstrainedOctetString(pub OctetString);

    #[derive(AsnType, Decode, Encode, Debug, PartialEq)]
    #[rasn(crate_root = "crate", delegate, value("-5..=5", extensible))]
    struct ConstrainedInt(pub Integer);

    #[derive(AsnType, Decode, Encode, Debug, PartialEq)]
    #[rasn(crate_root = "crate", delegate, size("3"))]
    struct ConstrainedBitString(pub BitString);

    #[derive(AsnType, Decode, Encode, Debug, PartialEq)]
    #[rasn(automatic_tags)]
    #[rasn(crate_root = "crate")]
    struct Renamed {
        #[rasn(identifier = "so-very")]
        very: Integer,
        #[rasn(identifier = "re_named")]
        renamed: Option<bool>,
    }

    #[derive(AsnType, Decode, Encode, Debug, Clone, PartialEq)]
    #[rasn(automatic_tags, choice)]
    #[rasn(crate_root = "crate")]
    enum Renumed {
        #[rasn(identifier = "test-1")]
        #[rasn(size("0..3"))]
        Test1(Utf8String),
    }

    #[test]
    fn bool() {
        round_trip_avn!(bool, true, "TRUE");
        round_trip_avn!(bool, false, "FALSE");
    }

    #[test]
    fn integer() {
        round_trip_avn!(u8, 1, "1");
        round_trip_avn!(i8, -1, "-1");
        round_trip_avn!(u16, 0, "0");
        round_trip_avn!(i16, -14321, "-14321");
        round_trip_avn!(i64, -1_213_428_598_524_996_264, "-1213428598524996264");
        round_trip_avn!(Integer, 1.into(), "1");
        round_trip_avn!(Integer, (-1_235_352).into(), "-1235352");
        round_trip_avn!(ConstrainedInt, ConstrainedInt(1.into()), "1");
    }

    #[test]
    fn null() {
        round_trip_avn!((), (), "NULL");
    }

    #[test]
    #[cfg(feature = "f32")]
    fn real_f32() {
        // Zero is special-cased
        round_trip_avn!(f32, 0.0, "0");
        round_trip_avn!(f32, f32::INFINITY, "PLUS-INFINITY");
        round_trip_avn!(f32, f32::NEG_INFINITY, "MINUS-INFINITY");
        // Non-zero values use { mantissa, base, exponent } format
        let encoded = crate::avn::encode(&1.0f32).unwrap();
        assert!(encoded.starts_with("{ ") && encoded.ends_with(" }"));
        let decoded: f32 = crate::avn::decode(&encoded).unwrap();
        assert!((decoded - 1.0f32).abs() < f32::EPSILON);
    }

    #[test]
    #[cfg(feature = "f64")]
    fn real_f64() {
        round_trip_avn!(f64, 0.0, "0");
        round_trip_avn!(f64, f64::INFINITY, "PLUS-INFINITY");
        round_trip_avn!(f64, f64::NEG_INFINITY, "MINUS-INFINITY");
        // Non-zero values use { mantissa, base, exponent } format
        let encoded = crate::avn::encode(&3.14f64).unwrap();
        assert!(encoded.starts_with("{ ") && encoded.ends_with(" }"));
        let decoded: f64 = crate::avn::decode(&encoded).unwrap();
        assert!((decoded - 3.14f64).abs() < 0.001);
    }

    #[test]
    fn bit_string() {
        // X.680 §22.18: hstring when multiple of 4 bits, bstring otherwise
        round_trip_avn!(
            BitString,
            [true, false, false, false, false, false, false, false]
                .into_iter()
                .collect::<BitString>(),
            "'80'H"
        );
        round_trip_avn!(
            ConstrainedBitString,
            ConstrainedBitString(
                [true, false, true, false, false, false, false, false]
                    .into_iter()
                    .collect::<BitString>()
            ),
            "'A0'H"
        );
        // 5 bits - not multiple of 4, uses binary format
        round_trip_avn!(
            BitString,
            [true, false, true, false, true]
                .into_iter()
                .collect::<BitString>(),
            "'10101'B"
        );
        // 3 bits - not multiple of 4, uses binary format
        round_trip_avn!(
            BitString,
            [true, true, false].into_iter().collect::<BitString>(),
            "'110'B"
        );
    }

    #[test]
    fn octet_string() {
        round_trip_avn!(OctetString, OctetString::from_static(&[1, 255]), "'01FF'H");
        round_trip_avn!(
            ConstrainedOctetString,
            ConstrainedOctetString(OctetString::from_static(&[1, 255, 0, 254])),
            "'01FF00FE'H"
        );
    }

    #[test]
    fn object_identifier() {
        // AVN OID format uses space-separated arcs in braces
        round_trip_avn!(
            ObjectIdentifier,
            ObjectIdentifier::from(Oid::JOINT_ISO_ITU_T_DS_NAME_FORM),
            "{ 2 5 15 }"
        );
    }

    #[test]
    fn string_types() {
        round_trip_string_type!(NumericString);
        round_trip_string_type!(GeneralString);
        round_trip_string_type!(VisibleString);
        round_trip_string_type!(PrintableString);
        round_trip_string_type!(Ia5String);
        round_trip_string_type!(Utf8String);
    }

    #[test]
    fn string_escaping() {
        // Test quote doubling escape mechanism
        let value = Utf8String::from("Hello \"World\"");
        let encoded = crate::avn::encode(&value).unwrap();
        assert_eq!(encoded, "\"Hello \"\"World\"\"\"");
        let decoded: Utf8String = crate::avn::decode(&encoded).unwrap();
        assert_eq!(value, decoded);
    }

    #[test]
    fn enumerated() {
        round_trip_avn!(SimpleEnum, SimpleEnum::Test1, "Test1");
        round_trip_avn!(SimpleEnum, SimpleEnum::Test2, "Test2");
        round_trip_avn!(ExtEnum, ExtEnum::Test1, "Test1");
        round_trip_avn!(ExtEnum, ExtEnum::Test2, "Test2");
        round_trip_avn!(ExtEnum, ExtEnum::Test3, "Test3");
    }

    #[test]
    fn choice() {
        round_trip_avn!(SimpleChoice, SimpleChoice::Test1(3), "Test1: 3");
        round_trip_avn!(
            SimpleChoice,
            SimpleChoice::Test2("foo".into()),
            "Test2: \"foo\""
        );
        round_trip_avn!(ExtChoice, ExtChoice::Test1(255), "Test1: 255");
        round_trip_avn!(ExtChoice, ExtChoice::Test2("bar".into()), "Test2: \"bar\"");
        round_trip_avn!(ExtChoice, ExtChoice::Test3(true), "Test3: TRUE");
    }

    #[test]
    fn sequence_of() {
        round_trip_avn!(
            SequenceOf<SimpleChoice>,
            alloc::vec![SimpleChoice::Test1(3)],
            "{ Test1: 3 }"
        );
        round_trip_avn!(
            SequenceOf<u8>,
            alloc::vec![1, 2, 3, 4, 5, 5, 3],
            "{ 1, 2, 3, 4, 5, 5, 3 }"
        );
        round_trip_avn!(SequenceOf<bool>, alloc::vec![], "{ }");
    }

    #[test]
    fn set_of() {
        round_trip_avn!(
            SetOf<SimpleChoice>,
            SetOf::from_vec(alloc::vec![SimpleChoice::Test1(3)]),
            "{ Test1: 3 }"
        );
        round_trip_avn!(SetOf<bool>, SetOf::from_vec(alloc::vec![]), "{ }");
    }

    #[test]
    fn sequence() {
        // Note: AVN hex format doesn't preserve non-byte-aligned bit counts,
        // so we use a byte-aligned bit string (8 bits = 1 byte)
        round_trip_avn!(
            TestTypeA,
            TestTypeA {
                juice: 0.into(),
                wine: Inner::Wine(4),
                grappa: [true, false, false, false, false, false, false, false]
                    .iter()
                    .collect::<BitString>()
            },
            "{ juice 0, wine Wine: 4, grappa '80'H }"
        );
        round_trip_avn!(
            Very,
            Very {
                a: Some(Nested {
                    very: Some(Struct { strct: None }),
                    nested: Some(false)
                })
            },
            "{ a { very { }, nested FALSE } }"
        );
    }

    #[test]
    fn with_identifier_annotation() {
        round_trip_avn!(
            Renamed,
            Renamed {
                very: 1.into(),
                renamed: Some(true),
            },
            "{ so-very 1, re_named TRUE }"
        );

        round_trip_avn!(Renumed, Renumed::Test1("hel".into()), "test-1: \"hel\"");
    }

    // Test OID format comparison with GSER
    #[test]
    fn avn_oid_format_differs_from_gser() {
        let oid = ObjectIdentifier::from(Oid::JOINT_ISO_ITU_T_DS_NAME_FORM);

        // AVN uses { arc arc arc } format
        let avn_encoded = crate::avn::encode(&oid).unwrap();
        assert_eq!(avn_encoded, "{ 2 5 15 }");

        // GSER uses dotted decimal format
        let gser_encoded = crate::gser::encode(&oid).unwrap();
        assert_eq!(gser_encoded, "2.5.15");

        // Both should decode to the same value
        let avn_decoded: ObjectIdentifier = crate::avn::decode(&avn_encoded).unwrap();
        let gser_decoded: ObjectIdentifier = crate::gser::decode(&gser_encoded).unwrap();
        assert_eq!(avn_decoded, gser_decoded);
    }

    // Test for TCA Profile-like nested structures
    #[derive(AsnType, Decode, Encode, Debug, PartialEq)]
    #[rasn(automatic_tags, choice)]
    #[rasn(crate_root = "crate")]
    enum ProfileElement {
        Header(ProfileHeader),
        SecurityDomain(SecurityDomain),
    }

    #[derive(AsnType, Decode, Encode, Debug, PartialEq)]
    #[rasn(automatic_tags)]
    #[rasn(crate_root = "crate")]
    struct ProfileHeader {
        #[rasn(identifier = "major-version")]
        major_version: u8,
        #[rasn(identifier = "minor-version")]
        minor_version: u8,
        #[rasn(identifier = "profileType")]
        profile_type: Utf8String,
        iccid: OctetString,
    }

    #[derive(AsnType, Decode, Encode, Debug, PartialEq)]
    #[rasn(automatic_tags)]
    #[rasn(crate_root = "crate")]
    struct SecurityDomain {
        #[rasn(identifier = "sd-aid")]
        sd_aid: OctetString,
        #[rasn(identifier = "key-list")]
        key_list: SequenceOf<KeyInfo>,
    }

    #[derive(AsnType, Decode, Encode, Debug, PartialEq)]
    #[rasn(automatic_tags)]
    #[rasn(crate_root = "crate")]
    struct KeyInfo {
        #[rasn(identifier = "keyIdentifier")]
        key_identifier: u8,
        #[rasn(identifier = "keyData")]
        key_data: OctetString,
    }

    #[test]
    fn tca_profile_header() {
        let header = ProfileElement::Header(ProfileHeader {
            major_version: 3,
            minor_version: 4,
            profile_type: "TCA Sample".into(),
            iccid: OctetString::from_static(&[0x89, 0x01, 0x99, 0x90, 0x00]),
        });

        let encoded = crate::avn::encode(&header).unwrap();
        let decoded: ProfileElement = crate::avn::decode(&encoded).unwrap();
        assert_eq!(header, decoded);
    }

    #[test]
    fn tca_security_domain() {
        let sd = ProfileElement::SecurityDomain(SecurityDomain {
            sd_aid: OctetString::from_static(&[0xA0, 0x00, 0x00, 0x01, 0x51]),
            key_list: alloc::vec![
                KeyInfo {
                    key_identifier: 1,
                    key_data: OctetString::from_static(&[0x11, 0x22, 0x33, 0x44]),
                },
                KeyInfo {
                    key_identifier: 2,
                    key_data: OctetString::from_static(&[0xAA, 0xBB, 0xCC, 0xDD]),
                },
            ],
        });

        let encoded = crate::avn::encode(&sd).unwrap();
        let decoded: ProfileElement = crate::avn::decode(&encoded).unwrap();
        assert_eq!(sd, decoded);
    }

    // Test sequence of OIDs (common in eSIM profiles)
    #[test]
    fn sequence_of_oids() {
        let oids: SequenceOf<ObjectIdentifier> = alloc::vec![
            ObjectIdentifier::new(alloc::vec![2, 23, 143, 1, 2, 1]).unwrap(),
            ObjectIdentifier::new(alloc::vec![2, 23, 143, 1, 2, 4, 2]).unwrap(),
        ];

        let encoded = crate::avn::encode(&oids).unwrap();
        // Should be: { { 2 23 143 1 2 1 }, { 2 23 143 1 2 4 2 } }
        assert!(encoded.contains("{ 2 23 143 1 2 1 }"));
        assert!(encoded.contains("{ 2 23 143 1 2 4 2 }"));

        let decoded: SequenceOf<ObjectIdentifier> = crate::avn::decode(&encoded).unwrap();
        assert_eq!(oids, decoded);
    }

    // ========================================================================
    // Error handling tests - exercise decoder error paths
    // ========================================================================

    #[test]
    fn avn_invalid_enum_discriminant() {
        let result: Result<SimpleEnum, _> = crate::avn::decode("InvalidVariant");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            alloc::format!("{err:?}").contains("InvalidEnumDiscriminant")
                || alloc::format!("{err:?}").contains("InvalidVariant")
        );
    }

    #[test]
    fn avn_invalid_choice_variant() {
        let result: Result<SimpleChoice, _> = crate::avn::decode("UnknownVariant: 42");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            alloc::format!("{err:?}").contains("InvalidChoiceVariant")
                || alloc::format!("{err:?}").contains("UnknownVariant")
        );
    }

    #[test]
    fn avn_type_mismatch_boolean() {
        let result: Result<bool, _> = crate::avn::decode("MAYBE");
        assert!(result.is_err());
    }

    #[test]
    fn avn_type_mismatch_integer() {
        let result: Result<i32, _> = crate::avn::decode("not_a_number");
        assert!(result.is_err());
    }

    #[test]
    fn avn_invalid_hex_string() {
        // GHIJ are not valid hex characters
        let result: Result<OctetString, _> = crate::avn::decode("'GHIJ'H");
        assert!(result.is_err());
    }

    #[test]
    fn avn_invalid_hex_string_odd_length() {
        // Odd-length hex string is invalid
        let result: Result<OctetString, _> = crate::avn::decode("'ABC'H");
        assert!(result.is_err());
    }

    #[test]
    fn avn_invalid_bit_string_binary() {
        // '2' is not a valid binary digit
        let result: Result<BitString, _> = crate::avn::decode("'1012'B");
        assert!(result.is_err());
    }

    #[test]
    fn avn_malformed_sequence() {
        // Incomplete sequence - missing closing brace
        let result: Result<TestTypeA, _> = crate::avn::decode("{ juice 0, wine");
        assert!(result.is_err());
    }

    #[test]
    fn avn_invalid_null() {
        let result: Result<(), _> = crate::avn::decode("NOTHING");
        assert!(result.is_err());
    }

    #[test]
    fn avn_invalid_oid_format() {
        // Invalid OID format - not in braces
        let result: Result<ObjectIdentifier, _> = crate::avn::decode("1.2.3.4");
        assert!(result.is_err());
    }

    #[test]
    fn avn_invalid_oid_content() {
        // Invalid OID content - not numeric arcs
        let result: Result<ObjectIdentifier, _> = crate::avn::decode("{ not arcs }");
        assert!(result.is_err());
    }

    #[test]
    #[cfg(feature = "f64")]
    fn avn_invalid_real_tuple() {
        // Missing exponent in tuple format
        let result: Result<f64, _> = crate::avn::decode("{ 314, 10 }");
        assert!(result.is_err());
    }

    #[test]
    #[cfg(feature = "f64")]
    fn avn_invalid_real() {
        let result: Result<f64, _> = crate::avn::decode("not-a-real-number");
        assert!(result.is_err());
    }

    // ========================================================================
    // BIT STRING edge cases
    // ========================================================================

    #[test]
    fn avn_bit_string_empty() {
        round_trip_avn!(BitString, BitString::new(), "''H");
    }

    #[test]
    fn avn_bit_string_uicc_capability_pattern() {
        // TCA UICCCapability pattern - bits 0-3 set, represented as 'F0'H
        round_trip_avn!(
            BitString,
            [true, true, true, true, false, false, false, false]
                .into_iter()
                .collect::<BitString>(),
            "'F0'H"
        );
    }

    #[test]
    fn avn_bit_string_non_aligned_lengths() {
        // Non-aligned lengths must use binary format to preserve exact bit count
        for len in [1, 2, 3, 5, 6, 7, 9, 10, 11, 13, 14, 15] {
            let bits: BitString = (0..len).map(|i| i % 2 == 0).collect();
            let encoded = crate::avn::encode(&bits).unwrap();
            // Non-aligned should use binary format
            assert!(
                encoded.ends_with("'B"),
                "Length {len} should use binary format, got: {encoded}"
            );
            let decoded: BitString = crate::avn::decode(&encoded).unwrap();
            assert_eq!(bits, decoded, "Round-trip failed for length {len}");
        }
    }

    #[test]
    fn avn_bit_string_aligned_lengths() {
        // Aligned lengths (multiple of 8 = byte-aligned) should use hex format and round-trip
        for len in [8, 16, 24, 32] {
            let bits: BitString = (0..len).map(|i| i % 2 == 0).collect();
            let encoded = crate::avn::encode(&bits).unwrap();
            // Byte-aligned should use hex format
            assert!(
                encoded.ends_with("'H"),
                "Length {len} should use hex format, got: {encoded}"
            );
            let decoded: BitString = crate::avn::decode(&encoded).unwrap();
            assert_eq!(bits, decoded, "Round-trip failed for length {len}");
        }
    }

    #[test]
    fn avn_bit_string_long() {
        // 64 bits - tests longer bit strings
        let bits: BitString = (0..64).map(|i| i % 3 == 0).collect();
        let encoded = crate::avn::encode(&bits).unwrap();
        let decoded: BitString = crate::avn::decode(&encoded).unwrap();
        assert_eq!(bits, decoded);
    }

    // ========================================================================
    // OID coverage tests (AVN uses { arc arc } format)
    // ========================================================================

    #[test]
    fn avn_oid_tca_profile() {
        // TCA OID: 2.23.143.1.2.1 (GlobalPlatform eSIM)
        round_trip_avn!(
            ObjectIdentifier,
            ObjectIdentifier::new_unchecked(alloc::vec![2, 23, 143, 1, 2, 1].into()),
            "{ 2 23 143 1 2 1 }"
        );
    }

    #[test]
    fn avn_oid_itu_arc() {
        // ITU-T arc (starts with 0): 0.4.0.127.0.7
        round_trip_avn!(
            ObjectIdentifier,
            ObjectIdentifier::new_unchecked(alloc::vec![0, 4, 0, 127, 0, 7].into()),
            "{ 0 4 0 127 0 7 }"
        );
    }

    #[test]
    fn avn_oid_large_arcs() {
        // SHA256 OID with large arc numbers: 2.16.840.1.101.3.4.2.1
        round_trip_avn!(
            ObjectIdentifier,
            ObjectIdentifier::new_unchecked(alloc::vec![2, 16, 840, 1, 101, 3, 4, 2, 1].into()),
            "{ 2 16 840 1 101 3 4 2 1 }"
        );
    }

    #[test]
    fn avn_oid_iso_arc() {
        // ISO arc (starts with 1): 1.2.840.113549.1.1.11 (sha256WithRSAEncryption)
        round_trip_avn!(
            ObjectIdentifier,
            ObjectIdentifier::new_unchecked(alloc::vec![1, 2, 840, 113549, 1, 1, 11].into()),
            "{ 1 2 840 113549 1 1 11 }"
        );
    }

    // ========================================================================
    // OCTET STRING variety tests
    // ========================================================================

    #[test]
    fn avn_octet_string_empty() {
        round_trip_avn!(OctetString, OctetString::from_static(&[]), "''H");
    }

    #[test]
    fn avn_octet_string_iccid() {
        // ICCID format (10 bytes) - common in eSIM profiles
        round_trip_avn!(
            OctetString,
            OctetString::from_static(&[0x89, 0x01, 0x99, 0x90, 0x00, 0x00, 0x00, 0x12, 0x34, 0x5F]),
            "'8901999000000012345F'H"
        );
    }

    #[test]
    fn avn_octet_string_aid() {
        // AID format (16 bytes) - Application Identifier
        round_trip_avn!(
            OctetString,
            OctetString::from_static(&[
                0xA0, 0x00, 0x00, 0x05, 0x59, 0x10, 0x10, 0xFF, 0xFF, 0xFF, 0xFF, 0x89, 0x00, 0x00,
                0x01, 0x00
            ]),
            "'A0000005591010FFFFFFFF8900000100'H"
        );
    }

    #[test]
    fn avn_octet_string_key_sizes() {
        // Test common cryptographic key sizes
        for size in [16, 24, 32] {
            let data: alloc::vec::Vec<u8> = (0..size).map(|i| (i * 17) as u8).collect();
            let octet = OctetString::from(data);
            let encoded = crate::avn::encode(&octet).unwrap();
            let decoded: OctetString = crate::avn::decode(&encoded).unwrap();
            assert_eq!(octet, decoded, "Round-trip failed for size {size}");
        }
    }

    // ========================================================================
    // TCA Profile structure tests - realistic eSIM profile structures
    // ========================================================================

    #[derive(AsnType, Decode, Encode, Debug, PartialEq)]
    #[rasn(automatic_tags)]
    #[rasn(crate_root = "crate")]
    struct FullProfileHeader {
        #[rasn(identifier = "major-version")]
        major_version: u8,
        #[rasn(identifier = "minor-version")]
        minor_version: u8,
        #[rasn(identifier = "profileType")]
        profile_type: Option<Utf8String>,
        iccid: OctetString,
        #[rasn(identifier = "eUICC-Mandatory-GFSTEList")]
        euicc_mandatory_gfste_list: SequenceOf<ObjectIdentifier>,
    }

    #[derive(AsnType, Decode, Encode, Debug, PartialEq)]
    #[rasn(automatic_tags)]
    #[rasn(crate_root = "crate")]
    struct KeyComponent {
        #[rasn(identifier = "keyType")]
        key_type: u8,
        #[rasn(identifier = "keyData")]
        key_data: OctetString,
    }

    #[derive(AsnType, Decode, Encode, Debug, PartialEq)]
    #[rasn(automatic_tags)]
    #[rasn(crate_root = "crate")]
    struct FullKeyInfo {
        #[rasn(identifier = "keyIdentifier")]
        key_identifier: u8,
        #[rasn(identifier = "keyVersionNumber")]
        key_version_number: u8,
        #[rasn(identifier = "keyComponents")]
        key_components: SequenceOf<KeyComponent>,
    }

    #[test]
    fn avn_full_profile_header_round_trip() {
        let header = FullProfileHeader {
            major_version: 2,
            minor_version: 3,
            profile_type: Some("telecom".into()),
            iccid: OctetString::from_static(&[
                0x89, 0x01, 0x99, 0x90, 0x00, 0x00, 0x00, 0x12, 0x34, 0x5F,
            ]),
            euicc_mandatory_gfste_list: alloc::vec![ObjectIdentifier::new_unchecked(
                alloc::vec![2, 23, 143, 1, 2, 1].into()
            ),],
        };
        let encoded = crate::avn::encode(&header).unwrap();
        let decoded: FullProfileHeader = crate::avn::decode(&encoded).unwrap();
        assert_eq!(header, decoded);
    }

    #[test]
    fn avn_full_profile_header_no_optional() {
        let header = FullProfileHeader {
            major_version: 1,
            minor_version: 0,
            profile_type: None,
            iccid: OctetString::from_static(&[0x89, 0x00, 0x00, 0x00, 0x00]),
            euicc_mandatory_gfste_list: alloc::vec![],
        };
        let encoded = crate::avn::encode(&header).unwrap();
        let decoded: FullProfileHeader = crate::avn::decode(&encoded).unwrap();
        assert_eq!(header, decoded);
    }

    #[test]
    fn avn_security_domain_multiple_keys() {
        let keys: SequenceOf<FullKeyInfo> = alloc::vec![
            FullKeyInfo {
                key_identifier: 1,
                key_version_number: 1,
                key_components: alloc::vec![
                    KeyComponent {
                        key_type: 0x88,
                        key_data: OctetString::from_static(&[0x11; 16])
                    },
                    KeyComponent {
                        key_type: 0x88,
                        key_data: OctetString::from_static(&[0x22; 16])
                    },
                ],
            },
            FullKeyInfo {
                key_identifier: 2,
                key_version_number: 1,
                key_components: alloc::vec![KeyComponent {
                    key_type: 0x88,
                    key_data: OctetString::from_static(&[0xAA; 16])
                },],
            },
        ];
        let encoded = crate::avn::encode(&keys).unwrap();
        let decoded: SequenceOf<FullKeyInfo> = crate::avn::decode(&encoded).unwrap();
        assert_eq!(keys, decoded);
    }

    #[test]
    fn avn_deeply_nested_optional() {
        // Test deeply nested optional fields
        let nested = Very {
            a: Some(Nested {
                very: Some(Struct { strct: Some(42) }),
                nested: None,
            }),
        };
        let encoded = crate::avn::encode(&nested).unwrap();
        let decoded: Very = crate::avn::decode(&encoded).unwrap();
        assert_eq!(nested, decoded);
    }

    #[test]
    fn avn_empty_nested() {
        // All optional fields absent
        let nested = Very { a: None };
        let encoded = crate::avn::encode(&nested).unwrap();
        let decoded: Very = crate::avn::decode(&encoded).unwrap();
        assert_eq!(nested, decoded);
    }

    // ========================================================================
    // Time type tests
    // ========================================================================

    #[test]
    fn avn_utc_time_round_trip() {
        use chrono::{TimeZone, Utc};
        let time: UtcTime = Utc.with_ymd_and_hms(2025, 6, 15, 12, 30, 45).unwrap();
        let encoded = crate::avn::encode(&time).unwrap();
        let decoded: UtcTime = crate::avn::decode(&encoded).unwrap();
        assert_eq!(time, decoded);
    }

    #[test]
    fn avn_generalized_time_round_trip() {
        use chrono::{FixedOffset, TimeZone};
        let time: GeneralizedTime = FixedOffset::east_opt(0)
            .unwrap()
            .with_ymd_and_hms(2025, 12, 31, 23, 59, 59)
            .unwrap();
        let encoded = crate::avn::encode(&time).unwrap();
        let decoded: GeneralizedTime = crate::avn::decode(&encoded).unwrap();
        assert_eq!(time, decoded);
    }

    #[test]
    fn avn_utc_time_year_2000() {
        use chrono::{TimeZone, Utc};
        // Test Y2K boundary
        let time: UtcTime = Utc.with_ymd_and_hms(2000, 1, 1, 0, 0, 0).unwrap();
        let encoded = crate::avn::encode(&time).unwrap();
        let decoded: UtcTime = crate::avn::decode(&encoded).unwrap();
        assert_eq!(time, decoded);
    }

    #[test]
    fn avn_generalized_time_with_offset() {
        use chrono::{FixedOffset, TimeZone};
        // Test with non-UTC offset
        let offset = FixedOffset::east_opt(5 * 3600).unwrap(); // UTC+5
        let time: GeneralizedTime = offset.with_ymd_and_hms(2025, 7, 4, 14, 30, 0).unwrap();
        let encoded = crate::avn::encode(&time).unwrap();
        let decoded: GeneralizedTime = crate::avn::decode(&encoded).unwrap();
        assert_eq!(time, decoded);
    }
}
