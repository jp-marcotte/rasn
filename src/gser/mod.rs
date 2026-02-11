//! Generic String Encoding Rules (GSER) - RFC 3641
//!
//! GSER provides a human-readable text representation of ASN.1 values.
//!
//! ## Format Summary
//!
//! | ASN.1 Type | GSER Format | Example |
//! |------------|-------------|---------|
//! | BOOLEAN | `TRUE` / `FALSE` | `TRUE` |
//! | INTEGER | Decimal number | `42` |
//! | ENUMERATED | identifier | `active` |
//! | NULL | `NULL` | `NULL` |
//! | BIT STRING | `'...'H` (hex) or `'...'B` (binary) | `'AB'H` |
//! | OCTET STRING | `'...'H` | `'48656C6C6F'H` |
//! | OBJECT IDENTIFIER | dotted decimal | `1.2.840.113549` |
//! | REAL | `0`, `PLUS-INFINITY`, `MINUS-INFINITY`, or decimal | `3.14` |
//! | Strings | `"..."` (doubled quotes for escaping) | `"Hello ""World"""` |
//! | SEQUENCE/SET | `{ field value, field value }` | `{ name "John", age 30 }` |
//! | SEQUENCE OF/SET OF | `{ value, value }` | `{ 1, 2, 3 }` |
//! | CHOICE | `identifier: value` | `request: { ... }` |

pub mod de;
pub mod enc;

/// Attempts to decode `T` from `input` using GSER.
///
/// # Errors
/// Returns error specific to GSER decoder if decoding is not possible.
pub fn decode<T: crate::Decode>(input: &str) -> Result<T, crate::error::DecodeError> {
    T::decode(&mut de::Decoder::new(input)?)
}

/// Attempts to encode `value` to GSER.
///
/// # Errors
/// Returns error specific to GSER encoder if encoding is not possible.
pub fn encode<T: crate::Encode>(
    value: &T,
) -> Result<alloc::string::String, crate::error::EncodeError> {
    let mut encoder = enc::Encoder::new();
    value.encode(&mut encoder)?;
    Ok(encoder.to_string())
}

#[cfg(test)]
mod tests {
    macro_rules! round_trip_gser {
        ($typ:ty, $value:expr, $expected:expr) => {{
            let value: $typ = $value;
            pretty_assertions::assert_eq!(value, round_trip_value!($typ, $value, $expected));
        }};
    }

    macro_rules! round_trip_value {
        ($typ:ty, $value:expr, $expected:expr) => {{
            let value: $typ = $value;
            let expected: &'static str = $expected;
            let actual_encoding = crate::gser::encode(&value).unwrap();

            pretty_assertions::assert_eq!(expected, &*actual_encoding);

            let decoded_value: $typ = crate::gser::decode(&actual_encoding).unwrap();
            decoded_value
        }};
    }

    macro_rules! round_trip_string_type {
        ($typ:ty) => {{
            let string = alloc::string::String::from(" 1234567890");
            let expected: &'static str = "\" 1234567890\"";
            let value: $typ = string.try_into().unwrap();
            let actual_encoding = crate::gser::encode(&value).unwrap();

            pretty_assertions::assert_eq!(expected, &actual_encoding);

            let decoded_value: $typ = crate::gser::decode(&actual_encoding).unwrap();

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
        round_trip_gser!(bool, true, "TRUE");
        round_trip_gser!(bool, false, "FALSE");
    }

    #[test]
    fn integer() {
        round_trip_gser!(u8, 1, "1");
        round_trip_gser!(i8, -1, "-1");
        round_trip_gser!(u16, 0, "0");
        round_trip_gser!(i16, -14321, "-14321");
        round_trip_gser!(i64, -1_213_428_598_524_996_264, "-1213428598524996264");
        round_trip_gser!(Integer, 1.into(), "1");
        round_trip_gser!(Integer, (-1_235_352).into(), "-1235352");
        round_trip_gser!(ConstrainedInt, ConstrainedInt(1.into()), "1");
    }

    #[test]
    fn null() {
        round_trip_gser!((), (), "NULL");
    }

    #[test]
    #[cfg(feature = "f32")]
    fn real_f32() {
        round_trip_gser!(f32, 0.0, "0");
        round_trip_gser!(f32, f32::INFINITY, "PLUS-INFINITY");
        round_trip_gser!(f32, f32::NEG_INFINITY, "MINUS-INFINITY");
        round_trip_gser!(f32, 1.0, "1E0");
        round_trip_gser!(f32, -1.0, "-1E0");
    }

    #[test]
    #[cfg(feature = "f64")]
    fn real_f64() {
        round_trip_gser!(f64, 0.0, "0");
        round_trip_gser!(f64, f64::INFINITY, "PLUS-INFINITY");
        round_trip_gser!(f64, f64::NEG_INFINITY, "MINUS-INFINITY");
        round_trip_gser!(f64, 1.0, "1E0");
        round_trip_gser!(f64, -1.0, "-1E0");
    }

    #[test]
    fn bit_string() {
        // RFC 3641 §3.5: Use hstring when multiple of 4 bits, bstring otherwise
        round_trip_gser!(
            BitString,
            [true, false, false, false, false, false, false, false]
                .into_iter()
                .collect::<BitString>(),
            "'80'H"
        );
        round_trip_gser!(
            ConstrainedBitString,
            ConstrainedBitString(
                [true, false, true, false, false, false, false, false]
                    .into_iter()
                    .collect::<BitString>()
            ),
            "'A0'H"
        );
        // 5 bits - not multiple of 4, uses binary format
        round_trip_gser!(
            BitString,
            [true, false, true, false, true]
                .into_iter()
                .collect::<BitString>(),
            "'10101'B"
        );
        // 3 bits - not multiple of 4, uses binary format
        round_trip_gser!(
            BitString,
            [true, true, false].into_iter().collect::<BitString>(),
            "'110'B"
        );
    }

    #[test]
    fn octet_string() {
        round_trip_gser!(OctetString, OctetString::from_static(&[1, 255]), "'01FF'H");
        round_trip_gser!(
            ConstrainedOctetString,
            ConstrainedOctetString(OctetString::from_static(&[1, 255, 0, 254])),
            "'01FF00FE'H"
        );
    }

    #[test]
    fn object_identifier() {
        round_trip_gser!(
            ObjectIdentifier,
            ObjectIdentifier::from(Oid::JOINT_ISO_ITU_T_DS_NAME_FORM),
            "2.5.15"
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
        let encoded = crate::gser::encode(&value).unwrap();
        assert_eq!(encoded, "\"Hello \"\"World\"\"\"");
        let decoded: Utf8String = crate::gser::decode(&encoded).unwrap();
        assert_eq!(value, decoded);
    }

    #[test]
    fn enumerated() {
        round_trip_gser!(SimpleEnum, SimpleEnum::Test1, "Test1");
        round_trip_gser!(SimpleEnum, SimpleEnum::Test2, "Test2");
        round_trip_gser!(ExtEnum, ExtEnum::Test1, "Test1");
        round_trip_gser!(ExtEnum, ExtEnum::Test2, "Test2");
        round_trip_gser!(ExtEnum, ExtEnum::Test3, "Test3");
    }

    #[test]
    fn choice() {
        round_trip_gser!(SimpleChoice, SimpleChoice::Test1(3), "Test1: 3");
        round_trip_gser!(
            SimpleChoice,
            SimpleChoice::Test2("foo".into()),
            "Test2: \"foo\""
        );
        round_trip_gser!(ExtChoice, ExtChoice::Test1(255), "Test1: 255");
        round_trip_gser!(ExtChoice, ExtChoice::Test2("bar".into()), "Test2: \"bar\"");
        round_trip_gser!(ExtChoice, ExtChoice::Test3(true), "Test3: TRUE");
    }

    #[test]
    fn sequence_of() {
        round_trip_gser!(
            SequenceOf<SimpleChoice>,
            alloc::vec![SimpleChoice::Test1(3)],
            "{ Test1: 3 }"
        );
        round_trip_gser!(
            SequenceOf<u8>,
            alloc::vec![1, 2, 3, 4, 5, 5, 3],
            "{ 1, 2, 3, 4, 5, 5, 3 }"
        );
        round_trip_gser!(SequenceOf<bool>, alloc::vec![], "{ }");
    }

    #[test]
    fn set_of() {
        round_trip_gser!(
            SetOf<SimpleChoice>,
            SetOf::from_vec(alloc::vec![SimpleChoice::Test1(3)]),
            "{ Test1: 3 }"
        );
        round_trip_gser!(SetOf<bool>, SetOf::from_vec(alloc::vec![]), "{ }");
    }

    #[test]
    fn sequence() {
        // Note: GSER hex format doesn't preserve non-byte-aligned bit counts,
        // so we use a byte-aligned bit string (8 bits = 1 byte)
        round_trip_gser!(
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
        round_trip_gser!(
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
        round_trip_gser!(
            Renamed,
            Renamed {
                very: 1.into(),
                renamed: Some(true),
            },
            "{ so-very 1, re_named TRUE }"
        );

        round_trip_gser!(Renumed, Renumed::Test1("hel".into()), "test-1: \"hel\"");
    }

    // ========================================================================
    // Error handling tests - exercise decoder error paths
    // ========================================================================

    #[test]
    fn gser_invalid_enum_discriminant() {
        let result: Result<SimpleEnum, _> = crate::gser::decode("InvalidVariant");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            alloc::format!("{err:?}").contains("InvalidEnumDiscriminant")
                || alloc::format!("{err:?}").contains("InvalidVariant")
        );
    }

    #[test]
    fn gser_invalid_choice_variant() {
        let result: Result<SimpleChoice, _> = crate::gser::decode("UnknownVariant: 42");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            alloc::format!("{err:?}").contains("InvalidChoiceVariant")
                || alloc::format!("{err:?}").contains("UnknownVariant")
        );
    }

    #[test]
    fn gser_type_mismatch_boolean() {
        let result: Result<bool, _> = crate::gser::decode("MAYBE");
        assert!(result.is_err());
    }

    #[test]
    fn gser_type_mismatch_integer() {
        let result: Result<i32, _> = crate::gser::decode("not_a_number");
        assert!(result.is_err());
    }

    #[test]
    fn gser_invalid_hex_string() {
        // GHIJ are not valid hex characters
        let result: Result<OctetString, _> = crate::gser::decode("'GHIJ'H");
        assert!(result.is_err());
    }

    #[test]
    fn gser_invalid_hex_string_odd_length() {
        // Odd-length hex string is invalid
        let result: Result<OctetString, _> = crate::gser::decode("'ABC'H");
        assert!(result.is_err());
    }

    #[test]
    fn gser_invalid_bit_string_binary() {
        // '2' is not a valid binary digit
        let result: Result<BitString, _> = crate::gser::decode("'1012'B");
        assert!(result.is_err());
    }

    #[test]
    fn gser_malformed_sequence() {
        // Incomplete sequence - missing closing brace
        let result: Result<TestTypeA, _> = crate::gser::decode("{ juice 0, wine");
        assert!(result.is_err());
    }

    #[test]
    fn gser_invalid_null() {
        let result: Result<(), _> = crate::gser::decode("NOTHING");
        assert!(result.is_err());
    }

    #[test]
    fn gser_invalid_oid() {
        // Invalid OID format
        let result: Result<ObjectIdentifier, _> = crate::gser::decode("not.an.oid.at.all");
        assert!(result.is_err());
    }

    #[test]
    #[cfg(feature = "f64")]
    fn gser_invalid_real() {
        let result: Result<f64, _> = crate::gser::decode("not-a-real-number");
        assert!(result.is_err());
    }

    // ========================================================================
    // BIT STRING edge cases
    // ========================================================================

    #[test]
    fn gser_bit_string_empty() {
        round_trip_gser!(BitString, BitString::new(), "''H");
    }

    #[test]
    fn gser_bit_string_uicc_capability_pattern() {
        // TCA UICCCapability pattern - bits 0-3 set, represented as 'F0'H
        round_trip_gser!(
            BitString,
            [true, true, true, true, false, false, false, false]
                .into_iter()
                .collect::<BitString>(),
            "'F0'H"
        );
    }

    #[test]
    fn gser_bit_string_non_aligned_lengths() {
        // Non-aligned lengths must use binary format to preserve exact bit count
        for len in [1, 2, 3, 5, 6, 7, 9, 10, 11, 13, 14, 15] {
            let bits: BitString = (0..len).map(|i| i % 2 == 0).collect();
            let encoded = crate::gser::encode(&bits).unwrap();
            // Non-aligned should use binary format
            assert!(
                encoded.ends_with("'B"),
                "Length {len} should use binary format, got: {encoded}"
            );
            let decoded: BitString = crate::gser::decode(&encoded).unwrap();
            assert_eq!(bits, decoded, "Round-trip failed for length {len}");
        }
    }

    #[test]
    fn gser_bit_string_aligned_lengths() {
        // Aligned lengths (multiple of 8 = byte-aligned) should use hex format and round-trip
        for len in [8, 16, 24, 32] {
            let bits: BitString = (0..len).map(|i| i % 2 == 0).collect();
            let encoded = crate::gser::encode(&bits).unwrap();
            // Byte-aligned should use hex format
            assert!(
                encoded.ends_with("'H"),
                "Length {len} should use hex format, got: {encoded}"
            );
            let decoded: BitString = crate::gser::decode(&encoded).unwrap();
            assert_eq!(bits, decoded, "Round-trip failed for length {len}");
        }
    }

    #[test]
    fn gser_bit_string_long() {
        // 64 bits - tests longer bit strings
        let bits: BitString = (0..64).map(|i| i % 3 == 0).collect();
        let encoded = crate::gser::encode(&bits).unwrap();
        let decoded: BitString = crate::gser::decode(&encoded).unwrap();
        assert_eq!(bits, decoded);
    }

    // ========================================================================
    // OID coverage tests
    // ========================================================================

    #[test]
    fn gser_oid_tca_profile() {
        // TCA OID: 2.23.143.1.2.1 (GlobalPlatform eSIM)
        round_trip_gser!(
            ObjectIdentifier,
            ObjectIdentifier::new_unchecked(alloc::vec![2, 23, 143, 1, 2, 1].into()),
            "2.23.143.1.2.1"
        );
    }

    #[test]
    fn gser_oid_itu_arc() {
        // ITU-T arc (starts with 0): 0.4.0.127.0.7
        round_trip_gser!(
            ObjectIdentifier,
            ObjectIdentifier::new_unchecked(alloc::vec![0, 4, 0, 127, 0, 7].into()),
            "0.4.0.127.0.7"
        );
    }

    #[test]
    fn gser_oid_large_arcs() {
        // SHA256 OID with large arc numbers: 2.16.840.1.101.3.4.2.1
        round_trip_gser!(
            ObjectIdentifier,
            ObjectIdentifier::new_unchecked(alloc::vec![2, 16, 840, 1, 101, 3, 4, 2, 1].into()),
            "2.16.840.1.101.3.4.2.1"
        );
    }

    #[test]
    fn gser_oid_iso_arc() {
        // ISO arc (starts with 1): 1.2.840.113549.1.1.11 (sha256WithRSAEncryption)
        round_trip_gser!(
            ObjectIdentifier,
            ObjectIdentifier::new_unchecked(alloc::vec![1, 2, 840, 113549, 1, 1, 11].into()),
            "1.2.840.113549.1.1.11"
        );
    }

    // ========================================================================
    // OCTET STRING variety tests
    // ========================================================================

    #[test]
    fn gser_octet_string_empty() {
        round_trip_gser!(OctetString, OctetString::from_static(&[]), "''H");
    }

    #[test]
    fn gser_octet_string_iccid() {
        // ICCID format (10 bytes) - common in eSIM profiles
        round_trip_gser!(
            OctetString,
            OctetString::from_static(&[0x89, 0x01, 0x99, 0x90, 0x00, 0x00, 0x00, 0x12, 0x34, 0x5F]),
            "'8901999000000012345F'H"
        );
    }

    #[test]
    fn gser_octet_string_aid() {
        // AID format (16 bytes) - Application Identifier
        round_trip_gser!(
            OctetString,
            OctetString::from_static(&[
                0xA0, 0x00, 0x00, 0x05, 0x59, 0x10, 0x10, 0xFF, 0xFF, 0xFF, 0xFF, 0x89, 0x00, 0x00,
                0x01, 0x00
            ]),
            "'A0000005591010FFFFFFFF8900000100'H"
        );
    }

    #[test]
    fn gser_octet_string_key_sizes() {
        // Test common cryptographic key sizes
        for size in [16, 24, 32] {
            let data: alloc::vec::Vec<u8> = (0..size).map(|i| (i * 17) as u8).collect();
            let octet = OctetString::from(data);
            let encoded = crate::gser::encode(&octet).unwrap();
            let decoded: OctetString = crate::gser::decode(&encoded).unwrap();
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
    fn gser_full_profile_header_round_trip() {
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
        let encoded = crate::gser::encode(&header).unwrap();
        let decoded: FullProfileHeader = crate::gser::decode(&encoded).unwrap();
        assert_eq!(header, decoded);
    }

    #[test]
    fn gser_full_profile_header_no_optional() {
        let header = FullProfileHeader {
            major_version: 1,
            minor_version: 0,
            profile_type: None,
            iccid: OctetString::from_static(&[0x89, 0x00, 0x00, 0x00, 0x00]),
            euicc_mandatory_gfste_list: alloc::vec![],
        };
        let encoded = crate::gser::encode(&header).unwrap();
        let decoded: FullProfileHeader = crate::gser::decode(&encoded).unwrap();
        assert_eq!(header, decoded);
    }

    #[test]
    fn gser_security_domain_multiple_keys() {
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
        let encoded = crate::gser::encode(&keys).unwrap();
        let decoded: SequenceOf<FullKeyInfo> = crate::gser::decode(&encoded).unwrap();
        assert_eq!(keys, decoded);
    }

    #[test]
    fn gser_deeply_nested_optional() {
        // Test deeply nested optional fields
        let nested = Very {
            a: Some(Nested {
                very: Some(Struct { strct: Some(42) }),
                nested: None,
            }),
        };
        let encoded = crate::gser::encode(&nested).unwrap();
        let decoded: Very = crate::gser::decode(&encoded).unwrap();
        assert_eq!(nested, decoded);
    }

    #[test]
    fn gser_empty_nested() {
        // All optional fields absent
        let nested = Very { a: None };
        let encoded = crate::gser::encode(&nested).unwrap();
        let decoded: Very = crate::gser::decode(&encoded).unwrap();
        assert_eq!(nested, decoded);
    }

    // ========================================================================
    // Time type tests
    // ========================================================================

    #[test]
    fn gser_utc_time_round_trip() {
        use chrono::{TimeZone, Utc};
        let time: UtcTime = Utc.with_ymd_and_hms(2025, 6, 15, 12, 30, 45).unwrap();
        let encoded = crate::gser::encode(&time).unwrap();
        let decoded: UtcTime = crate::gser::decode(&encoded).unwrap();
        assert_eq!(time, decoded);
    }

    #[test]
    fn gser_generalized_time_round_trip() {
        use chrono::{FixedOffset, TimeZone};
        let time: GeneralizedTime = FixedOffset::east_opt(0)
            .unwrap()
            .with_ymd_and_hms(2025, 12, 31, 23, 59, 59)
            .unwrap();
        let encoded = crate::gser::encode(&time).unwrap();
        let decoded: GeneralizedTime = crate::gser::decode(&encoded).unwrap();
        assert_eq!(time, decoded);
    }

    #[test]
    fn gser_utc_time_year_2000() {
        use chrono::{TimeZone, Utc};
        // Test Y2K boundary
        let time: UtcTime = Utc.with_ymd_and_hms(2000, 1, 1, 0, 0, 0).unwrap();
        let encoded = crate::gser::encode(&time).unwrap();
        let decoded: UtcTime = crate::gser::decode(&encoded).unwrap();
        assert_eq!(time, decoded);
    }

    #[test]
    fn gser_generalized_time_with_offset() {
        use chrono::{FixedOffset, TimeZone};
        // Test with non-UTC offset
        let offset = FixedOffset::east_opt(5 * 3600).unwrap(); // UTC+5
        let time: GeneralizedTime = offset.with_ymd_and_hms(2025, 7, 4, 14, 30, 0).unwrap();
        let encoded = crate::gser::encode(&time).unwrap();
        let decoded: GeneralizedTime = crate::gser::decode(&encoded).unwrap();
        assert_eq!(time, decoded);
    }
}
