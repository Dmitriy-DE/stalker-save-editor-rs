//! C# 1.3.1 shell/screen acceptance inventory.
//!
//! The inventory deliberately reuses screens::ScreenId. AddItemDialog.cs is
//! part of Inventory instead of inventing a twenty-first screen identifier.

use crate::screens::ScreenId;

/// Canonical C# screen inventory entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScreenSpec {
    /// Existing Rust screen identifier.
    pub id: ScreenId,
    /// Russian title from the C# view.
    pub title: &'static str,
    /// C# source view.
    pub source: &'static str,
    /// Ordered sections, controls, handlers and enable rules.
    pub inventory: &'static str,
}

/// MainWindow shell has no ScreenId and is tracked separately.
pub const SHELL: &str = r#"MainWindow:
sidebar [СОХРАНЕНИЯ / ИГРЫ / ИНСТРУМЕНТЫ] -> navigate ScreenId; always
save pane [Сохранения] -> select save; always
Сохранить -> commit draft; dirty writable save
Отменить -> undo draft; can undo
Повторить -> redo draft; can redo
notification banner -> dismiss/navigate; banner present
Мастер первого запуска -> wizard actions; first run"#;

/// All canonical screens in ScreenId::ALL order.
pub const SCREENS: &[ScreenSpec] = &[
    ScreenSpec {
        id: ScreenId::Overview,
        title: "ОБЗОР",
        source: "OverviewView.cs",
        inventory: r#"Информация о сохранении: Сравнить с другим сейвом или бэкапом -> navigate:compare [selected save]; ДЕНЬГИ / ПРЕДМЕТОВ / ТАЙНИКОВ / ИГРОВОЕ ВРЕМЯ; ПЕРСОНАЖ / ЗДОРОВЬЕ / РАНГ / РЕПУТАЦИЯ / ЗАДАНИЯ / УБИТО / ПОГОДА. Целостность и метаданные: Размер файла / Изменён / Сборка игры."#,
    },
    ScreenSpec {
        id: ScreenId::Inventory,
        title: "ИНВЕНТАРЬ",
        source: "InventoryView.cs + AddItemDialog.cs",
        inventory: r#"ДЕНЬГИ -> SaveLibraryViewModel.AddMoney [CanEditMoney]; Поиск предметов -> InventorySearchText; Очистить [search not empty]; категории ВСЕ/ОРУЖИЕ/БОЕПРИПАСЫ/СНАРЯЖЕНИЕ/РАСХОДНИКИ/АРТЕФАКТЫ/КЛЮЧИ/ПРОЧЕЕ -> SelectedCategory. Список инвентаря -> SelectedItem; Сбросить фильтры. Редактор: Состояние/прочность [CanEditCondition], Количество в пачке [CanEditCount], Слот/Пояс/Рюкзак [CanEditPlacement], Модификации, Удалить предмет -> RemoveSelectedItemCommand [CanDelete], + Добавить предмет -> AddItemDialog [CanAddItems]. AddItemDialog: поиск по названию/ключу секции; каталог; Количество; Добавить [valid item/quantity]; Отмена."#,
    },
    ScreenSpec {
        id: ScreenId::Factions,
        title: "ФРАКЦИИ",
        source: "FactionsView.cs",
        inventory: r#"Группировка игрока -> set player faction. Таблица ГРУППИРОВКА / ОЧКИ / СТАТУС -> edit relation. Друг (+1500), Нейтрал (0), Враг (-1500) [relation.CanEdit]."#,
    },
    ScreenSpec {
        id: ScreenId::Stashes,
        title: "ТАЙНИКИ",
        source: "StashesView.cs",
        inventory: r#"Управление тайниками Зоны; список -> SelectedSave.Stashes; В рюкзак -> StashSelectionChanged [item.CanEdit]; Предмет из рюкзака -> SelectedSave.Inventory; В ТАЙНИК -> ToggleStashPut; + СОЗДАТЬ В ТАЙНИКЕ -> AddItemDialog [CanAddItems]."#,
    },
    ScreenSpec {
        id: ScreenId::Transitions,
        title: "ПЕРЕХОДЫ",
        source: "TransitionsView.cs",
        inventory: r#"ПЕРЕНОС ПЕРСОНАЖА (ЭКСПЕРИМЕНТАЛЬНО): Куда перенести -> SelectedSave.RelocationAnchors; ПЕРЕНЕСТИ -> RelocateActor [anchor selected]. Переходы: таблица ПЕРЕХОД / ИМЯ В СОХРАНЕНИИ / ТОЧКА / ID В РЕЕСТРЕ."#,
    },
    ScreenSpec {
        id: ScreenId::Backups,
        title: "БЭКАПЫ",
        source: "BackupsView.cs",
        inventory: r#"РЕЗЕРВНЫЕ КОПИИ: Обновить бэкапы -> RefreshBackups; список -> SelectedBackup. СВЕДЕНИЯ О КОПИИ: СОЗДАНА / ИСХОДНЫЙ ФАЙЛ / ФАЙЛ КОПИИ / ФАЙЛ ПОСЛЕ ОПЕРАЦИИ / SHA-256. Восстановить на место [can restore]; Восстановить в копию."#,
    },
    ScreenSpec {
        id: ScreenId::Compare,
        title: "СРАВНЕНИЕ",
        source: "CompareView.cs",
        inventory: r#"Сравнить с -> target; Поменять местами -> CompareViewModel.SwapSides; summary РАЗЛИЧИЙ/ДОБАВЛЕНО/УДАЛЕНО/ИЗМЕНЕНО. Фильтры Категория -> SelectCategoryCommand, Тип изменения -> SelectChangeTypeCommand, Поиск. Таблица ТИП / ПАРАМЕТР / ОБЪЕКТ / ЗНАЧЕНИЕ A / ЗНАЧЕНИЕ B / КАТЕГОРИЯ. Экспорт CSV -> ExportCsvAsync; Копировать список -> CopyListAsync."#,
    },
    ScreenSpec {
        id: ScreenId::Timeline,
        title: "ИСТОРИЯ СОХРАНЕНИЙ",
        source: "TimelineView.cs",
        inventory: r#"Временная последовательность по времени изменения реальных файлов; Сравнить с предыдущим -> TimelineViewModel.CompareAdjacent [previous save exists]."#,
    },
    ScreenSpec {
        id: ScreenId::SaveDoctor,
        title: "ДОКТОР СОХРАНЕНИЯ",
        source: "SaveDoctorView.cs",
        inventory: r#"Файл; Открыть сохранение -> StorageProvider.OpenFilePicker; ПРОВЕРИТЬ СОХРАНЕНИЕ -> AnalyzeCommand [file selected]; ИСПРАВИТЬ КВЕСТЫ -> RepairQuestsCommand [verified repair]; УСТАНОВИТЬ ИСПРАВЛЕНИЕ ИГРЫ -> OpenGameFixCommand [fix available]."#,
    },
    ScreenSpec {
        id: ScreenId::Games,
        title: "ОБЗОР ИГР",
        source: "GamesView.cs",
        inventory: r#"НАЙДЕННЫЕ УСТАНОВКИ: Найти установки -> DiscoverInstallationsCommand; список -> SelectedInstallation; Открыть Доктор игры. Выбранная игра: Статус/Платформа/Номер сборки/Папка игры; навигация Исправления, Доктор игры, Среда игры, Компаньон [available], Достижения [Steam]."#,
    },
    ScreenSpec {
        id: ScreenId::GameFixes,
        title: "ИСПРАВЛЕНИЯ ИГРЫ",
        source: "GameFixesView.cs",
        inventory: r#"ИГРА -> SelectedTarget; ПАПКА ИГРЫ; Обзор; ПРОВЕРИТЬ СОВМЕСТИМОСТЬ -> CheckInstallationCommand. КАТЕГОРИИ; список ПРОБЛЕМА/ИЗМЕНЕНИЕ/ПОДДЕРЖИВАЕМЫЕ STEAM-СБОРКИ/ЗАТРАГИВАЕМЫЕ ФАЙЛЫ/ИСТОЧНИК. ПРИМЕНИТЬ РЕКОМЕНДУЕМЫЕ -> ApplyRecommendedPresetCommand; ОБЯЗАТЕЛЬНЫЕ -> ApplyEssentialPresetCommand; ВСЕ БЕЗОПАСНЫЕ -> ApplyAllSafePresetCommand; УСТАНОВИТЬ ВЫБРАННОЕ -> InstallCommand; ОБНОВИТЬ -> UpdateCommand; УДАЛИТЬ И ВОССТАНОВИТЬ -> RemoveCommand."#,
    },
    ScreenSpec {
        id: ScreenId::GameDoctor,
        title: "ДОКТОР ИГРЫ",
        source: "GameDoctorView.cs",
        inventory: r#"ИГРА; НАЙТИ УСТАНОВКИ -> DiscoverInstallationsCommand; ОБНАРУЖЕННЫЕ УСТАНОВКИ; ПАПКА ИГРЫ; Обзор; ПРОВЕРИТЬ УСТАНОВКУ -> AnalyzeCommand; ВРЕМЕННО ОТКЛЮЧИТЬ КАСТОМНЫЕ МОДЫ -> DisableS2ModsCommand; ВОССТАНОВИТЬ КАСТОМНЫЕ МОДЫ -> RestoreS2ModsCommand; МОДИФИКАЦИИ."#,
    },
    ScreenSpec {
        id: ScreenId::Environment,
        title: "СРЕДА ИГРЫ",
        source: "ToolkitEnvironmentView.cs",
        inventory: r#"УПРАВЛЯЕМЫЕ СНИМКИ: CreateSnapshot/RestoreSnapshot/DeleteSnapshot/Refresh. ПРОФИЛИ ИГРЫ: имя; SaveProfile/ApplyProfile/DeleteProfile. НАСТРОЙКИ user.ltx: путь; Обзор; LoadConfig; настройки Текущее/По умолчанию; Новое значение; ПРИМЕНИТЬ; ПО УМОЛЧАНИЮ. АУДИТ УСТАНОВКИ: AuditCommand; управляемые файлы; CleanupOrphanAsync [safe orphan]."#,
    },
    ScreenSpec {
        id: ScreenId::Companion,
        title: "КОМПАНЬОН",
        source: "CompanionView.cs",
        inventory: r#"СТАТУС И СВЯЗЬ: Целевая игра; Версия мода/Связь/Задержка/Путь; УСТАНОВИТЬ/ОБНОВИТЬ; УДАЛИТЬ; ПРОВЕРИТЬ СВЯЗЬ; ОБНОВИТЬ СТАТУС. ЖИВОЙ ИНСПЕКТОР: ПОЛУЧИТЬ ДАННЫЕ; info/list_inventory. S2 экспериментально: Бессмертие вкл/выкл, Полёт вкл/выкл, Время ×5/норма -> Stalker2CommandCommand. ГОРЯЧИЕ КЛАВИШИ: ВСЕ ИГРЫ; install/update selected; ручной путь; ПРИМЕНИТЬ; toggle hotkeys; СОХРАНИТЬ КЛАВИШИ; ПО УМОЛЧАНИЮ."#,
    },
    ScreenSpec {
        id: ScreenId::Achievements,
        title: "ДОСТИЖЕНИЯ",
        source: "AchievementsView.cs",
        inventory: r#"ДОСТИЖЕНИЯ STEAM: app selector ТЧ/ЧН/ЗП/S2 -> SelectedAppId; поиск; ОБНОВИТЬ -> RefreshCommand; прогресс; список ПОЛУЧИТЬ/СНЯТЬ -> RequestToggle; ПОДТВЕРДИТЬ -> ConfirmToggleCommand; ОТМЕНА -> CancelToggleCommand."#,
    },
    ScreenSpec {
        id: ScreenId::Cloud,
        title: "ОБЛАКО",
        source: "CloudView.cs",
        inventory: r#"СОХРАНЕНИЯ В STEAM CLOUD: Все игры -> SelectedAppId; ОБНОВИТЬ СПИСОК -> RefreshCommand; cloud save list. ДЕЙСТВИЯ: СКАЧАТЬ В ЛОКАЛЬНЫЕ -> DownloadSelectedCommand; ЗАПИСАТЬ В ОБЛАКО -> RequestWriteCommand; safety banner; подтверждение перезаписи; ЗАПИСАТЬ -> ConfirmWriteCommand; ОТМЕНА -> CancelWriteCommand."#,
    },
    ScreenSpec {
        id: ScreenId::Encyclopedia,
        title: "ЭНЦИКЛОПЕДИЯ",
        source: "EncyclopediaView.cs",
        inventory: r#"ЭНЦИКЛОПЕДИЯ ПРЕДМЕТОВ: список Вес/Цена/Секция; В сохранение -> AddToSave [compatible save/item]; В игру -> SpawnViaCompanionAsync [companion connected]."#,
    },
    ScreenSpec {
        id: ScreenId::Capabilities,
        title: "ВОЗМОЖНОСТИ",
        source: "CapabilitiesView.cs",
        inventory: r#"МАТРИЦА ВОЗМОЖНОСТЕЙ РЕДАКТОРА: ОПЕРАЦИЯ × ТЧ/ЧН/ЗП/ТЧ EE/ЧН EE/ЗП EE/S2; легенда Verified/Experimental/Research/Unsupported/UI block; warning: запись S2 выключена до проверки в игре."#,
    },
    ScreenSpec {
        id: ScreenId::Updates,
        title: "ОБНОВЛЕНИЯ",
        source: "UpdatesView.cs",
        inventory: r#"ОБНОВЛЕНИЕ ПРИЛОЖЕНИЯ: ТЕКУЩАЯ ВЕРСИЯ/ПОСЛЕДНЯЯ ВЕРСИЯ; ПРОВЕРИТЬ ОБНОВЛЕНИЯ -> CheckUpdatesCommand; СКАЧАТЬ -> DownloadCommand [update available]; УСТАНОВИТЬ -> InstallCommand [download complete]; progress; error banner."#,
    },
    ScreenSpec {
        id: ScreenId::Settings,
        title: "НАСТРОЙКИ",
        source: "SettingsView.cs",
        inventory: r#"ОБЩИЕ/ИНТЕРФЕЙС: Разделы; Язык; Тема; Акцент; Масштаб -> StalkerTheme.ApplyAppearance; Звуки; Музыка; Громкость. ПУТИ И АВТОПОИСК: каталоги сохранений; RemoveSaveDirectory; путь; Обзор; AddSaveDirectory; AutoDetectSaveDirectories; backup path. ПОДДЕРЖКА: обновления; исправления; RunChecks; экспорт/отправка отчёта; DismissCrash; SendReports. ВЕРСИЯ: S.T.A.L.K.E.R. Save Editor; Steam Cloud; Сохранить настройки -> SaveSettingsCommand."#,
    },
];

#[cfg(test)]
mod tests {
    use super::SCREENS;
    use crate::screens::ScreenId;
    use std::collections::BTreeSet;

    #[test]
    fn covers_screen_id_once() {
        assert_eq!(SCREENS.len(), ScreenId::ALL.len());
        let ids: BTreeSet<_> = SCREENS.iter().map(|screen| screen.id).collect();
        assert_eq!(ids.len(), ScreenId::ALL.len());
        assert!(ScreenId::ALL.iter().all(|id| ids.contains(id)));
    }
}
