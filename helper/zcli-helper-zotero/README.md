# zcli Helper for Zotero

The helper is the narrow Zotero-native bridge for operations the Zotero 10
Local API does not provide directly: translators, PDF recognition, linked or
imported local files, attachment file renaming, and move-to-trash semantics.
Standard tag, collection-membership, and note writes go through the Zotero 10
Local API and do not depend on this plugin.

This is an optional Zotero plugin for `zcli`. The CLI remains the public
interface; users and agents should call `zcli import ...` or `zcli write ...`,
not this endpoint directly. The helper only exposes a small localhost JSON
endpoint for Zotero translator and filesystem operations that the Local API
and read-only SQLite path cannot do cleanly.

The endpoint is `/zcli-helper` on Zotero's local HTTP server. It accepts only
whitelisted operations, requires the token written to `zcli-helper-token` in the
Zotero data directory, and does not expose arbitrary JavaScript execution.
Zotero 9 requires `applications.zotero.update_url` in the manifest; this helper
ships an empty update manifest until release publishing is wired up.
The CLI probes the unauthenticated status endpoint first, so
`zcli helper doctor` can distinguish a missing token from an unavailable local
HTTP server.

Fast-mode behavior:

- one endpoint only
- token cached in memory after startup
- compact responses by default
- storage checks only for attachment/file operations
- `batch` op for multiple whitelisted writes in one localhost round trip
- arXiv imports try Zotero translators first and fall back to arXiv Atom
  metadata plus PDF attachment when Zotero returns no item

`zcli` owns import planning and mutation safety. The helper receives only
validated helper payloads after `zcli import ... --dry-run` or
`zcli write ... --dry-run` has produced the preview; it should not be treated as
a public automation surface.

Supported operation names:

- `ping`
- `batch`
- `import_identifiers`
- `import_pdfs`
- `import_urls`
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

For standard writes, authorize the Zotero 10 Local API instead:

```sh
zcli local-api authorize --dry-run
zcli local-api authorize --execute
```

After installing or replacing the XPI, restart Zotero and run
`zcli helper doctor --format pretty`. If it reports
`not_installed_or_server_unreachable`, the XPI has not loaded yet or Zotero's
local HTTP server on `127.0.0.1:23119` is unavailable.

Preview writes without contacting the helper:

```sh
zcli import arxiv 2604.06240 --dry-run
zcli import ids 10.1145/1234567.1234568 --dry-run
zcli import pdf ./paper.pdf --dry-run
zcli import url https://arxiv.org/abs/2604.06240 --dry-run
zcli write tags ITEMKEY --add review --dry-run
zcli write attach ITEMKEY ./paper.pdf --mode import --dry-run
zcli write rename-attachment ATTACHMENTKEY --name paper.pdf --dry-run
```

Restart Zotero after installing or replacing the XPI.
