# zotero-cli

Fast local Zotero CLI for paper reading, discovery, import planning, and agent workflows. The npm package is `zotero-cli`; the installed binary is `zcli`.

This project is still evolving, but the current core loop is usable:

```text
discover papers -> triage whether they are worth reading -> preview import -> queue/tag -> build reading context
```

Core Zotero reads are local-first and do not require Zotero Web API credentials, MCP, an HTTP bridge, or the optional Zotero helper plugin. Networked discovery sources such as alphaXiv, Hugging Face Papers, and X/Bird are explicit opt-in command paths. Zotero mutations and imports are dry-run-first.

## Read In Codex App

```bash
zcli read "title / citation key / DOI / arXiv / Zotero key / PDF filename" \
  --output outputs/zotero-reader --format json
```

`read` makes the paper-reading contract explicit: native `full.md` is the primary agent source, generated Markdown is the fallback, and the local PDF is a visual preview for Codex App. Its output includes a ready-made `response.open_pdf_markdown` link that should remain at the end of every response while the same paper is active.

The PDF is hardlinked by default, so Zotero's original attachment is not modified or duplicated. Use `--copy` explicitly only when the output is on another filesystem volume; there is no silent fallback.

Optional ecosystem links:

| Project | How `zcli` uses it |
| --- | --- |
| [`llm-for-zotero`](https://github.com/yilewang/llm-for-zotero) | Optional LLM recap, Claude runtime metadata, and MinerU `full.md` reuse. |
| [Codex](https://github.com/openai/codex) | Optional agent skill target and export-pack target. |
| [Claude Code](https://code.claude.com/docs) | Optional agent skill target and lfz Claude runtime context. |
| [Hermes Agent](https://github.com/nousresearch/hermes-agent) | Optional agent skill target and export-pack target. |
| [OpenClaw](https://github.com/openclaw/openclaw) | Optional agent skill target and export-pack target. |

## What It Does Now

| Need | Start here |
| --- | --- |
| Read/search your local Zotero library | `zcli resolve`, `zcli paper`, `zcli context`, `zcli index search`, `zcli index chunks` |
| Find new papers | `zcli inbox fetch`, `zcli inbox triage`, `zcli alphaxiv brief` |
| Check X discussion around one paper | `zcli inbox discussion` |
| Preview paper import | `zcli import arxiv/ids/pdf/url --dry-run` |
| Keep a reading queue | `zcli queue add/list/done` |
| Generate agent context | `zcli context`, `zcli export pack`, installed `zotero-cli` skill |
| Read recent activity or lfz chats | `zcli recap reading`, `zcli recap lfz`, `zcli lfz turn` |
| Mirror Zotero to files | `zcli mirror rebuild/sync/watch` |
| Execute Zotero writes | Zotero 10 Local API for tags/collections/notes; helper for translators and files |
| Validate explicit remote access | `zcli web-api doctor` checks the configured key, library, permissions, and read access without writing |

## Contents

- [Quick Start](#quick-start)
- [Core Workflows](#core-workflows)
- [Setup](#setup)
- [Feature Map](#feature-map)
- [Local Library Workflows](#local-library-workflows)
- [Inbox](#inbox)
- [Paper Discovery](#paper-discovery)
- [Recaps](#recaps)
- [Optional llm-for-zotero Support](#optional-llm-for-zotero-support)
- [Markdown](#markdown)
- [Mirror](#mirror)
- [Optional Zotero Helper Plugin](#optional-zotero-helper-plugin)
- [Agent Skill](#agent-skill)
- [TODO / Roadmap](#todo--roadmap)
- [Safety Boundary](#safety-boundary)

## Quick Start

Install from this repo with npm:

```bash
npm install -g .
zcli setup
zcli doctor --format pretty
```

For local package testing:

```bash
npm pack
npm install -g ./zotero-cli-0.2.1.tgz
```

The npm package is a thin wrapper around the Rust binary. If no packaged `zcli` binary matches the current platform, npm `postinstall` falls back to:

```bash
cargo build --release --bin zcli
```

Useful install knobs:

| Variable | Effect |
| --- | --- |
| `ZCLI_BINARY=/path/to/zcli` | Force the npm wrapper to use a specific binary. |
| `ZOTERO_CLI_SKIP_POSTINSTALL=1` | Skip the npm postinstall build step. |

Rust developer install:

```bash
cargo install --path .
zcli doctor --format pretty
```

During development:

```bash
cargo run -- doctor --format pretty
```

## Core Workflows

Find and read a local paper:

```bash
zcli resolve "agent memory" --format json
zcli paper ITEMKEY --format json
zcli context ITEMKEY --budget 40k --format json
zcli index chunks "credit assignment" --item ITEMKEY --format json
```

Find new papers and decide what to read:

```bash
zcli inbox fetch "coding agent harness memory" --source alphaxiv --days 30 --date-field any --dry-run --format json
zcli inbox triage "agent memory" --source huggingface --days 30 --code-overview --dry-run --format json
zcli alphaxiv brief "coding agent harness memory" --days 30 --date-field any --limit 8 --format text
```

Investigate author/community discussion around a paper:

```bash
zcli inbox discussion "Agentic Harness Engineering" --days 30 --format json
zcli inbox discussion 2604.04979 --tweet https://x.com/author/status/123 --reply-limit 80 --format json
```

Preview import and then create a reading context:

```bash
zcli import arxiv 2604.25850 --dry-run --format json
zcli queue add ITEMKEY --note "from inbox candidate" --format json
zcli context ITEMKEY --budget 40k --format json
```

## Setup

`zcli setup` is the interactive setup wizard. It writes local config only; it does not contact Zotero Web API, import papers, or mutate your Zotero library.

Interactive setup starts with a human-readable config overview and a section menu. Press Enter for the full setup path, or choose only the area you want to edit:

| Section | What it configures |
| --- | --- |
| `1 local Zotero` | Zotero database and storage paths. |
| `2 inbox sources` | Curated X paper accounts for `zcli inbox fetch --source x`; low risk, stores public handles only. |
| `3 mirror` | Optional filesystem mirror root. |
| `4 Web API` | Optional Zotero Web API identity and API key source. |
| `5 llm-for-zotero` | Optional lfz recap/runtime integration. |
| `6 agent skills` | Optional Codex/Claude/Hermes/lfz/OpenClaw skill install targets. |
| `7 advanced/high-risk auth` | Cookie/token-backed features such as alphaXiv Recommended/auth commands. Default off for new users. |
| `s save` | Save the current config and exit. |

Risk levels are explicit. Low-risk features use local paths, public handles, public paper APIs, or dry-run-only previews. High-risk features can read login cookies, bearer tokens, browser session state, or authenticated write helpers, and stay disabled unless the user opts in on that machine.

It can configure:

| Area | Purpose | Required |
| --- | --- | --- |
| Zotero database path | Local metadata, collections, tags, notes, annotations, attachment indexes. | Yes, usually auto-detected. |
| Zotero storage path | PDFs, attachment files, and full-text cache files. | Yes, usually auto-detected. |
| Mirror root | Generated file/folder view of the library for agents and file-native workflows. | Optional. |
| Zotero Web API | Stores online library identity/API key for future remote sync or import workflows. | Optional. |
| [`llm-for-zotero`](https://github.com/yilewang/llm-for-zotero) runtime | Enables LLM chat recap and MinerU `full.md` reuse. | Optional. |
| Agent skill | Installs `SKILL.md` so agents know to call `zcli` directly. | Optional. |

Config commands:

```bash
zcli setup
zcli setup --dry-run
zcli setup --defaults
zcli config init
zcli config status --format text
zcli doctor --format text
```

`zcli doctor` is the broad health check. It reports the running binary and PATH `zcli`, config paths, local Zotero database/storage, Web API key presence without exposing the key, inbox schemas and X/Bird readiness, risk-gated auth flags, helper availability, llm-for-zotero tables, and agent skill distribution status. It does not use network for core Zotero checks.

Default config paths:

| Platform | Path |
| --- | --- |
| macOS | `~/Library/Application Support/zotero-cli/config.toml` |
| Linux | `~/.config/zotero-cli/config.toml` |

You can override the library paths per command:

```bash
zcli --db ~/Zotero/zotero.sqlite --storage ~/Zotero/storage doctor
```

## Output Modes

Output defaults to `auto`:

| Mode | Behavior |
| --- | --- |
| Interactive terminal | Human-readable text. |
| Piped/captured stdout | Compact JSON for agents and scripts. |
| `--format json` | Stable compact JSON. |
| `--format pretty` | Pretty JSON. |
| `--format text` | Human/plain text when supported. |

## Feature Map

| Area | Commands | What it does |
| --- | --- | --- |
| Health and examples | `doctor`, `examples` | Checks config, local Zotero paths, Local API, helper status, and lfz availability without using the network. |
| Config | `setup`, `config init`, `config status`, `config web-api` | Writes and inspects local config. |
| Resolve and paper surface | `resolve`, `find paper`, `find duplicates`, `paper`, `context` | Finds an item from natural inputs, detects likely duplicate records locally, returns a compact paper view, or builds an agent context pack. |
| Search | `search list`, `search grep`, `search context` | Searches metadata/full text and returns matching context. |
| Local paper index | `index status`, `index update`, `index search`, `index chunks`, `index chunk`, `index get` | Builds a local SQLite FTS5/BM25 sidecar index for repeated fast paper and passage search. No network or model is required. |
| Unified inbox | `inbox status`, `inbox fetch`, `inbox triage`, `inbox discussion`, `inbox sources x add/list/remove` | Multi-source paper intake from alphaXiv, Hugging Face Papers, and X/Bird. Returns `paper_candidate/v1` and `paper_discussion/v1` surfaces with time semantics, triage, local context match, workflow commands, and dry-run import plans. |
| Paper discovery | `alphaxiv feed`, `alphaxiv search`, `alphaxiv discover`, `alphaxiv brief`, `alphaxiv paper`, `alphaxiv markdown`, `alphaxiv pdf`, `alphaxiv zotero-plan` | Uses alphaXiv as a public paper discovery/metrics source, with time-window filtering, research triage, and dry-run Zotero import plans. |
| Item reads and citations | `item get`, `item extract`, `item annotations`, `item notes`, `item attachments`, `item cite`, `item export`, `item bibtex`, `item markdown` | Reads local item state; formats exact CSL citations and translator exports through Zotero 10 Local API. `item bibtex` remains the lightweight offline approximation. |
| Markdown status | `markdown status` | Shows whether Markdown will come from lfz MinerU cache or local fallback. |
| Library browsing | `collection list`, `collection items`, `tags list`, `tags items`, `recent` | Lists collections, tags, tagged items, collection items, and recently touched papers. |
| Reading queue | `queue add`, `queue list`, `queue done`, `todo list` | Local read-next queue for user and agent workflows. |
| Recaps | `recap reading`, `recap today`, `recap week`, `recap lfz` | Date-range reading activity and optional lfz conversation recap. |
| lfz drill-down | `lfz doctor`, `lfz turns`, `lfz turn` | Checks lfz tables and retrieves specific question/final-answer turns. |
| Mirror | `mirror status`, `mirror rebuild`, `mirror sync`, `mirror watch`, `mirror daemon-install` | Generates and maintains a filesystem mirror of the Zotero library. |
| Paper import | `import arxiv`, `import ids`, `import pdf`, `import url` | Dry-run-first high-level paper import through Zotero native translators, PDF import, and PDF metadata recognition. |
| Local writes | `write tags`, `write collection`, `write note`, `write metadata`, `write attach`, `write rename-attachment`, `write import-files`, `write trash` | Dry-run-first plans; standard writes use Local API, translator/filesystem writes use helper. |
| UI handoff | `open`, `reveal` | Dry-run-first commands for opening or revealing Zotero items/files. |
| Agent export | `export pack` | Builds a paper pack for [Codex](https://github.com/openai/codex), [Claude Code](https://code.claude.com/docs), [Hermes Agent](https://github.com/nousresearch/hermes-agent), or [OpenClaw](https://github.com/openclaw/openclaw) style workflows. |
| Agent skill | `skill doctor`, `skill install` | Installs the optional `zotero-cli` skill into supported agent skill roots. |
| Local API | `local-api doctor`, `local-api authorize` | Authorizes Zotero 10 standard writes without a custom plugin. |
| Web API | `web-api doctor` | Explicit read-only network probe for the configured key, library ID, permissions, and library access. |
| Helper plugin | `helper doctor`, `helper package`, `helper install` | Packages the optional translator and filesystem bridge. |
| Inbox source config | `inbox sources x add/list/remove` | Maintains local public X handles for curated paper-account scans. |

## Local Library Workflows

Find a paper from whatever identifier you have:

```bash
zcli resolve "title / short title / citation key / DOI / arXiv / URL / file path"
zcli find paper "agentic rl survey"
zcli paper ITEMKEY
```

Build context for an agent:

```bash
zcli context ITEMKEY --budget 40k
zcli export pack ITEMKEY --for codex --output ./pack --dry-run
```

Search local Zotero content:

```bash
zcli search list "query"
zcli search grep "regex-or-text"
zcli search context ITEMKEY "regex-or-text"
```

Build and search the local paper index:

```bash
zcli index update
zcli index search "agentic rl survey"
zcli index chunks "credit assignment" --item ITEMKEY
zcli index chunks "context compression" --collection "Agent Papers"
zcli index chunk ITEMKEY:annotation:2
zcli index get ITEMKEY
```

The index is a generated local sidecar in zcli's cache directory. It currently uses SQLite FTS5/BM25 over Zotero metadata, identifiers, short titles, citation keys, tags, collections, abstracts, notes, annotations, and optional full text. `index search` is optimized for paper candidates and does not rank every full-text page. `index chunks` returns passage candidates and can be scoped by item, collection, or tag. Chunk hits include best-effort page labels when they come from Zotero annotations or PDF page separators; missing pages mean the source text had no reliable page marker. `--include-full-text` adds extracted attachment text to the chunk layer when you want full-paper passage search. Future local embedding/reranker layers should attach to this index instead of replacing the CLI surface.

Preview paper imports before touching Zotero:

```bash
zcli import arxiv 2604.06240 --dry-run
zcli import ids 10.1145/1234567.1234568 --dry-run
zcli import pdf ./paper.pdf --dry-run
zcli import url https://arxiv.org/abs/2604.06240 --dry-run
```

`import arxiv`, `import ids`, `import pdf`, and `import url` share one import-plan layer. Dry-run output is the canonical preview: normalized source, duplicate check, helper payload, and the execution command. `import arxiv` and `import ids` use Zotero's Add Item by Identifier translator path. For arXiv, the helper falls back to arXiv Atom metadata plus PDF attachment if Zotero returns no item. `import pdf` copies a local or remote PDF through Zotero and asks Zotero to recognize metadata unless `--no-recognize` is set. `import url` first detects identifier-style URLs, then PDF URLs, then falls back to Zotero web translators or a webpage item.

## Paper Discovery

`zcli alphaxiv` is the current discovery-source adapter. It is Rust-native for normal feed/search/metadata/PDF work and does not require browser automation for public alphaXiv surfaces.

Useful entry points:

```bash
zcli alphaxiv feed --sort hot --days 7 --date-field first-seen --rank-metrics --limit 30
zcli alphaxiv search "agentic harness" --since 2026-04-28 --date-field published --rank-metrics
zcli alphaxiv discover "coding agent harness memory" --days 30 --date-field any --limit 10
zcli alphaxiv brief "coding agent harness memory" --days 30 --date-field any --limit 8 --format text
zcli alphaxiv zotero-plan 2604.25850 --format json
```

Time filtering is explicit:

| Field | Use when |
| --- | --- |
| `first-seen` | You want papers newly appearing on alphaXiv. |
| `published` | You want papers by publication date. |
| `updated` | You want recently changed entries. |
| `any` | You want recent activity across first-seen, published, or updated. |

`alphaxiv brief` is the agent-facing triage command. It combines alphaXiv search with a feed fallback, deduplicates, ranks by alphaXiv metrics, assigns `read_now` / `skim` / `watch` lanes, explains relevance, and attaches dry-run Zotero import commands. It does not mutate Zotero.

## Inbox

`zcli inbox` is the cross-source paper intake surface. Current adapters cover alphaXiv, Hugging Face Papers, and X via the local Bird CLI. The output is source-neutral so later arXiv, OpenAlex, Semantic Scholar, RSS, conference, and code-signal adapters can attach without changing downstream agent workflows.

```bash
zcli inbox status --format json
zcli inbox fetch --source alphaxiv --sort hot --interval "3 Days" --limit 30 --dry-run --format json
zcli inbox fetch --source alphaxiv --sort hot --interval "3 Days" --limit 30 --execute --format json
zcli inbox fetch "coding agent harness memory" --source alphaxiv --days 30 --date-field any --limit 20 --dry-run --format json
zcli inbox fetch --source huggingface --days 3 --date-field any --limit 20 --dry-run --format json
zcli inbox fetch "coding agent" --source huggingface --days 30 --date-field any --limit 10 --dry-run --format json
zcli inbox triage "coding agent" --source huggingface --days 30 --code-overview --dry-run --format json
zcli inbox discussion "Squeez: Task-Conditioned Tool-Output Pruning for Coding Agents" --handle paper_author --days 30 --format json
zcli inbox discussion 2604.04979 --tweet https://x.com/author/status/123 --reply-limit 80 --format json
zcli inbox sources x add paperreadingclub --format json
zcli inbox sources x list --format json
zcli inbox fetch "agent paper arxiv" --source x --days 1 --limit 10 --dry-run --format json
zcli inbox fetch "agent paper arxiv" --source x --handle paperreadingclub --days 1 --limit 10 --dry-run --format json
zcli inbox sources x remove paperreadingclub --format json
```

`inbox fetch` calls the selected source adapter, normalizes results into `paper_candidate/v1`, and preserves the raw source record for debugging. Each candidate includes source identifiers, URLs, explicit time fields plus per-source time semantics, metrics/resources/social signals, triage lane/reasons, local context match, workflow commands, and a dry-run Zotero plan when an arXiv ID can be inferred. `inbox triage` is an alias for the same candidate pipeline with the workflow-oriented output.

For alphaXiv, an empty query means feed discovery. Defaults are tuned for daily screening: `--source alphaxiv --sort hot --interval "3 Days" --limit 30`. Returned alphaXiv candidates cache overview markdown under the zcli cache directory with a 7-day TTL so the fast review step can read alphaXiv's own AI overview before any import. `--no-cache-overview` disables that cache.

Duplicate control is local. Candidates already present in Zotero are hidden by default; pass `--show-existing` to inspect them. Candidates already recorded in the inbox seen-state are hidden for 30 days by default; pass `--show-seen` or adjust `--seen-days`. `--execute` at this layer only records the displayed candidates as seen. It does not import papers, write notes, tag items, or touch the reading queue.

Hugging Face uses the public daily papers and paper-search endpoints. With an empty query it returns daily/trending papers over the requested window; with a query it searches Hugging Face Papers and supplements from daily papers when needed. X uses `bird --quote-depth 0`; `zcli inbox sources x add HANDLE` stores curated paper accounts in the normal zcli config, `--source x` reads that list by default, and temporary `--handle` values override the configured list for one fetch. Without configured handles or explicit handles, `--source x` uses Bird search.

By default, inbox candidates are lightly re-ranked against local Zotero recent papers and the reading queue. Use `--no-context` to skip that. Use `--code-overview` to attach a quick GitHub API overview for linked repositories: stars, forks, open issues, language, default branch, last push, and archived status. It does not clone repositories or run deep code analysis.

`inbox discussion` is the X community follow-up path for a known paper. Give it a title, arXiv ID, alphaXiv/Hugging Face paper URL, or known X post URL via `--tweet`. It searches likely announcement posts, prioritizes author/curator handles passed with `--handle`, expands replies and thread context with Bird, then returns `paper_discussion/v1`: announcement posts, high-value questions, possible author answers, limitation notes, benchmark/comparison comments, and code/data/reproduction discussion. It is read-only and bounded by `--limit`, `--reply-limit`, and `--max-pages`; quote repost coverage is best-effort through X search.
The output includes the generated X search queries so empty or sparse results are easy to debug without rerunning in verbose mode.

Read item data:

```bash
zcli item get ITEMKEY
zcli item extract ITEMKEY
zcli item annotations ITEMKEY
zcli item notes ITEMKEY
zcli item attachments ITEMKEY
zcli item bibtex ITEMKEY
zcli item cite ITEMKEY --style apa
zcli item cite ITEMKEY --mode citation --style ieee
zcli item export ITEMKEY --translator bibtex
zcli find duplicates --limit 20
```

`item cite` and `item export` use Zotero's own CSL/export engine, so Zotero must
be running. They are the exact path for publication-ready references and data
exports. `item bibtex` stays offline-capable but intentionally exposes only the
small metadata subset available from the local SQLite reader.

Preview a scalar metadata update before executing it:

```bash
zcli write metadata ITEMKEY --set shortTitle="Short title" --clear archiveLocation --dry-run
```

Open local UI/file targets safely:

```bash
zcli open ITEMKEY --dry-run
zcli reveal ITEMKEY --dry-run
```

## Zotero Web API

Core commands stay local-first. Web API access is explicit: configuration stores the online library identity and key source, while `web-api doctor` performs a read-only network check. No command silently changes from Local API to Web API when Zotero is closed.

Official Zotero API key page: [zotero.org/settings/keys](https://www.zotero.org/settings/keys)

Official library ID docs: [User and group library URLs](https://www.zotero.org/support/dev/web_api/v3/basics#user_and_group_library_urls)

`--library-id` is Zotero's numeric Web API ID. It is not your username, email, library name, or local SQLite `libraryID`.

| Library type | Which ID to use |
| --- | --- |
| `user` | The `Your userID for use in API calls` number shown on the API Keys page. |
| `group` | The numeric `groupID` from the group URL/settings link, or from `/users/<userID>/groups`. |

Use an environment variable for the key:

```bash
zcli config web-api \
  --enable \
  --library-type user \
  --library-id 1234567 \
  --api-key-env ZOTERO_API_KEY
```

Or store a key from stdin:

```bash
printf '%s' "$ZOTERO_API_KEY" | zcli config web-api --enable --api-key-stdin
```

`zcli doctor` reports whether the Web API is configured and whether a key is present, but redacts stored keys and does not use the network. Validate the actual key, permissions, library ID, item count, and remote library version explicitly:

```bash
zcli web-api doctor --format json
```

This probe performs no writes. With Zotero closed, local SQLite reads remain available, while Local API and helper writes require Zotero to be running. A future remote mutation path must remain explicit rather than becoming an automatic fallback.

## Recaps

`zcli recap reading` is for normal Zotero users. It returns date-range paper activity with metadata, not a chat summary.

Reading recap entries include:

| Field group | Examples |
| --- | --- |
| Metadata | Title, authors, year, DOI, arXiv ID, URL. |
| Zotero organization | Collections, tags, attachments. |
| Reading signals | Annotation count, note count, timestamp, provenance. |

Provenance labels are explicit:

| Label | Meaning |
| --- | --- |
| `cli_read_log` | `zcli` itself read or extracted the item. |
| `annotation` | An annotation changed in the requested date range. |
| `note` | A note changed in the requested date range. |
| `metadata_modified` | Zotero metadata changed. This is a touched-paper fallback, not proof of reading. |

Examples:

```bash
zcli recap reading --from 2026-04-01 --to 2026-04-25
zcli recap today --why
zcli recap week
zcli recap reading --item ITEMKEY --from 2026-04-01 --to 2026-04-25
zcli recap reading --no-lfz --from 2026-04-01 --to 2026-04-25
```

If [`llm-for-zotero`](https://github.com/yilewang/llm-for-zotero) support is enabled in config, reading recaps also attach the matching compact lfz recap. `--no-lfz` keeps the output pure reading. `--include-lfz` records an explicit request, but still follows local config; it does not force lfz access when lfz is disabled.

## Optional llm-for-zotero Support

`zcli recap lfz` is for optional [`llm-for-zotero`](https://github.com/yilewang/llm-for-zotero) users. It layers LLM conversation metadata on top of Zotero item metadata.

Compact lfz recap includes:

| Data | Notes |
| --- | --- |
| User prompts and assistant excerpts | Excerpts include `text_truncated`, `text_chars`, and `text_excerpt_chars`. |
| Final answers | Each turn has a stable `message_ref` and `turn_command`. |
| Runtime labels | Model/runtime labels and [Claude Code](https://code.claude.com/docs) session metadata when present. |
| Linked papers | Linked Zotero item metadata and selected/full-text paper context metadata. |
| Tool/action shape | Event counts only. Trace payloads are not exposed. |

Commands:

```bash
zcli lfz doctor
zcli lfz turns --item ITEMKEY
zcli lfz turn claude:123 --budget 40k
zcli recap lfz --from today --to today
zcli recap lfz --item ITEMKEY --from today --to today --limit 10
zcli recap lfz --full-text --from today --to today
zcli recap lfz --details --include-contexts --from today --to today
```

`zcli lfz turn MESSAGE_REF` returns the full question, matching answer messages, and matching agent final text for one turn. It does not expose trace/event payloads.

## Markdown

`zcli item markdown ITEMKEY` returns a Markdown version of a paper.

If [`llm-for-zotero`](https://github.com/yilewang/llm-for-zotero) support is enabled, `zcli` first looks for MinerU cache files generated by lfz:

```text
<ZoteroDataDir>/llm-for-zotero-mineru/<attachmentItemID>/full.md
<ZoteroDataDir>/llm-for-zotero-mineru/<attachmentItemID>/_content.md
<ZoteroDataDir>/llm-for-zotero-mineru/<attachmentItemID>.md
```

The lookup uses Zotero's internal PDF attachment item ID, not the parent item key. If no cache is found, `zcli` falls back to a local Markdown document built from metadata, abstract, notes, annotations, attachments, and extracted text.

```bash
zcli markdown status ITEMKEY
zcli item markdown ITEMKEY --format text
zcli item markdown ITEMKEY --output paper.md
```

## Mirror

`zcli mirror rebuild` creates a generated filesystem view inspired by high-value [ZoFiles](https://github.com/X1AOX1A/ZoFiles) behavior.

Mirror output can include:

| Output | Purpose |
| --- | --- |
| Collection folders | Browse papers by Zotero collection. |
| `Allin/` | Flat index for agent/file-native workflows. |
| `metadata.json` | Stable structured metadata per item. |
| `paper.md` | Optional Markdown paper view. |
| `arxiv.id` | Optional arXiv sidecar. |
| Attachment symlinks/copies | Symlink by default, copy with `--mode copy`. |

Refresh modes:

| Command | Behavior |
| --- | --- |
| `zcli mirror rebuild` | Full rebuild. |
| `zcli mirror sync` | Rebuild plus stale cleanup based on `.zcli-mirror-index.json`. |
| `zcli mirror watch` | Foreground auto-maintainer. Polls the Zotero DB signature and syncs after changes settle. |
| `zcli mirror daemon-install --dry-run` | Previews a macOS launchd wrapper for the watcher. |

Preview first:

```bash
zcli --mirror-root ~/ZoteroMirror mirror rebuild --dry-run --format pretty
```

Then execute:

```bash
zcli --mirror-root ~/ZoteroMirror mirror rebuild
zcli --mirror-root ~/ZoteroMirror mirror rebuild --write-markdown
zcli --mirror-root ~/ZoteroMirror mirror sync --dry-run
```

Watcher defaults are tuned for low local overhead: one DB metadata check every 60 seconds, a 5 second settle delay after change detection, and no recursive storage scan.

```bash
zcli --mirror-root ~/ZoteroMirror mirror watch
zcli --mirror-root ~/ZoteroMirror mirror watch --once
zcli --mirror-root ~/ZoteroMirror mirror watch --interval 300
```

Use `--include-storage` only if storage directory metadata should participate in the watch signature.

## Optional Zotero Helper Plugin

The helper plugin is optional. Zotero 10 Local API handles standard tag,
collection-membership, and note writes. The helper remains for Zotero
translators, PDF recognition, local-file operations, physical attachment
renaming, and recoverable move-to-trash semantics. Normal search, item reads,
recaps, Markdown, mirror, and agent skill usage do not depend on it.

Agents should call `zcli write ...`, not either private endpoint. All writes stay
dry-run-first. Standard writes require Zotero 10 to be running and one Local API
authorization:

```bash
zcli local-api doctor --format pretty
zcli local-api authorize --dry-run
zcli local-api authorize --execute
```

Translator and filesystem operations additionally require the helper plugin.

Helper lifecycle:

```bash
zcli helper doctor --format pretty
zcli helper package --dry-run
zcli helper install --dry-run
zcli helper install --execute
```

Write commands:

```bash
zcli import arxiv 2604.06240 --dry-run
zcli import pdf ./paper.pdf --dry-run
zcli import url https://arxiv.org/abs/2604.06240 --dry-run
zcli write tags ITEMKEY --add "review" --remove "old-tag" --dry-run
zcli write collection ITEMKEY --collection COLLECTIONKEY --action add --dry-run
zcli write note ITEMKEY --title "Reading note" --content "..." --dry-run
zcli write attach ITEMKEY ./paper.pdf --mode link --dry-run
zcli write attach ITEMKEY ./paper.pdf --mode import --dry-run
zcli write rename-attachment ATTACHMENTKEY --name paper.pdf --dry-run
zcli write import-files ./paper.pdf --dry-run
zcli write trash ITEMKEY --dry-run
```

Current helper capabilities are whitelisted:

| Capability | Notes |
| --- | --- |
| Paper imports | Import arXiv/DOI/ISBN/PubMed/ADS identifiers, local/remote PDFs, and URLs through Zotero native translator/recognition paths. arXiv has a helper-side Atom metadata/PDF fallback when Zotero returns no item. |
| Attachments | Link/import local files and rename attachments. |
| Import files | Import local files through Zotero runtime. |
| Trash | Move Zotero items to trash. |

The helper does not expose arbitrary JavaScript and does not write SQLite directly. Installing it copies an XPI into the selected Zotero profile and requires a Zotero restart. If `doctor` cannot connect after install, enable Zotero's local connector/API communication setting in Zotero preferences and restart Zotero.

`zcli helper doctor` probes both the unauthenticated helper status endpoint and the token-authenticated ping. `not_installed_or_server_unreachable` means the XPI is not loaded yet or Zotero's local HTTP server on `127.0.0.1:23119` is unavailable.

The helper is deliberately small and fast: startup ensures one token file and registers one local endpoint; the token is cached in memory after startup; execute calls use compact responses; file existence checks happen only for attachment/file operations; batch operation support allows future CLI flows to submit multiple whitelisted writes in one localhost round trip.

Internally, imports and writes share the same dry-run-first mutation executor. The public consequence is simple: preview shape and safety semantics should stay consistent across `zcli import ... --dry-run` and `zcli write ... --dry-run`.

## Agent Skill

Agent integration is a first-class part of this repo. The installed skills teach agents to call `zcli` directly and keep Zotero reads, discovery, imports, and writes on one dry-run-first surface.

Current skill surfaces:

| Skill | Purpose | Installed by |
| --- | --- | --- |
| `skills/zotero-cli/SKILL.md` | Portable Codex/Claude/Hermes/OpenClaw skill for Zotero library access, inbox discovery, X paper discussion, import plans, recaps, and safe writes. | `zcli skill install --target ...` |
| `skills/zotero-cli-lfz/SKILL.md` | Specialized [`llm-for-zotero`](https://github.com/yilewang/llm-for-zotero) Claude runtime skill. Starts from Zotero concepts and selected/pinned paper context. | `zcli skill install --target lfz` |
| `skills/alphaxiv/SKILL.md` | alphaXiv-specific discovery/metrics skill. It hands candidates back into zcli import/inbox workflows instead of creating a parallel Zotero mutation path. | Shared skill symlink distribution |

Default targets:

| Agent/runtime | Default skill path |
| --- | --- |
| [Codex](https://github.com/openai/codex) | `~/.codex/skills/zotero-cli` |
| [Claude Code](https://code.claude.com/docs) | `~/.claude/skills/zotero-cli` |
| [Hermes Agent](https://github.com/nousresearch/hermes-agent) | `~/.hermes/skills/zotero-cli` |
| [`llm-for-zotero`](https://github.com/yilewang/llm-for-zotero) Claude runtime | Detected profile roots such as `<Zotero data>/agent-runtime/profile-*/.claude/skills/zotero-cli` |
| [OpenClaw](https://github.com/openclaw/openclaw) | `~/.openclaw/skills/zotero-cli` |

Preview or refresh install paths:

```bash
zcli skill install --target codex --dry-run
zcli skill install --target claude --dry-run
zcli skill install --target hermes --dry-run
zcli skill install --target lfz --dry-run
zcli skill install --target openclaw --dry-run
```

On macOS/Linux the installer prefers symlinks. Use `--copy` for a copied install. [OpenClaw](https://github.com/openclaw/openclaw) is detected before install; it is not required for normal use. After editing skills, run `zcli skill doctor --format json` and refresh the target installs.

## TODO / Roadmap

The current local CLI path is usable, but these pieces still need work before treating v1 as broadly release-ready:

| Area | Remaining work |
| --- | --- |
| Explicit remote mode | `zcli web-api doctor` now validates the configured key, library ID, permissions, and read access. Any future remote mutation/import commands should remain explicit and dry-run-first. |
| Semantic index layer | Extend the new local index with optional GGUF embeddings, local reranking, warm daemon mode, and cached query expansion. The current shipped layer is model-free SQLite FTS5/BM25. |
| Inbox/import pipeline | `zcli inbox fetch` returns alphaXiv, Hugging Face, and X/Bird backed `paper_candidate/v1` previews. `zcli inbox discussion` returns `paper_discussion/v1` for X community context. Next work is stronger local duplicate scoring, profile-based queue/collection/tag handoff, and optional scheduled digest output. |
| Discovery source adapters | Add arXiv/OpenAlex/Semantic Scholar/RSS/conference feeds behind the same candidate -> import-plan shape. |
| Mirror watch hardening | Run long-duration `zcli mirror watch` tests, validate CPU/I/O over hours or days, and polish launchd/daemon installation. Current actual write testing covered small rebuilds; full-library sync has been dry-run tested. |
| Execute coverage | Expand real Local API tests beyond tags to notes and collections; expand helper tests for translator imports, file import/link, attachment rename, batch operations, and trash safety. |
| Cross-environment installs | Test npm package and helper XPI on fresh Zotero 7/8/9 profiles, macOS Intel/ARM, and Linux; verify fallback Cargo builds when no prebuilt native binary is present. |
| Agent skill installs | Codex, Claude Code, Hermes Agent, and detected [`llm-for-zotero`](https://github.com/yilewang/llm-for-zotero) runtime symlink installs are verified on the primary development machine. Remaining work is fresh-machine validation and OpenClaw install coverage. |
| Test matrix | Keep adding golden JSON tests for all public commands, fixture SQLite coverage, helper edge-case tests, and package smoke tests for npm release artifacts. |

Known testing notes:

| Case | Note |
| --- | --- |
| Sandboxed agents | `zcli helper doctor` can report unavailable unless localhost access is allowed. Validate helper status from a normal terminal or with localhost permission. |
| Real writes | Any `--execute` mutation can update Zotero item modified timestamps even if the visible tag, collection, or note state is later restored. |

## Safety Boundary

| Boundary | Policy |
| --- | --- |
| Core local reads | Read-only SQLite and storage access. |
| Network | Not required for core commands. `web-api doctor` is the explicit read-only network probe; key material remains redacted. |
| Mutations/imports | Dry-run-first and require an explicit execution flag. |
| Standard Zotero writes | Routed through the authenticated Zotero 10 Local API. |
| Translator/filesystem writes | Routed through the optional helper plugin. |
| Helper endpoint | Private implementation detail; agents should call `zcli`, not the helper endpoint. |

## Acknowledgements

`zcli` takes substantial product and interaction inspiration from the Zotero ecosystem. [ZoFiles](https://github.com/X1AOX1A/ZoFiles) shaped the filesystem mirror, `Allin/` index, Markdown-oriented paper surface, and agent-friendly local file workflow. [zotero-mcp](https://github.com/54yyyu/zotero-mcp) helped clarify high-value agent operations, local/API boundary tradeoffs, and the importance of compact, tool-friendly outputs. [`llm-for-zotero`](https://github.com/yilewang/llm-for-zotero) shaped the optional LLM recap and paper Markdown reuse path. `zcli` benefited from studying their work.
