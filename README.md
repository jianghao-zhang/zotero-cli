# zotero-cli

Rust CLI for fast local Zotero access. The package is `zotero-cli`; the shipped binary is `zcli`.

v1 is CLI-only. There is no MCP server and no required HTTP bridge. Core commands read local Zotero data by default and do not require `llm-for-zotero`, Claude Code, Codex, Hermes, OpenClaw, a Zotero Web API key, or the optional Zotero helper plugin.

## Install

For local npm installation from this repo:

```bash
npm install -g .
zcli doctor --format pretty
```

For local package testing:

```bash
npm pack
npm install -g ./zotero-cli-0.1.0.tgz
```

The npm package is a thin wrapper around the Rust binary. If a prebuilt `zcli` binary is not packaged for the current platform, npm `postinstall` falls back to `cargo build --release --bin zcli`. Set `ZCLI_BINARY=/path/to/zcli` to force a specific binary, or `ZOTERO_CLI_SKIP_POSTINSTALL=1` to skip the build step.

Rust developer install:

```bash
cargo install --path .
zcli doctor --format pretty
```

During development:

```bash
cargo run -- doctor --format pretty
```

## Data Sources

`zcli` reads local Zotero metadata from `zotero.sqlite` and attachment files from the sibling `storage/` directory. You can point it at a custom library with:

```bash
zcli --db ~/Zotero/zotero.sqlite --storage ~/Zotero/storage doctor
```

The default config lives in the platform config directory, for example:

```text
macOS: ~/Library/Application Support/zotero-cli/config.toml
Linux: ~/.config/zotero-cli/config.toml
```

Initialize or inspect it:

```bash
zcli setup
zcli setup --dry-run
zcli setup --defaults
zcli config init
zcli config status --format pretty
```

`zcli setup` is an interactive wizard for local Zotero paths, optional mirror output, optional Zotero Web API identity, optional llm-for-zotero runtime support, and optional agent skill installation. It does not run network requests or import papers.

## Zotero Web API Entry

Core v1 commands stay local-first, but the CLI has a Web API config entry for users who want to add a Zotero online library identity for later sync/import features.

Create or manage official Zotero API keys here:

```text
https://www.zotero.org/settings/keys
```

`--library-id` is Zotero's numeric Web API ID. It is not your username, email, library name, or the local SQLite `libraryID`.

For a personal library, use `--library-type user` and the `Your userID for use in API calls` number shown on the API Keys page. For a group library, use `--library-type group` and the numeric groupID from the group URL/settings link, or retrieve group IDs from `/users/<userID>/groups`. Zotero documents the URL shape as `/users/<userID>` and `/groups/<groupID>`:

```text
https://www.zotero.org/support/dev/web_api/v3/basics#user_and_group_library_urls
```

Use an environment variable for the key:

```bash
zcli config web-api \
  --enable \
  --library-type user \
  --library-id 1234567 \
  --api-key-env ZOTERO_API_KEY
```

Or explicitly store a key from stdin:

```bash
printf '%s' "$ZOTERO_API_KEY" | zcli config web-api --enable --api-key-stdin
```

`zcli doctor` reports whether the Web API is configured and whether a key is present, but redacts stored keys. v1 does not use the network for core local commands.

## Commands

```bash
zcli doctor
zcli examples
zcli resolve "title / DOI / arXiv / URL / file path"
zcli paper ITEMKEY
zcli context ITEMKEY --budget 40k
zcli config init
zcli config status
zcli config web-api --enable --library-type user --library-id 1234567 --api-key-env ZOTERO_API_KEY

zcli search list "query"
zcli search grep "regex-or-text"
zcli search context ITEMKEY "regex-or-text"

zcli item get ITEMKEY
zcli item extract ITEMKEY
zcli item annotations ITEMKEY
zcli item notes ITEMKEY
zcli item attachments ITEMKEY
zcli item bibtex ITEMKEY
zcli item markdown ITEMKEY --format text
zcli item markdown ITEMKEY --output paper.md
zcli markdown status ITEMKEY

zcli collection list
zcli collection items COLLECTIONKEY
zcli tags list
zcli tags items "tag name"
zcli write tags ITEMKEY --add "review" --dry-run
zcli write collection ITEMKEY --collection COLLECTIONKEY --action add --dry-run
zcli write note ITEMKEY --content "reading note" --dry-run
zcli write attach ITEMKEY ./paper.pdf --mode link --dry-run
zcli write rename-attachment ATTACHMENTKEY --name paper.pdf --dry-run
zcli write import-files ./paper.pdf --dry-run
zcli write trash ITEMKEY --dry-run
zcli recent --days 7

zcli mirror status
zcli mirror rebuild --dry-run
zcli mirror rebuild --write-markdown
zcli mirror sync --dry-run
zcli mirror watch
zcli mirror daemon-install --dry-run

zcli setup
zcli recap reading --from 2026-04-01 --to 2026-04-25
zcli recap today --why
zcli recap week
zcli recap reading --item ITEMKEY --from 2026-04-01 --to 2026-04-25
zcli recap reading --no-lfz --from 2026-04-01 --to 2026-04-25
zcli lfz doctor
zcli lfz turns --item ITEMKEY
zcli lfz turn claude:123 --budget 40k
zcli recap lfz --from today --to today
zcli recap lfz --item ITEMKEY --from today --to today --limit 10
zcli lfz turn claude:123
zcli recap lfz --details --include-contexts --from today --to today

zcli queue add ITEMKEY --note "read next"
zcli queue list
zcli queue done ITEMKEY
zcli todo list
zcli open ITEMKEY --dry-run
zcli reveal ITEMKEY --dry-run
zcli export pack ITEMKEY --for codex --output ./pack --dry-run
zcli skill doctor
zcli helper doctor
zcli helper install --dry-run
zcli inbox status
zcli inbox fetch --dry-run
```

Output defaults to `auto`: human-readable text in an interactive terminal, compact JSON when stdout is piped or captured by an agent. Use `--format json` for stable machine output, `--format pretty` for readable JSON, or `--format text` for human output.

## Workflow Commands

`zcli resolve` accepts the inputs users and agents naturally have: title fragments, DOI, arXiv ID, URL, Zotero key, or attachment path. `zcli paper ITEMKEY` returns a compact work surface for one paper: metadata, collections, tags, counts, attachment status, Markdown status, and the next useful commands. `zcli context ITEMKEY --budget 40k` returns an agent-oriented context pack with budget metadata, truncation markers, and fetch commands for full output.

## Recaps

`zcli recap reading` is for normal Zotero users. It returns date-range paper activity with metadata: title, authors, year, DOI/arXiv/url, collections, tags, attachments, annotation/note counts, timestamp, and provenance. If `llm-for-zotero` support is enabled in config, it also attaches the matching compact lfz recap; use `--no-lfz` for a pure reading recap. `--include-lfz` records an explicit request, but still follows local config. If `llm-for-zotero` is not enabled, the recap remains core-only and reports that lfz is not enabled in config.

Provenance is explicit:

- `cli_read_log`: `zcli` itself read or extracted the item.
- `annotation`: an annotation changed in the requested date range.
- `note`: a note changed in the requested date range.
- `metadata_modified`: Zotero metadata changed. This is a touched-paper fallback, not proof of reading.

`zcli recap lfz` is for optional `llm-for-zotero` users. By default it returns a compact agent-friendly recap: counts, conversation metadata, question excerpts, answer excerpts, agent final excerpts, model/runtime labels, linked Zotero item metadata, and event counts. Excerpts include truncation metadata such as `text_truncated`, `text_chars`, and `text_excerpt_chars`, plus a stable `message_ref` and `turn_command`. Trace/event payloads are intentionally unavailable; `zcli` exposes only `event_count`, not the intermediate Claude/runtime trace contents. Selected/full-text paper context JSON is also omitted unless explicitly requested with `--include-contexts`. Use `--item ITEMKEY` to recap one paper instead of only a date range, `--limit N` to cap compact excerpts, `--full-text` to include full question/answer/final text, and `--details` for full message/run rows.

## Markdown

`zcli item markdown ITEMKEY` returns a Markdown version of a paper. If llm-for-zotero support is enabled, `zcli` first looks for MinerU cache files generated by llm-for-zotero:

```text
<ZoteroDataDir>/llm-for-zotero-mineru/<attachmentItemID>/full.md
<ZoteroDataDir>/llm-for-zotero-mineru/<attachmentItemID>/_content.md
<ZoteroDataDir>/llm-for-zotero-mineru/<attachmentItemID>.md
```

The lookup uses Zotero's internal PDF attachment item ID, not the parent item key. If no cache is found, it falls back to a local Markdown document built from metadata, abstract, notes, annotations, attachments, and extracted text.

Use `--format text` to print raw Markdown, or `--output paper.md` to write a file:

```bash
zcli item markdown ITEMKEY --format text
zcli item markdown ITEMKEY --output paper.md
```

To expand one specific question from a recap:

```bash
zcli lfz turn claude:123
```

`lfz turn` returns the full question, matching answer messages, and matching agent final text for that turn. It does not expose trace/event payloads.

## Optional Zotero Helper Plugin

The helper plugin is optional. It exists only for local Zotero-runtime writes that the read-only SQLite path should not perform. Normal search, item reads, recaps, Markdown, mirror, and agent skill usage do not depend on it.

The CLI remains the public interface. Agents should call `zcli write ...`, not the helper endpoint. `zcli write ... --dry-run` previews locally without starting Zotero or installing the helper. `--execute` requires the helper plugin, Zotero's local HTTP server, and the token file written by the plugin.

Helper lifecycle:

```bash
zcli helper doctor --format pretty
zcli helper package --dry-run
zcli helper install --dry-run
zcli helper install --execute
```

`zcli helper doctor` probes both the unauthenticated helper status endpoint and the token-authenticated ping. `not_installed_or_server_unreachable` means the XPI is not loaded yet or Zotero's local HTTP server on `127.0.0.1:23119` is unavailable. The helper manifest includes Zotero 9's required `applications.zotero.update_url`; without it, Zotero reports the local XPI as incompatible. After installing or replacing the XPI, restart Zotero, then run `zcli helper doctor --format pretty` before using `zcli write ... --execute`.

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

Current helper capabilities are whitelisted: tag add/remove, collection add/remove, note creation, local file import/link, attachment rename, and Zotero trash moves. It does not expose arbitrary JavaScript, does not write SQLite directly, and defaults to dry-run previews at the CLI boundary. Installing the helper copies an XPI into the selected Zotero profile and requires a Zotero restart. If the helper installs but `doctor` still cannot connect, enable Zotero's local connector/API communication setting in Zotero preferences and restart Zotero.

The helper is deliberately small and fast: startup only ensures one token file and registers one Zotero local endpoint; the token is cached in memory after startup; execute calls use compact responses by default; file existence checks happen only for attachment/file operations; and the protocol supports a batch operation so future CLI flows can submit multiple whitelisted writes in one localhost round trip.

## Mirror

`zcli mirror rebuild` creates a filesystem mirror inspired by high-value ZoFiles behavior:

- collection folders
- `Allin/` flat index
- per-item `metadata.json`
- optional `arxiv.id`
- attachment symlink mode by default, copy mode with `--mode copy`

The mirror is a generated filesystem view for agents, search tools, editors, and file-native workflows. It is not part of Zotero itself.

Refresh modes:

- `zcli mirror rebuild`: full rebuild.
- `zcli mirror sync`: rebuild plus stale cleanup based on the previous `.zcli-mirror-index.json`.
- `zcli mirror watch`: foreground auto-maintainer. It polls the Zotero DB signature and runs sync after changes settle.

Add `--write-markdown` to write `paper.md` in each mirrored item directory, following the same llm-for-zotero MinerU cache first, local fallback second order.

`watch` is intentionally foreground in v1:

```bash
zcli --mirror-root ~/ZoteroMirror mirror watch
```

The default watcher is tuned for low local overhead: one DB metadata check every 60 seconds, a 5 second settle delay after change detection, and no recursive storage scan. Use `--include-storage` only if you need storage directory metadata to participate in the watch signature. Use a larger `--interval`, such as `--interval 300`, for an almost idle background watcher.

For one-shot automation tests or launch wrappers:

```bash
zcli --mirror-root ~/ZoteroMirror mirror watch --once
```

`zcli mirror daemon-install --dry-run` previews a macOS launchd wrapper for the foreground watcher. Use `--execute` only after reviewing the generated command.

Preview first:

```bash
zcli --mirror-root ~/ZoteroMirror mirror rebuild --dry-run --format pretty
```

Then execute:

```bash
zcli --mirror-root ~/ZoteroMirror mirror rebuild
```

## Agent Skill

The skill is optional. It teaches agents to call `zcli` directly and not depend on MCP or an adapter API.

Preview install paths:

```bash
zcli skill install --target codex --dry-run
zcli skill install --target claude --dry-run
zcli skill install --target hermes --dry-run
zcli skill install --target lfz --dry-run
zcli skill install --target openclaw --dry-run
```

Default targets:

```text
Codex: ~/.codex/skills/zotero-cli
Claude Code: ~/.claude/skills/zotero-cli
Hermes: ~/.hermes/skills/zotero-cli
llm-for-zotero Claude runtime: ~/Zotero/agent-runtime/.claude/skills/zotero-cli
OpenClaw: ~/.openclaw/skills/zotero-cli
```

On macOS/Linux the installer prefers symlinks. Use `--copy` for a copied install. OpenClaw is detected before install; it is not required for normal use.

## TODO / Roadmap

The current local CLI path is usable, but these pieces still need work before treating v1 as broadly release-ready:

- Web API smoke and remote mode: add a `zcli web-api doctor` or `zcli web-api ping` command that validates auth, library ID, permissions, and read access against Zotero's official API. Current Web API support is configuration-only; core commands still do not use the network.
- Inbox/import pipeline: implement `zcli inbox fetch` as the external "papers to read" entry point, with source adapters, duplicate detection, dry-run previews, and explicit execution for imports.
- Mirror watch hardening: run long-duration `zcli mirror watch` tests, validate CPU and I/O over hours or days, and polish launchd/daemon installation. Current actual write testing covered small rebuilds; full-library sync has been dry-run tested.
- Helper execute coverage: expand real helper tests beyond tag add/remove to notes, collections, local file import/link, attachment rename, batch operations, and trash safety. Add clearer diagnostics for sandboxed agents that cannot reach Zotero localhost even when the helper is available in a normal shell.
- Cross-environment installs: test npm package and helper XPI on fresh Zotero 7/8/9 profiles, macOS Intel/ARM, and Linux; verify fallback Cargo builds when no prebuilt native binary is present.
- Agent skill installs: verify actual symlink/copy installs for Codex, Claude Code, Hermes, llm-for-zotero runtime, and OpenClaw, not only dry-run path detection.
- Test matrix: keep adding golden JSON tests for all public commands, fixture SQLite coverage for metadata/annotations/notes/attachments/collections, helper edge-case tests, and package smoke tests for npm release artifacts.

Known testing notes:

- `zcli helper doctor` can report unavailable inside sandboxed agent runtimes unless localhost access is allowed; run it from a normal terminal or with localhost permission when validating the helper.
- Any real helper `--execute` write can update Zotero item modified timestamps even if the visible tag/note state is later restored.

## Safety Boundary

Core Zotero access is local and read-only. Import, inbox, and mutation paths are dry-run-first and require an explicit execution flag before they can do anything. Web API credentials are optional configuration, and the optional Zotero helper plugin is the only local Zotero-runtime write bridge.

## Acknowledgements

`zcli` takes substantial product and interaction inspiration from the Zotero ecosystem. [ZoFiles](https://github.com/X1AOX1A/ZoFiles) shaped the filesystem mirror, `Allin/` index, Markdown-oriented paper surface, and agent-friendly local file workflow. [zotero-mcp](https://github.com/54yyyu/zotero-mcp) helped clarify high-value agent operations, local/API boundary tradeoffs, and the importance of compact, tool-friendly outputs. [llm-for-zotero](https://github.com/yilewang/llm-for-zotero) shaped the optional LLM recap and paper Markdown reuse path. This project is not affiliated with those projects, but it benefited from studying their work.
