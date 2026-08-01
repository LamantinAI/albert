"""Resolve a human city name to an Avito region slug (the avito.ru/<slug> part)."""
import re

# Avito region slugs for the largest RU cities + common aliases.
_CITY_SLUGS = {
    "москва": "moskva", "мск": "moskva",
    "санкт-петербург": "sankt-peterburg", "санкт петербург": "sankt-peterburg",
    "спб": "sankt-peterburg", "питер": "sankt-peterburg", "петербург": "sankt-peterburg",
    "новосибирск": "novosibirsk",
    "екатеринбург": "ekaterinburg", "екб": "ekaterinburg",
    "казань": "kazan",
    "нижний новгород": "nizhniy_novgorod", "нижний": "nizhniy_novgorod", "нн": "nizhniy_novgorod",
    "челябинск": "chelyabinsk",
    "красноярск": "krasnoyarsk",
    "самара": "samara",
    "уфа": "ufa",
    "ростов-на-дону": "rostov-na-donu", "ростов на дону": "rostov-na-donu", "ростов": "rostov-na-donu",
    "краснодар": "krasnodar",
    "омск": "omsk",
    "воронеж": "voronezh",
    "пермь": "perm",
    "волгоград": "volgograd",
    "саратов": "saratov",
    "тюмень": "tyumen",
    "тольятти": "tolyatti",
    "ижевск": "izhevsk",
    "барнаул": "barnaul",
    "ульяновск": "ulyanovsk",
    "иркутск": "irkutsk",
    "хабаровск": "habarovsk",
    "ярославль": "yaroslavl",
    "владивосток": "vladivostok",
    "махачкала": "mahachkala",
    "томск": "tomsk",
    "оренбург": "orenburg",
    "кемерово": "kemerovo",
    "новокузнецк": "novokuznetsk",
    "рязань": "ryazan",
    "астрахань": "astrahan",
    "набережные челны": "naberezhnye_chelny",
    "пенза": "penza",
    "липецк": "lipetsk",
    "тула": "tula",
    "калининград": "kaliningrad",
    "курск": "kursk",
    "чебоксары": "cheboksary",
    "ставрополь": "stavropol",
    "сочи": "sochi",
    "брянск": "bryansk",
    "иваново": "ivanovo",
    "тверь": "tver",
    "белгород": "belgorod",
    "сургут": "surgut",
    "владимир": "vladimir",
    "калуга": "kaluga",
    "смоленск": "smolensk",
    "вся россия": "rossiya", "россия": "rossiya",
}


def _norm(name: str) -> str:
    s = name.strip().lower().replace("ё", "е")
    s = re.sub(r"\s+", " ", s)
    s = re.sub(r"[^a-zа-я0-9 _-]", "", s)
    return s.strip()


def resolve_slug(name: str):
    """Return an Avito slug, or None if it cannot be determined."""
    n = _norm(name)
    if n in _CITY_SLUGS:
        return _CITY_SLUGS[n]
    # User may have passed the slug itself (latin, e.g. "rostov-na-donu").
    if re.fullmatch(r"[a-z0-9][a-z0-9_-]*", n):
        return n
    return None
