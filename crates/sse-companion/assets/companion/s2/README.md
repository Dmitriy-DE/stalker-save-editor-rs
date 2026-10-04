# S.T.A.L.K.E.R. 2 Save Editor Companion (UE4SS Lua Prototype)

Внутриигровой мод-компаньон для **S.T.A.L.K.E.R. 2: Heart of Chornobyl** на базе **UE4SS (Unreal Engine 4/5 Scripting System)**, реализующий [протокол v1](../../../docs/COMPANION.md).

---

## 1. Возможности
- Приём и выполнение команд редактора во время работы игры (интервал опроса 2000 мс через `LoopAsync`).
- `ping` / `info` (проверка связи и версии).
- `money <delta>` — изменение количества купонов через нативную команду `XAddMoneyToPlayer`.
- `give <prototype_id> [count]` — спавн предметов в инвентарь через `XCreateItemInInventoryByID <PrototypeID> 0 <count> 1.0`.
- `pos` — получение текущих мировых координат игрока (`K2_GetActorLocation`).
- `teleport <x> <y> <z>` — мгновенное перемещение игрока через `K2_SetActorLocation` или `XTeleportTo`.

---

## 2. Установка
1. Установите актуальную версию **UE4SS** (ветка experimental UE5 для S.T.A.L.K.E.R. 2) в директорию исполняемых файлов игры:
   ```
   <GameRoot>/Stalker2/Binaries/Win64/
   ```
2. Скопируйте папку `SaveEditorCompanion/` в каталог модов UE4SS:
   ```
   <GameRoot>/Stalker2/Binaries/Win64/ue4ss/Mods/SaveEditorCompanion/
   ├── enabled.txt
   └── Scripts/
       └── main.lua
   ```
3. Запустите игру. При загрузке мира мод начнёт периодический опрос файлов команд в `%LOCALAPPDATA%\Stalker2\Saved\`.
