# S.T.A.L.K.E.R. Save Editor 2.0.2

*English first, then Russian and Ukrainian. Русский текст — ниже. Український текст — нижче.*

A fix for Linux. Nothing else changed since [2.0.1](RELEASE_NOTES_2.0.1.md); the notes of
[2.0.0](RELEASE_NOTES_2.0.0.md), its known limitations and the list of what is not verified apply in full.

- **Linux, GNOME on Wayland: the editor did not start** ("Wayland: Broken pipe"). The compositor refused the
  editor's own Wayland backend. The editor now uses its X11 backend (through XWayland) whenever an X server is
  available, which is the case on a normal desktop session. The Wayland backend is still used when there is no X
  server, and can be forced with `SSE_WAYLAND=1`; it is not fixed yet.

Not verified: other Wayland compositors (KDE, Sway) were not tried.

---

# S.T.A.L.K.E.R. Save Editor 2.0.2

Исправление для Linux. Больше с [2.0.1](RELEASE_NOTES_2.0.1.md) ничего не менялось; заметки
[2.0.0](RELEASE_NOTES_2.0.0.md), её известные ограничения и список непроверенного действуют полностью.

- **Linux, GNOME на Wayland: редактор не запускался** («Wayland: Broken pipe»). Оболочка отвергала собственный
  Wayland-код редактора. Теперь редактор работает через X11 (XWayland), когда X-сервер доступен, — так на любом
  обычном рабочем столе. Wayland-код используется, только если X-сервера нет, и включается принудительно
  переменной `SSE_WAYLAND=1`; сам он пока не исправлен.

Не проверено: другие оболочки Wayland (KDE, Sway) не пробовались.

---

# S.T.A.L.K.E.R. Save Editor 2.0.2

Виправлення для Linux. Більше з [2.0.1](RELEASE_NOTES_2.0.1.md) нічого не змінювалося; нотатки
[2.0.0](RELEASE_NOTES_2.0.0.md), її відомі обмеження і перелік неперевіреного діють повністю.

- **Linux, GNOME на Wayland: редактор не запускався** («Wayland: Broken pipe»). Оболонка відкидала власний
  Wayland-код редактора. Тепер редактор працює через X11 (XWayland), коли X-сервер доступний, — так на будь-якому
  звичайному робочому столі. Wayland-код використовується, лише якщо X-сервера немає, і вмикається примусово
  змінною `SSE_WAYLAND=1`; сам він поки не виправлений.

Не перевірено: інші оболонки Wayland (KDE, Sway) не пробувалися.
