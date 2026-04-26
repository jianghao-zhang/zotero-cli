var ZcliHelper = {
  endpointPath: "/zcli-helper",
  version: "0.1.0",
  protocolVersion: 1,
  notifierID: "zcli-helper",
  token: null,

  async startup() {
    this.token = await this.ensureToken();
    this.registerEndpoint();
  },

  async shutdown() {
    if (Zotero.Server && Zotero.Server.Endpoints) {
      delete Zotero.Server.Endpoints[this.endpointPath];
    }
  },

  log(message) {
    try {
      Zotero.debug("[zcli-helper] " + message);
    } catch (_) {}
  },

  tokenPath() {
    return PathUtils.join(Zotero.DataDirectory.dir, "zcli-helper-token");
  },

  async ensureToken() {
    var path = this.tokenPath();
    try {
      if (await IOUtils.exists(path)) {
        var bytes = await IOUtils.read(path);
        var existing = new TextDecoder().decode(bytes).trim();
        if (existing) return existing;
      }
    } catch (_) {}
    var token = this.randomToken();
    await IOUtils.write(path, new TextEncoder().encode(token + "\n"));
    return token;
  },

  async readToken() {
    if (this.token) return this.token;
    var bytes = await IOUtils.read(this.tokenPath());
    this.token = new TextDecoder().decode(bytes).trim();
    return this.token;
  },

  randomToken() {
    var bytes = new Uint8Array(32);
    crypto.getRandomValues(bytes);
    return Array.from(bytes)
      .map(function (b) {
        return b.toString(16).padStart(2, "0");
      })
      .join("");
  },

  registerEndpoint() {
    var self = this;
    class Endpoint {
      supportedMethods = ["GET", "POST"];
      supportedDataTypes = ["application/json"];

      init = async function (options) {
        try {
          if (options.method === "GET") {
            return [200, "application/json", JSON.stringify(self.publicStatus())];
          }
          var postData =
            typeof options.data === "string"
              ? options.data
              : JSON.stringify(options.data || {});
          var request = JSON.parse(postData);
          var result = await self.handle(request);
          return [200, "application/json", JSON.stringify(result)];
        } catch (error) {
          return [
            500,
            "application/json",
            JSON.stringify({
              ok: false,
              error: error && error.message ? error.message : String(error),
            }),
          ];
        }
      };
    }
    Zotero.Server.Endpoints[this.endpointPath] = Endpoint;
    this.log("registered endpoint " + this.endpointPath);
  },

  publicStatus() {
    return {
      ok: true,
      name: "zcli-helper",
      version: this.version,
      protocolVersion: this.protocolVersion,
      mode: "fast",
      tokenPath: this.tokenPath(),
      capabilities: this.capabilities(),
      performance: {
        tokenCachedInMemory: !!this.token,
        defaultCompactResponse: true,
        batch: true,
        storageChecks: "on_demand",
      },
      arbitraryJS: false,
    };
  },

  capabilities() {
    return [
      "ping",
      "batch",
      "apply_tags",
      "move_to_collection",
      "create_note",
      "import_local_files",
      "link_attachment",
      "rename_attachment",
      "trash_items",
    ];
  },

  async handle(request) {
    if (!request || typeof request !== "object") {
      throw new Error("JSON object request required");
    }
    var token = await this.readToken();
    if (request.token !== token) {
      throw new Error("invalid zcli helper token");
    }
    var op = request.op || "ping";
    var params = request.params || {};
    var dryRun = request.dry_run !== false;
    var compact = request.compact !== false && params.compact !== false;
    switch (op) {
      case "ping":
        return this.publicStatus();
      case "batch":
        return this.batch(params, dryRun, compact);
      case "apply_tags":
        return this.applyTags(params, dryRun, compact);
      case "move_to_collection":
        return this.moveToCollection(params, dryRun, compact);
      case "create_note":
        return this.createNote(params, dryRun, compact);
      case "import_local_files":
        return this.importLocalFiles(params, dryRun, compact);
      case "link_attachment":
        return this.linkAttachment(params, dryRun, compact);
      case "rename_attachment":
        return this.renameAttachment(params, dryRun, compact);
      case "trash_items":
        return this.trashItems(params, dryRun, compact);
      default:
        throw new Error("unsupported zcli helper op: " + op);
    }
  },

  async batch(params, defaultDryRun, compact) {
    var operations = params.operations || params.ops || [];
    if (!Array.isArray(operations)) throw new Error("batch operations array required");
    var results = [];
    for (var operation of operations) {
      if (!operation || typeof operation !== "object") {
        throw new Error("invalid batch operation");
      }
      if (operation.op === "batch") throw new Error("nested batch is not supported");
      var operationDryRun = defaultDryRun
        ? true
        : operation.dry_run === undefined
          ? false
          : operation.dry_run !== false;
      var result = await this.handle({
        op: operation.op,
        token: this.token,
        dry_run: operationDryRun,
        compact,
        params: operation.params || {},
      });
      results.push(result);
    }
    return {
      ok: true,
      op: "batch",
      dry_run: defaultDryRun,
      compact,
      count: results.length,
      results,
    };
  },

  libraryID(params) {
    return (
      Number(params.libraryID) ||
      (Zotero.Libraries && Zotero.Libraries.userLibraryID) ||
      1
    );
  },

  itemByKeyOrID(params) {
    if (params.itemID) return Zotero.Items.get(Number(params.itemID));
    if (params.itemId) return Zotero.Items.get(Number(params.itemId));
    var key = params.itemKey || params.key;
    if (!key) return null;
    if (Zotero.Items.getByLibraryAndKey) {
      return Zotero.Items.getByLibraryAndKey(this.libraryID(params), String(key));
    }
    return null;
  },

  itemsFromParams(params) {
    var out = [];
    var ids = params.itemIDs || params.itemIds || [];
    var keys = params.itemKeys || params.keys || [];
    for (var id of ids) {
      var item = Zotero.Items.get(Number(id));
      if (item) out.push(item);
    }
    for (var key of keys) {
      var item = this.itemByKeyOrID({
        libraryID: params.libraryID,
        libraryId: params.libraryId,
        itemKey: key,
      });
      if (item) out.push(item);
    }
    return out;
  },

  compactItem(item) {
    return {
      itemID: item.id,
      key: item.key,
    };
  },

  resultItem(item, extra, compact) {
    return Object.assign(compact ? this.compactItem(item) : this.snapshotItem(item), extra || {});
  },

  snapshotItem(item) {
    var itemType = "";
    try {
      itemType = Zotero.ItemTypes.getName(item.itemTypeID);
    } catch (_) {}
    return {
      itemID: item.id,
      key: item.key,
      title: item.getDisplayTitle ? item.getDisplayTitle() : item.getField("title"),
      itemType,
    };
  },

  async applyTags(params, dryRun, compact) {
    var items = this.itemsFromParams(params);
    var add = params.add_tags || params.addTags || [];
    var remove = params.remove_tags || params.removeTags || [];
    var results = [];
    for (var item of items) {
      var added = [];
      var removed = [];
      for (var tag of add) {
        if (tag && !item.hasTag(tag)) {
          added.push(tag);
          if (!dryRun) item.addTag(tag, 0);
        }
      }
      for (var tagToRemove of remove) {
        if (tagToRemove && item.hasTag(tagToRemove)) {
          removed.push(tagToRemove);
          if (!dryRun) item.removeTag(tagToRemove);
        }
      }
      if (!dryRun && (added.length || removed.length)) {
        await item.saveTx();
      }
      results.push(this.resultItem(item, { added, removed }, compact));
    }
    return { ok: true, op: "apply_tags", dry_run: dryRun, compact, count: results.length, items: results };
  },

  collectionFromParams(params) {
    if (params.collectionID || params.collectionId) {
      return Zotero.Collections.get(Number(params.collectionID || params.collectionId));
    }
    if (params.collectionKey && Zotero.Collections.getByLibraryAndKey) {
      return Zotero.Collections.getByLibraryAndKey(
        this.libraryID(params),
        String(params.collectionKey),
      );
    }
    return null;
  },

  async moveToCollection(params, dryRun, compact) {
    var collection = this.collectionFromParams(params);
    if (!collection) throw new Error("collection not found");
    var action = params.action === "remove" ? "remove" : "add";
    var items = this.itemsFromParams(params);
    var results = [];
    for (var item of items) {
      if (!dryRun) {
        if (action === "remove") item.removeFromCollection(collection.id);
        else item.addToCollection(collection.id);
        await item.saveTx();
      }
      results.push(this.resultItem(item, { collectionID: collection.id, action }, compact));
    }
    return { ok: true, op: "move_to_collection", dry_run: dryRun, compact, count: results.length, items: results };
  },

  async createNote(params, dryRun, compact) {
    var parent = this.itemByKeyOrID(params);
    var content = String(params.content || params.note || "");
    if (!content.trim()) throw new Error("note content is required");
    var html = /<p|<div|<h[1-6]/i.test(content)
      ? content
      : "<p>" + content.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/\n/g, "<br/>") + "</p>";
    if (params.title) {
      html =
        "<h1>" +
        String(params.title).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;") +
        "</h1>" +
        html;
    }
    if (dryRun) {
      return {
        ok: true,
        op: "create_note",
        dry_run: true,
        compact,
        parent: parent ? this.resultItem(parent, null, compact) : null,
        contentChars: content.length,
      };
    }
    var note = new Zotero.Item("note");
    note.libraryID = parent ? parent.libraryID : this.libraryID(params);
    if (parent) note.parentID = parent.id;
    note.setNote(html);
    var itemID = await note.saveTx();
    return { ok: true, op: "create_note", dry_run: false, compact, itemID, parentID: parent ? parent.id : null };
  },

  async importLocalFiles(params, dryRun, compact) {
    var files = params.filePaths || params.files || [];
    var parent = this.itemByKeyOrID(params);
    var results = [];
    for (var filePath of files) {
      var exists = await IOUtils.exists(filePath);
      if (dryRun || !exists) {
        results.push({
          filePath,
          status: exists ? "would_import" : "not_found",
          parent: parent ? this.resultItem(parent, null, compact) : null,
        });
        continue;
      }
      var request = {
        file: filePath,
        libraryID: this.libraryID(params),
      };
      if (parent) request.parentItemID = parent.id;
      var item = await Zotero.Attachments.importFromFile(request);
      results.push({
        filePath,
        status: "imported",
        itemID: item.id,
        parentID: item.parentID || null,
      });
    }
    return { ok: true, op: "import_local_files", dry_run: dryRun, compact, count: results.length, items: results };
  },

  async linkAttachment(params, dryRun, compact) {
    var parent = this.itemByKeyOrID(params);
    if (!parent) throw new Error("parent item not found");
    var filePath = String(params.filePath || params.path || "");
    if (!filePath) throw new Error("filePath is required");
    if (dryRun) {
      return { ok: true, op: "link_attachment", dry_run: true, compact, parent: this.resultItem(parent, null, compact), filePath };
    }
    var item = await Zotero.Attachments.linkFromFile({
      file: filePath,
      parentItemID: parent.id,
      title: params.title || PathUtils.filename(filePath),
    });
    return { ok: true, op: "link_attachment", dry_run: false, compact, itemID: item.id, parentID: parent.id };
  },

  async renameAttachment(params, dryRun, compact) {
    var item = this.itemByKeyOrID(params);
    if (!item || !item.isAttachment()) throw new Error("attachment not found");
    var newName = String(params.newName || params.name || "");
    if (!newName) throw new Error("newName is required");
    if (dryRun) {
      return { ok: true, op: "rename_attachment", dry_run: true, compact, attachment: this.resultItem(item, null, compact), newName };
    }
    if (Zotero.Attachments.renameAttachmentFile) {
      await Zotero.Attachments.renameAttachmentFile(item, newName);
    } else {
      item.setField("title", newName);
      await item.saveTx();
    }
    return { ok: true, op: "rename_attachment", dry_run: false, compact, itemID: item.id, newName };
  },

  async trashItems(params, dryRun, compact) {
    var items = this.itemsFromParams(params);
    var results = [];
    for (var item of items) {
      if (!dryRun) {
        item.deleted = true;
        await item.saveTx();
      }
      results.push(this.resultItem(item, null, compact));
    }
    return { ok: true, op: "trash_items", dry_run: dryRun, compact, count: results.length, items: results };
  },
};

function install(data, reason) {}

async function startup(data, reason) {
  await ZcliHelper.startup();
}

async function shutdown(data, reason) {
  if (reason === APP_SHUTDOWN) return;
  await ZcliHelper.shutdown();
}

function uninstall(data, reason) {}
