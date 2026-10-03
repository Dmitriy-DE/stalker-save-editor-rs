//! S5 (Gemini): companion, achievements, Steam Cloud, updates.
//!
//! Replace each placeholder with a struct implementing [`Screen`]; keep the order.

use super::{Placeholder, Screen, ScreenId};

/// Screens of this package.
#[must_use]
pub fn screens() -> Vec<Box<dyn Screen>> {
    vec![
        Box::new(Placeholder::new(ScreenId::Companion, "Мод-компаньон в игре")),
        Box::new(Placeholder::new(ScreenId::Achievements, "Достижения")),
        Box::new(Placeholder::new(ScreenId::Cloud, "Сохранения в Steam Cloud")),
        Box::new(Placeholder::new(ScreenId::Updates, "Обновления редактора")),
    ]
}
