#!/usr/bin/env python3
"""Backend for the OpenClaw "avito-price" skill.

Wraps Duff89/parser_avito: reuses its HTTP client (anti-block, retries, proxy)
and its SERP-JSON extractor. Always prints a single JSON object to stdout.

Subcommands:
  get-city                          -> {"status":"ok"|"need_city", ...}
  set-city  --name N [--slug S]     -> {"status":"ok"|"need_slug", ...}
  search    --query Q [--limit 5] [--min-price N] [--max-price N]
  details   --url URL

Environment:
  PARSER_AVITO_DIR        path to a cloned Duff89/parser_avito (default ~/parser_avito)
  AVITO_PROXY             optional proxy string (e.g. http://user:pass@host:port)
  AVITO_PROXY_CHANGE_URL  optional rotate-IP URL (mobile proxy only)
  AVITO_SKILL_HOME        where to store the saved city (default ~/.config/avito-price-skill)
"""
import argparse
import json
import os
import sys
from pathlib import Path
from urllib.parse import quote_plus

SCRIPT_DIR = Path(__file__).resolve().parent
SKILL_DIR = SCRIPT_DIR.parent
sys.path.insert(0, str(SCRIPT_DIR))

# Self-contained dependency folder built by setup.sh (pip --target).
# Lets the skill run on the system python3 without polluting site-packages.
_VENDOR_LIB = SKILL_DIR / "vendor" / "pylib"
if _VENDOR_LIB.is_dir():
    sys.path.insert(0, str(_VENDOR_LIB))

from cities import resolve_slug  # noqa: E402

STATE_DIR = Path(
    os.environ.get("AVITO_SKILL_HOME", Path.home() / ".config" / "avito-price-skill")
).expanduser()
CITY_FILE = STATE_DIR / "city.json"
DESC_LIMIT = 600


def emit(obj):
    sys.stdout.write(json.dumps(obj, ensure_ascii=False, indent=2) + "\n")
    sys.exit(0)


# --- city persistence -------------------------------------------------------

def load_city():
    if CITY_FILE.exists():
        try:
            return json.loads(CITY_FILE.read_text(encoding="utf-8"))
        except Exception:
            return None
    return None


def save_city(name, slug):
    STATE_DIR.mkdir(parents=True, exist_ok=True)
    CITY_FILE.write_text(
        json.dumps({"name": name, "slug": slug}, ensure_ascii=False), encoding="utf-8"
    )


# --- parser_avito bridge ----------------------------------------------------

def _avito_proxy():
    """Resolve the proxy string: $AVITO_PROXY, or <skill>/proxy.conf if present.
    A file keeps deployment independent of how OpenClaw passes env vars."""
    raw = (os.environ.get("AVITO_PROXY") or "").strip()
    if raw:
        return raw
    pf = SKILL_DIR / "proxy.conf"
    if pf.is_file():
        return pf.read_text(encoding="utf-8").strip()
    return ""


def _patch_proxy_scheme():
    """parser_avito's ServerProxy hardcodes the http:// scheme. When the proxy
    already carries a scheme (e.g. socks5h://127.0.0.1:1080 for an SSH tunnel),
    override the parser's proxy factory so the scheme is passed through as-is.
    Must be called after the parser dir is on sys.path."""
    raw = _avito_proxy()
    if "://" not in raw:
        return  # plain host:port — the parser's own ServerProxy handles it
    import parser_cls
    from parser.proxies.proxy import Proxy as _BaseProxy

    class _SchemeProxy(_BaseProxy):
        def __init__(self, url):
            self._url = url

        def get_httpx_proxy(self):
            return self._url

        def handle_block(self):
            pass

    parser_cls.build_proxy = lambda _config: _SchemeProxy(raw)


def setup_parser_imports():
    env_dir = os.environ.get("PARSER_AVITO_DIR")
    if env_dir:
        d = Path(env_dir).expanduser()
    elif (SKILL_DIR / "vendor" / "parser_avito" / "parser_cls.py").exists():
        d = SKILL_DIR / "vendor" / "parser_avito"
    else:
        d = Path.home() / "parser_avito"
    if not (d / "parser_cls.py").exists():
        emit({
            "status": "error", "error": "parser_not_installed",
            "message": f"parser_avito не найден в '{d}'. Склонируйте Duff89/parser_avito "
                       f"и/или задайте переменную окружения PARSER_AVITO_DIR.",
        })
    # parser_avito imports the stdlib `tomllib` (Python 3.11+).
    # On Python 3.10 fall back to the `tomli` backport under the same name.
    if "tomllib" not in sys.modules and sys.version_info < (3, 11):
        try:
            import tomli
            sys.modules["tomllib"] = tomli
        except ModuleNotFoundError:
            emit({
                "status": "error", "error": "missing_dependency",
                "message": "Нужен Python 3.11+ либо пакет tomli (pip install tomli).",
            })
    os.chdir(d)  # parser writes logs/ and its sqlite db relative to cwd
    sys.path.insert(0, str(d))
    _patch_proxy_scheme()


def make_config(urls):
    from dto import AvitoConfig
    return AvitoConfig(
        urls=urls,
        count=1,
        save_xlsx=False,
        use_webdriver=False,
        proxy_string=_avito_proxy() or None,
        proxy_change_url=os.environ.get("AVITO_PROXY_CHANGE_URL") or None,
        one_time_start=True,
        max_count_of_retry=3,
        retry_delay=4,
        timeout=25,
    )


def fetch_html(url):
    """Return page HTML using parser_avito's HTTP client, or None."""
    from parser_cls import AvitoParse
    try:
        parser = AvitoParse(make_config([url]))
        return parser.fetch_data(url)
    except Exception as exc:  # noqa: BLE001
        emit({"status": "error", "error": "runtime", "message": str(exc)})


def fetch_serp(url, max_hops=4):
    """Fetch an Avito search page following Avito's internal SPA redirects.

    Avito 301-redirects a free-text query (avito.ru/<city>?q=...) to a
    canonical category URL; the first response only carries a redirect stub
    ({"redirected": true, "url": ...}) instead of the catalog. We follow it.
    Returns the extracted page-state dict, or None if a page wasn't fetched.
    """
    from parser_cls import AvitoParse
    try:
        parser = AvitoParse(make_config([url]))
    except Exception as exc:  # noqa: BLE001
        emit({"status": "error", "error": "runtime", "message": str(exc)})
    current = url
    data = {}
    for _ in range(max_hops):
        html = parser.fetch_data(current)
        if not html:
            return None
        data = AvitoParse.find_json_on_page(html) or {}
        if data.get("redirected") and data.get("url"):
            current = "https://www.avito.ru" + data["url"]
            continue
        return data
    return data


# --- formatting -------------------------------------------------------------

def fmt_item(it, rank):
    price, price_text = None, None
    if it.priceDetailed:
        price = it.priceDetailed.value
        price_text = it.priceDetailed.string or it.priceDetailed.fullString
    location = None
    if it.geo and it.geo.formattedAddress:
        location = it.geo.formattedAddress
    elif it.addressDetailed:
        location = it.addressDetailed.locationName
    elif it.location:
        location = it.location.name
    desc = (it.description or "").strip()
    if len(desc) > DESC_LIMIT:
        desc = desc[:DESC_LIMIT].rstrip() + "…"
    return {
        "rank": rank,
        "id": it.id if isinstance(it.id, int) else None,
        "title": it.title,
        "price": price,
        "price_text": price_text,
        "url": f"https://www.avito.ru{it.urlPath}" if it.urlPath else None,
        "location": location,
        "description": desc or None,
    }


# --- subcommands ------------------------------------------------------------

def cmd_get_city(args):
    city = load_city()
    if city:
        emit({"status": "ok", "city": city})
    emit({"status": "need_city",
          "message": "Город ещё не задан. Спросите у пользователя, в каком городе искать."})


def cmd_set_city(args):
    slug = args.slug or resolve_slug(args.name)
    if not slug:
        emit({"status": "need_slug", "name": args.name,
              "message": "Не удалось определить slug города. Передайте --slug "
                         "(латинская часть адреса avito.ru/<slug>, напр. moskva)."})
    save_city(args.name, slug)
    emit({"status": "ok", "city": {"name": args.name, "slug": slug}})


def cmd_search(args):
    city = load_city()
    if args.city:
        slug = resolve_slug(args.city)
        if not slug:
            emit({"status": "need_slug", "name": args.city,
                  "message": "Не удалось определить slug города. Вызовите set-city с --slug."})
        save_city(args.city, slug)
        city = {"name": args.city, "slug": slug}
    if not city:
        emit({"status": "need_city",
              "message": "Город не задан. Спросите у пользователя город, "
                         "затем повторите вызов search с параметром --city."})

    setup_parser_imports()
    from models import ItemsResponse

    url = f"https://www.avito.ru/{city['slug']}?q={quote_plus(args.query)}"
    if args.min_price:
        url += f"&pmin={int(args.min_price)}"
    if args.max_price:
        url += f"&pmax={int(args.max_price)}"

    data = fetch_serp(url)
    if data is None:
        emit({"status": "blocked", "url": url,
              "message": "Avito не отдал страницу (блокировка или капча). "
                         "Попробуйте позже или настройте прокси (AVITO_PROXY)."})

    catalog = data.get("catalog") or {}
    try:
        items = ItemsResponse(**catalog).items
    except Exception:  # noqa: BLE001
        emit({"status": "blocked", "url": url,
              "message": "Не удалось разобрать выдачу Avito (капча или изменение вёрстки)."})

    items = [i for i in items if isinstance(i.id, int)]
    results = [fmt_item(it, n + 1) for n, it in enumerate(items[: args.limit])]
    if not results:
        emit({"status": "empty", "url": url, "city": city, "query": args.query,
              "message": "По запросу ничего не найдено. Предложите смягчить фильтры."})
    emit({"status": "ok", "city": city, "query": args.query, "url": url,
          "count": len(results), "results": results})


def cmd_details(args):
    setup_parser_imports()
    from bs4 import BeautifulSoup

    html = fetch_html(args.url)
    if not html:
        emit({"status": "blocked", "url": args.url,
              "message": "Avito не отдал карточку (блокировка или капча)."})

    soup = BeautifulSoup(html, "html.parser")
    item = {"url": args.url}

    for script in soup.find_all("script", attrs={"type": "application/ld+json"}):
        try:
            ld = json.loads(script.string or script.text)
        except Exception:  # noqa: BLE001
            continue
        for node in (ld if isinstance(ld, list) else [ld]):
            if not isinstance(node, dict):
                continue
            if node.get("name"):
                item.setdefault("title", node["name"])
            if node.get("description"):
                item.setdefault("description", node["description"])
            offers = node.get("offers")
            if isinstance(offers, dict) and offers.get("price"):
                item.setdefault("price", offers["price"])

    desc_el = soup.select_one('[data-marker="item-view/item-description"]')
    if desc_el:
        item["description"] = desc_el.get_text(" ", strip=True)
    title_el = soup.select_one('[data-marker="item-view/title-info"]')
    if title_el:
        item["title"] = title_el.get_text(" ", strip=True)
    price_el = soup.select_one('[data-marker="item-view/item-price"]')
    if price_el:
        item["price_text"] = price_el.get("content") or price_el.get_text(strip=True)

    params = {}
    for block in soup.select('[data-marker="item-view/item-params"]'):
        for li in block.find_all("li"):
            txt = li.get_text(" ", strip=True)
            if ":" in txt:
                key, _, val = txt.partition(":")
                if key.strip() and val.strip():
                    params[key.strip()] = val.strip()
    if params:
        item["params"] = params

    if len(item) <= 1:
        emit({"status": "blocked", "url": args.url,
              "message": "Не удалось извлечь данные карточки (капча или изменение вёрстки)."})
    emit({"status": "ok", "item": item})


def main():
    ap = argparse.ArgumentParser(description="Avito search backend for the avito-price skill")
    sub = ap.add_subparsers(dest="cmd", required=True)

    sub.add_parser("get-city")

    sp = sub.add_parser("set-city")
    sp.add_argument("--name", required=True)
    sp.add_argument("--slug", default=None)

    sp = sub.add_parser("search")
    sp.add_argument("--query", required=True)
    sp.add_argument("--limit", type=int, default=5)
    sp.add_argument("--min-price", type=int, default=None)
    sp.add_argument("--max-price", type=int, default=None)
    sp.add_argument("--city", default=None,
                    help="set the city on this call (used on first run)")

    sp = sub.add_parser("details")
    sp.add_argument("--url", required=True)

    args = ap.parse_args()
    {"get-city": cmd_get_city, "set-city": cmd_set_city,
     "search": cmd_search, "details": cmd_details}[args.cmd](args)


if __name__ == "__main__":
    main()
