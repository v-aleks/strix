# Changelog

Все значимые изменения проекта Strix фиксируются в этом файле.

Формат основан на [Keep a Changelog](https://keepachangelog.com/ru/1.1.0/),
проект придерживается [семантического версионирования](https://semver.org/lang/ru/).

## [Unreleased]

### Added — Rust-движок, фаза 0 (каркас и контракт)

Rust-порт Strix развивается параллельно с Python-реализацией, которая остаётся
в репозитории как эталон. Подробный контракт порта — в
[`strix-rs/CONTRACT.md`](strix-rs/CONTRACT.md).

- **Cargo workspace `strix-rs/`** — новый параллельный крейт-воркспейс с
  крейтами `strix-config`, `strix-llm`, `strix-agent`, `strix-tools`,
  `strix-runtime`, `strix-report`, `strix-tui`, `strix-cli`, а также
  вспомогательными `strix-mock-llm` и `strix-contracts`.
- **`strix-config`** — чтение окружения и профили моделей. Единственный
  поддерживаемый провайдер — локальный `llama-server`; LiteLLM не используется.
  Профили Gemma и Qwen описывают поведение `tool_call_id` (синтез / позиционная
  нумерация), параллельные вызовы и лимит вызовов за ход. Неизвестная модель
  получает консервативный профиль.
- **`strix-llm`** — транспорт чат-комплишенов: сборка тела запроса без
  облачных возможностей (`reasoning_effort`, prompt caching, `strict`-схемы,
  `tool_choice: required`), не-стриминговый вызов и предполётная проверка
  доступности провайдера.
- **`strix-mock-llm`** — mock `llama-server` для тестов с одиннадцатью
  сценариями отказа: пустой `id` вызова инструмента, дублирующиеся `id`,
  обрезанные аргументы, параллельные вызовы, `finish_reason=length`,
  превышение контекстного окна (HTTP 400), HTTP 500, HTTP 429 и зависающий
  поток.
- **`strix-contracts`** — контрактные тесты поверх golden-фикстур.
- **`scripts/export_golden_fixtures.py`** — экспорт 20 эталонных сценариев из
  Python-реализации: `run.json`, `vulnerabilities.json`, `vulnerabilities.csv`,
  `findings.sarif`, `coverage.json`, отчёты по находкам, а также JSON-схемы всех
  43 инструментов агента. Временные метки и идентификаторы скана нормализуются,
  чтобы вывод Rust-сборки сравнивался с эталоном.
- **`strix-rs/CONTRACT.md`** — границы порта, инвентарь переменных окружения
  (сохраняемые и удаляемые), форматы артефактов, коды возврата, протокол TUI
  версии 3, профили моделей и карта соответствия модулей Python → Rust.
- **CI `.github/workflows/rust.yml`** — форматирование, clippy с `-D warnings`,
  сборка и тесты (в том числе против mock-провайдера) на Linux и macOS, а также
  проверка того, что golden-фикстуры соответствуют Python-реализации.

### Changed

- Ничего в поведении Python-движка не менялось: фаза 0 добавляет только
  параллельную реализацию и документацию.

[Unreleased]: https://github.com/usestrix/strix/compare/v1.7.1...HEAD
