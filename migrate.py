#!/usr/bin/env python3
"""
lazer2stable: перенос всех карт из osu!lazer в osu!stable.

Шаги:
  1. realm-bridge (C#) читает client.realm и отдаёт список OnlineID битмапсетов.
  2. Уже установленные в stable карты (папки в Songs вида "<id> ...") пропускаются.
  3. .osz скачиваются параллельно (asyncio + httpx) с ретраями, учётом 429 и фолбэком по источникам.
  4. По мере скачивания каждый .osz открывается в osu!stable (режим open)
     либо распаковывается прямо в Songs (режим extract, потом F5 на экране выбора песни).
"""
from __future__ import annotations

import argparse
import asyncio
import json
import os
import platform
import re
import shlex
import shutil
import subprocess
import sys
import tempfile
import zipfile
from dataclasses import dataclass
from pathlib import Path

import httpx

# ----------------------------------------------------------------------------
# Источники .osz
# ----------------------------------------------------------------------------
# У официального API osu! v2 нет публичного эндпоинта скачивания карт для OAuth-клиентов:
# скачивание на osu.ppy.sh доступно только из веб-сессии (cookie osu_session).
# Поэтому "official" включается только если задан cookie, а дальше идут зеркала.


@dataclass(frozen=True)
class Source:
    name: str
    url: str          # шаблон, {id} подставляется
    url_novideo: str  # шаблон без видео
    needs_cookie: bool = False


SOURCES: dict[str, Source] = {
    "official": Source(
        "official",
        "https://osu.ppy.sh/beatmapsets/{id}/download",
        "https://osu.ppy.sh/beatmapsets/{id}/download?noVideo=1",
        needs_cookie=True,
    ),
    "catboy": Source("catboy", "https://catboy.best/d/{id}", "https://catboy.best/d/{id}n"),
    "nerinyan": Source(
        "nerinyan",
        "https://api.nerinyan.moe/d/{id}",
        "https://api.nerinyan.moe/d/{id}?noVideo=true",
    ),
    "osudirect": Source("osudirect", "https://osu.direct/api/d/{id}", "https://osu.direct/api/d/{id}"),
}
DEFAULT_ORDER = ["official", "catboy", "nerinyan", "osudirect"]


class NotFound(Exception):
    pass


class RateLimited(Exception):
    def __init__(self, retry_after: float):
        super().__init__(f"429, retry after {retry_after}s")
        self.retry_after = retry_after


class BadPayload(Exception):
    pass


# ----------------------------------------------------------------------------
# Пути
# ----------------------------------------------------------------------------
def default_lazer_dir() -> Path:
    system = platform.system()
    if system == "Windows":
        return Path(os.environ.get("APPDATA", str(Path.home() / "AppData/Roaming"))) / "osu"
    if system == "Darwin":
        return Path.home() / "Library/Application Support/osu"
    flatpak = Path.home() / ".var/app/sh.ppy.osu/data/osu"
    return flatpak if flatpak.exists() else Path.home() / ".local/share/osu"


def default_stable_dir() -> Path:
    if platform.system() == "Windows":
        return Path(os.environ.get("LOCALAPPDATA", str(Path.home() / "AppData/Local"))) / "osu!"
    return Path.home() / "osu!"


# ----------------------------------------------------------------------------
# Чтение id через realm-bridge
# ----------------------------------------------------------------------------
def split_cmd(cmd: str) -> list[str]:
    parts = shlex.split(cmd, posix=(os.name != "nt"))
    return [p.strip('"') for p in parts]


def ids_from_json(text: str) -> list[int]:
    data = json.loads(text)
    ids: list[int] = []
    for item in data:
        if isinstance(item, int):
            ids.append(item)
        elif isinstance(item, dict):
            for key in ("online_id", "OnlineID", "onlineId", "id"):
                if key in item:
                    ids.append(int(item[key]))
                    break
    return ids


def read_ids_from_bridge(bridge_cmd: str, realm_path: Path) -> list[int]:
    if not realm_path.is_file():
        sys.exit(f"client.realm не найден: {realm_path}")
    workdir = Path(tempfile.mkdtemp(prefix="lazer2stable-"))
    try:
        # Работаем с копией: lazer может быть запущен и держать базу открытой.
        copy = workdir / "client.realm"
        shutil.copy2(realm_path, copy)
        cmd = [*split_cmd(bridge_cmd), "dump-beatmaps", "--realm", str(copy)]
        print(f"[bridge] {' '.join(cmd)}")
        proc = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8")
        if proc.stderr.strip():
            print(proc.stderr.strip(), file=sys.stderr)
        if proc.returncode != 0:
            sys.exit(f"realm-bridge завершился с кодом {proc.returncode}")
        return ids_from_json(proc.stdout)
    finally:
        shutil.rmtree(workdir, ignore_errors=True)


# ----------------------------------------------------------------------------
# Уже установленные в stable
# ----------------------------------------------------------------------------
_ID_RE = re.compile(r"^(\d+)(?:\s|$)")


def installed_in_stable(songs_dir: Path) -> set[int]:
    found: set[int] = set()
    if songs_dir.is_dir():
        for entry in songs_dir.iterdir():
            if entry.is_dir() and (m := _ID_RE.match(entry.name)):
                found.add(int(m.group(1)))
    return found


# ----------------------------------------------------------------------------
# Скачивание
# ----------------------------------------------------------------------------
async def download_from(
    client: httpx.AsyncClient, src: Source, sid: int, dest: Path, *, no_video: bool, cookie: str | None
) -> None:
    template = src.url_novideo if no_video else src.url
    url = template.format(id=sid)
    headers: dict[str, str] = {}
    if src.needs_cookie:
        headers["Cookie"] = f"osu_session={cookie}"
        headers["Referer"] = f"https://osu.ppy.sh/beatmapsets/{sid}"

    part = dest.with_suffix(".part")
    try:
        async with client.stream("GET", url, headers=headers) as r:
            if r.status_code in (404, 410):
                raise NotFound(src.name)
            if r.status_code == 429:
                try:
                    wait = float(r.headers.get("Retry-After", "10"))
                except ValueError:
                    wait = 10.0
                raise RateLimited(wait)
            r.raise_for_status()
            first = True
            with part.open("wb") as fh:
                async for chunk in r.aiter_bytes(65536):
                    if first:
                        if not chunk.startswith(b"PK"):
                            raise BadPayload("ответ не похож на .osz (zip)")
                        first = False
                    fh.write(chunk)
            if first:
                raise BadPayload("пустой ответ")
        os.replace(part, dest)
    finally:
        part.unlink(missing_ok=True)


async def fetch_set(
    client: httpx.AsyncClient,
    sources: list[Source],
    sid: int,
    dest: Path,
    *,
    no_video: bool,
    cookie: str | None,
    retries: int,
) -> str:
    """Возвращает имя источника, из которого скачали. Бросает RuntimeError, если не вышло."""
    errors: list[str] = []
    for round_no in range(retries):
        for src in sources:
            for _ in range(3):  # повторы только при 429
                try:
                    await download_from(client, src, sid, dest, no_video=no_video, cookie=cookie)
                    return src.name
                except RateLimited as e:
                    await asyncio.sleep(min(e.retry_after, 60) + 0.5)
                    continue
                except NotFound:
                    errors.append(f"{src.name}: 404")
                except (httpx.HTTPError, BadPayload, OSError) as e:
                    errors.append(f"{src.name}: {e}")
                break
        if round_no < retries - 1:
            await asyncio.sleep(2**round_no)
    raise RuntimeError("; ".join(errors[-len(sources):]) or "unknown error")


# ----------------------------------------------------------------------------
# Импорт в stable
# ----------------------------------------------------------------------------
def open_in_stable(osz: Path, stable_exe: Path | None) -> None:
    if stable_exe and stable_exe.is_file():
        subprocess.Popen([str(stable_exe), str(osz)])
    elif platform.system() == "Windows":
        os.startfile(str(osz))  # type: ignore[attr-defined]  # ассоциация .osz -> osu!
    else:
        raise RuntimeError("укажи --stable-exe (например, wine-обёртку) для открытия .osz")


def extract_to_songs(osz: Path, songs_dir: Path, sid: int) -> None:
    target = songs_dir / str(sid)
    target.mkdir(parents=True, exist_ok=True)
    root = target.resolve()
    with zipfile.ZipFile(osz) as zf:
        for member in zf.infolist():
            out = (target / member.filename).resolve()
            if root != out and root not in out.parents:  # защита от zip-slip
                continue
            if member.is_dir():
                out.mkdir(parents=True, exist_ok=True)
                continue
            out.parent.mkdir(parents=True, exist_ok=True)
            with zf.open(member) as src, out.open("wb") as dst:
                shutil.copyfileobj(src, dst)


# ----------------------------------------------------------------------------
# Основной цикл
# ----------------------------------------------------------------------------
async def run(args: argparse.Namespace) -> int:
    # --- id карт
    if args.ids_file:
        ids = ids_from_json(Path(args.ids_file).read_text(encoding="utf-8"))
    else:
        realm_path = Path(args.realm) if args.realm else default_lazer_dir() / "client.realm"
        ids = read_ids_from_bridge(args.bridge, realm_path)
    ids = sorted({i for i in ids if i > 0})
    print(f"Карт в lazer (с online id): {len(ids)}")

    stable_dir = Path(args.stable_dir) if args.stable_dir else default_stable_dir()
    songs_dir = Path(args.songs_dir) if args.songs_dir else stable_dir / "Songs"
    stable_exe = Path(args.stable_exe) if args.stable_exe else stable_dir / "osu!.exe"

    if not args.no_skip_installed:
        have = installed_in_stable(songs_dir)
        before = len(ids)
        ids = [i for i in ids if i not in have]
        print(f"Уже есть в stable: {before - len(ids)}, к загрузке: {len(ids)}")
    if args.limit:
        ids = ids[: args.limit]
    if not ids:
        print("Нечего переносить.")
        return 0

    # --- источники
    cookie = args.osu_session or os.environ.get("OSU_SESSION")
    sources: list[Source] = []
    if args.source_url:
        tpl = args.source_url
        sources.append(Source("custom", tpl, tpl))
    for name in args.sources.split(","):
        name = name.strip()
        if not name:
            continue
        src = SOURCES.get(name)
        if not src:
            sys.exit(f"неизвестный источник: {name} (доступны: {', '.join(SOURCES)})")
        if src.needs_cookie and not cookie:
            continue
        sources.append(src)
    if not sources:
        sys.exit("нет доступных источников")
    print("Источники:", " -> ".join(s.name for s in sources))

    out_dir = Path(args.out)
    out_dir.mkdir(parents=True, exist_ok=True)
    if args.mode == "extract":
        songs_dir.mkdir(parents=True, exist_ok=True)

    sem = asyncio.Semaphore(args.concurrency)
    queue: asyncio.Queue[tuple[int, Path] | None] = asyncio.Queue()
    stats = {"done": 0, "fail": 0, "imported": 0}
    failed: dict[int, str] = {}
    total = len(ids)

    async def importer() -> None:
        while (item := await queue.get()) is not None:
            sid, osz = item
            try:
                if args.mode == "open":
                    await asyncio.to_thread(open_in_stable, osz, stable_exe)
                    await asyncio.sleep(args.open_delay)
                else:
                    await asyncio.to_thread(extract_to_songs, osz, songs_dir, sid)
                stats["imported"] += 1
            except Exception as e:  # noqa: BLE001
                failed[sid] = f"import: {e}"
                print(f"\n[!] {sid}: не удалось импортировать: {e}")

    async def worker(client: httpx.AsyncClient, sid: int) -> None:
        dest = out_dir / f"{sid}.osz"
        async with sem:
            try:
                if dest.is_file() and zipfile.is_zipfile(dest):
                    used = "cache"
                else:
                    used = await fetch_set(
                        client, sources, sid, dest, no_video=args.no_video, cookie=cookie, retries=args.retries
                    )
                stats["done"] += 1
                await queue.put((sid, dest))
            except Exception as e:  # noqa: BLE001
                stats["fail"] += 1
                failed[sid] = str(e)
                used = "FAIL"
            n = stats["done"] + stats["fail"]
            print(f"\r[{n}/{total}] ok={stats['done']} fail={stats['fail']} (last: {sid} <- {used})   ", end="", flush=True)

    limits = httpx.Limits(max_connections=args.concurrency * 2, max_keepalive_connections=args.concurrency)
    timeout = httpx.Timeout(30.0, read=120.0)
    headers = {"User-Agent": "lazer2stable/1.0"}
    async with httpx.AsyncClient(timeout=timeout, limits=limits, headers=headers, follow_redirects=True) as client:
        imp = asyncio.create_task(importer())
        await asyncio.gather(*(worker(client, sid) for sid in ids))
        await queue.put(None)
        await imp

    print()
    print(f"Скачано: {stats['done']}, импортировано: {stats['imported']}, ошибок: {len(failed)}")
    if failed:
        report = out_dir / "failed.json"
        report.write_text(json.dumps(failed, ensure_ascii=False, indent=2), encoding="utf-8")
        print(f"Список неудач: {report}")
    if args.mode == "extract":
        print("Готово. Запусти osu!stable и нажми F5 на экране выбора песни, чтобы он пересканировал Songs.")
    return 1 if failed else 0


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(description="Перенос карт osu!lazer -> osu!stable")
    g = p.add_argument_group("откуда брать id")
    g.add_argument("--realm", help="путь к client.realm (по умолчанию ищется в папке lazer)")
    g.add_argument("--bridge", default=os.environ.get("REALM_BRIDGE", "realm-bridge"),
                   help='команда моста, например: realm-bridge  или  "dotnet run --project bridge --"')
    g.add_argument("--ids-file", help="JSON со списком id (в обход моста)")

    g = p.add_argument_group("куда")
    g.add_argument("--stable-dir", help="папка osu!stable (по умолчанию %%LOCALAPPDATA%%/osu!)")
    g.add_argument("--songs-dir", help="папка Songs, если она нестандартная")
    g.add_argument("--stable-exe", help="путь к osu!.exe")
    g.add_argument("--mode", choices=["open", "extract"], default="open",
                   help="open: открыть .osz в stable; extract: распаковать в Songs (затем F5)")
    g.add_argument("--open-delay", type=float, default=1.5, help="пауза между открытиями .osz, сек")
    g.add_argument("--out", default="./osz_cache", help="куда складывать скачанные .osz")

    g = p.add_argument_group("скачивание")
    g.add_argument("--concurrency", type=int, default=4, help="параллельных загрузок (зеркала режут при большом числе)")
    g.add_argument("--retries", type=int, default=3)
    g.add_argument("--sources", default=",".join(DEFAULT_ORDER), help="порядок источников через запятую")
    g.add_argument("--source-url", help="свой шаблон ссылки, например http://host/{id}.osz (идёт первым)")
    g.add_argument("--osu-session", help="значение cookie osu_session для официального скачивания (или env OSU_SESSION)")
    g.add_argument("--no-video", action="store_true", help="качать без видео (меньше трафика)")
    g.add_argument("--no-skip-installed", action="store_true", help="не пропускать карты, уже лежащие в Songs")
    g.add_argument("--limit", type=int, help="обработать только первые N карт (для проверки)")
    return p


def main() -> None:
    args = build_parser().parse_args()
    try:
        code = asyncio.run(run(args))
    except KeyboardInterrupt:
        print("\nПрервано. Уже скачанные .osz лежат в кэше и при повторном запуске не качаются заново.")
        code = 130
    sys.exit(code)


if __name__ == "__main__":
    main()
