#![allow(missing_docs)]

use sse_ui::strings::{t_in, LANGUAGES};

#[test]
fn known_keys_translate_and_unknown_fall_back() {
    assert_eq!(LANGUAGES.len(), 15);
    assert_eq!(t_in("en", "НАСТРОЙКИ"), "SETTINGS");
    assert_eq!(t_in("uk", "НАСТРОЙКИ"), "НАЛАШТУВАННЯ");
    assert_eq!(t_in("ru", "НАСТРОЙКИ"), "НАСТРОЙКИ");
    assert_eq!(t_in("en", "нет такого ключа"), "нет такого ключа");
}
