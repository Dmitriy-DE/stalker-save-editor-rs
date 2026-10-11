# S.T.A.L.K.E.R. Save Editor 2.0.0

*English first, Russian below. Русский текст — ниже.*

Version 2.0.0 is a new program: the editor was rewritten from C# into Rust. It opens the same saves as 1.3.1
and keeps the same safety rules — a backup and a journal before every write, an atomic replace, a read-back
check, and the original restored if anything fails.

**Read this first.** The automated tests of 2.0.0 pass, but almost nothing written by this version has been
loaded in a game: one edited Call of Pripyat save was. Everything else below that changes a save is marked in
the program as *Experimental* or is listed here as not checked. Keep the backups the editor makes.

## What is new for the player, compared with 1.3.1

- **Rewritten in Rust.** One native program per platform.
- **A new look.** The interface was redrawn: new layout, colours and icons.
- **Enhanced Editions: more can be edited.** Money, ammunition stacks, adding and removing items, equipment
  condition, the player's faction; upgrades in Clear Sky EE and Call of Pripyat EE; item placement in Call of
  Pripyat EE. All of it *Experimental*: measured on real Enhanced Edition saves, not loaded in the games.
- **Stashes.** Shadow of Chernobyl and Call of Pripyat: take an item from a stash into the backpack.
  S.T.A.L.K.E.R. 2: move items between a stash and the backpack, both ways.
- **Save overview.** Health, rank, reputation, kills, weather and PDA tasks, where the save format proves them.
- **Linux AppImage**, next to the `.tar.gz` and the `.deb`.
- **Local performance metrics**, sent only if you turn sending on and confirm each upload.

## Carried over from 1.3.1, written anew

These were in 1.3.1 and are meant to behave the same; all of them were reimplemented, so treat them as new code:
the in-game companion (Esc, then F1; installed only when you ask), the game fixes (149 definitions, each checked
against file hashes and removable), signed updates checked on request, item pictures, game menu sounds, the
browser edition (the file never leaves the browser; the edited copy is downloaded back), Steam Cloud
transactions, the save doctor and the game doctor.

## Known limitations

- S.T.A.L.K.E.R. 2: adding an item is not available (the length of an item record in the save is not confirmed).
- S.T.A.L.K.E.R. 2: upgrades are not available (the structure was not found in the save; there is no catalogue
  of names).
- S.T.A.L.K.E.R. 2: relations are not available (the numeric relations table was not isolated).
- S.T.A.L.K.E.R. 2: saves written by game version 1.0.x can be read, but equipped gear cannot be edited — load
  and save once in the current game first.
- Shadow of Chernobyl (original): upgrades are not available — the edit is marked unsupported for this format.
- Enhanced Editions: relations are not available — none of the 26 Enhanced Edition saves examined holds a
  relation entry, so there is nothing to edit.
- Enhanced Editions: taking items out of a stash is not available.
- Shadow of Chernobyl and Call of Pripyat: an item the editor took out of a stash cannot be put back until the
  game itself has recorded its place (load and save in the game).
- Clear Sky: taking items out of a stash is not available — a stash item has no proven placement field; the
  write refuses on 62 of 62 saves.
- Shadow of Chernobyl EE and Clear Sky EE: item placement is not available — the slot field is not proven.
- Shadow of Chernobyl EE: upgrades are not available — no upgrade catalogue, and no upgraded items in the saves.

## Not verified

- **In the games.** Edits of Shadow of Chernobyl, Clear Sky, the three Enhanced Editions and S.T.A.L.K.E.R. 2
  were not loaded in the games. Only one edited Call of Pripyat save was.
- **Windows and macOS.** Built and tested in CI; the program was not started by a person on either. The macOS
  images are not signed or notarized.
- **The companion and the game fixes in a running game**, and sound — nobody has listened to it.
- **Steam Workshop.** The companion packages for the Enhanced Editions are not uploaded; the editor shows "not
  published".
- **APT.** There is no signed APT repository yet; the `.deb` is installed by hand.
- **Translations** were not proofread by native speakers.

## Where to get it and how to report a problem

Source and issues: https://github.com/Dmitriy-DE/stalker-save-editor-rs

This is an unofficial fan project. It is not made, endorsed or supported by GSC Game World.

---

# S.T.A.L.K.E.R. Save Editor 2.0.0

Версия 2.0.0 — новая программа: редактор переписан с C# на Rust. Он открывает те же сохранения, что и 1.3.1, и
сохраняет те же правила безопасности — резервная копия и журнал перед каждой записью, атомарная замена файла,
проверка чтением после записи и возврат оригинала, если что-то пошло не так.

**Сначала главное.** Автоматические тесты 2.0.0 проходят, но почти ничего из записанного этой версией в игре не
загружалось: загружен один изменённый сейв «Зова Припяти». Всё остальное, что меняет сейв, помечено в программе
как «экспериментально» или перечислено ниже как непроверенное. Не удаляйте резервные копии, которые делает
редактор.

## Что нового для игрока по сравнению с 1.3.1

- **Переписан на Rust.** Одна «родная» программа на платформу.
- **Новый вид.** Интерфейс перерисован: новая раскладка, цвета и значки.
- **Enhanced Edition: правится больше.** Деньги, пачки патронов, добавление и удаление предметов, прочность
  снаряжения, фракция игрока; апгрейды в ЧН EE и ЗП EE; размещение предметов в ЗП EE. Всё это
  «экспериментально»: измерено на настоящих сейвах EE, в играх не загружалось.
- **Тайники.** «Тень Чернобыля» и «Зов Припяти»: забрать предмет из тайника в рюкзак. S.T.A.L.K.E.R. 2: перенос
  между тайником и рюкзаком в обе стороны.
- **Обзор сейва.** Здоровье, ранг, репутация, убийства, погода и задания КПК — там, где формат сейва это
  доказывает.
- **AppImage для Linux** — рядом с `.tar.gz` и `.deb`.
- **Локальные метрики производительности**; отправляются, только если вы включили отправку и подтвердили каждую.

## Перенесено из 1.3.1 и написано заново

Это было в 1.3.1 и должно работать так же; всё реализовано заново, поэтому относитесь к этому как к новому коду:
компаньон в игре (Esc, затем F1; ставится только по вашей команде), исправления игр (149 определений, каждое
сверяет хеши файлов и снимается обратно), подписанные обновления с проверкой по запросу, значки предметов, звуки
меню игры, версия для браузера (файл не покидает браузер, изменённая копия скачивается обратно), транзакции Steam
Cloud, доктор сейва и доктор игры.

## Известные ограничения

- S.T.A.L.K.E.R. 2: добавление предмета не включено (длина записи предмета в сейве не подтверждена).
- S.T.A.L.K.E.R. 2: апгрейды не включены (структура в сейве не найдена, каталога имён нет).
- S.T.A.L.K.E.R. 2: отношения не включены (числовая таблица отношений не выделена).
- S.T.A.L.K.E.R. 2: сейвы версии игры 1.0.x читаются, но надетое снаряжение в них не правится — сначала
  загрузите и сохраните сейв в текущей версии игры.
- «Тень Чернобыля» (оригинал): апгрейды не включены — правка отмечена как неподдерживаемая для этого формата.
- Enhanced Edition: отношения не включены — ни в одном из 26 изученных сейвов EE нет записи отношений,
  редактировать нечего.
- Enhanced Edition: перенос из тайника не включён.
- «Тень Чернобыля» и «Зов Припяти»: предмет, взятый редактором из тайника, нельзя вернуть в тайник, пока игра
  сама не запишет его место (загрузить и сохранить в игре).
- «Чистое небо»: перенос из тайника не включён — у предметов тайника нет доказанного поля размещения; запись
  отказывает на 62 из 62 сейвов.
- ТЧ EE и ЧН EE: размещение предметов не включено — поле слота не доказано.
- ТЧ EE: апгрейды не включены — нет каталога апгрейдов и нет предметов с апгрейдами в сейвах.

## Не проверено

- **В играх.** Правки «Тени Чернобыля», «Чистого неба», трёх Enhanced Edition и S.T.A.L.K.E.R. 2 в играх не
  загружались. Загружен только один изменённый сейв «Зова Припяти».
- **Windows и macOS.** Собраны и протестированы в CI; человеком программа ни там, ни там не запускалась. Образы
  macOS не подписаны и не нотаризованы.
- **Компаньон и исправления игр в запущенной игре**, а также звук — ушами его никто не слушал.
- **Steam Workshop.** Пакеты компаньона для Enhanced Edition не загружены; редактор показывает «не опубликован».
- **APT.** Подписанного репозитория APT пока нет; `.deb` ставится вручную.
- **Переводы** не вычитаны носителями языков.

## Где взять и куда писать об ошибках

Исходный код и сообщения об ошибках: https://github.com/Dmitriy-DE/stalker-save-editor-rs

Это неофициальный фанатский проект. Он не создан, не одобрен и не поддерживается GSC Game World.
