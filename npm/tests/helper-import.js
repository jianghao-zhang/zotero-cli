"use strict";

const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");

const source = fs.readFileSync(path.join(__dirname, "../../helper/zcli-helper-zotero/bootstrap.js"), "utf8");
function setup() {
  const requests = [];
  const saved = [];
  const attachments = [];
  let searchCalls = 0;
  const Zotero = {
    Libraries: { userLibraryID: 1 },
    Items: { getAll: async () => [] },
    ItemTypes: { getID: () => 1 },
    Item: class {
      constructor(type) { this.itemType = type; this.fields = {}; }
      setField(name, value) { this.fields[name] = value; }
      setCreators(value) { this.creators = value; }
      setCollections(value) { this.collections = value; }
      async saveTx() { this.id = saved.length + 1; saved.push(this); }
    },
    Utilities: { cleanAuthor: name => ({ name, fieldMode: 1 }) },
    HTTP: { request: async (...args) => { requests.push(args); throw Object.assign(new Error("Rate exceeded"), { status: 429 }); } },
    Attachments: { importFromURL: async options => { attachments.push(options); return { id: 2 }; } },
    Translate: { Search: class {
      constructor() { searchCalls++; }
      setIdentifier(id) { this.identifier = id; }
      async getTranslators() { return [{}]; }
      setTranslator() {}
      async translate() { return [{ id: 3 }]; }
    } },
  };
  const context = vm.createContext({ Zotero, TextDecoder, TextEncoder });
  vm.runInContext(source, context);
  const helper = context.ZcliHelper;
  helper.addTagsToItem = async () => {};
  helper.importResultItem = item => ({ id: item.id, fields: item.fields });
  return { helper, Zotero, requests, saved, attachments, searchCalls: () => searchCalls };
}

(async () => {
  const params = { identifiers: ["2601.16175"] };
  let env = setup();
  let result = await env.helper.importIdentifiers(params, true, true);
  assert.equal(result.items[0].status, "would_import");
  assert.equal(env.requests.length, 0);
  assert.equal(env.saved.length, 0);

  env = setup();
  result = await env.helper.importIdentifiers(params, false, true);
  assert.equal(result.ok, false);
  assert.match(result.error, /HTTP 429/);
  assert.match(result.error, /No item was created/);
  assert.equal(env.requests.length, 1);
  assert.equal(env.requests[0][1], "https://arxiv.org/abs/2601.16175");
  const options = env.requests[0][2];
  assert.equal(options.responseType, "document");
  assert.equal(options.errorDelayMax, 0);
  assert.equal(options.noRetryOnThrottle, true);
  assert.equal(options.timeout, 30000);
  assert.equal(env.searchCalls(), 0);
  assert.equal(env.saved.length, 0);

  env = setup();
  result = await env.helper.importUrls({ urls: ["https://arxiv.org/abs/2601.16175"] }, false, true);
  assert.equal(result.ok, false);
  assert.match(result.error, /HTTP 429/);
  assert.equal(env.requests.length, 1);

  env = setup();
  env.helper.existingItemsForIdentifier = async () => [{ key: "EXISTING" }];
  result = await env.helper.importIdentifiers(params, false, true);
  assert.equal(result.items[0].status, "skipped_existing");
  assert.equal(env.requests.length, 0);

  env = setup();
  env.helper.fetchArxivMetadata = async () => ({
    arxivID: "2601.16175v1", title: "Learning to Discover at Test Time", abstractNote: "Abstract",
    date: "2026-01-22", authors: ["Mert Yuksekgonul"], doi: "", categories: ["cs.LG"],
    url: "https://arxiv.org/abs/2601.16175v1", pdfURL: "https://arxiv.org/pdf/2601.16175v1",
  });
  result = await env.helper.importIdentifiers(params, false, true);
  assert.equal(result.ok, true);
  assert.equal(result.items[0].source, "arxiv_page");
  assert.equal(env.saved.length, 1);
  assert.equal(env.saved[0].itemType, "preprint");
  assert.equal(env.saved[0].fields.title, "Learning to Discover at Test Time");
  assert.equal(env.saved[0].creators.length, 1);
  assert.equal(env.attachments.length, 1);
  assert.equal(env.attachments[0].parentItemID, env.saved[0].id);

  env = setup();
  result = await env.helper.importIdentifiers({ identifiers: [{ kind: "doi", value: "10.1000/example" }] }, false, true);
  assert.equal(result.ok, true);
  assert.equal(result.items[0].source, "zotero_translator");
  assert.equal(env.searchCalls(), 1);
  assert.equal(env.requests.length, 0);
  // Synthetic citation tags exercise identity validation and version handling
  // without including a user's library, cached HTML, or local file paths.
  const document = (entries, subjects = "Computer Science (cs.LG)") => ({
    querySelectorAll: () => entries.map(([name, content]) => ({ getAttribute: key => key === "name" ? name : content })),
    querySelector: () => ({ textContent: subjects }),
  });
  const tags = [
    ["citation_arxiv_id", "2601.16175"],
    ["citation_title", "An Example & Its Results"],
    ["citation_author", "Example, Alice"],
    ["citation_author", "Example, Bob"],
    ["citation_date", "2026/01/22"],
    ["citation_abstract", "An example abstract."],
  ];
  env = setup();
  const meta = env.helper.parseArxivMetadata(document(tags), "2601.16175v1");
  assert.equal(meta.title, "An Example & Its Results");
  assert.equal(meta.authors.length, 2);
  assert.equal(meta.date, "2026-01-22");
  assert.equal(meta.pdfURL, "https://arxiv.org/pdf/2601.16175v1");
  assert.equal(meta.categories[0], "cs.LG");
  assert.throws(() => env.helper.parseArxivMetadata(document([]), "2601.16175"), /missing valid citation/);
  assert.throws(() => env.helper.parseArxivMetadata(document(tags), "2601.00001"), /missing valid citation/);
  const legacy = tags.map(([name, value]) => [name, name === "citation_arxiv_id" ? "hep-th/9901001" : value]);
  assert.equal(env.helper.parseArxivMetadata(document(legacy), "hep-th/9901001v2").pdfURL, "https://arxiv.org/pdf/hep-th/9901001v2");
  env.Zotero.HTTP.request = async () => ({ response: document(tags) });
  result = await env.helper.importIdentifiers(params, false, true);
  assert.equal(result.ok, true);
  assert.equal(env.saved[0].fields.abstractNote, "An example abstract.");
  assert.equal(env.saved[0].fields.archiveID, "2601.16175");
  console.log("helper import tests ok");
})().catch(error => { console.error(error); process.exitCode = 1; });
