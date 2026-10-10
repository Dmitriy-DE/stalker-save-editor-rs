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
        "EDITOR DEI SALVATAGGI"
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
fn japanese_companion_snapshot_uses_included_term() {
    let service = I18nService::instance();

    assert_eq!(service.tr_in(Some("ja"), "включён", &[]), "含まれています");
}

#[test]
fn polish_item_metadata_uses_section_term() {
    let service = I18nService::instance();

    assert_eq!(
        service.tr_in(Some("pl"), "Вес: {0} · Цена: {1} · Секция: {2}", &[]),
        "Waga: {0} · Cena: {1} · Sekcja: {2}"
    );
}

#[test]
fn polish_game_default_label_preserves_value_meaning() {
    let service = I18nService::instance();

    assert_eq!(
        service.tr_in(Some("pl"), "Значение по умолчанию игры", &[]),
        "Wartość domyślna gry"
    );
}

#[test]
fn polish_level_changer_label_preserves_registry_source() {
    let service = I18nService::instance();

    assert_eq!(
        service.tr_in(
            Some("pl"),
            "Объекты переходов между локациями (Level Changers) из реестра X-Ray.",
            &[]
        ),
        "Przejścia między lokacjami (Level Changers) z rejestru X-Ray."
    );
}

#[test]
fn polish_profile_summary_keeps_companion_as_product_name() {
    let service = I18nService::instance();

    assert_eq!(
        service.tr_in(
            Some("pl"),
            "Профиль сохраняет установленные идентификаторы Game Fix, состояние Companion и только явные настройки user.ltx, управляемые инструментом.",
            &[]
        ),
        "Profil zachowuje zainstalowane identyfikatory poprawek do gier, stan moda Companion i tylko jawne ustawienia user.ltx zarządzane przez narzędzie."
    );
}

#[test]
fn polish_manifest_label_keeps_software_term() {
    let service = I18nService::instance();

    assert_eq!(
        service.tr_in(Some("pl"), "СОСТОЯНИЕ БЕЗ МАНИФЕСТА", &[]),
        "STAN BEZ MANIFESTU"
    );
}

#[test]
fn polish_companion_controls_keep_product_name() {
    let service = I18nService::instance();

    assert_eq!(
        service.tr_in(Some("pl"), "Состояние и управление компаньоном", &[]),
        "Stan i obsługa Companion"
    );
}

#[test]
fn polish_compare_with_previous_uses_singular() {
    let service = I18nService::instance();

    assert_eq!(
        service.tr_in(Some("pl"), "Сравнить с предыдущим", &[]),
        "Porównaj z poprzednim"
    );
}

#[test]
fn polish_transition_placement_note_uses_natural_save_wording() {
    let service = I18nService::instance();

    assert_eq!(
        service.tr_in(
            Some("pl"),
            "Только точки, куда игра сама ставит персонажа после перехода. Сохранение записывается сразу, с бэкапом. В игре это ещё не проверено.",
            &[]
        ),
        "Tylko miejsca, w których gra sama umieszcza postać po przejściu. Zapis następuje od razu po utworzeniu kopii zapasowej. Nie sprawdzono tego jeszcze w grze."
    );
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
