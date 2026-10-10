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
fn level_changer_descriptions_name_xray_registry_source_across_locales() {
    let service = I18nService::instance();
    let expected = [
        (
            "ru",
            "Объекты переходов между локациями (Level Changers) из реестра X-Ray.",
        ),
        (
            "uk",
            "Об'єкти переходів між локаціями (Level Changers) із реєстру X-Ray.",
        ),
        ("en", "Level changers between locations, from the X-Ray registry."),
        ("de", "Übergänge zwischen Orten (Level Changer) aus der X-Ray-Registry."),
        (
            "fr",
            "Transitions entre lieux (level changers) issues du registre X-Ray.",
        ),
        ("it", "Transizioni tra luoghi (level changer) dal registro X-Ray."),
        (
            "es",
            "Transiciones entre ubicaciones (level changers) del registro de X-Ray.",
        ),
        ("pl", "Przejścia między lokacjami (Level Changers) z rejestru X-Ray."),
        ("cs", "Přechody mezi lokacemi (level changers) z registru X-Ray."),
        (
            "pt-BR",
            "Transições entre locais (level changers) do registro do X-Ray.",
        ),
        (
            "tr",
            "X-Ray kayıt defterinden alınan konumlar arası geçiş nesneleri (level changer).",
        ),
        (
            "ja",
            "ロケーション間の移動地点（level changer）はX-Rayレジストリから取得。",
        ),
        (
            "ko",
            "지역 간 이동 지점(level changer)은 X-Ray 레지스트리에서 가져옵니다.",
        ),
        ("zh-CN", "地点之间的转换点（level changer）来自 X-Ray 注册表。"),
        ("zh-TW", "地點之間的轉換點（level changer）來自 X-Ray 登錄檔。"),
    ];

    for (language, value) in expected {
        assert_eq!(
            service.tr_in(
                Some(language),
                "Объекты переходов между локациями (Level Changers) из реестра X-Ray.",
                &[]
            ),
            value,
            "{language}"
        );
    }
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
fn czech_game_and_save_labels_use_their_contextual_meanings() {
    let service = I18nService::instance();

    assert_eq!(service.tr_in(Some("cs"), "включён", &[]), "zahrnutý");
    assert_eq!(service.tr_in(Some("cs"), "ИГРЫ И ИНСТРУМЕНТЫ", &[]), "HRY A NÁSTROJE");
    assert_eq!(
        service.tr_in(Some("cs"), "СОСТОЯНИЕ БЕЗ МАНИФЕСТА", &[]),
        "STAV BEZ MANIFESTU"
    );
    assert_eq!(
        service.tr_in(Some("cs"), "Сохранения появятся после выбора папок в настройках.", &[]),
        "Uložené pozice se objeví po výběru složek v nastavení."
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
fn brazilian_portuguese_level_changer_description_names_xray_registry() {
    let service = I18nService::instance();

    assert_eq!(
        service.tr_in(
            Some("pt-BR"),
            "Объекты переходов между локациями (Level Changers) из реестра X-Ray.",
            &[]
        ),
        "Transições entre locais (level changers) do registro do X-Ray."
    );
}

#[test]
fn player_inventory_title_uses_singular_owner_across_locales() {
    let service = I18nService::instance();
    let expected = [
        ("fr", "Inventaire du joueur"),
        ("it", "Inventario del giocatore"),
        ("es", "Inventario del jugador"),
        ("cs", "Inventář hráče"),
        ("pt-BR", "Inventário do jogador"),
    ];

    for (language, value) in expected {
        assert_eq!(
            service.tr_in(Some(language), "Инвентарь игрока", &[]),
            value,
            "{language}"
        );
    }
}

#[test]
fn german_item_addition_help_names_the_game_save_and_items() {
    let service = I18nService::instance();

    assert_eq!(
        service.tr_in(
            Some("de"),
            "Выберите совместимое сохранение с поддержкой добавления предметов.",
            &[]
        ),
        "Wählen Sie einen kompatiblen Spielstand aus, der das Hinzufügen von Gegenständen unterstützt."
    );
}

#[test]
fn ukrainian_item_addition_help_says_the_save_supports_item_addition() {
    let service = I18nService::instance();

    assert_eq!(
        service.tr_in(
            Some("uk"),
            "Выберите совместимое сохранение с поддержкой добавления предметов.",
            &[]
        ),
        "Виберіть сумісне збереження, яке підтримує додавання предметів."
    );
}

#[test]
fn polish_item_addition_help_names_game_items() {
    let service = I18nService::instance();

    assert_eq!(
        service.tr_in(
            Some("pl"),
            "Выберите совместимое сохранение с поддержкой добавления предметов.",
            &[]
        ),
        "Wybierz kompatybilny zapis gry obsługujący dodawanie przedmiotów."
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
