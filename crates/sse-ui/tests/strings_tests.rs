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
