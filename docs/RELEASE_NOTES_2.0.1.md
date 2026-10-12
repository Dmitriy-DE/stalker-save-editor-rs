# S.T.A.L.K.E.R. Save Editor 2.0.1

*English first, Russian below. Русский текст — ниже.*

A fix for upgrading from 1.3.x. Nothing else changed since [2.0.0](RELEASE_NOTES_2.0.0.md); its notes, known
limitations and the list of what is not verified apply in full.

- **Linux `.deb`:** after the upgrade from 1.3.x a launcher pinned from the old version failed with
  "Program 'stalker-save-editor' not found in $PATH". The package now also installs that name.
- **Windows installer:** shortcuts and taskbar pins made for the 1.3.x `StalkerSaveEditor.exe` start the new
  editor.
- **Update manifest:** the AppImage is no longer listed in it. A 1.3.x editor refuses a manifest with an entry it
  does not know, which would have kept 1.3.x from updating. The AppImage is downloaded from the release page and
  does not update itself.

Not verified: the Windows part was not run on Windows.

---

# S.T.A.L.K.E.R. Save Editor 2.0.1

Исправление обновления с 1.3.x. Больше с [2.0.0](RELEASE_NOTES_2.0.0.md) ничего не менялось; её заметки,
известные ограничения и список непроверенного действуют полностью.

- **Linux `.deb`:** после обновления с 1.3.x ярлык, закреплённый от старой версии, не запускался:
  «Program 'stalker-save-editor' not found in $PATH». Пакет теперь ставит и это имя.
- **Установщик Windows:** ярлыки и закрепления на панели задач, сделанные для `StalkerSaveEditor.exe` из 1.3.x,
  запускают новый редактор.
- **Файл обновлений:** AppImage в нём больше не указан. Редактор 1.3.x отвергает файл с незнакомой записью, и
  пользователи 1.3.x не смогли бы обновиться. AppImage скачивается со страницы выпуска и сам не обновляется.

Не проверено: часть для Windows на Windows не запускалась.
