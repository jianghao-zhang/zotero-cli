var ZcliHelper = {
  endpointPath: "/zcli-helper",
  version: "0.1.0",
  protocolVersion: 1,
  notifierID: "zcli-helper",
  token: null,
  startupError: null,

  async startup() {
    try {
      if (Zotero.initializationPromise) {
        await Zotero.initializationPromise;
      }
      this.token = await this.ensureToken();
      this.registerEndpoint();
      this.startupError = null;
    } catch (error) {
      this.startupError = error && error.message ? error.message : String(error);
      this.log("startup failed: " + this.startupError);
      try {
        Zotero.logError(error);
      } catch (_) {}
    }
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
    return this.pathUtils().join(Zotero.DataDirectory.dir, "zcli-helper-token");
  },

  pathUtils() {
    if (typeof PathUtils !== "undefined") return PathUtils;
    if (typeof ChromeUtils !== "undefined" && ChromeUtils.importESModule) {
      return ChromeUtils.importESModule("resource://gre/modules/PathUtils.sys.mjs").PathUtils;
    }
    throw new Error("PathUtils is not available");
  },

  ioUtils() {
    if (typeof IOUtils !== "undefined") return IOUtils;
    if (typeof ChromeUtils !== "undefined" && ChromeUtils.importESModule) {
      return ChromeUtils.importESModule("resource://gre/modules/IOUtils.sys.mjs").IOUtils;
    }
    throw new Error("IOUtils is not available");
  },

  async ensureToken() {
    var path = this.tokenPath();
    var io = this.ioUtils();
    try {
      if (await io.exists(path)) {
        var bytes = await io.read(path);
        var existing = new TextDecoder().decode(bytes).trim();
        if (existing) return existing;
      }
    } catch (_) {}
    var token = this.randomToken();
    await io.write(path, new TextEncoder().encode(token + "\n"));
    return token;
  },

  async readToken() {
    if (this.token) return this.token;
    var bytes = await this.ioUtils().read(this.tokenPath());
    this.token = new TextDecoder().decode(bytes).trim();
    return this.token;
  },

  randomToken() {
    var bytes = new Uint8Array(32);
    if (typeof crypto !== "undefined" && crypto.getRandomValues) {
      crypto.getRandomValues(bytes);
      return Array.from(bytes)
        .map(function (b) {
          return b.toString(16).padStart(2, "0");
        })
        .join("");
    }
    var uuidGenerator = Components.classes[
      "@mozilla.org/uuid-generator;1"
    ].getService(Components.interfaces.nsIUUIDGenerator);
    var token = "";
    for (var i = 0; i < 4; i++) {
      token += uuidGenerator.generateUUID().toString().replace(/[{}-]/g, "");
    }
    return token;
  },

  registerEndpoint() {
    if (!Zotero.Server || !Zotero.Server.Endpoints) {
      throw new Error("Zotero local HTTP server is not available");
    }
    var self = this;
    function Endpoint() {}
    Endpoint.prototype.supportedMethods = ["GET", "POST"];
    Endpoint.prototype.supportedDataTypes = ["application/json"];
    Endpoint.prototype.init = async function (options) {
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
      startupError: this.startupError,
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
      "import_identifiers",
      "import_pdfs",
      "import_urls",
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
      case "import_identifiers":
        return this.importIdentifiers(params, dryRun, compact);
      case "import_pdfs":
        return this.importPdfs(params, dryRun, compact);
      case "import_urls":
        return this.importUrls(params, dryRun, compact);
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

  collectionIDsFromParams(params) {
    var libraryID = this.libraryID(params);
    var raw = [];
    for (var name of ["collectionIDs", "collectionIds", "collectionID", "collectionId", "collectionKeys", "collectionKey", "collections", "collectionNames", "collectionName"]) {
      var value = params[name];
      if (value === undefined || value === null) continue;
      if (Array.isArray(value)) raw.push(...value);
      else raw.push(value);
    }
    var ids = [];
    for (var input of raw) {
      var value = String(input).trim();
      if (!value) continue;
      var collection = null;
      if (/^\d+$/.test(value)) {
        collection = Zotero.Collections.get(Number(value));
      }
      if (!collection && /^[A-Z0-9]{8}$/.test(value) && Zotero.Collections.getByLibraryAndKey) {
        collection = Zotero.Collections.getByLibraryAndKey(libraryID, value);
      }
      if (!collection) {
        var matches = Zotero.Collections.getByLibrary(libraryID, true, false)
          .filter((candidate) => candidate.name.toLowerCase() === value.toLowerCase());
        if (matches.length > 1) {
          throw new Error("collection name is ambiguous: " + value);
        }
        collection = matches[0] || null;
      }
      if (!collection) throw new Error("collection not found: " + value);
      if (!ids.includes(collection.id)) ids.push(collection.id);
    }
    return ids;
  },

  async addTagsToItem(item, tags) {
    if (!item || !tags || !tags.length) return;
    var changed = false;
    for (var tag of tags) {
      tag = String(tag || "").trim();
      if (!tag || item.hasTag(tag)) continue;
      item.addTag(tag, 0);
      changed = true;
    }
    if (changed) await item.saveTx();
  },

  importResultItem(item, extra, compact) {
    return Object.assign(this.resultItem(item, extra, compact), {
      itemType: item.itemType || "",
      title: item.getDisplayTitle ? item.getDisplayTitle() : item.getField("title"),
    });
  },

  safeSetField(item, field, value) {
    if (value === undefined || value === null || value === "") return false;
    try {
      item.setField(field, value);
      return true;
    } catch (_) {
      return false;
    }
  },

  identifierFromSpec(spec) {
    var input;
    var kind;
    var value;
    if (spec && typeof spec === "object" && !Array.isArray(spec)) {
      input = String(spec.input || spec.value || "");
      kind = String(spec.kind || spec.type || "").toLowerCase();
      value = String(spec.value || spec.identifier || input || "").trim();
    } else {
      input = String(spec || "");
      value = input.trim();
      kind = "";
    }
    var clean = value.trim();
    if (!clean) throw new Error("empty identifier");

    var arxivURL = clean.match(/arxiv\.org\/(?:abs|pdf)\/([0-9]{4}\.[0-9]{4,5}(?:v[0-9]+)?|[a-z-]+(?:\.[A-Z]{2})?\/[0-9]{7}(?:v[0-9]+)?)(?:\.pdf)?/i);
    if (arxivURL) {
      kind = "arxiv";
      clean = arxivURL[1];
    }
    var arxivPrefix = clean.match(/^arxiv[:\s]+(.+)$/i);
    if (arxivPrefix) {
      kind = "arxiv";
      clean = arxivPrefix[1].trim();
    }
    if (kind === "arxiv" || /^[0-9]{4}\.[0-9]{4,5}(?:v[0-9]+)?$/i.test(clean) || /^[a-z-]+(?:\.[A-Z]{2})?\/[0-9]{7}(?:v[0-9]+)?$/i.test(clean)) {
      return { input, kind: "arxiv", value: clean, zotero: { arXiv: clean } };
    }

    var doiURL = clean.match(/^https?:\/\/(?:dx\.)?doi\.org\/(.+)$/i);
    if (doiURL) {
      kind = "doi";
      clean = decodeURIComponent(doiURL[1]);
    }
    var doiMatch = clean.match(/\b(10\.[0-9]{4,9}\/[^\s"'<>]+[^\s"'<>.,;:)])/i);
    if (kind === "doi" || doiMatch) {
      clean = doiMatch ? doiMatch[1] : clean;
      return { input, kind: "doi", value: clean, zotero: { DOI: clean } };
    }

    if (kind === "pmid") return { input, kind, value: clean, zotero: { PMID: clean } };
    if (kind === "isbn") return { input, kind, value: clean, zotero: { ISBN: clean } };
    if (kind === "ads" || kind === "adsbibcode") {
      return { input, kind: "adsBibcode", value: clean, zotero: { adsBibcode: clean } };
    }

    var extracted = Zotero.Utilities.extractIdentifiers(clean);
    if (extracted && extracted.length) {
      return this.identifierFromObject(input, extracted[0]);
    }
    throw new Error("could not recognize identifier: " + clean);
  },

  identifierFromObject(input, identifier) {
    if (identifier.arXiv) return { input, kind: "arxiv", value: identifier.arXiv, zotero: { arXiv: identifier.arXiv } };
    if (identifier.DOI) return { input, kind: "doi", value: identifier.DOI, zotero: { DOI: identifier.DOI } };
    if (identifier.PMID) return { input, kind: "pmid", value: identifier.PMID, zotero: { PMID: identifier.PMID } };
    if (identifier.ISBN) return { input, kind: "isbn", value: identifier.ISBN, zotero: { ISBN: identifier.ISBN } };
    if (identifier.adsBibcode) return { input, kind: "adsBibcode", value: identifier.adsBibcode, zotero: { adsBibcode: identifier.adsBibcode } };
    throw new Error("unsupported identifier object");
  },

  xmlFirst(parent, localName) {
    if (!parent) return null;
    if (parent.getElementsByTagNameNS) {
      var nsNodes = parent.getElementsByTagNameNS("*", localName);
      if (nsNodes && nsNodes.length) return nsNodes[0];
    }
    var nodes = parent.getElementsByTagName(localName);
    return nodes && nodes.length ? nodes[0] : null;
  },

  xmlText(parent, localName) {
    var node = this.xmlFirst(parent, localName);
    return node && node.textContent ? node.textContent.replace(/\s+/g, " ").trim() : "";
  },

  xmlTexts(parent, localName) {
    if (!parent) return [];
    var nodes = parent.getElementsByTagNameNS
      ? parent.getElementsByTagNameNS("*", localName)
      : parent.getElementsByTagName(localName);
    return Array.from(nodes || [])
      .map((node) => (node.textContent || "").replace(/\s+/g, " ").trim())
      .filter(Boolean);
  },

  async fetchArxivMetadata(arxivID) {
    var url = "https://export.arxiv.org/api/query?id_list=" + encodeURIComponent(arxivID);
    var response = await Zotero.HTTP.request("GET", url, { timeout: 60000 });
    var text = response.responseText || response.response || "";
    var doc = new DOMParser().parseFromString(text, "application/xml");
    var entry = this.xmlFirst(doc, "entry");
    if (!entry) throw new Error("arXiv API returned no entry for " + arxivID);

    var idURL = this.xmlText(entry, "id");
    var match = idURL.match(/arxiv\.org\/abs\/([^/?#]+)/i);
    var normalizedID = match ? match[1] : arxivID;
    var links = entry.getElementsByTagNameNS
      ? entry.getElementsByTagNameNS("*", "link")
      : entry.getElementsByTagName("link");
    var pdfURL = "";
    for (var link of Array.from(links || [])) {
      if (String(link.getAttribute("title") || "").toLowerCase() === "pdf" || String(link.getAttribute("type") || "").toLowerCase() === "application/pdf") {
        pdfURL = link.getAttribute("href") || "";
        break;
      }
    }
    if (!pdfURL) pdfURL = "https://arxiv.org/pdf/" + normalizedID;

    var categories = [];
    var categoryNodes = entry.getElementsByTagNameNS
      ? entry.getElementsByTagNameNS("*", "category")
      : entry.getElementsByTagName("category");
    for (var category of Array.from(categoryNodes || [])) {
      var term = category.getAttribute("term");
      if (term && !categories.includes(term)) categories.push(term);
    }

    return {
      arxivID: normalizedID,
      title: this.xmlText(entry, "title"),
      abstractNote: this.xmlText(entry, "summary"),
      date: (this.xmlText(entry, "published") || this.xmlText(entry, "updated")).slice(0, 10),
      authors: this.xmlTexts(entry, "name"),
      doi: this.xmlText(entry, "doi"),
      url: "https://arxiv.org/abs/" + normalizedID,
      pdfURL,
      categories,
    };
  },

  async importArxivFallback(identifier, libraryID, collections, tags, saveAttachments, compact) {
    var meta = await this.fetchArxivMetadata(identifier.value);
    var itemType = Zotero.ItemTypes.getID("preprint") ? "preprint" : "journalArticle";
    var item = new Zotero.Item(itemType);
    item.libraryID = libraryID;
    this.safeSetField(item, "title", meta.title);
    this.safeSetField(item, "abstractNote", meta.abstractNote);
    this.safeSetField(item, "date", meta.date);
    this.safeSetField(item, "url", meta.url);
    this.safeSetField(item, "DOI", meta.doi);
    this.safeSetField(item, "archive", "arXiv");
    this.safeSetField(item, "archiveLocation", meta.arxivID);
    this.safeSetField(item, "repository", "arXiv");
    var extra = ["arXiv: " + meta.arxivID];
    if (meta.categories.length) extra.push("arXiv categories: " + meta.categories.join(", "));
    this.safeSetField(item, "extra", extra.join("\n"));
    if (meta.authors.length) {
      item.setCreators(
        meta.authors.map((name) => {
          var creator = Zotero.Utilities.cleanAuthor(name, "author", false);
          creator.creatorType = "author";
          return creator;
        }),
      );
    }
    if (collections && collections.length) item.setCollections(collections);
    await item.saveTx();
    await this.addTagsToItem(item, tags);

    var attachments = [];
    if (saveAttachments && meta.pdfURL) {
      try {
        var attachment = await Zotero.Attachments.importFromURL({
          url: meta.pdfURL,
          parentItemID: item.id,
          title: "Full Text PDF",
          contentType: "application/pdf",
        });
        attachments.push(this.importResultItem(attachment, { kind: "pdf" }, compact));
      } catch (error) {
        attachments.push({
          kind: "pdf",
          status: "attachment_error",
          error: error && error.message ? error.message : String(error),
        });
      }
    }
    return { item, attachments, source: "arxiv_api_fallback" };
  },

  async existingItemsForIdentifier(identifier, libraryID, compact) {
    var items = await Zotero.Items.getAll(libraryID, true, false);
    var value = String(identifier.value || "").toLowerCase();
    var matches = [];
    for (var item of items) {
      if (item.isAttachment && item.isAttachment()) continue;
      var found = false;
      if (identifier.kind === "doi") {
        found = String(item.getField("DOI") || "").toLowerCase() === value;
      } else if (identifier.kind === "arxiv") {
        var haystack = [
          item.getField("extra"),
          item.getField("url"),
          item.getField("DOI"),
          item.getField("title"),
        ].filter(Boolean).join("\n").toLowerCase();
        found = haystack.includes(value) || haystack.includes("arxiv:" + value) || haystack.includes("/abs/" + value);
      } else if (identifier.kind === "isbn") {
        found = String(item.getField("ISBN") || "").toLowerCase().includes(value);
      } else {
        var text = [item.getField("extra"), item.getField("url")].filter(Boolean).join("\n").toLowerCase();
        found = text.includes(value);
      }
      if (found) matches.push(this.importResultItem(item, null, compact));
      if (matches.length >= 3) break;
    }
    return matches;
  },

  async importIdentifiers(params, dryRun, compact) {
    var raw = params.identifiers || params.ids || [];
    if (!Array.isArray(raw)) throw new Error("identifiers array required");
    var libraryID = this.libraryID(params);
    var collections = this.collectionIDsFromParams(params);
    var tags = params.tags || [];
    var saveAttachments = params.saveAttachments !== false;
    var allowDuplicates = params.allowDuplicates === true;
    var results = [];
    for (var spec of raw) {
      var identifier = this.identifierFromSpec(spec);
      var existing = allowDuplicates ? [] : await this.existingItemsForIdentifier(identifier, libraryID, compact);
      if (existing.length) {
        results.push({ input: identifier.input, kind: identifier.kind, value: identifier.value, status: "skipped_existing", existing });
        continue;
      }
      if (dryRun) {
        results.push({ input: identifier.input, kind: identifier.kind, value: identifier.value, status: "would_import" });
        continue;
      }
      var translate = new Zotero.Translate.Search();
      translate.setIdentifier(identifier.zotero);
      var translators = await translate.getTranslators();
      if (!translators.length) {
        if (identifier.kind === "arxiv") {
          try {
            var fallback = await this.importArxivFallback(identifier, libraryID, collections, tags, saveAttachments, compact);
            results.push({
              input: identifier.input,
              kind: identifier.kind,
              value: identifier.value,
              status: "imported",
              source: fallback.source,
              items: [this.importResultItem(fallback.item, null, compact)],
              attachments: fallback.attachments,
            });
          } catch (error) {
            results.push({ input: identifier.input, kind: identifier.kind, value: identifier.value, status: "error", error: error && error.message ? error.message : String(error) });
          }
        } else {
          results.push({ input: identifier.input, kind: identifier.kind, value: identifier.value, status: "no_translator" });
        }
        continue;
      }
      translate.setTranslator(translators);
      var items = [];
      var source = "zotero_translator";
      try {
        items = await translate.translate({ libraryID, collections, saveAttachments });
      } catch (error) {
        if (identifier.kind === "arxiv") {
          try {
            var fallbackImport = await this.importArxivFallback(identifier, libraryID, collections, tags, saveAttachments, compact);
            items = [fallbackImport.item];
            source = fallbackImport.source;
            results.push({
              input: identifier.input,
              kind: identifier.kind,
              value: identifier.value,
              status: "imported",
              source,
              translator_error: error && error.message ? error.message : String(error),
              items: items.map((item) => this.importResultItem(item, null, compact)),
              attachments: fallbackImport.attachments,
            });
            continue;
          } catch (fallbackError) {
            results.push({
              input: identifier.input,
              kind: identifier.kind,
              value: identifier.value,
              status: "error",
              error: error && error.message ? error.message : String(error),
              fallback_error: fallbackError && fallbackError.message ? fallbackError.message : String(fallbackError),
            });
            continue;
          }
        }
        results.push({ input: identifier.input, kind: identifier.kind, value: identifier.value, status: "error", error: error && error.message ? error.message : String(error) });
        continue;
      }
      for (var item of items) {
        await this.addTagsToItem(item, tags);
      }
      results.push({
        input: identifier.input,
        kind: identifier.kind,
        value: identifier.value,
        status: items.length ? "imported" : "no_items_returned",
        source,
        items: items.map((item) => this.importResultItem(item, null, compact)),
      });
    }
    return { ok: true, op: "import_identifiers", dry_run: dryRun, compact, count: results.length, items: results };
  },

  async importPdfs(params, dryRun, compact) {
    var sources = params.sources || params.files || [];
    if (!Array.isArray(sources)) throw new Error("sources array required");
    var libraryID = this.libraryID(params);
    var collections = this.collectionIDsFromParams(params);
    var tags = params.tags || [];
    var recognize = params.recognize !== false;
    var results = [];
    for (var source of sources) {
      var input = String((source && source.path) || (source && source.url) || source || "");
      if (!input) continue;
      if (/^https?:\/\//i.test(input)) {
        if (dryRun) {
          results.push({ input, kind: "pdf_url", status: "would_import", recognize });
        } else {
          results.push(await this.importPdfUrl(input, libraryID, collections, tags, recognize, compact));
        }
        continue;
      }
      var exists = await IOUtils.exists(input);
      if (dryRun || !exists) {
        results.push({ input, kind: "local_pdf", status: exists ? "would_import" : "not_found", recognize });
        continue;
      }
      var attachment = await Zotero.Attachments.importFromFile({ file: input, libraryID, collections });
      results.push(await this.finishImportedAttachment(input, "local_pdf", attachment, tags, recognize, compact));
    }
    return { ok: true, op: "import_pdfs", dry_run: dryRun, compact, count: results.length, items: results };
  },

  async importUrls(params, dryRun, compact) {
    var urls = params.urls || [];
    if (!Array.isArray(urls)) throw new Error("urls array required");
    var libraryID = this.libraryID(params);
    var collections = this.collectionIDsFromParams(params);
    var tags = params.tags || [];
    var results = [];
    for (var url of urls) {
      url = String(url || "").trim();
      if (!url) continue;
      try {
        var identifier = this.identifierFromSpec(url);
        if (dryRun) {
          results.push({ input: url, kind: identifier.kind, value: identifier.value, status: "would_import_identifier" });
        } else {
          var imported = await this.importIdentifiers({ identifiers: [{ input: url, kind: identifier.kind, value: identifier.value }], tags, collections, saveAttachments: true }, false, compact);
          results.push(Object.assign({ input: url, via: "identifier" }, imported.items[0] || {}));
        }
        continue;
      } catch (_) {}
      if (this.isProbablyPdfUrl(url)) {
        if (dryRun) results.push({ input: url, kind: "pdf_url", status: "would_import" });
        else results.push(await this.importPdfUrl(url, libraryID, collections, tags, true, compact));
        continue;
      }
      if (dryRun) {
        results.push({ input: url, kind: "web_url", status: "would_translate_or_create_webpage" });
        continue;
      }
      results.push(await this.importWebUrl(url, libraryID, collections, tags, compact));
    }
    return { ok: true, op: "import_urls", dry_run: dryRun, compact, count: results.length, items: results };
  },

  isProbablyPdfUrl(url) {
    return /\.pdf(?:$|[?#])/i.test(url) || /\/pdf\//i.test(url);
  },

  async importPdfUrl(url, libraryID, collections, tags, recognize, compact) {
    var attachment = await Zotero.Attachments.importFromURL({ url, libraryID, collections });
    return this.finishImportedAttachment(url, "pdf_url", attachment, tags, recognize, compact);
  },

  async finishImportedAttachment(input, kind, attachment, tags, recognize, compact) {
    var recognition = recognize ? "not_supported" : "not_requested";
    var parent = null;
    if (recognize && Zotero.RecognizeDocument && Zotero.RecognizeDocument.canRecognize(attachment)) {
      recognition = "attempted";
      await Zotero.RecognizeDocument.recognizeItems([attachment]);
      attachment = await Zotero.Items.getAsync(attachment.id);
      var parentID = attachment.parentItemID || attachment.parentID || null;
      if (parentID) {
        parent = await Zotero.Items.getAsync(parentID);
        recognition = "recognized";
      } else {
        recognition = "no_match";
      }
    }
    await this.addTagsToItem(parent || attachment, tags);
    return {
      input,
      kind,
      status: "imported",
      recognition,
      item: parent ? this.importResultItem(parent, { role: "parent" }, compact) : null,
      attachment: this.importResultItem(attachment, { role: "attachment" }, compact),
    };
  },

  async importWebUrl(url, libraryID, collections, tags, compact) {
    var hiddenBrowser = null;
    try {
      var { HiddenBrowser } = ChromeUtils.importESModule("chrome://zotero/content/HiddenBrowser.mjs");
      hiddenBrowser = new HiddenBrowser({ docShell: { allowImages: true } });
      await hiddenBrowser.load(url, { requireSuccessfulStatus: true });
      var doc = await hiddenBrowser.getDocument();
      var translate = new Zotero.Translate.Web();
      translate.setDocument(doc);
      translate.setHandler("select", function (translate, items, callback) {
        var selected = {};
        for (var id in items) selected[id] = items[id];
        callback(selected);
      });
      var translators = await translate.getTranslators();
      if (translators.length) {
        translate.setTranslator(translators);
        var items = await translate.translate({ libraryID, collections, saveAttachments: true });
        for (var item of items) {
          await this.addTagsToItem(item, tags);
        }
        return { input: url, kind: "web_url", status: "imported", translator_count: translators.length, items: items.map((item) => this.importResultItem(item, null, compact)) };
      }
      var item = new Zotero.Item("webpage");
      item.libraryID = libraryID;
      item.setField("title", doc.title || url);
      item.setField("url", url);
      item.setField("accessDate", "CURRENT_TIMESTAMP");
      if (collections.length) item.setCollections(collections);
      await item.saveTx();
      await this.addTagsToItem(item, tags);
      return { input: url, kind: "web_url", status: "created_webpage", items: [this.importResultItem(item, null, compact)] };
    } finally {
      if (hiddenBrowser) hiddenBrowser.destroy();
    }
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
