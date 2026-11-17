# strix-rs — Rust-движок Strix

Параллельная Rust-реализация ядра Strix. Python-реализация (`../strix/`)
остаётся в репозитории как эталон и не меняется.

Границы порта, инвентарь переменных окружения, форматы артефактов и коды
возврата описаны в [`CONTRACT.md`](CONTRACT.md).

## Крейты

| Крейт | Назначение |
|---|---|
| `strix-config` | окружение, профили моделей, бюджет |
| `strix-llm` | клиент чат-комплишенов для локального `llama-server` |
| `strix-agent` | цикл агента, бюджет, сессии, компакция контекста |
| `strix-tools` | реестр инструментов, JSON-схемы, приведение аргументов |
| `strix-runtime` | песочница Docker, рабочая папка, управление Caido |
| `strix-report` | находки, SARIF 2.1.0, покрытие, учёт токенов |
| `strix-tui` | backend протокола TUI версии 3 (Go-sidecar не меняется) |
| `strix-cli` | точка входа CLI |
| `strix-mock-llm` | mock `llama-server` со сценариями отказа |
| `strix-contracts` | контрактные тесты по golden-фикстурам |

## Сборка и проверка

```bash
cd strix-rs
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --workspace
```

## Эталонные фикстуры

Фикстуры генерируются из Python-реализации и служат контрактом для порта:

```bash
cd ..
uv run python scripts/export_golden_fixtures.py
```

Результат: `crates/strix-contracts/tests/fixtures/golden/` — 20 сценариев и
JSON-схемы 43 инструментов агента.

## Провайдер

Один локальный провайдер, совместимый с OpenAI chat-completions:

```bash
export STRIX_LLM="qwen3-coder-30b"
export LLM_API_BASE="http://127.0.0.1:8080/v1"
cargo run -p strix-mock-llm -- --port 8080   # для локальных проверок
```
