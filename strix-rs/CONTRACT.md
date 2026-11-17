# Strix Rust — контракт порта

Документ фиксирует границу между Python-реализацией и Rust-портом: что
переносится, что переписывается, что удаляется и по каким правилам Rust-движок
обязан вести себя, чтобы оставаться совместимым с Python-артефактами.

Ссылка на реализацию: `strix/` (Python, остаётся в репозитории как эталон).
Ссылка на порт: `strix-rs/` (Cargo workspace).

---

## 1. Границы порта

| Область | Решение |
|---|---|
| Ядро агента (`strix/core`) | порт в `strix-agent` |
| CLI (`strix/interface/cli`) | порт в `strix-cli` |
| TUI-backend (`strix/interface/tui/backend`) | порт в `strix-tui`; Go-sidecar `strix/interface/tui/cmd/strix-tui` остаётся как есть |
| Viewer (`strix/interface/viewer`) | **не портируется** — направление развития — полноценный TUI |
| Отчёты (`strix/report`) | порт в `strix-report`; формат артефактов не меняется |
| Инструменты (`strix/tools`) | порт в `strix-tools` |
| Рантайм/песочница (`strix/runtime`) | порт в `strix-runtime` |
| Навыки (`strix/skills`, 79 markdown-паков) | переносятся **как данные**, без изменений |
| Промпты (`strix/agents/prompts/*.jinja`) | переносятся как данные; рендеринг — `minijinja` |

Python-реализация не удаляется и остаётся рабочей. Переключение движка
планируется через переменную окружения `STRIX_ENGINE=python|rust`.

---

## 2. Провайдер: единственный локальный llama.cpp

LiteLLM в порту **нет**. Это главное архитектурное отличие.

### 2.1 Что используется из протокола

| Эндпоинт | Назначение |
|---|---|
| `POST {LLM_API_BASE}/chat/completions` | основной вызов (stream и non-stream) |
| `GET  {LLM_API_BASE}/models` | проверка доступности модели |
| `GET  {LLM_API_BASE}/props` | размер контекстного окна, шаблон чата |
| `POST {LLM_API_BASE}/tokenize` | точный подсчёт токенов |
| `GET  {LLM_API_BASE}/health` | предполётная проверка |

### 2.2 Что не используется

* Responses API — llama.cpp его не реализует;
* префиксы провайдеров (`openrouter/`, `ollama/`, …) — в `STRIX_LLM` пишется
  чистое имя модели, как его знает сервер;
* `reasoning_effort` — локальные модели не имеют отдельного reasoning-канала;
* prompt caching (`CACHE_POINT`, `cached_tokens`) — у локального сервера нет
  кеша между запросами в API-семантике;
* `strict`-схемы инструментов — всегда выключены;
* `tool_choice: "required"` — никогда; локальная модель отвечает прозой;
* `parallel_tool_calls` — по умолчанию `false`.

### 2.3 Обязательные поля запроса

```json
{
  "model": "<имя модели>",
  "messages": [ ... ],
  "stream": true,
  "parallel_tool_calls": false,
  "tools": [ ... ],
  "tool_choice": "auto"
}
```

Дополнительные поля (`top_p`, `top_k`, `seed`, …) передаются через
`LLM_EXTRA_BODY` и **мерджатся в тело запроса** без вмешательства кода.

### 2.4 Профили моделей

Ожидаются две семьи: Gemma и Qwen (разные версии). Профиль описания в
`strix-config` (`ModelProfile`), переопределяется через `STRIX_MODEL_PROFILE`
(JSON или имя семейства).

| Поле | Gemma | Qwen | Неизвестная модель |
|---|---|---|---|
| `tool_call_id_mode` | `synthesize` | `positional` | `synthesize` |
| `supports_parallel_tool_calls` | `false` | `true` | `false` |
| `supports_reasoning` | `false` | `false` | `false` |
| `max_tool_calls_per_turn` | `1` | `8` | `8` |

Правило: **неизвестная модель получает консервативный профиль** — движок никогда
не полагается на оптимистичное поведение.

### 2.5 Гигиена tool-call

Переносится из `strix/config/tool_call_{ids,arguments,limits}.py`:

* пустой `id` → синтезируется `call_<uuid4>`;
* повторяющиеся `id` в одном ходе → перенумеровываются по позиции;
* обрезанные аргументы (`finish_reason=length` посреди JSON) → попытка починки,
  иначе вызов отклоняется с диагностикой, а не падает;
* число вызовов за ход ограничивается `max_tool_calls_per_turn` профиля.

---

## 3. Переменные окружения

### 3.1 Сохраняются

| Переменная | Назначение | Значение по умолчанию |
|---|---|---|
| `STRIX_LLM` | имя модели на сервере | **обязательна** |
| `LLM_API_BASE` | базовый URL провайдера | `http://127.0.0.1:8080/v1` |
| `LLM_API_KEY` | необязательный ключ (для обратного прокси) | — |
| `LLM_EXTRA_HEADERS` | JSON-объект дополнительных заголовков | — |
| `LLM_EXTRA_BODY` | JSON-объект дополнительных полей тела | — |
| `LLM_DISABLE_STREAMING` | выключить SSE | `false` |
| `LLM_TIMEOUT` | таймаут запроса, с | `300` |
| `LLM_STREAM_FIRST_EVENT_TIMEOUT` | таймаут первого события, с | `120` |
| `LLM_STREAM_IDLE_TIMEOUT` | таймаут паузы в потоке, с | `300` |
| `LLM_STREAM_TOTAL_TIMEOUT` | полный таймаут потока, с | `600` |
| `STRIX_MAX_BUDGET_TOKENS` | бюджет скана в токенах | без ограничений |
| `STRIX_MAX_BUDGET` | бюджет в USD (необязателен) | — |
| `STRIX_MAX_TURNS` | максимум ходов агента | `500` |
| `STRIX_LLM_INPUT_COST_PER_MTOK` | цена входа, $/Mtok | — |
| `STRIX_LLM_OUTPUT_COST_PER_MTOK` | цена выхода, $/Mtok | — |
| `STRIX_LLM_CACHED_INPUT_COST_PER_MTOK` | цена кешированного входа, $/Mtok | — |
| `STRIX_CONTEXT_AUTO_COMPACT` | авто-компакция | `true` |
| `STRIX_CONTEXT_KEEP_TOKENS` | хвост, сохраняемый при компакции | `8000` |
| `STRIX_CONTEXT_BUFFER_TOKENS` | резерв под ответ модели | `20000` |
| `STRIX_TOOL_OUTPUT_MAX_TOKENS` | порог усечения вывода инструмента | `8000` |
| `STRIX_TOOL_OUTPUT_MAX_LINES` | порог усечения по строкам | `2000` |
| `STRIX_TOOL_OUTPUT_MAX_BYTES` | порог усечения по байтам | `51200` |
| `STRIX_IMAGE` | образ песочницы | `ghcr.io/usestrix/strix-sandbox:1.4.0` |
| `STRIX_MAX_CONTEXT_IMAGES` | максимум изображений в контексте | `3` |
| `STRIX_DEDUPE_ENABLED` | LLM-дедупликация находок | `true` |
| `STRIX_TELEMETRY` | телеметрия | `false` |
| `STRIX_MODEL_PROFILE` | переопределение профиля модели | — |

### 3.2 Удаляются (LiteLLM-специфичные)

`STRIX_API_TYPE`, `OPENAI_API_KEY`/`OPENAI_API_BASE`/`OPENAI_BASE_URL`/
`LITELLM_BASE_URL`/`OLLAMA_API_BASE` как **основной** путь (алиасы `LLM_API_BASE`
и `LLM_API_KEY` поддерживаются для совместимости), `STRIX_REASONING_EFFORT`,
`STRIX_PROMPT_CACHE`, `STRIX_CACHE_BLOCK_TOKENS`,
`STRIX_FORCE_REQUIRED_TOOL_CHOICE`, `STRIX_PARALLEL_TOOL_CALLS`,
`STRIX_LLM_CACHED_INPUT_COST_PER_MTOK` (кеша нет; переменная читается, но
влияет только на отчётность).

### 3.3 Бюджет

Бюджет измеряется **в токенах** (`STRIX_MAX_BUDGET_TOKENS`). Денежный бюджет
остаётся факультативным: локальный провайдер стоимости не имеет, поэтому при
отсутствии заданных цен стоимость считается нулевой.

---

## 4. Форматы артефактов

Rust-движок обязан писать **байт в байт те же** файлы, что и Python-движок.

```
strix_runs/<run-name>/
├── run.json                      # запись скана
├── penetration_test_report.md    # executive summary
├── vulnerabilities/
│   ├── vuln-0001.md              # одна находка
│   └── ...
├── vulnerabilities.json          # индекс находок (пишется, если есть находки)
├── vulnerabilities.csv           # индекс для spreadsheets (CRLF, экранирование формул)
├── findings.sarif                # SARIF 2.1.0
└── coverage.json                 # леджер покрытия
```

Ключевые инварианты:

* `run.json`: ключи `run_id`, `run_name`, `start_time`, `end_time`, `status`,
  `auth_mode`, `targets_info`, `llm_usage`;
* `findings.sarif`: `version = "2.1.0"`, `$schema` =
  `https://json.schemastore.org/sarif-2.1.0.json`, `tool.driver.name = "Strix"`;
  документ пишется **всегда**, даже когда находок нет — иначе `codeql-action`
  не сможет закрыть старые алерты;
* идентификаторы находок `vuln-NNNN` выдаются по порядку и **никогда не
  переиспользуются**, в том числе после удаления находки;
* `vulnerabilities.csv` использует `\r\n` и экранирует значения, начинающиеся с
  `=`, `+`, `-`, `@`, `\t`, `\r` (префикс `'`), как в `strix/report/writer.py`;
* `coverage.json`: `schema_version = 1`, секции `entries`, `gaps`, `summary`,
  `machine_observed`, `completeness`;
* `llm_usage` в `run.json`: у локального провайдера `cost = 0.0`, если цены не
  заданы.

Эталонные примеры — в `strix-rs/crates/strix-contracts/tests/fixtures/golden/`.

---

## 5. Golden-фикстуры

Генератор: `scripts/export_golden_fixtures.py` (Python, работает поверх
эталонной реализации).

```bash
cd strix
uv run python scripts/export_golden_fixtures.py
```

Результат: 20 сценариев + `tool-schemas.json` (43 схемы инструментов) в
`strix-rs/crates/strix-contracts/tests/fixtures/golden/`.

Сценарии покрывают: пустой скан, одну находку, находку с привязкой к коду,
две находки одного класса, endpoint-находки, все пять уровней severity,
зависимость с CVE, заголовок с префиксом формулы, заголовок с обратными
кавычками, все пять исходов покрытия, смешанное покрытие, находку с
предложенным исправлением, находку с двумя файлами, находку без CWE и без
привязки, смешанные классы, длинный заголовок.

Нормализация при экспорте: timestamps → `<TIMESTAMP>`, `end_time` →
`<END_TIME>`, `run_id` → `<RUN_ID>`. Сравнение Rust-вывода с эталоном обязано
применять ту же нормализацию.

---

## 6. Mock-провайдер

`strix-rs/crates/strix-mock-llm` — mock `llama-server` для тестов.

Сценарии выбираются заголовком `X-Mock-Scenario` или полем `mock_scenario` тела:

| Сценарий | Что проверяет |
|---|---|
| `tool_call_ok` | корректный одиночный вызов |
| `tool_call_empty_id` | пустой `id` → синтез |
| `tool_call_duplicate_ids` | два одинаковых `id` → перенумерация |
| `tool_call_truncated_arguments` | обрезанный JSON аргументов |
| `tool_call_parallel` | два параллельных вызова с `id` `0`/`1` |
| `text_only` | ответ без вызова инструмента |
| `finish_length` | `finish_reason = "length"` |
| `context_overflow` | HTTP 400 «превышен контекст» |
| `server_error` | HTTP 500 |
| `rate_limited` | HTTP 429 |
| `stall_after_first_chunk` | поток замирает → клиент обязан сработать по таймауту |

---

## 7. Коды возврата

| Режим | Коды |
|---|---|
| headless-скан | `0` — чисто, `1` — фатальная ошибка, `2` — найдены уязвимости |
| интерактивный | всегда `0` |
| cloud | `0` успех, `1` ошибка, `2` неверное использование, `4` нет прав/лимита, `5` требуется оплата, `130` прерывание |

---

## 8. Протокол TUI

Формат кадра: `{"version": 3, "type": ..., "request_id": ..., "payload": ...}`.
Транспорт: POSIX `socketpair` + `STRIX_TUI_FD`; на Windows — loopback +
`STRIX_TUI_ADDR`/`STRIX_TUI_TOKEN` (HMAC). Framing: `struct.Struct(">I")`.
Возможности: `state-revisions`, `collection-deltas`,
`structured-command-errors`, `agents-collection`. Ограничения коллекций:
`events: 5000`, `vulnerabilities: 1000`.

Rust-реализация заменяет Python-backend; Go-sidecar не меняется.

---

## 9. Сессии

Схема сессий Rust — собственная (`strix_rs_session_v1`). Сессии Python
(`agents.db`) **не возобновляются из Rust**. Обратное тоже не гарантируется:
совместимость форматов не заявлена и не проверяется.

Формат сообщений координатора сохраняется, чтобы TUI и промпты оставались
совместимыми:

```json
{"role": "user", "content": "[Message from {name} ({id}) | type={type} | priority={priority}]\n{content}"}
```

`child_initial_input` передаётся одним сообщением пользователя — так
чередуются роли, как того требует локальный chat-template.

---

## 10. Соответствие модулей Python → Rust

| Python | Rust |
|---|---|
| `strix/core/execution.py` | `strix-agent` |
| `strix/core/hooks.py` | `strix-agent` (бюджет, паузы) |
| `strix/core/agents.py` | `strix-agent` (граф агентов) |
| `strix/core/sessions.py` | `strix-agent` (`strix_rs_session_v1`) |
| `strix/core/inputs.py` | `strix-cli` / `strix-agent` |
| `strix/llm/compaction.py` | `strix-agent` (компакция) |
| `strix/config/models.py` | `strix-config` (профили) |
| `strix/config/tool_call_*.py` | `strix-llm` (гигиена вызовов) |
| `strix/agents/factory.py` | `strix-tools` (43 инструмента) |
| `strix/tools/**` | `strix-tools` |
| `strix/runtime/**` | `strix-runtime` |
| `strix/report/**` | `strix-report` |
| `strix/interface/tui/backend/**` | `strix-tui` |
| `strix/interface/cli/**` | `strix-cli` |

---

## 11. Что порт **не** переносит

* LiteLLM целиком: `model_cost`, `token_counter`, `completion_cost`,
  `get_llm_provider`, `validate_environment`, `drop_params`, `modify_params`,
  колбэки успеха/провала, патч `litellm.OpenrouterConfig`;
* SDK `openai-agents`: `Runner.run_streamed`, `RunHooks`,
  `agents.memory.SQLiteSession`, `agents.sandbox.*`, `agents.mcp.*`,
  `agents.models.interface.Model`, `agents.usage.Usage`, `agents.retry.*` —
  это переписывается, а не переносится;
* viewer (React/Vite SPA и `viewer/server.py`), PDF-отчёт (`reportlab`).
