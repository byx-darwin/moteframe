//! Regression checks for the V2 presentation contract.
use wattch5_parity_ui_demo::*;
#[test]
fn test_should_use_bounded_chinese_subset_and_explicit_missing_glyph() {
    assert_eq!(GRAPHIC_DATA_BYTES, 6730);
    let g = glyph('心').unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(g.width, 12);
    assert!(g.rows.iter().any(|r| *r != 0));
    assert_eq!(glyph('a'), glyph('A'));
    assert_eq!(glyph('龘'), Err(UiError::UnsupportedGlyph));
}
#[test]
fn test_should_map_default_chinese_locale_to_chinese_contract_copy() {
    assert_eq!(Label::Heart.text(Locale::Zh), "心率");
    assert_eq!(Label::Heart.text(Locale::En), "HEART");
}
