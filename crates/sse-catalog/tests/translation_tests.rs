//! Integration tests for i18n translation service and completeness checker.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use sse_catalog::{I18nCompletenessChecker, I18nService, SUPPORTED_LANGUAGES};

#[test]
fn every_interface_language_loads_its_translations() {
    let service = I18nService::instance();

    for &(code, _) in SUPPORTED_LANGUAGES {
        if code != "ru" {
            let catalog = service.get_catalog(code).unwrap();
            assert!(
                catalog.len() > 1000,
                "{code} catalog is missing or has only {} entries",
                catalog.len()
            );
        }
    }

    assert_eq!(service.tr_in(Some("en"), "ИНВЕНТАРЬ", &[]), "INVENTORY");
}

#[test]
fn french_write_confirmation_translation_is_french() {
    let service = I18nService::instance();

    assert_eq!(
        service.tr_in(
            Some("fr"),
            "Подтверждение перед записью не настраивается: изменения проходят через проверяемый черновик.",
            &[]
        ),
        "La confirmation avant l’écriture n’est pas configurable ; les modifications passent par un brouillon vérifié."
    );
}

#[test]
fn italian_save_editor_title_is_italian() {
    let service = I18nService::instance();

    assert_eq!(
        service.tr_in(Some("it"), "РЕДАКТОР СОХРАНЕНИЙ", &[]),
        "EDITOR DI SALVATAGGI"
    );
}

#[test]
fn japanese_player_information_title_uses_player_term() {
    let service = I18nService::instance();

    assert_eq!(service.tr_in(Some("ja"), "Информация игрока", &[]), "プレイヤー情報");
}

#[test]
fn japanese_burer_references_do_not_mix_scripts() {
    let service = I18nService::instance();

    assert_eq!(
        service.tr_in(
            Some("ja"),
            "Попадание по буреру снаружи ограничителя лаборатории X8 полностью восстанавливает его здоровье.",
            &[]
        ),
        "X8研究所の制限区域外でブーラーに命中すると、体力が全回復します。"
    );
    assert_eq!(
        service.tr_in(
            Some("ja"),
            "Удалить три обработчика попаданий, которые восстанавливают здоровье буреров при атаке извне X8.",
            &[]
        ),
        "X8の外から攻撃されたときにブーラーを回復させる3つのヒット処理を削除します。"
    );
    assert_eq!(
        service.tr_in(
            Some("ja"),
            "Устранить восстановление здоровья буреров за пределами X8",
            &[]
        ),
        "X8研究所の外でブーラーが体力を回復する動作を停止"
    );
}

#[test]
fn japanese_companion_enabled_status_uses_enabled_term() {
    let service = I18nService::instance();

    assert_eq!(service.tr_in(Some("ja"), "включён", &[]), "有効");
}

#[test]
fn completeness_checker_validates_coverage_and_placeholders() {
    let result = I18nCompletenessChecker::validate(None);

    assert!(
        result.success,
        "Completeness checker failed with errors:\n{}",
        result.errors.join("\n")
    );
    assert!(result.total_messages > 1000);
    assert_eq!(result.checked_locales, 14);
}
