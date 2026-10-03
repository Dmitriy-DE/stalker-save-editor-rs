//! S3 (Codex): backups, compare, history, save doctor.
//!
//! Replace each placeholder with a struct implementing [`Screen`]; keep the order.

use super::{Placeholder, Screen, ScreenId};

/// Screens of this package.
#[must_use]
pub fn screens() -> Vec<Box<dyn Screen>> {
    vec![
        Box::new(Placeholder::new(ScreenId::Backups, "Резервные копии сохранений")),
        Box::new(Placeholder::new(ScreenId::Compare, "Разница между двумя сохранениями")),
        Box::new(Placeholder::new(ScreenId::Timeline, "Хронология сохранений")),
        Box::new(Placeholder::new(ScreenId::SaveDoctor, "Проверка и лечение сохранения")),
    ]
}
