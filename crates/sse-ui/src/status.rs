//! Localization boundary for errors coming from save writers and durable storage.
//!
//! Core crates deliberately keep stable English diagnostics for CLI/oracle compatibility.
//! The desktop window translates those diagnostics here instead of leaking implementation
//! strings into the Russian UI.

const WRITER_REFUSALS: &[(&str, &str)] = &[
    ("This save was written by game version 1.0.x. It can be read, but its layout is not supported for editing; load it in the current game and save again.", "Этот сейв создан версией игры 1.0.x. Его можно читать, но запись для этой раскладки не поддерживается; загрузите сейв в актуальной игре и сохраните заново."),
    ("S2 write requires at least one change", "Для записи S2 требуется хотя бы одно изменение"),
    ("S2 stack handle is missing or ambiguous", "Пачка S2 не найдена однозначно"),
    ("S2 stack handle is not uniquely owned", "Пачка S2 не принадлежит инвентарю однозначно"),
    ("S2 stack item is not on the backpack grid", "Пачка S2 находится вне подтверждённой сетки рюкзака"),
    ("selected S2 stack is not confirmed editable", "Выбранная пачка S2 не подтверждена для безопасного редактирования"),
    ("X-Ray stack edit cannot be applied to an S2 save", "Правку пачки X-Ray нельзя применить к сейву S2"),
    ("S2 stack edit cannot be applied to an X-Ray save", "Правку пачки S2 нельзя применить к сейву X-Ray"),
    ("S2 durability handle is not uniquely owned", "Предмет S2 для изменения прочности не принадлежит инвентарю однозначно"),
    ("S2 durability handle is missing or ambiguous", "Предмет S2 для изменения прочности не найден однозначно"),
    ("S2 durability item is unresolved", "Предмет S2 для изменения прочности не удалось однозначно разобрать"),
    ("S2 durability field is not confirmed editable", "Поле прочности S2 не подтверждено для безопасной записи"),
    ("S2 stash transfer is disabled until the saved result is validated in-game", "Перенос из тайника S2 отключён до проверки результата в игре"),
    ("S2 stash move requires a fully resolved inventory", "Для переноса из тайника S2 инвентарь должен быть разобран без неоднозначностей"),
    ("S2 save has no confirmed stash block", "В сейве S2 не найден подтверждённый блок тайника"),
    ("S2 stash item handle is missing or ambiguous", "Предмет тайника S2 не найден однозначно"),
    ("S2 handle is not owned by the stash", "Предмет S2 не принадлежит выбранному тайнику"),
    ("S2 stash handle is duplicated", "Идентификатор предмета тайника S2 продублирован"),
    ("S2 object is not marked as stash-owned", "Объект S2 не помечен как принадлежащий тайнику"),
    ("S2 stash item footprint is missing", "У предмета тайника S2 нет подтверждённого размера в сетке"),
    ("S2 stash item does not fit a backpack", "Предмет из тайника S2 не помещается в рюкзак"),
    ("S2 backpack has no fitting free cells", "В рюкзаке S2 нет подходящих свободных ячеек"),
    ("S2 backpack owned-handle limit would be exceeded", "Будет превышен лимит предметов рюкзака S2"),
    ("S2 backpack grid-cell limit would be exceeded", "Будет превышен лимит ячеек рюкзака S2"),
    ("X-Ray change set is empty", "Список изменений X-Ray пуст"),
    ("X-Ray change set exceeds 100000 entries", "Список изменений X-Ray превышает допустимый размер"),
    ("money edit target is not the actor", "Изменение денег направлено не на объект игрока"),
    ("stack count must be in the range 1..65535", "Количество в пачке должно быть от 1 до 65535"),
    ("the actor object cannot be removed", "Объект игрока нельзя удалить"),
    ("stash take destination must be the actor", "Предмет из тайника можно перенести только игроку"),
    ("destination is not a verified level-changer anchor", "Точка перехода не подтверждена"),
    ("X-Ray changes overlap the same image bytes", "Изменения X-Ray пересекаются в одних и тех же байтах сейва"),
    ("X-Ray edit changed the detected save format", "После правки изменилось определение формата сейва X-Ray"),
    ("replacement save is empty", "Подготовленный к записи сейв пуст"),
    ("prepared source hash is not a SHA-256 value", "Хэш исходного сейва имеет неверный формат SHA-256"),
    ("source save has no parent directory", "Не удалось определить папку исходного сейва"),
    ("Source save has no parent directory.", "Не удалось определить папку исходного сейва."),
    ("Restore output has no parent directory.", "Не удалось определить папку назначения восстановления."),
    ("Restore output cannot be the backup file.", "Нельзя восстанавливать сейв поверх самого файла резервной копии."),
    ("Backup changed after its last verification.", "Резервная копия изменилась после последней проверки."),
    ("Restoring a symbolic-link save is refused.", "Восстановление сейва через символическую ссылку запрещено."),
    ("draft belongs to a different source save", "Черновик относится к другому исходному сейву"),
    ("unsupported draft schema", "Версия формата черновика не поддерживается"),
    ("draft has no current edits", "В черновике нет текущих изменений"),
    ("draft journal disappeared after editing", "Журнал черновика пропал после изменения"),
    ("draft journal disappeared after stash edit", "Журнал черновика пропал после изменения тайника"),
];

const INTERNAL_MARKERS: &[&str] = &[
    "S2 write ",
    "S2 stack ",
    "S2 durability ",
    "S2 stash ",
    "S2 backpack ",
    "X-Ray change ",
    "X-Ray edit ",
    "X-Ray stack ",
    "replacement save ",
    "prepared source ",
    "source save ",
    "Source save ",
    "Backup ",
    "Restore ",
    "Restoring ",
    "draft journal ",
    "unsupported draft ",
];

/// Converts stable core diagnostics into user-facing Russian text.
///
/// Unknown diagnostics that are clearly from the save-writer/storage boundary are deliberately
/// collapsed to a Russian safety refusal instead of exposing a new English implementation string.
#[must_use]
pub fn localize_writer_status(text: &str) -> String {
    let mut localized = text.to_owned();
    for (source, translated) in WRITER_REFUSALS {
        if localized.contains(source) {
            localized = localized.replace(source, translated);
        }
    }
    if INTERNAL_MARKERS.iter().any(|marker| localized.contains(marker)) {
        let prefix = if localized.starts_with("Не удалось сохранить черновик:") {
            "Не удалось сохранить черновик: "
        } else if localized.starts_with("Не удалось сохранить:") {
            "Не удалось сохранить: "
        } else if localized.starts_with("Ошибка:") {
            "Ошибка: "
        } else {
            ""
        };
        return format!("{prefix}операция отклонена внутренней проверкой безопасности формата.");
    }
    localized
}

#[cfg(test)]
mod tests {
    use super::localize_writer_status;

    #[test]
    fn translates_known_writer_refusal_inside_status() {
        assert_eq!(
            localize_writer_status("Не удалось сохранить: selected S2 stack is not confirmed editable"),
            "Не удалось сохранить: Выбранная пачка S2 не подтверждена для безопасного редактирования"
        );
    }

    #[test]
    fn hides_unknown_internal_writer_diagnostic() {
        let text = localize_writer_status("Ошибка: S2 stack future internal diagnostic");
        assert_eq!(
            text,
            "Ошибка: операция отклонена внутренней проверкой безопасности формата."
        );
        assert!(!text.contains("future internal diagnostic"));
    }

    #[test]
    fn leaves_normal_user_status_unchanged() {
        let text = "Сейв прочитан и проверен.";
        assert_eq!(localize_writer_status(text), text);
    }
}
