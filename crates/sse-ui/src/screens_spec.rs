//! C# 1.3.1 shell/screen acceptance inventory.
//!
//! AddItemDialog.cs is part of Inventory instead of inventing a twenty-first ScreenId.

use crate::screens::ScreenId;

/// C# control category.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlKind { Table, List, Button, Toggle, TextField, DropDown, Card, Chart, Banner, Progress }

/// One required control.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControlSpec {
    /// Stable checklist id.
    pub id: &'static str,
    /// Control category.
    pub kind: ControlKind,
    /// Russian source label/key.
    pub label_key: &'static str,
    /// C# command/handler.
    pub action: &'static str,
    /// C# enable/visibility rule.
    pub enabled_when: &'static str,
    /// Table columns, empty for non-tables.
    pub columns: &'static [&'static str],
}

/// Ordered screen section.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SectionSpec {
    /// Stable section id.
    pub id: &'static str,
    /// C# Russian heading.
    pub title_key: &'static str,
    /// Controls in source order.
    pub controls: &'static [ControlSpec],
}

/// One canonical screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScreenSpec {
    /// Existing screens/mod.rs id.
    pub id: ScreenId,
    /// C# Russian title.
    pub title_key: &'static str,
    /// Ordered sections.
    pub sections: &'static [SectionSpec],
}

const N: &[&str] = &[];
macro_rules! c {
    ($id:literal,$k:ident,$l:literal,$a:literal,$e:literal) => {
        ControlSpec{id:$id,kind:ControlKind::$k,label_key:$l,action:$a,enabled_when:$e,columns:N}
    };
    ($id:literal,$k:ident,$l:literal,$a:literal,$e:literal,[$($x:literal),+]) => {
        ControlSpec{id:$id,kind:ControlKind::$k,label_key:$l,action:$a,enabled_when:$e,columns:&[$($x),+]}
    };
}

const OVERVIEW:&[SectionSpec]=&[
    SectionSpec{id:"summary",title_key:"Информация о сохранении",controls:&[
        c!("compare",Button,"Сравнить с другим сейвом или бэкапом","navigate:compare","selected save"),
        c!("stats",Card,"ДЕНЬГИ / ПРЕДМЕТОВ / ТАЙНИКОВ / ИГРОВОЕ ВРЕМЯ","","selected save"),
        c!("actor",Card,"ПЕРСОНАЖ / ЗДОРОВЬЕ / РАНГ / РЕПУТАЦИЯ / ЗАДАНИЯ / УБИТО / ПОГОДА","","selected save")]},
    SectionSpec{id:"metadata",title_key:"Целостность и метаданные",controls:&[
        c!("metadata",Card,"Размер файла: / Изменён: / Сборка игры:","","selected save")]}];
const INVENTORY:&[SectionSpec]=&[
    SectionSpec{id:"filters",title_key:"Предметы",controls:&[
        c!("money",TextField,"ДЕНЬГИ:","SaveLibraryViewModel.AddMoney","CanEditMoney"),
        c!("search",TextField,"Поиск предметов…","InventorySearchText","selected save"),
        c!("clear",Button,"Очистить","InventorySearchText = empty","search not empty"),
        c!("category",DropDown,"ВСЕ / ОРУЖИЕ / БОЕПРИПАСЫ / СНАРЯЖЕНИЕ / РАСХОДНИКИ / АРТЕФАКТЫ / КЛЮЧИ / ПРОЧЕЕ","SelectedCategory","selected save")]},
    SectionSpec{id:"items",title_key:"Предметы в инвентаре",controls:&[
        c!("list",List,"Инвентарь пуст / Предметы не найдены","SelectedItem","selected save"),
        c!("reset",Button,"Сбросить фильтры","clear search/category","filtered empty")]},
    SectionSpec{id:"editor",title_key:"Выбранный предмет",controls:&[
        c!("condition",TextField,"Состояние / прочность","edit condition","SelectedItem.CanEditCondition"),
        c!("count",TextField,"Количество в пачке","edit count","SelectedItem.CanEditCount"),
        c!("placement",DropDown,"Слот / Пояс / Рюкзак","edit placement","SelectedItem.CanEditPlacement"),
        c!("upgrades",List,"Модификации","edit upgrades","item supports upgrades"),
        c!("remove",Button,"Удалить предмет","SaveLibraryViewModel.RemoveSelectedItemCommand","SelectedItem.CanDelete"),
        c!("add",Button,"+ Добавить предмет","open AddItemDialog","SelectedSave.CanAddItems")]},
    SectionSpec{id:"add-item-dialog",title_key:"Добавить предмет в инвентарь",controls:&[
        c!("dialog-search",TextField,"Поиск по названию или ключу секции…","filter catalog","dialog open"),
        c!("dialog-list",List,"Поиск по каталогу предметов","select item","dialog open"),
        c!("dialog-qty",TextField,"Количество:","set quantity","item selected"),
        c!("dialog-add",Button,"Добавить","add selected item","valid item/quantity"),
        c!("dialog-cancel",Button,"Отмена","close","dialog open")]}];
const FACTIONS:&[SectionSpec]=&[SectionSpec{id:"relations",title_key:"Отношения с группировками",controls:&[
    c!("player-faction",DropDown,"Группировка игрока","set player faction","save supports edit"),
    c!("relations",Table,"ГРУППИРОВКА / ОЧКИ / СТАТУС","edit relation","selected save",["ГРУППИРОВКА","ОЧКИ","СТАТУС"]),
    c!("friend",Button,"Друг (+1500)","set +1500","relation.CanEdit"),
    c!("neutral",Button,"Нейтрал (0)","set 0","relation.CanEdit"),
    c!("enemy",Button,"Враг (-1500)","set -1500","relation.CanEdit")]}];
const STASHES:&[SectionSpec]=&[SectionSpec{id:"stashes",title_key:"Управление тайниками Зоны",controls:&[
    c!("list",List,"Управление тайниками Зоны","SelectedSave.Stashes","selected save"),
    c!("take",Toggle,"В рюкзак","StashSelectionChanged","item.CanEdit"),
    c!("inventory",DropDown,"Предмет из рюкзака","SelectedSave.Inventory","stash selected"),
    c!("put",Button,"В ТАЙНИК","ToggleStashPut","inventory item selected"),
    c!("create",Button,"+ СОЗДАТЬ В ТАЙНИКЕ","open AddItemDialog","SelectedSave.CanAddItems")]}];
const TRANSITIONS:&[SectionSpec]=&[
    SectionSpec{id:"relocation",title_key:"ПЕРЕНОС ПЕРСОНАЖА (ЭКСПЕРИМЕНТАЛЬНО)",controls:&[
        c!("anchors",DropDown,"Куда перенести","SelectedSave.RelocationAnchors","anchors available"),
        c!("move",Button,"ПЕРЕНЕСТИ","SaveLibraryViewModel.RelocateActor","anchor selected")]},
    SectionSpec{id:"list",title_key:"Переходы между локациями",controls:&[
        c!("table",Table,"ПЕРЕХОД / ИМЯ В СОХРАНЕНИИ / ТОЧКА / ID В РЕЕСТРЕ","","selected save",["ПЕРЕХОД","ИМЯ В СОХРАНЕНИИ / ТОЧКА","ID В РЕЕСТРЕ"])]}];
const BACKUPS:&[SectionSpec]=&[
    SectionSpec{id:"history",title_key:"РЕЗЕРВНЫЕ КОПИИ",controls:&[
        c!("refresh",Button,"Обновить бэкапы","SaveLibraryViewModel.RefreshBackups","always"),
        c!("list",List,"РЕЗЕРВНЫХ КОПИЙ ПОКА НЕТ","SelectedBackup","always")]},
    SectionSpec{id:"details",title_key:"СВЕДЕНИЯ О КОПИИ",controls:&[
        c!("details",Card,"СОЗДАНА / ИСХОДНЫЙ ФАЙЛ / ФАЙЛ КОПИИ / ФАЙЛ ПОСЛЕ ОПЕРАЦИИ / SHA-256 ИСХОДНОГО ФАЙЛА / SHA-256 КОПИИ","","SelectedBackup"),
        c!("restore",Button,"Восстановить на место","restore in place","SelectedBackup can restore"),
        c!("restore-copy",Button,"Восстановить в копию","restore to copy","SelectedBackup")]}];
const COMPARE:&[SectionSpec]=&[
    SectionSpec{id:"source",title_key:"СРАВНЕНИЕ СОХРАНЕНИЙ",controls:&[
        c!("target",DropDown,"Сравнить с","select target","targets available"),
        c!("swap",Button,"Поменять местами только отображение столбцов A и B","CompareViewModel.SwapSides","comparison loaded"),
        c!("summary",Card,"РАЗЛИЧИЙ / ДОБАВЛЕНО / УДАЛЕНО / ИЗМЕНЕНО","","comparison loaded")]},
    SectionSpec{id:"filters",title_key:"ФИЛЬТРЫ СРАВНЕНИЯ",controls:&[
        c!("category",DropDown,"Категория","CompareViewModel.SelectCategoryCommand","comparison loaded"),
        c!("change",DropDown,"Тип изменения","CompareViewModel.SelectChangeTypeCommand","comparison loaded"),
        c!("search",TextField,"Поиск по различиям","filter","comparison loaded")]},
    SectionSpec{id:"diff",title_key:"Различия",controls:&[
        c!("table",Table,"ТИП / ПАРАМЕТР / ОБЪЕКТ / ЗНАЧЕНИЕ A / ЗНАЧЕНИЕ B / КАТЕГОРИЯ","","comparison loaded",["ТИП","ПАРАМЕТР / ОБЪЕКТ","ЗНАЧЕНИЕ A","ЗНАЧЕНИЕ B","КАТЕГОРИЯ"]),
        c!("export",Button,"Экспорт CSV","ExportCsvAsync","comparison loaded"),
        c!("copy",Button,"Копировать список","CopyListAsync","comparison loaded")]}];
const TIMELINE:&[SectionSpec]=&[SectionSpec{id:"timeline",title_key:"ИСТОРИЯ СОХРАНЕНИЙ",controls:&[
    c!("entries",List,"Временная последовательность строится только по времени изменения реальных файлов.","","always"),
    c!("compare",Button,"Сравнить с предыдущим","TimelineViewModel.CompareAdjacent","previous save exists")]}];
const SAVE_DOCTOR:&[SectionSpec]=&[SectionSpec{id:"doctor",title_key:"ДОКТОР СОХРАНЕНИЯ",controls:&[
    c!("file",TextField,"Файл","set path","always"),c!("browse",Button,"Открыть сохранение","StorageProvider.OpenFilePicker","picker available"),
    c!("analyze",Button,"ПРОВЕРИТЬ СОХРАНЕНИЕ","SaveDoctorViewModel.AnalyzeCommand","file selected"),
    c!("repair",Button,"ИСПРАВИТЬ КВЕСТЫ","SaveDoctorViewModel.RepairQuestsCommand","verified repair available"),
    c!("fix",Button,"УСТАНОВИТЬ ИСПРАВЛЕНИЕ ИГРЫ","SaveDoctorViewModel.OpenGameFixCommand","fix available")]}];
const GAMES:&[SectionSpec]=&[
    SectionSpec{id:"installs",title_key:"НАЙДЕННЫЕ УСТАНОВКИ",controls:&[
        c!("discover",Button,"Найти установки","GameDoctorViewModel.DiscoverInstallationsCommand","not busy"),
        c!("list",List,"Найденные установки игр","SelectedInstallation","always"),
        c!("doctor",Button,"Открыть Доктор игры","OpenGameDoctor","always")]},
    SectionSpec{id:"selected",title_key:"Выбранная игра",controls:&[
        c!("game",Card,"Статус / Платформа / Номер сборки / Папка игры","","installation selected"),
        c!("fixes",Button,"Исправления","navigate:game-fixes","installation selected"),
        c!("game-doctor",Button,"Доктор игры","navigate:game-doctor","installation selected"),
        c!("environment",Button,"Среда игры","navigate:environment","installation selected"),
        c!("companion",Button,"Компаньон","navigate:companion","companion available"),
        c!("achievements",Button,"Достижения","navigate:achievements","Steam game")]}];
const GAME_FIXES:&[SectionSpec]=&[
    SectionSpec{id:"target",title_key:"ИСПРАВЛЕНИЯ ИГРЫ",controls:&[
        c!("game",DropDown,"ИГРА","SelectedTarget","always"),c!("path",TextField,"ПАПКА ИГРЫ (РУЧНОЙ ВЫБОР)","set path","target selected"),
        c!("browse",Button,"Обзор…","StorageProvider.PickFolder","picker available"),
        c!("check",Button,"ПРОВЕРИТЬ СОВМЕСТИМОСТЬ","GameFixesViewModel.CheckInstallationCommand","target/path valid")]},
    SectionSpec{id:"fixes",title_key:"ДОСТУПНЫЕ ИСПРАВЛЕНИЯ",controls:&[
        c!("categories",List,"КАТЕГОРИИ","select category","checked"),c!("list",List,"ПРОБЛЕМА / ИЗМЕНЕНИЕ / ПОДДЕРЖИВАЕМЫЕ STEAM-СБОРКИ / ЗАТРАГИВАЕМЫЕ ФАЙЛЫ / ИСТОЧНИК","select fixes","checked"),
        c!("recommended",Button,"ПРИМЕНИТЬ: РЕКОМЕНДУЕМЫЕ","GameFixesViewModel.ApplyRecommendedPresetCommand","compatible"),
        c!("essential",Button,"ПРИМЕНИТЬ: ОБЯЗАТЕЛЬНЫЕ","GameFixesViewModel.ApplyEssentialPresetCommand","compatible"),
        c!("safe",Button,"ПРИМЕНИТЬ: ВСЕ БЕЗОПАСНЫЕ","GameFixesViewModel.ApplyAllSafePresetCommand","compatible"),
        c!("install",Button,"УСТАНОВИТЬ ВЫБРАННОЕ","GameFixesViewModel.InstallCommand","installable selected"),
        c!("update",Button,"ОБНОВИТЬ ВЫБРАННОЕ","GameFixesViewModel.UpdateCommand","updatable selected"),
        c!("remove",Button,"УДАЛИТЬ И ВОССТАНОВИТЬ","GameFixesViewModel.RemoveCommand","managed installed selected")]}];
const GAME_DOCTOR:&[SectionSpec]=&[SectionSpec{id:"doctor",title_key:"ДОКТОР ИГРЫ",controls:&[
    c!("game",DropDown,"ИГРА","select target","always"),c!("discover",Button,"НАЙТИ УСТАНОВКИ","GameDoctorViewModel.DiscoverInstallationsCommand","not busy"),
    c!("installs",List,"ОБНАРУЖЕННЫЕ УСТАНОВКИ","select installation","always"),c!("path",TextField,"ПАПКА ИГРЫ (РУЧНОЙ ВЫБОР)","set path","target selected"),
    c!("browse",Button,"Обзор…","StorageProvider.PickFolder","picker available"),c!("analyze",Button,"ПРОВЕРИТЬ УСТАНОВКУ","GameDoctorViewModel.AnalyzeCommand","path/install selected"),
    c!("disable-mods",Button,"ВРЕМЕННО ОТКЛЮЧИТЬ КАСТОМНЫЕ МОДЫ","GameDoctorViewModel.DisableS2ModsCommand","S2 mods detected"),
    c!("restore-mods",Button,"ВОССТАНОВИТЬ КАСТОМНЫЕ МОДЫ","GameDoctorViewModel.RestoreS2ModsCommand","disabled-mod backup exists"),
    c!("mods",List,"МОДИФИКАЦИИ","","analysis available")]}];
const ENVIRONMENT:&[SectionSpec]=&[
    SectionSpec{id:"snapshots",title_key:"УПРАВЛЯЕМЫЕ СНИМКИ",controls:&[
        c!("list",List,"Снимков пока нет: создайте первый кнопкой ниже.","select snapshot","always"),
        c!("create",Button,"СОЗДАТЬ СНИМОК","ToolkitEnvironmentViewModel.CreateSnapshotCommand","not busy"),
        c!("restore",Button,"ВОССТАНОВИТЬ ВЫБРАННЫЙ","ToolkitEnvironmentViewModel.RestoreSnapshotCommand","selected"),
        c!("delete",Button,"УДАЛИТЬ СНИМОК","ToolkitEnvironmentViewModel.DeleteSnapshotCommand","selected"),
        c!("refresh",Button,"ОБНОВИТЬ СПИСОК","ToolkitEnvironmentViewModel.RefreshCommand","not busy")]},
    SectionSpec{id:"profiles",title_key:"ПРОФИЛИ ИГРЫ",controls:&[
        c!("name",TextField,"ИМЯ ПРОФИЛЯ","set name","always"),c!("profiles",List,"Профилей пока нет: введите имя и сохраните текущее состояние.","select profile","always"),
        c!("save",Button,"СОХРАНИТЬ ТЕКУЩЕЕ СОСТОЯНИЕ","ToolkitEnvironmentViewModel.SaveProfileCommand","valid name"),
        c!("apply",Button,"ПРИМЕНИТЬ ПРОФИЛЬ","ToolkitEnvironmentViewModel.ApplyProfileCommand","selected"),
        c!("delete-profile",Button,"УДАЛИТЬ ПРОФИЛЬ","ToolkitEnvironmentViewModel.DeleteProfileCommand","selected")]},
    SectionSpec{id:"config",title_key:"НАСТРОЙКИ user.ltx",controls:&[
        c!("path",TextField,"ПУТЬ К СУЩЕСТВУЮЩЕМУ user.ltx","set path","always"),c!("browse",Button,"ОБЗОР…","StorageProvider.OpenFilePicker","picker available"),
        c!("load",Button,"ЗАГРУЗИТЬ","ToolkitEnvironmentViewModel.LoadConfigCommand","valid path"),c!("settings",List,"Текущее: / По умолчанию:","select setting","loaded"),
        c!("value",TextField,"Новое значение","set value","setting selected"),c!("apply-value",Button,"ПРИМЕНИТЬ","ToolkitEnvironmentViewModel.ApplyConfig","row.CanApplyValue"),
        c!("default",Button,"ПО УМОЛЧАНИЮ","ToolkitEnvironmentViewModel.RestoreConfigDefault","setting selected")]},
    SectionSpec{id:"audit",title_key:"АУДИТ УСТАНОВКИ",controls:&[
        c!("audit",Button,"ПРОВЕРИТЬ","ToolkitEnvironmentViewModel.AuditCommand","not busy"),c!("rows",List,"Файлы с валидными манифестами отмечаются как управляемые.","","audit complete"),
        c!("cleanup",Button,"ОЧИСТИТЬ УСТАРЕВШИЙ ФИКС","ToolkitEnvironmentViewModel.CleanupOrphanAsync","safe orphan row")]}];
const COMPANION:&[SectionSpec]=&[
    SectionSpec{id:"status",title_key:"СТАТУС И СВЯЗЬ",controls:&[
        c!("target",DropDown,"Целевая игра:","select target","always"),c!("status",Card,"Версия мода: / Связь / Задержка: / Путь установки:","","always"),
        c!("install",Button,"УСТАНОВИТЬ / ОБНОВИТЬ","install/update","supported target"),c!("remove",Button,"УДАЛИТЬ","remove","installed"),
        c!("ping",Button,"ПРОВЕРИТЬ СВЯЗЬ","ping","installed"),c!("refresh",Button,"ОБНОВИТЬ СТАТУС","refresh","always")]},
    SectionSpec{id:"inspector",title_key:"ЖИВОЙ ИНСПЕКТОР",controls:&[
        c!("inspect",Button,"ПОЛУЧИТЬ ДАННЫЕ","info + list_inventory","connected"),c!("info",Card,"Информация игрока","","data loaded"),c!("inventory",List,"Инвентарь игрока","","data loaded")]},
    SectionSpec{id:"s2",title_key:"S.T.A.L.K.E.R. 2 — команды игры (экспериментально)",controls:&[
        c!("god-on",Button,"Бессмертие: вкл","Stalker2CommandCommand","SupportsStalker2Commands"),c!("god-off",Button,"Бессмертие: выкл","Stalker2CommandCommand","SupportsStalker2Commands"),
        c!("fly-on",Button,"Полёт: вкл","Stalker2CommandCommand","SupportsStalker2Commands"),c!("fly-off",Button,"Полёт: выкл","Stalker2CommandCommand","SupportsStalker2Commands"),
        c!("time5",Button,"Время ×5","Stalker2CommandCommand","SupportsStalker2Commands"),c!("time1",Button,"Время: норма","Stalker2CommandCommand","SupportsStalker2Commands")]},
    SectionSpec{id:"hotkeys",title_key:"ГОРЯЧИЕ КЛАВИШИ",controls:&[
        c!("all-games",List,"ВСЕ ИГРЫ","select targets","always"),c!("install-all",Button,"УСТАНОВИТЬ / ОБНОВИТЬ ВО ВСЕ ОТМЕЧЕННЫЕ","install selected","targets selected"),
        c!("manual-path",TextField,"ПАПКА ИГРЫ (РУЧНОЙ ВЫБОР)","set path","target selected"),c!("apply-path",Button,"ПРИМЕНИТЬ","apply path","valid path"),
        c!("enabled",Toggle,"Горячие клавиши","ToggleHotkeysCommand","companion available"),c!("save-hotkeys",Button,"СОХРАНИТЬ КЛАВИШИ","save hotkeys","valid"),
        c!("defaults",Button,"ПО УМОЛЧАНИЮ","reset hotkeys","always")]}];
const ACHIEVEMENTS:&[SectionSpec]=&[SectionSpec{id:"achievements",title_key:"ДОСТИЖЕНИЯ STEAM",controls:&[
    c!("app",DropDown,"S.T.A.L.K.E.R.: Зов Припяти / Чистое Небо / Тень Чернобыля / S.T.A.L.K.E.R. 2","SelectedAppId","Steam available"),
    c!("search",TextField,"Поиск по названию или описанию...","filter","app selected"),c!("refresh",Button,"ОБНОВИТЬ","AchievementsViewModel.RefreshCommand","app selected"),
    c!("progress",Progress,"Прогресс","","loaded"),c!("list",List,"ПОЛУЧИТЬ / СНЯТЬ","RequestToggle","loaded"),
    c!("confirm",Button,"ПОДТВЕРДИТЬ","AchievementsViewModel.ConfirmToggleCommand","confirmation open"),c!("cancel",Button,"ОТМЕНА","AchievementsViewModel.CancelToggleCommand","confirmation open")]}];
const CLOUD:&[SectionSpec]=&[
    SectionSpec{id:"cloud",title_key:"СОХРАНЕНИЯ В STEAM CLOUD",controls:&[
        c!("app",DropDown,"Все игры","SelectedAppId","Steam available"),c!("refresh",Button,"ОБНОВИТЬ СПИСОК","CloudViewModel.RefreshCommand","Steam available"),
        c!("list",List,"Сейвов в облаке не найдено. Выберите игру или нажмите «Обновить список».","select cloud save","Steam available")]},
    SectionSpec{id:"actions",title_key:"ДЕЙСТВИЯ С СОХРАНЕНИЕМ",controls:&[
        c!("download",Button,"СКАЧАТЬ В ЛОКАЛЬНЫЕ","CloudViewModel.DownloadSelectedCommand","cloud save selected"),
        c!("write",Button,"ЗАПИСАТЬ В ОБЛАКО...","CloudViewModel.RequestWriteCommand","writable pair"),
        c!("safety",Banner,"Безопасность Steam Cloud:","","always"),c!("confirm",Toggle,"Я подтверждаю перезапись","set confirmation","confirmation open"),
        c!("confirm-write",Button,"ЗАПИСАТЬ","CloudViewModel.ConfirmWriteCommand","confirmed"),c!("cancel",Button,"ОТМЕНА","CloudViewModel.CancelWriteCommand","confirmation open")]}];
const ENCYCLOPEDIA:&[SectionSpec]=&[SectionSpec{id:"catalog",title_key:"ЭНЦИКЛОПЕДИЯ ПРЕДМЕТОВ",controls:&[
    c!("items",List,"Вес: {0} · Цена: {1} · Секция: {2}","select item","catalog loaded"),
    c!("save",Button,"В сохранение","EncyclopediaViewModel.AddToSave","save/item compatible"),
    c!("game",Button,"В игру","EncyclopediaViewModel.SpawnViaCompanionAsync","companion connected")]}];
const CAPABILITIES:&[SectionSpec]=&[SectionSpec{id:"matrix",title_key:"МАТРИЦА ВОЗМОЖНОСТЕЙ РЕДАКТОРА",controls:&[
    c!("matrix",Table,"ОПЕРАЦИЯ","","always",["ОПЕРАЦИЯ","ТЧ","ЧН","ЗП","ТЧ EE","ЧН EE","ЗП EE","S2"]),
    c!("legend",Card,"Запись (Verified) / Эксперим. (Experimental) / Чтение (Research) / Нет (Unsupported) / Блок UI","","always"),
    c!("warning",Banner,"Запись сейвов S.T.A.L.K.E.R. 2 выключена, пока изменения не проверены в самой игре.","","always")]}];
const UPDATES:&[SectionSpec]=&[SectionSpec{id:"updates",title_key:"ОБНОВЛЕНИЕ ПРИЛОЖЕНИЯ",controls:&[
    c!("versions",Card,"ТЕКУЩАЯ ВЕРСИЯ / ПОСЛЕДНЯЯ ВЕРСИЯ","","always"),
    c!("check",Button,"ПРОВЕРИТЬ ОБНОВЛЕНИЯ","UpdatesViewModel.CheckUpdatesCommand","not busy"),
    c!("download",Button,"СКАЧАТЬ ОБНОВЛЕНИЕ","UpdatesViewModel.DownloadCommand","update available"),
    c!("install",Button,"УСТАНОВИТЬ ОБНОВЛЕНИЕ","UpdatesViewModel.InstallCommand","download complete"),
    c!("progress",Progress,"Статус обновления","","busy"),c!("error",Banner,"Ошибка обновления","","error")]}];
const SETTINGS:&[SectionSpec]=&[
    SectionSpec{id:"general",title_key:"ОБЩИЕ / ИНТЕРФЕЙС",controls:&[
        c!("sections",List,"Разделы","SelectCategory","always"),c!("language",DropDown,"Язык интерфейса:","set language","always"),
        c!("theme",DropDown,"Тема оформления:","StalkerTheme.ApplyAppearance","always"),c!("accent",DropDown,"Акцентный цвет:","StalkerTheme.ApplyAppearance","always"),
        c!("scale",DropDown,"Масштаб интерфейса:","StalkerTheme.ApplyAppearance","always"),c!("sound",Toggle,"Звуки интерфейса","set sound","always"),
        c!("music",Toggle,"Музыка главного меню игры открытого сейва","set music","always"),c!("volume",TextField,"Громкость звуков:","set volume","sound enabled")]},
    SectionSpec{id:"paths",title_key:"ПУТИ И АВТОПОИСК",controls:&[
        c!("dirs",List,"Каталоги сохранений","","always"),c!("remove",Button,"Удалить","SettingsViewModel.RemoveSaveDirectoryCommand","directory selected"),
        c!("path",TextField,"Путь к папке с сейвами (savedgames или SaveGames)…","set pending path","always"),c!("browse",Button,"Обзор…","StorageProvider.PickFolder","picker available"),
        c!("add",Button,"Добавить папку","SettingsViewModel.AddSaveDirectoryCommand","valid path"),c!("detect",Button,"Автопоиск папок на диске","SettingsViewModel.AutoDetectSaveDirectoriesCommand","not busy"),
        c!("backup",TextField,"Папка для создания резервных копий и журналов восстановления:","set backup path","always")]},
    SectionSpec{id:"support",title_key:"ИНСТРУМЕНТЫ ДЛЯ ПОДДЕРЖКИ",controls:&[
        c!("updates",Button,"Открыть обновления приложения","navigate:updates","always"),c!("fixes",Button,"Управление исправлениями игры","navigate:game-fixes","always"),
        c!("diagnostics",Button,"Проверить окружение","DiagnosticsViewModel.RunChecksCommand","diagnostics available"),c!("export",Button,"Сохранить отчёт…","export report","report available"),
        c!("send",Button,"Отправить отчёт сейчас","DiagnosticsViewModel.SendNowCommand","report available"),c!("dismiss",Button,"Скрыть ошибку","DiagnosticsViewModel.DismissCrashCommand","previous crash"),
        c!("reports",Toggle,"Отправлять разработчику журнал раз в сутки и после сбоя (без путей, имён, Steam ID и сейвов)","set SendReports","always")]},
    SectionSpec{id:"about",title_key:"ВЕРСИЯ",controls:&[
        c!("version",Card,"S.T.A.L.K.E.R. Save Editor {0}","","always"),c!("cloud",Button,"Открыть Steam Cloud","navigate:cloud","always"),
        c!("save",Button,"Сохранить настройки","SettingsViewModel.SaveSettingsCommand","settings valid")]}];

/// MainWindow shell inventory (shell has no ScreenId).
pub const SHELL:&[SectionSpec]=&[SectionSpec{id:"shell",title_key:"S.T.A.L.K.E.R. Save Editor",controls:&[
    c!("sidebar",List,"СОХРАНЕНИЯ / ИГРЫ / ИНСТРУМЕНТЫ","navigate ScreenId","always"),
    c!("save-pane",List,"Сохранения","select save","always"),c!("save",Button,"Сохранить","commit draft","dirty writable save"),
    c!("undo",Button,"Отменить","undo draft","can undo"),c!("redo",Button,"Повторить","redo draft","can redo"),
    c!("banner",Banner,"Уведомления","dismiss/navigate","banner present"),c!("wizard",Card,"Мастер первого запуска","wizard actions","first run")]}];

/// All canonical screens in ScreenId::ALL order.
pub const SCREENS:&[ScreenSpec]=&[
    ScreenSpec{id:ScreenId::Overview,title_key:"ОБЗОР",sections:OVERVIEW},
    ScreenSpec{id:ScreenId::Inventory,title_key:"ИНВЕНТАРЬ",sections:INVENTORY},
    ScreenSpec{id:ScreenId::Factions,title_key:"ФРАКЦИИ",sections:FACTIONS},
    ScreenSpec{id:ScreenId::Stashes,title_key:"ТАЙНИКИ",sections:STASHES},
    ScreenSpec{id:ScreenId::Transitions,title_key:"ПЕРЕХОДЫ",sections:TRANSITIONS},
    ScreenSpec{id:ScreenId::Backups,title_key:"БЭКАПЫ",sections:BACKUPS},
    ScreenSpec{id:ScreenId::Compare,title_key:"СРАВНЕНИЕ",sections:COMPARE},
    ScreenSpec{id:ScreenId::Timeline,title_key:"ИСТОРИЯ СОХРАНЕНИЙ",sections:TIMELINE},
    ScreenSpec{id:ScreenId::SaveDoctor,title_key:"ДОКТОР СОХРАНЕНИЯ",sections:SAVE_DOCTOR},
    ScreenSpec{id:ScreenId::Games,title_key:"ОБЗОР ИГР",sections:GAMES},
    ScreenSpec{id:ScreenId::GameFixes,title_key:"ИСПРАВЛЕНИЯ ИГРЫ",sections:GAME_FIXES},
    ScreenSpec{id:ScreenId::GameDoctor,title_key:"ДОКТОР ИГРЫ",sections:GAME_DOCTOR},
    ScreenSpec{id:ScreenId::Environment,title_key:"СРЕДА ИГРЫ",sections:ENVIRONMENT},
    ScreenSpec{id:ScreenId::Companion,title_key:"КОМПАНЬОН",sections:COMPANION},
    ScreenSpec{id:ScreenId::Achievements,title_key:"ДОСТИЖЕНИЯ",sections:ACHIEVEMENTS},
    ScreenSpec{id:ScreenId::Cloud,title_key:"ОБЛАКО",sections:CLOUD},
    ScreenSpec{id:ScreenId::Encyclopedia,title_key:"ЭНЦИКЛОПЕДИЯ",sections:ENCYCLOPEDIA},
    ScreenSpec{id:ScreenId::Capabilities,title_key:"ВОЗМОЖНОСТИ",sections:CAPABILITIES},
    ScreenSpec{id:ScreenId::Updates,title_key:"ОБНОВЛЕНИЯ",sections:UPDATES},
    ScreenSpec{id:ScreenId::Settings,title_key:"НАСТРОЙКИ",sections:SETTINGS},
];

#[cfg(test)]
mod tests {
    use super::{SCREENS, SHELL};
    use crate::screens::ScreenId;
    use std::collections::BTreeSet;

    #[test]
    fn covers_screen_id_once() {
        assert_eq!(SCREENS.len(), ScreenId::ALL.len());
        let ids: BTreeSet<_> = SCREENS.iter().map(|screen| screen.id).collect();
        assert_eq!(ids.len(), ScreenId::ALL.len());
        assert!(ScreenId::ALL.iter().all(|id| ids.contains(id)));
    }

    #[test]
    fn control_ids_are_unique_per_screen() {
        for screen in SCREENS {
            let mut ids = BTreeSet::new();
            for section in screen.sections {
                for control in section.controls {
                    assert!(ids.insert(control.id), "duplicate {} in {:?}", control.id, screen.id);
                }
            }
        }
        let mut ids = BTreeSet::new();
        for section in SHELL {
            for control in section.controls { assert!(ids.insert(control.id)); }
        }
    }
}
