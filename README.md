# lazer2stable

Перенос всех карт из osu!lazer в osu!stable.

```
client.realm ──(realm-bridge, C#)──> id карт ──(Python, httpx async)──> .osz ──> osu!stable
```

## 1. Мост (C#)

Папка `bridge/` — команда `dump-beatmaps` для realm-bridge. Читает `client.realm` только на чтение
в динамическом режиме (схему lazer описывать не нужно) и печатает JSON с `online_id` каждого сета.

```
cd bridge
dotnet build -c Release
# проверка:
dotnet run -c Release -- dump-beatmaps --realm "%APPDATA%\osu\client.realm"
```

Если у тебя уже есть свой realm-bridge, добавь в него `case "dump-beatmaps"` и метод `DumpBeatmaps` из `Program.cs`.
Пакет Realm — 20.1.0, как и в основном мосту.

## 2. Скрипт (Python 3.10+)

```
pip install -r requirements.txt

# режим по умолчанию: каждый .osz открывается в osu!stable (запусти stable заранее)
python migrate.py --bridge "bridge/bin/Release/net8.0/realm-bridge.exe"

# быстрее и надёжнее для тысяч карт: распаковать прямо в Songs, потом F5 в stable
python migrate.py --bridge "bridge/bin/Release/net8.0/realm-bridge.exe" --mode extract
```

Полезные флаги: `--limit 20` (пробный прогон), `--no-video`, `--concurrency 4`,
`--stable-dir`, `--songs-dir`, `--realm`, `--ids-file ids.json` (без моста).
Скачанные `.osz` лежат в `osz_cache/`, при перезапуске повторно не качаются; неудачи пишутся в `osz_cache/failed.json`.

## Про «официальный API»

У osu! API v2 нет эндпоинта скачивания карт для OAuth-клиентов, скачать можно только из веб-сессии.
Поэтому источники идут по цепочке: `official` (включается, если задан cookie `osu_session` через
`--osu-session` или переменную `OSU_SESSION`) → зеркала `catboy`, `nerinyan`, `osudirect`.
Порядок меняется через `--sources`. Не ставь `--concurrency` слишком большим: зеркала отдают 429,
скрипт ждёт `Retry-After`, но лучше не злоупотреблять.
