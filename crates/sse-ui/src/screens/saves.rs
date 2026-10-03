//! S2 (Codex): save overview, inventory, factions, stashes, transitions.
//!
//! Replace each placeholder with a struct implementing [`Screen`]; keep the order.

use super::{Placeholder, Screen, ScreenId};

/// Screens of this package.
#[must_use]
pub fn screens() -> Vec<Box<dyn Screen>> {
    vec![
        Box::new(Placeholder::new(ScreenId::Overview, "Сводка выбранного сохранения")),
        Box::new(Placeholder::new(ScreenId::Inventory, "Предметы, деньги, количество")),
        Box::new(Placeholder::new(ScreenId::Factions, "Отношения с группировками")),
        Box::new(Placeholder::new(ScreenId::Stashes, "Тайники и их содержимое")),
        Box::new(Placeholder::new(ScreenId::Transitions, "Переходы между локациями")),
    ]
}
