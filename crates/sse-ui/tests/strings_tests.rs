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

#[test]
fn save_history_screen_title_is_a_noun() {
    let expected = [
        ("de", "SPIELSTANDVERLAUF"),
        ("fr", "HISTORIQUE DES SAUVEGARDES"),
        ("it", "CRONOLOGIA DEI SALVATAGGI"),
        ("es", "HISTORIAL DE PARTIDAS"),
        ("pl", "HISTORIA ZAPISÓW"),
        ("cs", "HISTORIE ULOŽENÝCH HER"),
        ("pt-BR", "HISTÓRICO DE SALVAMENTOS"),
        ("tr", "KAYIT GEÇMİŞİ"),
        ("ja", "セーブ履歴"),
        ("ko", "저장 기록"),
        ("zh-CN", "存档历史"),
        ("zh-TW", "存檔歷史"),
    ];
    for (language, value) in expected {
        assert_eq!(t_in(language, "ИСТОРИЯ СОХРАНЕНИЙ"), value, "{language}");
    }
}

#[test]
fn localized_settings_keep_the_user_ltx_filename_exact() {
    let expected = [
        (
            "fr",
            "ВЫБЕРИТЕ СУЩЕСТВУЮЩИЙ user.ltx",
            "SÉLECTIONNER UN FICHIER user.ltx EXISTANT",
        ),
        (
            "es",
            "ВЫБЕРИТЕ СУЩЕСТВУЮЩИЙ user.ltx",
            "SELECCIONE EL ARCHIVO user.ltx EXISTENTE",
        ),
        (
            "cs",
            "ВЫБЕРИТЕ СУЩЕСТВУЮЩИЙ user.ltx",
            "VYBERTE EXISTUJÍCÍ SOUBOR user.ltx",
        ),
        (
            "tr",
            "ВЫБЕРИТЕ СУЩЕСТВУЮЩИЙ user.ltx",
            "MEVCUT user.ltx DOSYASINI SEÇİN",
        ),
        ("zh-CN", "ВЫБЕРИТЕ СУЩЕСТВУЮЩИЙ user.ltx", "选择现有的 user.ltx 文件"),
        ("zh-TW", "ВЫБЕРИТЕ СУЩЕСТВУЮЩИЙ user.ltx", "選擇現有的 user.ltx 檔案"),
        ("fr", "НАСТРОЙКИ user.ltx", "PARAMÈTRES user.ltx"),
        ("it", "НАСТРОЙКИ user.ltx", "IMPOSTAZIONI user.ltx"),
        ("es", "НАСТРОЙКИ user.ltx", "AJUSTES user.ltx"),
        ("pl", "НАСТРОЙКИ user.ltx", "USTAWIENIA user.ltx"),
        ("cs", "НАСТРОЙКИ user.ltx", "NASTAVENÍ user.ltx"),
        ("pt-BR", "НАСТРОЙКИ user.ltx", "CONFIGURAÇÕES user.ltx"),
        ("zh-CN", "НАСТРОЙКИ user.ltx", "设置 user.ltx"),
        ("zh-TW", "НАСТРОЙКИ user.ltx", "設定 user.ltx"),
        (
            "es",
            "ПУТЬ К СУЩЕСТВУЮЩЕМУ user.ltx",
            "RUTA AL ARCHIVO user.ltx EXISTENTE",
        ),
        ("tr", "ПУТЬ К СУЩЕСТВУЮЩЕМУ user.ltx", "MEVCUT user.ltx DOSYASININ YOLU"),
        ("zh-CN", "ПУТЬ К СУЩЕСТВУЮЩЕМУ user.ltx", "现有 user.ltx 文件的路径"),
        ("zh-TW", "ПУТЬ К СУЩЕСТВУЮЩЕМУ user.ltx", "現有 user.ltx 檔案的路徑"),
    ];
    for (language, key, value) in expected {
        assert_eq!(t_in(language, key), value, "{language}: {key}");
    }
    assert_eq!(
        t_in("de", "Не удалось прочитать user.ltx: {0}"),
        "user.ltx konnte nicht gelesen werden: {0}"
    );
}

#[test]
fn turkish_steam_manifest_labels_keep_the_manifest_term() {
    let expected = [
        ("Манифест Steam не найден", "Steam manifesti bulunamadı"),
        ("Манифест доступен", "Güncelleme manifesti mevcut"),
        ("Манифест недоступен", "Güncelleme manifesti kullanılamıyor"),
        ("Манифест обновления", "Güncelleme manifesti"),
        (
            "Проверьте манифест игры в Steam.",
            "Steam'deki oyun manifestini kontrol edin.",
        ),
        (
            "Проверьте файл манифеста обновления.",
            "Güncelleme manifesti dosyasını kontrol edin.",
        ),
        (
            "Снимки включают только файлы и манифесты Game Fix, Companion и настроек, которыми владеет инструмент. Восстановление повторно применяет их через исходные провайдеры.",
            "Anlık görüntüler yalnızca Game Fix, Companion ve Settings dosyalarını ve araca ait manifestleri içerir. Recovery, bunları orijinal sağlayıcılar aracılığıyla yeniden uygular.",
        ),
    ];
    for (key, value) in expected {
        assert_eq!(t_in("tr", key), value, "{key}");
    }
}

#[test]
fn player_labels_do_not_use_the_athlete_word_in_simplified_chinese() {
    assert_eq!(t_in("zh-CN", "Инвентарь игрока"), "玩家物品栏");
    assert_eq!(t_in("zh-CN", "Информация игрока"), "玩家信息");
    assert_eq!(t_in("zh-TW", "Инвентарь игрока"), "玩家物品欄");
    assert_eq!(t_in("zh-TW", "Информация игрока"), "玩家資訊");
}

#[test]
fn korean_companion_status_distinguishes_installed_from_missing() {
    assert_eq!(t_in("ko", "включён"), "설치됨");
    assert_eq!(t_in("ko", "выключен"), "설치되지 않음");
}

#[test]
fn static_french_confirmation_text_is_french() {
    assert_eq!(
        t_in(
            "fr",
            "Подтверждение перед записью не настраивается: изменения проходят через проверяемый черновик."
        ),
        "La confirmation avant l’écriture n’est pas configurable ; les modifications passent par un brouillon vérifié."
    );
}

#[test]
fn static_italian_save_editor_title_matches_catalog() {
    assert_eq!(t_in("it", "РЕДАКТОР СОХРАНЕНИЙ"), "EDITOR DEI SALVATAGGI");
}

#[test]
fn static_japanese_player_title_uses_player_term() {
    assert_eq!(t_in("ja", "Информация игрока"), "プレイヤー情報");
}

#[test]
fn static_japanese_burer_text_has_no_latin_fragment() {
    assert_eq!(
        t_in(
            "ja",
            "Удалить три обработчика попаданий, которые восстанавливают здоровье буреров при атаке извне X8."
        ),
        "X8の外から攻撃されたときにブーラーを回復させる3つのヒット処理を削除します。"
    );
}

#[test]
fn static_polish_item_metadata_uses_section_term() {
    assert_eq!(
        t_in("pl", "Вес: {0} · Цена: {1} · Секция: {2}"),
        "Waga: {0} · Cena: {1} · Sekcja: {2}"
    );
}

#[test]
fn static_polish_game_default_label_preserves_value_meaning() {
    assert_eq!(t_in("pl", "Значение по умолчанию игры"), "Wartość domyślna gry");
}

#[test]
fn static_level_changer_descriptions_name_xray_registry_source_across_locales() {
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
            t_in(
                language,
                "Объекты переходов между локациями (Level Changers) из реестра X-Ray."
            ),
            value,
            "{language}"
        );
    }
}

#[test]
fn static_polish_profile_summary_keeps_companion_as_product_name() {
    assert_eq!(
        t_in(
            "pl",
            "Профиль сохраняет установленные идентификаторы Game Fix, состояние Companion и только явные настройки user.ltx, управляемые инструментом."
        ),
        "Profil zachowuje zainstalowane identyfikatory poprawek do gier, stan moda Companion i tylko jawne ustawienia user.ltx zarządzane przez narzędzie."
    );
}

#[test]
fn static_polish_manifest_label_keeps_software_term() {
    assert_eq!(t_in("pl", "СОСТОЯНИЕ БЕЗ МАНИФЕСТА"), "STAN BEZ MANIFESTU");
}

#[test]
fn static_polish_companion_controls_keep_product_name() {
    assert_eq!(
        t_in("pl", "Состояние и управление компаньоном"),
        "Stan i obsługa Companion"
    );
}

#[test]
fn static_polish_compare_with_previous_uses_singular() {
    assert_eq!(t_in("pl", "Сравнить с предыдущим"), "Porównaj z poprzednim");
}

#[test]
fn static_polish_transition_placement_note_uses_natural_save_wording() {
    assert_eq!(
        t_in(
            "pl",
            "Только точки, куда игра сама ставит персонажа после перехода. Сохранение записывается сразу, с бэкапом. В игре это ещё не проверено."
        ),
        "Tylko miejsca, w których gra sama umieszcza postać po przejściu. Zapis następuje od razu po utworzeniu kopii zapasowej. Nie sprawdzono tego jeszcze w grze."
    );
}

#[test]
fn static_brazilian_portuguese_level_changer_description_names_xray_registry() {
    assert_eq!(
        t_in(
            "pt-BR",
            "Объекты переходов между локациями (Level Changers) из реестра X-Ray."
        ),
        "Transições entre locais (level changers) do registro do X-Ray."
    );
}

#[test]
fn static_player_inventory_title_uses_singular_owner_across_locales() {
    let expected = [
        ("fr", "Inventaire du joueur"),
        ("it", "Inventario del giocatore"),
        ("es", "Inventario del jugador"),
        ("cs", "Inventář hráče"),
        ("pt-BR", "Inventário do jogador"),
    ];

    for (language, value) in expected {
        assert_eq!(t_in(language, "Инвентарь игрока"), value, "{language}");
    }
}
