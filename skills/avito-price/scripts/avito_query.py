#!/usr/bin/env python3
"""Backend for the "avito-price" skill.

Fetch strategy (rewritten 2026-07-23 after a hard perf audit):
  A single request through the RU proxy is ~1.5s — latency is NOT the bottleneck.
  Avito's anti-bot blocks PER-IMPERSONATION (edge/edge99/safari often pass while
  chrome/firefox get 403/429), and it RATE-LIMITS the shared exit IP by request
  COUNT. So the winning strategy is: minimise requests. Try one good impersonation
  at a time, in sequence (one connection in flight — gentle), stop at the first
  200, no sleeps, no concurrency, no redirect-hop amplification. That lands a
  fresh-IP search in ~1.5-3s and fails honestly in <10s instead of thrashing for
  120s and burning the IP's budget (which is what caused most blocks).

Parsing is unchanged: reuse parser_avito v3.2.16's find_json_on_page (reads the
script[type=mime/invalid][data-mfe-state] state blob) + its pydantic models.

Subcommands:
  get-city                          -> {"status":"ok"|"need_city", ...}
  set-city  --name N [--slug S]     -> {"status":"ok"|"need_slug", ...}
  search    --query Q [--limit 5] [--min-price N] [--max-price N] [--city C]
  details   --url URL

Environment:
  AVITO_PROXY             proxy string (else <skill>/proxy.conf); e.g. socks5h://host:port
  AVITO_PROXY_CHANGE_URL  optional rotate-IP URL (rotating/mobile proxy); hit once
                          when every impersonation is blocked, then retried
  AVITO_SKILL_HOME        where to store the saved city (default ~/.config/avito-price-skill)
"""
import argparse
import json
import os
import sys
import time
from pathlib import Path
from urllib.parse import quote_plus

SCRIPT_DIR = Path(__file__).resolve().parent
SKILL_DIR = SCRIPT_DIR.parent
sys.path.insert(0, str(SCRIPT_DIR))

# Self-contained dependency folder built by setup.sh (pip --target).
_VENDOR_LIB = SKILL_DIR / "vendor" / "pylib"
if _VENDOR_LIB.is_dir():
    sys.path.insert(0, str(_VENDOR_LIB))

from cities import resolve_slug  # noqa: E402

STATE_DIR = Path(
    os.environ.get("AVITO_SKILL_HOME", Path.home() / ".config" / "avito-price-skill")
).expanduser()
CITY_FILE = STATE_DIR / "city.json"
IMP_FILE = STATE_DIR / "last_imp.json"
DESC_LIMIT = 600

# curl_cffi impersonation profiles, ordered by measured pass-rate against Avito's
# anti-bot (edge/edge99 best; the "good" one rotates, so the list is diverse across
# browser families). Plain desktop chrome/firefox are omitted — measured ~0% pass.
GOOD_IMPS = [
    "edge", "edge99", "safari180", "tor145",
    "edge101", "safari260", "chrome131_android", "safari184_ios",
]
DESKTOP_UA = (
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 "
    "(KHTML, like Gecko) Chrome/145.0.0.0 Safari/537.36"
)
# Hard wall-clock budget for the whole fetch (a search must return well under the
# 10s target). Checked before each attempt; per-request timeout is separate.
FETCH_BUDGET_S = 9.0
REQ_TIMEOUT_S = 7


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


# --- proxy + gentle fetch ---------------------------------------------------

def _avito_proxy():
    """Resolve the proxy string: $AVITO_PROXY, or <skill>/proxy.conf if present.
    A file keeps deployment independent of how the host passes env vars."""
    raw = (os.environ.get("AVITO_PROXY") or "").strip()
    if raw:
        return raw
    pf = SKILL_DIR / "proxy.conf"
    if pf.is_file():
        return pf.read_text(encoding="utf-8").strip()
    return ""


def _session(imp, proxy):
    from curl_cffi import requests as creq
    s = creq.Session(impersonate=imp)
    s.headers.update({"user-agent": DESKTOP_UA})
    if proxy:
        s.proxies = {"http": proxy, "https": proxy}
    return s


def _rotate_ip():
    """If a rotating/mobile proxy exposes a change-IP URL, hit it once. No-op
    otherwise. Returns True if a rotation was attempted."""
    url = (os.environ.get("AVITO_PROXY_CHANGE_URL") or "").strip()
    if not url:
        return False
    try:
        import requests as _rq
        _rq.get(url, params={"format": "json"}, timeout=10)
        time.sleep(1.5)  # give the new IP a moment to settle
        return True
    except Exception:
        return False


def _load_last_imp():
    """The impersonation profile that last got a 200. Avito's per-profile block
    state persists for a while, so the last winner is the best first guess —
    turning the usual case into a single ~1.5s request instead of walking the pool."""
    try:
        v = json.loads(IMP_FILE.read_text(encoding="utf-8"))
        return v if v in GOOD_IMPS else None
    except Exception:
        return None


def _save_last_imp(imp):
    try:
        STATE_DIR.mkdir(parents=True, exist_ok=True)
        IMP_FILE.write_text(json.dumps(imp), encoding="utf-8")
    except Exception:
        pass


def _imp_order():
    """GOOD_IMPS with the last-winning profile moved to the front (deduped)."""
    last = _load_last_imp()
    if not last:
        return list(GOOD_IMPS)
    return [last] + [i for i in GOOD_IMPS if i != last]


def gentle_get(url):
    """Fetch `url` through the RU proxy trying good impersonations one at a time
    (single connection in flight), stopping at the first HTTP 200. Returns the
    response text, or None if every profile was blocked within the budget.

    Sequential-not-concurrent is deliberate: concurrency multiplies the per-IP
    request rate and trips Avito's clamp. The last-winning profile is tried first
    (see _imp_order). If the whole pool is blocked and a rotate-IP URL is
    configured, rotate once and try the pool again."""
    proxy = _avito_proxy()
    start = time.time()
    for _rotation in range(2):  # 0: as-is, 1: after an IP rotation (if available)
        for imp in _imp_order():
            if time.time() - start > FETCH_BUDGET_S:
                return None
            try:
                r = _session(imp, proxy).get(
                    url, timeout=REQ_TIMEOUT_S, allow_redirects=True
                )
            except Exception:
                continue  # transient network/proxy error — next profile
            if r.status_code == 200:
                _save_last_imp(imp)
                return r.text
            # 401/403/429 (or other) -> next profile immediately, no sleep
        if not _rotate_ip():
            break
    return None


def fetch_serp(url):
    """Fetch an Avito search page and return its extracted page-state dict.

    Avito occasionally answers a free-text ?q= query with a small SPA redirect
    stub ({"redirected": true, "url": ...}) instead of the catalog; follow it
    once. (The current markup usually returns the catalog directly.)"""
    from parser_cls import AvitoParse
    html = gentle_get(url)
    if not html:
        return None
    data = AvitoParse.find_json_on_page(html) or {}
    if data.get("redirected") and data.get("url"):
        html2 = gentle_get("https://www.avito.ru" + data["url"])
        if html2:
            data = AvitoParse.find_json_on_page(html2) or {}
    return data


def setup_parser_imports():
    """Put parser_avito on sys.path so find_json_on_page + the pydantic models
    import. We do our own HTTP now, so the parser's HTTP client/proxy is unused."""
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
    # parser_avito imports stdlib `tomllib` (3.11+); backfill from `tomli` on 3.10.
    if "tomllib" not in sys.modules and sys.version_info < (3, 11):
        try:
            import tomli
            sys.modules["tomllib"] = tomli
        except ModuleNotFoundError:
            emit({
                "status": "error", "error": "missing_dependency",
                "message": "Нужен Python 3.11+ либо пакет tomli (pip install tomli).",
            })
    sys.path.insert(0, str(d))


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
    items = None
    if data:
        try:
            items = ItemsResponse(**(data.get("catalog") or {})).items
        except Exception:  # noqa: BLE001
            items = None

    if items is None:
        emit({"status": "blocked", "url": url,
              "message": "Avito не отдал разборчивую страницу (антибот заблокировал "
                         "исходящий IP). Попробуйте ещё раз чуть позже."})

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

    html = gentle_get(args.url)
    if not html:
        emit({"status": "blocked", "url": args.url,
              "message": "Avito не отдал карточку (антибот заблокировал IP)."})

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
