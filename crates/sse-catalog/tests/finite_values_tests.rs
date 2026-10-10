//! Non-finite numbers must not reach catalogs or the JSON writer (R3-006, R3-007).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use sse_catalog::models::ItemDefinition;
use sse_catalog::JsonValue;

fn item_with_weight(weight: Option<f64>) -> sse_core::Result<ItemDefinition> {
    ItemDefinition::new(
        "ammo_9x18".to_string(),
        None,
        None,
        weight,
        None,
        None,
        None,
        Vec::new(),
        "test".to_string(),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )
}

#[test]
fn item_weight_accepts_finite_non_negative_values() {
    assert!(item_with_weight(None).is_ok());
    assert!(item_with_weight(Some(0.0)).is_ok());
    assert!(item_with_weight(Some(1.5)).is_ok());
}

#[test]
fn item_weight_rejects_negative_values() {
    assert!(item_with_weight(Some(-0.1)).is_err());
}

#[test]
fn item_weight_rejects_nan() {
    assert!(item_with_weight(Some(f64::NAN)).is_err());
}

#[test]
fn item_weight_rejects_infinity_from_json_style_overflow() {
    // `1e400` parses to infinity; such a weight would make inventory totals infinite.
    let overflowed = sse_catalog::parse_json("1e400").unwrap();
    let weight = overflowed.as_f64().expect("number");
    assert!(weight.is_infinite());
    assert!(item_with_weight(Some(weight)).is_err());
    assert!(item_with_weight(Some(f64::NEG_INFINITY)).is_err());
}

#[test]
fn json_writer_refuses_non_finite_numbers_instead_of_empty_output() {
    assert!(JsonValue::Number(f64::NAN).to_indented_string().is_err());
    assert!(JsonValue::Number(f64::INFINITY).to_indented_string().is_err());
    let nested = JsonValue::Array(vec![JsonValue::Number(f64::NEG_INFINITY)]);
    assert!(nested.to_indented_string().is_err());
}

#[test]
fn json_writer_still_writes_finite_numbers_unchanged() {
    let value = JsonValue::Array(vec![JsonValue::Number(2.0), JsonValue::Number(0.25)]);
    assert_eq!(value.to_indented_string().unwrap(), "[\n  2,\n  0.25\n]");
}
