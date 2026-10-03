//! S4 (Gemini): installed games, fixes, game doctor, environment, encyclopedia.
//!
//! Replace each placeholder with a struct implementing [`Screen`]; keep the order.

use super::{Placeholder, Screen, ScreenId};

/// Screens of this package.
#[must_use]
pub fn screens() -> Vec<Box<dyn Screen>> {
    vec![
        Box::new(Placeholder::new(ScreenId::Games, "Найденные игры")),
        Box::new(Placeholder::new(ScreenId::GameFixes, "Исправления вылетов и ошибок")),
        Box::new(Placeholder::new(ScreenId::GameDoctor, "Проверка установки игры")),
        Box::new(Placeholder::new(ScreenId::Environment, "Инструменты и среда игры")),
        Box::new(Placeholder::new(ScreenId::Encyclopedia, "Предметы, персонажи, локации")),
    ]
}
