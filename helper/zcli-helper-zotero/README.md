# zcli Helper for Zotero

This is an optional Zotero plugin for `zcli`. The CLI remains the public
interface; users and agents should call `zcli write ...`, not this endpoint
directly. The helper only exposes a small localhost JSON endpoint for local
Zotero runtime operations that the Web API and read-only SQLite path cannot do
cleanly.

The endpoint is `/zcli-helper` on Zotero's local HTTP server. It accepts only
whitelisted operations, requires the token written to `zcli-helper-token` in the
Zotero data directory, and does not expose arbitrary JavaScript execution.
The CLI probes the unauthenticated status endpoint first, so
`zcli helper doctor` can distinguish a missing token from an unavailable local
HTTP server.

Fast-mode behavior:

- one endpoint only
- token cached in memory after startup
- compact responses by default
- storage checks only for attachment/file operations
- `batch` op for multiple whitelisted writes in one localhost round trip

Supported operation names:

- `ping`
- `batch`
- `apply_tags`
- `move_to_collection`
- `create_note`
- `import_local_files`
- `link_attachment`
- `rename_attachment`
- `trash_items`

Install from `zcli`:

```sh
zcli helper doctor --format pretty
zcli helper install --dry-run
zcli helper install --execute
```

After installing or replacing the XPI, restart Zotero and run
`zcli helper doctor --format pretty`. If it reports
`not_installed_or_server_unreachable`, the XPI has not loaded yet or Zotero's
local HTTP server on `127.0.0.1:23119` is unavailable.

Preview writes without contacting the helper:

```sh
zcli write tags ITEMKEY --add review --dry-run
zcli write attach ITEMKEY ./paper.pdf --mode import --dry-run
zcli write rename-attachment ATTACHMENTKEY --name paper.pdf --dry-run
```

Restart Zotero after installing or replacing the XPI.
