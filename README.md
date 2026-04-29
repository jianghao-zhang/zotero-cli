# zotero-cli

Fast local Zotero CLI. The npm package is `zotero-cli`; the installed binary is `zcli`.

This is currently a personal-use draft project, not a polished public release.

`zcli` is CLI-only in v1. Core commands read local Zotero data and do not require external agent runtimes, Zotero Web API credentials, an MCP server, an HTTP bridge, or the optional Zotero helper plugin.

Optional ecosystem links:

| Project | How `zcli` uses it |
| --- | --- |
| [`llm-for-zotero`](https://github.com/yilewang/llm-for-zotero) | Optional LLM recap, Claude runtime metadata, and MinerU `full.md` reuse. |
| [Codex](https://github.com/openai/codex) | Optional agent skill target and export-pack target. |
| [Claude Code](https://code.claude.com/docs) | Optional agent skill target and lfz Claude runtime context. |
| [Hermes Agent](https://github.com/nousresearch/hermes-agent) | Optional agent skill target and export-pack target. |
| [OpenClaw](https://github.com/openclaw/openclaw) | Optional agent skill target and export-pack target. |

## Contents

- [Quick Start](#quick-start)
- [Setup](#setup)
- [Feature Map](#feature-map)
- [Common Workflows](#common-workflows)
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
npm install -g ./zotero-cli-0.1.0.tgz
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

## Setup

`zcli setup` is the interactive setup wizard. It writes local config only; it does not contact Zotero Web API, import papers, or mutate your Zotero library.

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
zcli config status --format pretty
```

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
| Health and examples | `doctor`, `examples` | Checks config, local Zotero paths, optional Web API config, helper status, and lfz availability. |
| Config | `setup`, `config init`, `config status`, `config web-api` | Writes and inspects local config. |
| Resolve and paper surface | `resolve`, `find paper`, `paper`, `context` | Finds an item from natural inputs such as title, short title, citation key, DOI, arXiv, URL, or file path; returns a compact paper view or builds an agent context pack. |
| Search | `search list`, `search grep`, `search context` | Searches metadata/full text and returns matching context. |
| Local paper index | `index status`, `index update`, `index search`, `index chunks`, `index chunk`, `index get` | Builds a local SQLite FTS5/BM25 sidecar index for repeated fast paper and passage search. No network or model is required. |
| Item reads | `item get`, `item extract`, `item annotations`, `item notes`, `item attachments`, `item bibtex`, `item markdown` | Reads Zotero item metadata, extracted text, annotations, notes, attachments, BibTeX, and paper Markdown. |
| Markdown status | `markdown status` | Shows whether Markdown will come from lfz MinerU cache or local fallback. |
| Library browsing | `collection list`, `collection items`, `tags list`, `tags items`, `recent` | Lists collections, tags, tagged items, collection items, and recently touched papers. |
| Reading queue | `queue add`, `queue list`, `queue done`, `todo list` | Local read-next queue for user and agent workflows. |
| Recaps | `recap reading`, `recap today`, `recap week`, `recap lfz` | Date-range reading activity and optional lfz conversation recap. |
| lfz drill-down | `lfz doctor`, `lfz turns`, `lfz turn` | Checks lfz tables and retrieves specific question/final-answer turns. |
| Mirror | `mirror status`, `mirror rebuild`, `mirror sync`, `mirror watch`, `mirror daemon-install` | Generates and maintains a filesystem mirror of the Zotero library. |
| Local writes | `write tags`, `write collection`, `write note`, `write attach`, `write rename-attachment`, `write import-files`, `write trash` | Dry-run-first write plans; execution requires the optional Zotero helper plugin. |
| UI handoff | `open`, `reveal` | Dry-run-first commands for opening or revealing Zotero items/files. |
| Agent export | `export pack` | Builds a paper pack for [Codex](https://github.com/openai/codex), [Claude Code](https://code.claude.com/docs), [Hermes Agent](https://github.com/nousresearch/hermes-agent), or [OpenClaw](https://github.com/openclaw/openclaw) style workflows. |
| Agent skill | `skill doctor`, `skill install` | Installs the optional `zotero-cli` skill into supported agent skill roots. |
| Helper plugin | `helper doctor`, `helper package`, `helper install` | Packages and installs the optional Zotero runtime helper for writes. |
| Inbox | `inbox status`, `inbox fetch --dry-run` | Reserved external paper intake entry point. Mutation/import remains explicit and dry-run-first. |

## Common Workflows

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

Read item data:

```bash
zcli item get ITEMKEY
zcli item extract ITEMKEY
zcli item annotations ITEMKEY
zcli item notes ITEMKEY
zcli item attachments ITEMKEY
zcli item bibtex ITEMKEY
```

Open local UI/file targets safely:

```bash
zcli open ITEMKEY --dry-run
zcli reveal ITEMKEY --dry-run
```

## Zotero Web API

Core v1 commands stay local-first. Web API config exists so users can save a Zotero online library identity/API key for future sync, remote read, or import workflows.

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

`zcli doctor` reports whether the Web API is configured and whether a key is present, but redacts stored keys. v1 core commands do not use the network.

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

The helper plugin is optional. It exists only for local Zotero-runtime writes that the read-only SQLite path should not perform. Normal search, item reads, recaps, Markdown, mirror, and agent skill usage do not depend on it.

Agents should call `zcli write ...`, not the helper endpoint. `zcli write ... --dry-run` previews locally without starting Zotero or installing the helper. `--execute` requires the helper plugin, Zotero's local HTTP server, and the token file written by the plugin.

Helper lifecycle:

```bash
zcli helper doctor --format pretty
zcli helper package --dry-run
zcli helper install --dry-run
zcli helper install --execute
```

Write commands:

```bash
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
| Tags | Add/remove tags. |
| Collections | Add/remove item membership. |
| Notes | Create child notes. |
| Attachments | Link/import local files and rename attachments. |
| Import files | Import local files through Zotero runtime. |
| Trash | Move Zotero items to trash. |

The helper does not expose arbitrary JavaScript and does not write SQLite directly. Installing it copies an XPI into the selected Zotero profile and requires a Zotero restart. If `doctor` cannot connect after install, enable Zotero's local connector/API communication setting in Zotero preferences and restart Zotero.

`zcli helper doctor` probes both the unauthenticated helper status endpoint and the token-authenticated ping. `not_installed_or_server_unreachable` means the XPI is not loaded yet or Zotero's local HTTP server on `127.0.0.1:23119` is unavailable.

The helper is deliberately small and fast: startup ensures one token file and registers one local endpoint; the token is cached in memory after startup; execute calls use compact responses; file existence checks happen only for attachment/file operations; batch operation support allows future CLI flows to submit multiple whitelisted writes in one localhost round trip.

## Agent Skill

The skill is optional. It teaches agents to call `zcli` directly and not depend on MCP or an adapter API.

There are two skill surfaces: `skills/zotero-cli/SKILL.md` is the portable external-agent skill for Codex, Claude Code, Hermes Agent, and OpenClaw; `skills/zotero-cli-lfz/SKILL.md` is the specialized [`llm-for-zotero`](https://github.com/yilewang/llm-for-zotero) Claude runtime skill that frames `zcli` as Zotero-native paper/library access.

The `lfz` target is special: it uses `skills/zotero-cli-lfz/SKILL.md` and installs into detected [`llm-for-zotero`](https://github.com/yilewang/llm-for-zotero) Claude runtime roots. Different Zotero profiles can have different runtime folders, so dry-run output may list multiple `target_paths`.

Preview install paths:

```bash
zcli skill install --target codex --dry-run
zcli skill install --target claude --dry-run
zcli skill install --target hermes --dry-run
zcli skill install --target lfz --dry-run
zcli skill install --target openclaw --dry-run
```

Default targets:

| Agent/runtime | Default skill path |
| --- | --- |
| [Codex](https://github.com/openai/codex) | `~/.codex/skills/zotero-cli` |
| [Claude Code](https://code.claude.com/docs) | `~/.claude/skills/zotero-cli` |
| [Hermes Agent](https://github.com/nousresearch/hermes-agent) | `~/.hermes/skills/zotero-cli` |
| [`llm-for-zotero`](https://github.com/yilewang/llm-for-zotero) Claude runtime | Detected profile roots such as `<Zotero data>/agent-runtime/profile-*/.claude/skills/zotero-cli` |
| [OpenClaw](https://github.com/openclaw/openclaw) | `~/.openclaw/skills/zotero-cli` |

On macOS/Linux the installer prefers symlinks. Use `--copy` for a copied install. [OpenClaw](https://github.com/openclaw/openclaw) is detected before install; it is not required for normal use.

## TODO / Roadmap

The current local CLI path is usable, but these pieces still need work before treating v1 as broadly release-ready:

| Area | Remaining work |
| --- | --- |
| Web API smoke and remote mode | Add `zcli web-api doctor` or `zcli web-api ping` to validate auth, library ID, permissions, and read access against Zotero's official API. Current Web API support is configuration-only. |
| Semantic index layer | Extend the new local index with optional GGUF embeddings, local reranking, warm daemon mode, and cached query expansion. The current shipped layer is model-free SQLite FTS5/BM25. |
| Inbox/import pipeline | Implement `zcli inbox fetch` as the external "papers to read" entry point with source adapters, duplicate detection, dry-run previews, and explicit execution for imports. |
| Mirror watch hardening | Run long-duration `zcli mirror watch` tests, validate CPU/I/O over hours or days, and polish launchd/daemon installation. Current actual write testing covered small rebuilds; full-library sync has been dry-run tested. |
| Helper execute coverage | Expand real helper tests beyond tag add/remove to notes, collections, file import/link, attachment rename, batch operations, and trash safety. |
| Cross-environment installs | Test npm package and helper XPI on fresh Zotero 7/8/9 profiles, macOS Intel/ARM, and Linux; verify fallback Cargo builds when no prebuilt native binary is present. |
| Agent skill installs | Verify actual symlink/copy installs for [Codex](https://github.com/openai/codex), [Claude Code](https://code.claude.com/docs), [Hermes Agent](https://github.com/nousresearch/hermes-agent), [`llm-for-zotero`](https://github.com/yilewang/llm-for-zotero) runtime, and [OpenClaw](https://github.com/openclaw/openclaw), not only dry-run path detection. |
| Test matrix | Keep adding golden JSON tests for all public commands, fixture SQLite coverage, helper edge-case tests, and package smoke tests for npm release artifacts. |

Known testing notes:

| Case | Note |
| --- | --- |
| Sandboxed agents | `zcli helper doctor` can report unavailable unless localhost access is allowed. Validate helper status from a normal terminal or with localhost permission. |
| Real helper writes | Any `--execute` write can update Zotero item modified timestamps even if the visible tag/note state is later restored. |

## Safety Boundary

| Boundary | Policy |
| --- | --- |
| Core local access | Read-only. Reads local Zotero SQLite and storage data. |
| Network | Not required for core commands. Web API config is optional and redacted in output. |
| Mutations/imports | Dry-run-first and require an explicit execution flag. |
| Zotero runtime writes | Routed only through the optional helper plugin. |
| Helper endpoint | Private implementation detail; agents should call `zcli`, not the helper endpoint. |

## Acknowledgements

`zcli` takes substantial product and interaction inspiration from the Zotero ecosystem. [ZoFiles](https://github.com/X1AOX1A/ZoFiles) shaped the filesystem mirror, `Allin/` index, Markdown-oriented paper surface, and agent-friendly local file workflow. [zotero-mcp](https://github.com/54yyyu/zotero-mcp) helped clarify high-value agent operations, local/API boundary tradeoffs, and the importance of compact, tool-friendly outputs. [`llm-for-zotero`](https://github.com/yilewang/llm-for-zotero) shaped the optional LLM recap and paper Markdown reuse path. `zcli` benefited from studying their work.
