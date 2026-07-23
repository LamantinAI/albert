#!/usr/bin/env bash
# One-time setup for the avito-price skill.
#
# Makes the skill fully self-contained: clones the Avito parser and installs
# every Python dependency INTO this skill folder (vendor/), without touching
# system site-packages and without needing the python3-venv apt package.
#
# Re-running it is safe — it updates the parser and refreshes deps.
set -euo pipefail

SKILL_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SKILL_DIR"
mkdir -p vendor

# Minimal dependency set actually needed to import parser_cls and run the
# skill (flet / playwright from the parser's full requirements.txt are the
# GUI / browser-cookie path and are intentionally skipped).
DEPS=(
  beautifulsoup4 curl_cffi loguru pydantic tomli tomli_w
  httpx requests openpyxl tzlocal tzdata pyexcel pyexcel-xlsx
)

echo "[1/3] parser_avito ..."
if [ -d vendor/parser_avito/.git ]; then
  git -C vendor/parser_avito pull --ff-only -q || true
else
  git clone --depth 1 -q https://github.com/Duff89/parser_avito.git vendor/parser_avito
fi

echo "[2/3] Python-зависимости -> vendor/pylib ..."
python3 -m pip install --no-cache-dir --upgrade --target vendor/pylib "${DEPS[@]}"

echo "[3/3] проверка импорта ..."
PYTHONPATH=vendor/pylib python3 -c \
  "import curl_cffi, bs4, pydantic, loguru, tomli, tzlocal, pyexcel; print('  deps OK')"

echo
echo "Готово. Скилл avito-price самодостаточен:"
echo "  - парсер:        vendor/parser_avito"
echo "  - зависимости:   vendor/pylib"
echo "Запуск (env-переменные не нужны):"
echo "  python3 \"$SKILL_DIR/scripts/avito_query.py\" get-city"
