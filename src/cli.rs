use std::{
    env, fs,
    io::{self, Read},
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
};

use anyhow::{anyhow, Result};
use chrono::{Duration, Local};
use clap::{Args, Parser, Subcommand, ValueEnum};
use serde_json::{json, Value};

use crate::{
    alphaxiv,
    config::{Config, WebApiConfig},
    date_range::DateRange,
    helper::{
        self, HelperInstallOptions, HelperPackageOptions, API_KEY_URL, API_LIBRARY_ID_HELP_URL,
    },
    import_plan::{self, PlanOptions},
    inbox::{self, InboxSource},
    index, lfz, local_api,
    mirror::{self, MirrorMode, MirrorOptions},
    mutation::{self, MutationPlan},
    output::OutputFormat,
    reading::{self, MaterializeMode, ReadOptions},
    skill::{self, SkillInstallOptions, SkillTarget},
    zotero::ZoteroDb,
};

#[derive(Debug, Parser)]
#[command(
    name = "zcli",
    version,
    about = "Fast local Zotero CLI for humans and agents"
)]
pub struct Cli {
    #[arg(long, global = true, value_enum, default_value = "auto")]
    pub format: OutputFormat,
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,
    #[arg(long, global = true, help = "Path to zotero.sqlite")]
    pub db: Option<PathBuf>,
    #[arg(long, global = true, help = "Path to Zotero storage directory")]
    pub storage: Option<PathBuf>,
    #[arg(
        long,
        global = true,
        hide = true,
        help = "Root directory for zcli mirror output"
    )]
    pub mirror_root: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    Doctor,
    #[command(hide = true)]
    Examples,
    Resolve(ResolveArgs),
    Find {
        #[command(subcommand)]
        command: FindCommands,
    },
    Paper(PaperArgs),
    Read(ReadArgs),
    Context(ContextPackArgs),
    #[command(hide = true)]
    Open(OpenArgs),
    #[command(hide = true)]
    Reveal(OpenArgs),
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
    #[command(hide = true)]
    Search {
        #[command(subcommand)]
        command: SearchCommands,
    },
    Index {
        #[command(subcommand)]
        command: IndexCommands,
    },
    Item {
        #[command(subcommand)]
        command: ItemCommands,
    },
    #[command(hide = true)]
    Markdown {
        #[command(subcommand)]
        command: MarkdownCommands,
    },
    Collection {
        #[command(subcommand)]
        command: CollectionCommands,
    },
    Tags {
        #[command(subcommand)]
        command: TagsCommands,
    },
    Write {
        #[command(subcommand)]
        command: WriteCommands,
    },
    Import {
        #[command(subcommand)]
        command: ImportCommands,
    },
    Recent(RecentArgs),
    #[command(hide = true)]
    Mirror {
        #[command(subcommand)]
        command: MirrorCommands,
    },
    Setup(SetupArgs),
    Recap {
        #[command(subcommand)]
        command: RecapCommands,
    },
    Lfz {
        #[command(subcommand)]
        command: LfzCommands,
    },
    Inbox {
        #[command(subcommand)]
        command: InboxCommands,
    },
    #[command(hide = true)]
    Queue {
        #[command(subcommand)]
        command: QueueCommands,
    },
    #[command(hide = true)]
    Todo {
        #[command(subcommand)]
        command: QueueCommands,
    },
    #[command(hide = true)]
    Export {
        #[command(subcommand)]
        command: ExportCommands,
    },
    Skill {
        #[command(subcommand)]
        command: SkillCommands,
    },
    Helper {
        #[command(subcommand)]
        command: HelperCommands,
    },
    LocalApi {
        #[command(subcommand)]
        command: LocalApiCommands,
    },
    WebApi {
        #[command(subcommand)]
        command: WebApiCommands,
    },
    #[command(hide = true)]
    Alphaxiv {
        #[command(subcommand)]
        command: alphaxiv::AlphaXivCommands,
    },
}

#[derive(Debug, Args)]
pub struct ResolveArgs {
    pub query: String,
    #[arg(long, default_value_t = 10)]
    pub limit: usize,
}

#[derive(Debug, Subcommand)]
pub enum FindCommands {
    Paper {
        query: String,
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
    #[command(about = "Find likely duplicate Zotero items by DOI or normalized title")]
    Duplicates {
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
}

#[derive(Debug, Args)]
pub struct PaperArgs {
    pub key: String,
    #[arg(long, default_value = "40k")]
    pub budget: String,
}

#[derive(Debug, Args)]
pub struct ReadArgs {
    #[arg(help = "Zotero key, citation key, DOI, arXiv ID, title, or PDF filename")]
    pub query: String,
    #[arg(
        long,
        default_value = "outputs/zotero-reader",
        help = "Directory for the Codex App preview PDF"
    )]
    pub output: PathBuf,
    #[arg(
        long,
        help = "Copy instead of hardlinking; use explicitly for a different filesystem volume"
    )]
    pub copy: bool,
}

#[derive(Debug, Args)]
pub struct ContextPackArgs {
    pub key: String,
    #[arg(long, default_value = "40k")]
    pub budget: String,
    #[arg(
        long,
        help = "Request llm-for-zotero context when it is enabled in config"
    )]
    pub include_lfz: bool,
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommands {
    Init {
        #[arg(long)]
        force: bool,
    },
    Status,
    WebApi(WebApiArgs),
}

#[derive(Debug, Args)]
pub struct WebApiArgs {
    #[arg(long)]
    pub enable: bool,
    #[arg(long)]
    pub disable: bool,
    #[arg(long, value_enum)]
    pub library_type: Option<WebApiLibraryType>,
    #[arg(
        long,
        help = "Numeric Zotero userID/groupID for Web API calls, not username or library name"
    )]
    pub library_id: Option<String>,
    #[arg(long)]
    pub base_url: Option<String>,
    #[arg(long, help = "Environment variable that contains the Zotero API key")]
    pub api_key_env: Option<String>,
    #[arg(
        long,
        help = "Read an API key from stdin and store it in the config file"
    )]
    pub api_key_stdin: bool,
    #[arg(long, help = "Clear any API key stored directly in the config file")]
    pub clear_stored_key: bool,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum WebApiLibraryType {
    User,
    Group,
}

#[derive(Debug, Subcommand)]
pub enum SearchCommands {
    List {
        query: Option<String>,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    Grep {
        pattern: String,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    Context {
        item_key: String,
        pattern: String,
        #[arg(long, default_value_t = 600)]
        context_chars: usize,
    },
}

#[derive(Debug, Subcommand)]
pub enum IndexCommands {
    Status,
    Update(IndexUpdateArgs),
    Rebuild(IndexUpdateArgs),
    Search(IndexSearchArgs),
    Chunks(IndexChunkSearchArgs),
    Chunk(IndexChunkGetArgs),
    Get(IndexGetArgs),
}

#[derive(Debug, Args)]
pub struct IndexUpdateArgs {
    #[arg(
        long,
        help = "Include extracted attachment/full-text body in the local index"
    )]
    pub include_full_text: bool,
    #[arg(long, default_value_t = 80_000)]
    pub max_chars: usize,
}

#[derive(Debug, Args)]
pub struct IndexSearchArgs {
    pub query: String,
    #[arg(long, default_value_t = 10)]
    pub limit: usize,
    #[arg(long, default_value_t = 420)]
    pub snippet_chars: usize,
}

#[derive(Debug, Args)]
pub struct IndexChunkSearchArgs {
    pub query: String,
    #[arg(
        long,
        help = "Limit chunk search to one Zotero key, citation key, or short title"
    )]
    pub item: Option<String>,
    #[arg(
        long,
        help = "Limit chunk search to collections whose name contains this text"
    )]
    pub collection: Option<String>,
    #[arg(
        long,
        help = "Limit chunk search to tags whose name contains this text"
    )]
    pub tag: Option<String>,
    #[arg(long, default_value_t = 10)]
    pub limit: usize,
    #[arg(long, default_value_t = 700)]
    pub snippet_chars: usize,
}

#[derive(Debug, Args)]
pub struct IndexChunkGetArgs {
    pub chunk_id: String,
    #[arg(long, default_value_t = 4000)]
    pub max_chars: usize,
}

#[derive(Debug, Args)]
pub struct IndexGetArgs {
    pub key: String,
    #[arg(long, default_value_t = 4000)]
    pub max_chars: usize,
}

#[derive(Debug, Subcommand)]
pub enum ItemCommands {
    Get {
        key: String,
    },
    Extract {
        key: String,
        #[arg(long, default_value_t = 120_000)]
        max_chars: usize,
    },
    Markdown(ItemMarkdownArgs),
    Annotations {
        key: String,
    },
    Notes {
        key: String,
    },
    Attachments {
        key: String,
    },
    Bibtex {
        key: String,
    },
    #[command(about = "Format exact citations with Zotero's CSL engine")]
    Cite(ItemCiteArgs),
    #[command(about = "Export exact item records with Zotero translators")]
    Export(ItemExportArgs),
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum CitationMode {
    Bibliography,
    Citation,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum CitationOutputFormat {
    Text,
    Html,
}

#[derive(Debug, Args)]
pub struct ItemCiteArgs {
    #[arg(required = true, help = "One or more Zotero item keys")]
    pub keys: Vec<String>,
    #[arg(long, value_enum, default_value = "bibliography")]
    pub mode: CitationMode,
    #[arg(long, help = "CSL style name or URL, for example apa or ieee")]
    pub style: Option<String>,
    #[arg(long, default_value = "en-US")]
    pub locale: String,
    #[arg(long, value_enum, default_value = "text")]
    pub output: CitationOutputFormat,
}

#[derive(Debug, Args)]
pub struct ItemExportArgs {
    #[arg(required = true, help = "One or more Zotero item keys")]
    pub keys: Vec<String>,
    #[arg(
        long,
        default_value = "bibtex",
        help = "Zotero export format, e.g. bibtex, biblatex, ris, csljson, csv"
    )]
    pub translator: String,
}

#[derive(Debug, Subcommand)]
pub enum MarkdownCommands {
    Status { key: String },
}

#[derive(Debug, Subcommand)]
pub enum CollectionCommands {
    List,
    Items {
        key: String,
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
}

#[derive(Debug, Subcommand)]
pub enum TagsCommands {
    List,
    Items {
        tag: String,
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
}

#[derive(Debug, Subcommand)]
pub enum WriteCommands {
    Tags(WriteTagsArgs),
    Collection(WriteCollectionArgs),
    Note(WriteNoteArgs),
    Attach(WriteAttachArgs),
    RenameAttachment(WriteRenameAttachmentArgs),
    ImportFiles(WriteImportFilesArgs),
    Trash(WriteTrashArgs),
    Metadata(WriteMetadataArgs),
}

#[derive(Debug, Subcommand)]
pub enum ImportCommands {
    Arxiv(ImportIdentifiersArgs),
    Ids(ImportIdentifiersArgs),
    Pdf(ImportPdfArgs),
    Url(ImportUrlArgs),
}

#[derive(Debug, Args)]
pub struct ImportIdentifiersArgs {
    pub identifiers: Vec<String>,
    #[arg(long = "collection", help = "Collection key, id, or exact name")]
    pub collections: Vec<String>,
    #[arg(long = "tag")]
    pub tags: Vec<String>,
    #[arg(
        long,
        help = "Allow Zotero to create duplicates instead of skipping exact local matches"
    )]
    pub allow_duplicates: bool,
    #[arg(long)]
    pub dry_run: bool,
    #[arg(long)]
    pub execute: bool,
}

#[derive(Debug, Args)]
pub struct ImportPdfArgs {
    pub sources: Vec<String>,
    #[arg(long = "collection", help = "Collection key, id, or exact name")]
    pub collections: Vec<String>,
    #[arg(long = "tag")]
    pub tags: Vec<String>,
    #[arg(
        long,
        help = "Import the PDF as a standalone attachment without metadata recognition"
    )]
    pub no_recognize: bool,
    #[arg(
        long,
        help = "Allow Zotero to create duplicates instead of skipping exact local matches"
    )]
    pub allow_duplicates: bool,
    #[arg(long)]
    pub dry_run: bool,
    #[arg(long)]
    pub execute: bool,
}

#[derive(Debug, Args)]
pub struct ImportUrlArgs {
    pub urls: Vec<String>,
    #[arg(long = "collection", help = "Collection key, id, or exact name")]
    pub collections: Vec<String>,
    #[arg(long = "tag")]
    pub tags: Vec<String>,
    #[arg(
        long,
        help = "Allow Zotero to create duplicates instead of skipping exact local matches"
    )]
    pub allow_duplicates: bool,
    #[arg(long)]
    pub dry_run: bool,
    #[arg(long)]
    pub execute: bool,
}

#[derive(Debug, Args)]
pub struct WriteTagsArgs {
    pub key: String,
    #[arg(long = "add")]
    pub add_tags: Vec<String>,
    #[arg(long = "remove")]
    pub remove_tags: Vec<String>,
    #[arg(long)]
    pub dry_run: bool,
    #[arg(long)]
    pub execute: bool,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum CollectionWriteAction {
    Add,
    Remove,
}

#[derive(Debug, Args)]
pub struct WriteCollectionArgs {
    pub key: String,
    #[arg(long)]
    pub collection: String,
    #[arg(long, value_enum, default_value = "add")]
    pub action: CollectionWriteAction,
    #[arg(long)]
    pub dry_run: bool,
    #[arg(long)]
    pub execute: bool,
}

#[derive(Debug, Args)]
pub struct WriteNoteArgs {
    pub key: String,
    #[arg(long)]
    pub title: Option<String>,
    #[arg(long)]
    pub content: Option<String>,
    #[arg(long, help = "Read note content from this local file")]
    pub file: Option<PathBuf>,
    #[arg(long)]
    pub dry_run: bool,
    #[arg(long)]
    pub execute: bool,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum AttachmentWriteMode {
    Link,
    Import,
}

#[derive(Debug, Args)]
pub struct WriteAttachArgs {
    pub key: String,
    pub file: PathBuf,
    #[arg(long, value_enum, default_value = "link")]
    pub mode: AttachmentWriteMode,
    #[arg(long)]
    pub title: Option<String>,
    #[arg(long)]
    pub dry_run: bool,
    #[arg(long)]
    pub execute: bool,
}

#[derive(Debug, Args)]
pub struct WriteRenameAttachmentArgs {
    pub key: String,
    #[arg(long)]
    pub name: String,
    #[arg(long)]
    pub dry_run: bool,
    #[arg(long)]
    pub execute: bool,
}

#[derive(Debug, Args)]
pub struct WriteImportFilesArgs {
    pub files: Vec<PathBuf>,
    #[arg(long)]
    pub dry_run: bool,
    #[arg(long)]
    pub execute: bool,
}

#[derive(Debug, Args)]
pub struct WriteTrashArgs {
    pub key: String,
    #[arg(long)]
    pub dry_run: bool,
    #[arg(long)]
    pub execute: bool,
}

#[derive(Debug, Args)]
pub struct WriteMetadataArgs {
    pub key: String,
    #[arg(
        long = "set",
        value_name = "FIELD=VALUE",
        help = "Set a scalar Zotero metadata field; repeat for multiple fields"
    )]
    pub set_fields: Vec<String>,
    #[arg(
        long = "clear",
        value_name = "FIELD",
        help = "Clear a scalar Zotero metadata field; repeat for multiple fields"
    )]
    pub clear_fields: Vec<String>,
    #[arg(long)]
    pub dry_run: bool,
    #[arg(long)]
    pub execute: bool,
}

#[derive(Debug, Args)]
pub struct RecentArgs {
    #[arg(long, default_value_t = 7)]
    pub days: i64,
    #[arg(long, default_value_t = 30)]
    pub limit: usize,
}

#[derive(Debug, Subcommand)]
pub enum MirrorCommands {
    Status,
    Rebuild(MirrorCommandArgs),
    Sync(MirrorCommandArgs),
    Watch(MirrorWatchArgs),
    DaemonInstall(MirrorDaemonInstallArgs),
}

#[derive(Debug, Args)]
pub struct MirrorDaemonInstallArgs {
    #[arg(long)]
    pub dry_run: bool,
    #[arg(long)]
    pub execute: bool,
    #[arg(long, default_value_t = 300)]
    pub interval: u64,
    #[arg(long)]
    pub write_markdown: bool,
}

#[derive(Debug, Args)]
pub struct MirrorCommandArgs {
    #[arg(long)]
    pub dry_run: bool,
    #[arg(long, value_enum, default_value = "symlink")]
    pub mode: MirrorMode,
    #[arg(long, default_value_t = 10_000)]
    pub limit: usize,
    #[arg(
        long,
        help = "Keep stale mirror entries instead of removing dirs from the prior zcli index"
    )]
    pub no_cleanup: bool,
    #[arg(long, help = "Write a paper.md file in each mirrored item directory")]
    pub write_markdown: bool,
    #[arg(long, default_value_t = 240_000)]
    pub markdown_max_chars: usize,
}

#[derive(Debug, Args)]
pub struct MirrorWatchArgs {
    #[arg(long, default_value_t = 60, help = "Polling interval in seconds")]
    pub interval: u64,
    #[arg(
        long,
        default_value_t = 5000,
        help = "Debounce delay after a detected change, in milliseconds"
    )]
    pub settle_ms: u64,
    #[arg(
        long,
        help = "Also include Zotero storage directory metadata in the watch signature"
    )]
    pub include_storage: bool,
    #[arg(long, help = "Run one sync immediately, then exit")]
    pub once: bool,
    #[arg(long, help = "Run a sync once when the watcher starts")]
    pub run_on_start: bool,
    #[arg(long)]
    pub dry_run: bool,
    #[arg(long, value_enum, default_value = "symlink")]
    pub mode: MirrorMode,
    #[arg(long, default_value_t = 10_000)]
    pub limit: usize,
    #[arg(
        long,
        help = "Keep stale mirror entries instead of removing dirs from the prior zcli index"
    )]
    pub no_cleanup: bool,
    #[arg(long, help = "Write a paper.md file in each mirrored item directory")]
    pub write_markdown: bool,
    #[arg(long, default_value_t = 240_000)]
    pub markdown_max_chars: usize,
    #[arg(long, hide = true)]
    pub max_events: Option<usize>,
}

#[derive(Debug, Args)]
pub struct ItemMarkdownArgs {
    pub key: String,
    #[arg(long, default_value_t = 240_000)]
    pub max_chars: usize,
    #[arg(long, help = "Write markdown to this path instead of returning it")]
    pub output: Option<PathBuf>,
    #[arg(long, help = "Do not use llm-for-zotero full.md even when configured")]
    pub no_lfz_full_md: bool,
}

#[derive(Debug, Args)]
pub struct SetupArgs {
    #[arg(
        long,
        help = "Preview the config and optional skill operations without writing"
    )]
    pub dry_run: bool,
    #[arg(long, help = "Overwrite an existing config file")]
    pub force: bool,
    #[arg(long, help = "Use detected/default values without interactive prompts")]
    pub defaults: bool,
    #[arg(long, help = "Do not prompt for optional agent skill installation")]
    pub no_skills: bool,
}

#[derive(Debug, Subcommand)]
pub enum RecapCommands {
    Reading(RecapReadingArgs),
    Today(RecapPresetArgs),
    Week(RecapPresetArgs),
    Lfz(RecapRangeArgs),
}

#[derive(Debug, Args)]
pub struct RecapPresetArgs {
    #[arg(long)]
    pub no_lfz: bool,
    #[arg(long)]
    pub why: bool,
}

#[derive(Debug, Args)]
pub struct RecapReadingArgs {
    #[arg(long)]
    pub from: Option<String>,
    #[arg(long)]
    pub to: Option<String>,
    #[arg(
        long,
        help = "Request optional llm-for-zotero recap when it is enabled in config"
    )]
    pub include_lfz: bool,
    #[arg(
        long,
        help = "Skip llm-for-zotero recap even if it is enabled in config"
    )]
    pub no_lfz: bool,
    #[arg(long, help = "Explain reading provenance labels")]
    pub why: bool,
    #[arg(
        long,
        help = "Restrict optional llm-for-zotero recap to a Zotero item key"
    )]
    pub item: Option<String>,
}

#[derive(Debug, Args)]
pub struct RecapRangeArgs {
    #[arg(long)]
    pub from: Option<String>,
    #[arg(long)]
    pub to: Option<String>,
    #[arg(long, help = "Restrict recap to a Zotero item key")]
    pub item: Option<String>,
    #[arg(
        long,
        default_value_t = 20,
        help = "Maximum questions/finals in the compact recap"
    )]
    pub limit: usize,
    #[arg(
        long,
        help = "Return detailed message and run rows instead of compact recap"
    )]
    pub details: bool,
    #[arg(
        long,
        help = "Include full question/answer/final text without trace payloads"
    )]
    pub full_text: bool,
    #[arg(long, help = "Include selected text and paper-context JSON excerpts")]
    pub include_contexts: bool,
}

#[derive(Debug, Subcommand)]
pub enum LfzCommands {
    Doctor,
    Turn(LfzTurnArgs),
    Turns(LfzTurnsArgs),
}

#[derive(Debug, Args)]
pub struct LfzTurnArgs {
    pub message_ref: String,
    #[arg(long, help = "Approximate token budget such as 20k or 100k")]
    pub budget: Option<String>,
    #[arg(long, help = "Include selected text and paper-context JSON excerpts")]
    pub include_contexts: bool,
}

#[derive(Debug, Args)]
pub struct LfzTurnsArgs {
    #[arg(long)]
    pub item: String,
    #[arg(long, default_value_t = 50)]
    pub limit: usize,
}

#[derive(Debug, Subcommand)]
pub enum InboxCommands {
    Status,
    Fetch(InboxFetchArgs),
    Triage(InboxFetchArgs),
    Discussion(InboxDiscussionArgs),
    Sources {
        #[command(subcommand)]
        command: InboxSourceCommands,
    },
}

#[derive(Debug, Subcommand)]
pub enum InboxSourceCommands {
    X {
        #[command(subcommand)]
        command: XSourceCommands,
    },
}

#[derive(Debug, Subcommand)]
pub enum XSourceCommands {
    List,
    Add(XHandleArgs),
    Remove(XHandleArgs),
}

#[derive(Debug, Args)]
pub struct XHandleArgs {
    pub handle: String,
}

#[derive(Debug, Subcommand)]
pub enum QueueCommands {
    Add(QueueAddArgs),
    List,
    Done(QueueDoneArgs),
}

#[derive(Debug, Args)]
pub struct QueueAddArgs {
    pub key: String,
    #[arg(long)]
    pub note: Option<String>,
}

#[derive(Debug, Args)]
pub struct QueueDoneArgs {
    pub key: String,
}

#[derive(Debug, Subcommand)]
pub enum ExportCommands {
    Pack(ExportPackArgs),
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum AgentPackTarget {
    Codex,
    Claude,
    Hermes,
    Openclaw,
}

#[derive(Debug, Args)]
pub struct ExportPackArgs {
    pub key: String,
    #[arg(long = "for", value_enum, default_value = "codex")]
    pub for_agent: AgentPackTarget,
    #[arg(long)]
    pub output: Option<PathBuf>,
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Args)]
pub struct InboxFetchArgs {
    pub query: Option<String>,
    #[arg(long, value_enum, default_value = "alphaxiv")]
    pub source: InboxSource,
    #[arg(long, help = "X handle to read when --source x; repeatable")]
    pub handle: Vec<String>,
    #[arg(
        long,
        default_value_t = 40,
        help = "Posts per handle/search for --source x"
    )]
    pub tweet_limit: usize,
    #[arg(long, default_value_t = 30)]
    pub limit: usize,
    #[arg(long, value_enum, default_value = "hot", alias = "sort")]
    pub fallback_sort: alphaxiv::FeedSort,
    #[arg(long, default_value = "3 Days", alias = "interval")]
    pub fallback_interval: String,
    #[arg(long = "topic")]
    pub topics: Vec<String>,
    #[arg(long)]
    pub min_likes: Option<i64>,
    #[arg(long)]
    pub min_github_stars: Option<i64>,
    #[arg(long)]
    pub min_visits: Option<i64>,
    #[arg(long)]
    pub since: Option<String>,
    #[arg(long)]
    pub days: Option<i64>,
    #[arg(long, value_enum, default_value = "any")]
    pub date_field: alphaxiv::DateField,
    #[arg(long, default_value_t = 20)]
    pub timeout: u64,
    #[arg(long, help = "Skip local Zotero/queue context-aware ranking hints")]
    pub no_context: bool,
    #[arg(
        long,
        help = "Attach a quick GitHub repository overview when a candidate links code"
    )]
    pub code_overview: bool,
    #[arg(
        long,
        help = "Show candidates already recorded in the inbox seen state"
    )]
    pub show_seen: bool,
    #[arg(
        long,
        help = "Show candidates that appear to already exist in local Zotero"
    )]
    pub show_existing: bool,
    #[arg(long, default_value_t = 30)]
    pub seen_days: i64,
    #[arg(
        long,
        help = "Do not cache alphaXiv overview markdown for returned candidates"
    )]
    pub no_cache_overview: bool,
    #[arg(long, default_value_t = 7)]
    pub overview_ttl_days: i64,
    #[arg(long)]
    pub dry_run: bool,
    #[arg(long)]
    pub execute: bool,
}

#[derive(Debug, Args)]
pub struct InboxDiscussionArgs {
    #[arg(help = "Paper title, arXiv ID, alphaXiv/Hugging Face URL, or X post URL")]
    pub paper: String,
    #[arg(long, help = "Author or curator X handle to prioritize; repeatable")]
    pub handle: Vec<String>,
    #[arg(long, help = "Known X post URL or status id to expand directly")]
    pub tweet: Vec<String>,
    #[arg(long, default_value_t = 30)]
    pub days: i64,
    #[arg(long, default_value_t = 30, help = "Search results per query")]
    pub search_limit: usize,
    #[arg(long, default_value_t = 3, help = "Announcement posts to expand")]
    pub limit: usize,
    #[arg(long, default_value_t = 80, help = "Discussion replies/items to keep")]
    pub reply_limit: usize,
    #[arg(long, default_value_t = 3)]
    pub max_pages: usize,
    #[arg(
        long,
        help = "Skip reply/thread expansion and only return matched posts"
    )]
    pub no_expand: bool,
}

#[derive(Debug, Subcommand)]
pub enum SkillCommands {
    Doctor,
    Install(SkillInstallCommandArgs),
}

#[derive(Debug, Subcommand)]
pub enum HelperCommands {
    Doctor,
    Package(HelperPackageCommandArgs),
    Install(HelperInstallCommandArgs),
}

#[derive(Debug, Subcommand)]
pub enum LocalApiCommands {
    Doctor,
    Authorize(LocalApiAuthorizeArgs),
}

#[derive(Debug, Subcommand)]
pub enum WebApiCommands {
    Doctor,
}

#[derive(Debug, Args)]
pub struct LocalApiAuthorizeArgs {
    #[arg(long)]
    pub dry_run: bool,
    #[arg(long)]
    pub execute: bool,
}

#[derive(Debug, Args)]
pub struct OpenArgs {
    pub key: String,
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Args)]
pub struct SkillInstallCommandArgs {
    #[arg(long, value_enum)]
    pub target: SkillTarget,
    #[arg(long)]
    pub dry_run: bool,
    #[arg(long)]
    pub copy: bool,
}

#[derive(Debug, Args)]
pub struct HelperPackageCommandArgs {
    #[arg(long)]
    pub output: Option<PathBuf>,
    #[arg(long)]
    pub dry_run: bool,
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Args)]
pub struct HelperInstallCommandArgs {
    #[arg(long)]
    pub dry_run: bool,
    #[arg(long)]
    pub execute: bool,
    #[arg(long, help = "Explicit Zotero profile directory")]
    pub profile: Option<PathBuf>,
    #[arg(long)]
    pub force: bool,
}

pub struct Context {
    pub config_path: PathBuf,
    pub config: Config,
}

impl Cli {
    pub fn to_context(&self) -> Result<Context> {
        let config_path = self.config.clone().unwrap_or_else(Config::default_path);
        let mut config = Config::load(Some(&config_path))?;
        config.apply_overrides(
            self.db.clone(),
            self.storage.clone(),
            self.mirror_root.clone(),
        );
        Ok(Context {
            config_path,
            config,
        })
    }
}

pub fn dispatch(cli: &Cli, context: &Context) -> Result<Value> {
    match &cli.command {
        Commands::Doctor => doctor(context),
        Commands::Examples => examples(),
        Commands::Resolve(args) => dispatch_resolve(context, args),
        Commands::Find { command } => dispatch_find(context, command),
        Commands::Paper(args) => dispatch_paper(context, args),
        Commands::Read(args) => dispatch_read(context, args),
        Commands::Context(args) => dispatch_context_pack(context, args),
        Commands::Open(args) => dispatch_open_reveal(context, args, false),
        Commands::Reveal(args) => dispatch_open_reveal(context, args, true),
        Commands::Config { command } => dispatch_config(context, command),
        Commands::Search { command } => dispatch_search(context, command),
        Commands::Index { command } => dispatch_index(context, command),
        Commands::Item { command } => dispatch_item(context, command),
        Commands::Markdown { command } => dispatch_markdown(context, command),
        Commands::Collection { command } => dispatch_collection(context, command),
        Commands::Tags { command } => dispatch_tags(context, command),
        Commands::Write { command } => dispatch_write(context, command),
        Commands::Import { command } => dispatch_import(context, command),
        Commands::Recent(args) => {
            let db = ZoteroDb::open(&context.config)?;
            Ok(json!({
                "ok": true,
                "items": db.recent(args.days, args.limit)?,
            }))
        }
        Commands::Mirror { command } => dispatch_mirror(context, command),
        Commands::Setup(args) => crate::setup::run(context, args),
        Commands::Recap { command } => dispatch_recap(context, command),
        Commands::Lfz { command } => dispatch_lfz(context, command),
        Commands::Inbox { command } => dispatch_inbox(context, command),
        Commands::Queue { command } => dispatch_queue(context, command),
        Commands::Todo { command } => dispatch_queue(context, command),
        Commands::Export { command } => dispatch_export(context, command),
        Commands::Skill { command } => dispatch_skill(context, command),
        Commands::Helper { command } => dispatch_helper(context, command),
        Commands::LocalApi { command } => dispatch_local_api(context, command),
        Commands::WebApi { command } => dispatch_web_api(context, command),
        Commands::Alphaxiv { command } => alphaxiv::dispatch(command, &context.config),
    }
}

fn doctor(context: &Context) -> Result<Value> {
    let db_path = context.config.zotero_db_path.clone();
    let storage_path = context.config.zotero_storage_path.clone();
    let mirror_root = context.config.mirror_root.clone();
    let cache_dir = context.config.cache_dir.clone();
    let state_dir = context.config.state_dir.clone();
    let db_available = db_path
        .as_deref()
        .map(|path| path.exists())
        .unwrap_or(false);
    let storage_available = storage_path
        .as_deref()
        .map(|path| path.exists())
        .unwrap_or(false);
    let db = ZoteroDb::open(&context.config).ok();
    let current_exe = env::current_exe().ok();
    let path_zcli = find_on_path("zcli");
    let current_exe_canonical = current_exe.as_deref().and_then(canonicalize_ok);
    let path_zcli_canonical = path_zcli.as_deref().and_then(canonicalize_ok);
    Ok(json!({
        "ok": true,
        "mode": "local_first",
        "version": env!("CARGO_PKG_VERSION"),
        "runtime": {
            "current_exe": current_exe,
            "path_zcli": path_zcli,
            "path_zcli_matches_current_exe": current_exe_canonical.is_some() && current_exe_canonical == path_zcli_canonical,
        },
        "config_path": context.config_path,
        "paths": {
            "mirror_root": path_status(mirror_root.as_deref()),
            "cache_dir": path_status(cache_dir.as_deref()),
            "state_dir": path_status(state_dir.as_deref()),
        },
        "zotero": {
            "db_path": db_path,
            "db_available": db_available,
            "storage_path": storage_path,
            "storage_available": storage_available,
        },
        "web_api": web_api_status(&context.config.web_api),
        "local_api": local_api::doctor(&context.config)?,
        "risk": {
            "high_risk_auth_enabled": context.config.risk.high_risk_auth_enabled,
            "alphaxiv_auth_enabled": context.config.risk.alphaxiv_auth_enabled,
            "defaults_for_new_users": {
                "high_risk_auth_enabled": false,
                "alphaxiv_auth_enabled": false,
            }
        },
        "inbox": {
            "schema": "paper_candidate/v1",
            "discussion_schema": "paper_discussion/v1",
            "dry_run_first": true,
            "sources": ["alphaxiv", "huggingface", "x"],
            "x_handles": context.config.inbox.x_handles,
            "x_handle_count": context.config.inbox.x_handles.len(),
            "bird_cli": path_status(find_on_path("bird").as_deref()),
            "capabilities": [
                "multi_source_fetch",
                "triage",
                "time_semantics",
                "local_context_match",
                "zotero_import_plan",
                "quick_code_overview",
                "x_paper_discussion"
            ],
        },
        "helper": helper::doctor(&context.config)?,
        "lfz": lfz::doctor(&context.config, db.as_ref())?,
        "skills": skill::doctor(&context.config)?,
        "boundaries": {
            "mcp_server": false,
            "http_bridge": false,
            "optional_zotero_helper": true,
            "local_mutations_default": true,
            "network_required_for_core": false,
            "zotero_closed": {
                "reads": "local_sqlite",
                "local_writes": "unavailable",
                "remote_api": "explicit_web_api_only"
            }
        }
    }))
}

fn path_status(path: Option<&Path>) -> Value {
    json!({
        "path": path,
        "exists": path.map(|path| path.exists()).unwrap_or(false),
    })
}

fn canonicalize_ok(path: &Path) -> Option<PathBuf> {
    fs::canonicalize(path).ok()
}

fn find_on_path(bin: &str) -> Option<PathBuf> {
    let paths = env::var_os("PATH")?;
    for dir in env::split_paths(&paths) {
        let candidate = dir.join(bin);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn examples() -> Result<Value> {
    Ok(json!({
        "ok": true,
        "examples": [
            {"name": "find a paper", "command": "zcli resolve \"paper title, short title, citation key, DOI, arXiv, URL, or file path\""},
            {"name": "hybrid paper finder", "command": "zcli find paper \"agentic rl survey\" --format json"},
            {"name": "find likely duplicates", "command": "zcli find duplicates --limit 20 --format json"},
            {"name": "build local paper index", "command": "zcli index update --format json"},
            {"name": "search local paper index", "command": "zcli index search \"agent memory\" --format json"},
            {"name": "search paper passages", "command": "zcli index chunks \"agent memory\" --item ITEMKEY --format json"},
            {"name": "paper work surface", "command": "zcli paper ITEMKEY --format pretty"},
            {"name": "agent context pack", "command": "zcli context ITEMKEY --budget 40k --format json"},
            {"name": "raw markdown", "command": "zcli item markdown ITEMKEY --format text"},
            {"name": "exact APA bibliography", "command": "zcli item cite ITEMKEY --style apa --format json"},
            {"name": "exact BibTeX export", "command": "zcli item export ITEMKEY --translator bibtex --format json"},
            {"name": "markdown source status", "command": "zcli markdown status ITEMKEY --format pretty"},
            {"name": "today recap", "command": "zcli recap today --format pretty"},
            {"name": "llm-for-zotero turns for one paper", "command": "zcli lfz turns --item ITEMKEY --format json"},
            {"name": "preview arXiv import", "command": "zcli import arxiv 2604.06240 --dry-run"},
            {"name": "preview PDF import and metadata recognition", "command": "zcli import pdf ./paper.pdf --dry-run"},
            {"name": "preview metadata update", "command": "zcli write metadata ITEMKEY --set shortTitle=Short --dry-run"},
            {"name": "mirror with paper.md", "command": "zcli --mirror-root ~/ZoteroMirror mirror sync --write-markdown"},
            {"name": "agent skill check", "command": "zcli skill doctor --format pretty"},
            {"name": "Zotero helper plugin check", "command": "zcli helper doctor --format pretty"},
            {"name": "explicit Zotero Web API check", "command": "zcli web-api doctor --format json"},
            {"name": "export agent pack", "command": "zcli export pack ITEMKEY --for codex --output ./pack --dry-run"},
        ]
    }))
}

fn dispatch_resolve(context: &Context, args: &ResolveArgs) -> Result<Value> {
    let db = ZoteroDb::open(&context.config)?;
    let matches = db.resolve_items(&args.query, args.limit)?;
    Ok(json!({
        "ok": true,
        "query": args.query,
        "count": matches.len(),
        "matches": matches,
    }))
}

fn dispatch_find(context: &Context, command: &FindCommands) -> Result<Value> {
    let db = ZoteroDb::open(&context.config)?;
    match command {
        FindCommands::Paper { query, limit } => Ok(json!({
            "ok": true,
            "query": query,
            "mode": "local_hybrid_lexical",
            "note": "Local zero-network weighted search over Zotero metadata; embedding semantic search can be layered on later.",
            "hits": db.find_papers(query, *limit)?,
        })),
        FindCommands::Duplicates { limit } => {
            let result = db.duplicate_groups(*limit)?;
            Ok(json!({
                "ok": true,
                "mode": "local_duplicate_detection",
                "matching": ["exact_normalized_doi", "exact_normalized_title"],
                "total_groups": result.total_groups,
                "returned_groups": result.groups.len(),
                "groups": result.groups,
            }))
        }
    }
}

fn dispatch_paper(context: &Context, args: &PaperArgs) -> Result<Value> {
    let db = ZoteroDb::open(&context.config)?;
    let detail = db.get_item(&args.key)?;
    db.log_read(&context.config, "paper", &detail.summary);
    let markdown_status = db.markdown_status(&context.config, &args.key)?;
    let budget_tokens = parse_budget_tokens(&args.budget)?;
    Ok(json!({
        "ok": true,
        "kind": "paper",
        "item": detail.summary,
        "collections": detail.collections,
        "tags": detail.tags,
        "counts": {
            "attachments": detail.attachments.len(),
            "notes": detail.note_count,
            "annotations": detail.annotation_count,
        },
        "attachments": detail.attachments,
        "markdown_status": markdown_status,
        "commands": {
            "read": format!("zcli read {} --output outputs/zotero-reader", args.key),
            "context": format!("zcli context {} --budget {}", args.key, args.budget),
            "markdown": format!("zcli item markdown {} --format text", args.key),
            "annotations": format!("zcli item annotations {}", args.key),
            "notes": format!("zcli item notes {}", args.key),
        },
        "budget": budget_meta(budget_tokens),
    }))
}

fn dispatch_read(context: &Context, args: &ReadArgs) -> Result<Value> {
    reading::prepare(
        &context.config,
        &args.query,
        &ReadOptions {
            output_dir: args.output.clone(),
            mode: if args.copy {
                MaterializeMode::Copy
            } else {
                MaterializeMode::Hardlink
            },
        },
    )
}

fn dispatch_context_pack(context: &Context, args: &ContextPackArgs) -> Result<Value> {
    let db = ZoteroDb::open(&context.config)?;
    let detail = db.get_item(&args.key)?;
    let budget_tokens = parse_budget_tokens(&args.budget)?;
    let max_chars = budget_tokens.saturating_mul(4).max(1);
    let doc = db.markdown_for_item(&context.config, &args.key, max_chars, true)?;
    db.log_read(&context.config, "context", &detail.summary);
    let (markdown, markdown_meta) = clip_text(&doc.markdown, max_chars, || {
        format!("zcli item markdown {} --format text", args.key)
    });
    let mut value = json!({
        "ok": true,
        "kind": "context_pack",
        "item": detail.summary,
        "budget": budget_meta(budget_tokens),
        "markdown_meta": {
            "source": doc.source,
            "source_path": doc.source_path,
            "fallback_used": doc.fallback_used,
            "extracted_truncated": doc.extracted_truncated,
            "output": markdown_meta,
        },
        "markdown": markdown,
        "notes": db.notes_for_item(detail.summary.id)?,
        "annotations": db.annotations_for_item(detail.summary.id)?,
    });
    if context.config.lfz.enabled.unwrap_or(false) {
        let range = DateRange::parse(Some("1970-01-01"), Some("2100-01-01"))?;
        value["lfz"] = lfz::recap(
            &context.config,
            &db,
            &range,
            lfz::RecapOptions {
                item_id: Some(detail.summary.id),
                compact: true,
                limit: 20,
                full_text: false,
                include_contexts: false,
            },
        )?;
    }
    Ok(value)
}

fn dispatch_markdown(context: &Context, command: &MarkdownCommands) -> Result<Value> {
    let db = ZoteroDb::open(&context.config)?;
    match command {
        MarkdownCommands::Status { key } => db.markdown_status(&context.config, key),
    }
}

fn dispatch_config(context: &Context, command: &ConfigCommands) -> Result<Value> {
    match command {
        ConfigCommands::Init { force } => {
            Config::write_default(&context.config_path, *force)?;
            Ok(json!({
                "ok": true,
                "message": "config initialized",
                "config_path": context.config_path,
            }))
        }
        ConfigCommands::Status => Ok(json!({
            "ok": true,
            "config_path": context.config_path,
            "config": redacted_config(&context.config),
        })),
        ConfigCommands::WebApi(args) => {
            let mut config = context.config.clone();
            apply_web_api_args(&mut config.web_api, args)?;
            config.save(&context.config_path, true)?;
            Ok(json!({
                "ok": true,
                "message": "web api config updated",
                "config_path": context.config_path,
                "web_api": web_api_status(&config.web_api),
                "api_key_url": API_KEY_URL,
            }))
        }
    }
}

fn dispatch_search(context: &Context, command: &SearchCommands) -> Result<Value> {
    let db = ZoteroDb::open(&context.config)?;
    match command {
        SearchCommands::List { query, limit } => Ok(json!({
            "ok": true,
            "items": db.list_items(query.as_deref(), *limit)?,
        })),
        SearchCommands::Grep { pattern, limit } => Ok(json!({
            "ok": true,
            "pattern": pattern,
            "hits": db.grep(pattern, *limit)?,
        })),
        SearchCommands::Context {
            item_key,
            pattern,
            context_chars,
        } => {
            let value = db.context(item_key, pattern, *context_chars)?;
            if let Some(item) = value.get("item") {
                if let Ok(summary) = serde_json::from_value(item.clone()) {
                    db.log_read(&context.config, "search context", &summary);
                }
            }
            Ok(value)
        }
    }
}

fn dispatch_index(context: &Context, command: &IndexCommands) -> Result<Value> {
    match command {
        IndexCommands::Status => index::status(&context.config),
        IndexCommands::Update(args) | IndexCommands::Rebuild(args) => index::update(
            &context.config,
            &index::IndexOptions {
                include_full_text: args.include_full_text,
                max_chars: args.max_chars,
            },
        ),
        IndexCommands::Search(args) => index::search(
            &context.config,
            &args.query,
            &index::SearchOptions {
                limit: args.limit,
                snippet_chars: args.snippet_chars,
            },
        ),
        IndexCommands::Chunks(args) => index::search_chunks(
            &context.config,
            &args.query,
            &index::ChunkSearchOptions {
                limit: args.limit,
                snippet_chars: args.snippet_chars,
                item: args.item.clone(),
                collection: args.collection.clone(),
                tag: args.tag.clone(),
            },
        ),
        IndexCommands::Chunk(args) => index::get_chunk(
            &context.config,
            &args.chunk_id,
            &index::ChunkGetOptions {
                max_chars: args.max_chars,
            },
        ),
        IndexCommands::Get(args) => index::get(
            &context.config,
            &args.key,
            &index::GetOptions {
                max_chars: args.max_chars,
            },
        ),
    }
}

fn dispatch_item(context: &Context, command: &ItemCommands) -> Result<Value> {
    let db = ZoteroDb::open(&context.config)?;
    match command {
        ItemCommands::Get { key } => {
            let item = db.get_item(key)?;
            db.log_read(&context.config, "item get", &item.summary);
            Ok(json!({ "ok": true, "item": item }))
        }
        ItemCommands::Extract { key, max_chars } => {
            let extracted = db.extract_text(key, *max_chars)?;
            db.log_read(&context.config, "item extract", &extracted.item);
            Ok(json!({ "ok": true, "extract": extracted }))
        }
        ItemCommands::Markdown(args) => {
            let doc = db.markdown_for_item(
                &context.config,
                &args.key,
                args.max_chars,
                !args.no_lfz_full_md,
            )?;
            db.log_read(&context.config, "item markdown", &doc.item);
            let metadata = json!({
                "source": doc.source,
                "source_path": doc.source_path,
                "fallback_used": doc.fallback_used,
                "extracted_truncated": doc.extracted_truncated,
                "chars": doc.markdown.chars().count(),
                "estimated_tokens": estimated_tokens(doc.markdown.chars().count()),
                "truncated": false,
                "fetch_command": format!("zcli item markdown {} --format text", args.key),
            });
            if let Some(output) = &args.output {
                if let Some(parent) = output
                    .parent()
                    .filter(|parent| !parent.as_os_str().is_empty())
                {
                    fs::create_dir_all(parent)?;
                }
                fs::write(output, &doc.markdown)?;
                Ok(json!({
                    "ok": true,
                    "item": doc.item,
                    "markdown_meta": metadata,
                    "output_path": output,
                }))
            } else {
                Ok(json!({
                    "ok": true,
                    "item": doc.item,
                    "markdown_meta": metadata,
                    "markdown": doc.markdown,
                }))
            }
        }
        ItemCommands::Annotations { key } => {
            let item = db.get_item(key)?;
            db.log_read(&context.config, "item annotations", &item.summary);
            Ok(json!({
                "ok": true,
                "item": item.summary,
                "annotations": db.annotations_for_item(item.summary.id)?,
            }))
        }
        ItemCommands::Notes { key } => {
            let item = db.get_item(key)?;
            db.log_read(&context.config, "item notes", &item.summary);
            Ok(json!({
                "ok": true,
                "item": item.summary,
                "notes": db.notes_for_item(item.summary.id)?,
            }))
        }
        ItemCommands::Attachments { key } => {
            let item = db.get_item(key)?;
            db.log_read(&context.config, "item attachments", &item.summary);
            Ok(json!({
                "ok": true,
                "item": item.summary,
                "attachments": db.attachments_for_item(item.summary.id)?,
            }))
        }
        ItemCommands::Bibtex { key } => {
            let item = db.get_item(key)?;
            db.log_read(&context.config, "item bibtex", &item.summary);
            Ok(json!({
                "ok": true,
                "item": item.summary,
                "bibtex": db.bibtex(key)?,
                "source": "local_sqlite_approximation",
                "exact_command": format!(
                    "zcli item export {} --translator bibtex --format json",
                    key
                ),
            }))
        }
        ItemCommands::Cite(args) => {
            for key in &args.keys {
                let item = db.get_item(key)?;
                db.log_read(&context.config, "item cite", &item.summary);
            }
            local_api::cite_items(
                &context.config,
                &args.keys,
                match args.mode {
                    CitationMode::Bibliography => "bibliography",
                    CitationMode::Citation => "citation",
                },
                args.style.as_deref(),
                &args.locale,
                match args.output {
                    CitationOutputFormat::Text => "text",
                    CitationOutputFormat::Html => "html",
                },
            )
        }
        ItemCommands::Export(args) => {
            for key in &args.keys {
                let item = db.get_item(key)?;
                db.log_read(&context.config, "item export", &item.summary);
            }
            local_api::export_items(&context.config, &args.keys, &args.translator)
        }
    }
}

fn dispatch_collection(context: &Context, command: &CollectionCommands) -> Result<Value> {
    let db = ZoteroDb::open(&context.config)?;
    match command {
        CollectionCommands::List => Ok(json!({
            "ok": true,
            "collections": db.list_collections()?,
        })),
        CollectionCommands::Items { key, limit } => Ok(json!({
            "ok": true,
            "collection_key": key,
            "items": db.collection_items(key, *limit)?,
        })),
    }
}

fn dispatch_tags(context: &Context, command: &TagsCommands) -> Result<Value> {
    let db = ZoteroDb::open(&context.config)?;
    match command {
        TagsCommands::List => Ok(json!({
            "ok": true,
            "tags": db.list_tags()?,
        })),
        TagsCommands::Items { tag, limit } => Ok(json!({
            "ok": true,
            "tag": tag,
            "items": db.tag_items(tag, *limit)?,
        })),
    }
}

fn dispatch_write(context: &Context, command: &WriteCommands) -> Result<Value> {
    match command {
        WriteCommands::Tags(args) => {
            mutation::require_intent(args.dry_run, args.execute)?;
            if args.add_tags.is_empty() && args.remove_tags.is_empty() {
                return Err(anyhow!("pass at least one --add or --remove tag"));
            }
            let db = ZoteroDb::open(&context.config)?;
            let item = db.get_item(&args.key)?.summary;
            let params = json!({
                "itemKeys": [args.key],
                "addTags": args.add_tags,
                "removeTags": args.remove_tags,
            });
            mutation::preview_or_execute(
                &context.config,
                args.dry_run,
                MutationPlan::local_api(
                    "apply_tags",
                    params,
                    json!({
                        "target": item,
                        "add_tags": args.add_tags,
                        "remove_tags": args.remove_tags,
                        "execute_command": format!("zcli write tags {} --execute", args.key),
                    }),
                ),
            )
        }
        WriteCommands::Collection(args) => {
            mutation::require_intent(args.dry_run, args.execute)?;
            let db = ZoteroDb::open(&context.config)?;
            let item = db.get_item(&args.key)?.summary;
            let collection_key = resolve_collection_key(&db, &args.collection)?;
            let action = match args.action {
                CollectionWriteAction::Add => "add",
                CollectionWriteAction::Remove => "remove",
            };
            let params = json!({
                "itemKeys": [args.key],
                "collectionKey": collection_key,
                "action": action,
            });
            mutation::preview_or_execute(
                &context.config,
                args.dry_run,
                MutationPlan::local_api(
                    "move_to_collection",
                    params,
                    json!({
                        "target": item,
                        "collection": args.collection,
                        "collection_key": collection_key,
                        "action": action,
                        "execute_command": format!(
                            "zcli write collection {} --collection {} --action {} --execute",
                            args.key, args.collection, action
                        ),
                    }),
                ),
            )
        }
        WriteCommands::Note(args) => {
            mutation::require_intent(args.dry_run, args.execute)?;
            let content = write_note_content(args)?;
            let db = ZoteroDb::open(&context.config)?;
            let item = db.get_item(&args.key)?.summary;
            let params = json!({
                "itemKey": args.key,
                "title": args.title,
                "content": content,
            });
            mutation::preview_or_execute(
                &context.config,
                args.dry_run,
                MutationPlan::local_api(
                    "create_note",
                    params,
                    json!({
                        "target": item,
                        "title": args.title,
                        "content_chars": content.chars().count(),
                        "execute_command": format!("zcli write note {} --execute --content <text>", args.key),
                    }),
                ),
            )
        }
        WriteCommands::Attach(args) => {
            mutation::require_intent(args.dry_run, args.execute)?;
            let db = ZoteroDb::open(&context.config)?;
            let item = db.get_item(&args.key)?.summary;
            let file_exists = args.file.exists();
            let op = match args.mode {
                AttachmentWriteMode::Link => "link_attachment",
                AttachmentWriteMode::Import => "import_local_files",
            };
            let params = match args.mode {
                AttachmentWriteMode::Link => json!({
                    "itemKey": args.key,
                    "filePath": args.file,
                    "title": args.title,
                }),
                AttachmentWriteMode::Import => json!({
                    "itemKey": args.key,
                    "filePaths": [args.file],
                    "title": args.title,
                }),
            };
            mutation::preview_or_execute(
                &context.config,
                args.dry_run,
                MutationPlan::helper(
                    op,
                    params,
                    json!({
                        "target": item,
                        "file": args.file,
                        "file_exists": file_exists,
                        "mode": match args.mode {
                            AttachmentWriteMode::Link => "link",
                            AttachmentWriteMode::Import => "import",
                        },
                        "title": args.title,
                        "execute_command": format!(
                            "zcli write attach {} {} --mode {} --execute",
                            args.key,
                            args.file.display(),
                            match args.mode {
                                AttachmentWriteMode::Link => "link",
                                AttachmentWriteMode::Import => "import",
                            }
                        ),
                    }),
                ),
            )
        }
        WriteCommands::RenameAttachment(args) => {
            mutation::require_intent(args.dry_run, args.execute)?;
            let params = json!({
                "itemKey": args.key,
                "newName": args.name,
            });
            mutation::preview_or_execute(
                &context.config,
                args.dry_run,
                MutationPlan::helper(
                    "rename_attachment",
                    params,
                    json!({
                        "attachment_key": args.key,
                        "new_name": args.name,
                        "execute_command": format!(
                            "zcli write rename-attachment {} --name {} --execute",
                            args.key, args.name
                        ),
                    }),
                ),
            )
        }
        WriteCommands::ImportFiles(args) => {
            mutation::require_intent(args.dry_run, args.execute)?;
            if args.files.is_empty() {
                return Err(anyhow!("pass at least one file path"));
            }
            let files: Vec<Value> = args
                .files
                .iter()
                .map(|path| {
                    json!({
                        "path": path,
                        "exists": path.exists(),
                    })
                })
                .collect();
            let params = json!({
                "filePaths": args.files,
            });
            mutation::preview_or_execute(
                &context.config,
                args.dry_run,
                MutationPlan::helper(
                    "import_local_files",
                    params,
                    json!({
                        "files": files,
                        "mode": "standalone_attachment_import",
                        "note": "This imports local files into Zotero storage; metadata recognition is not claimed in v1.",
                        "execute_command": "zcli write import-files <files...> --execute",
                    }),
                ),
            )
        }
        WriteCommands::Trash(args) => {
            mutation::require_intent(args.dry_run, args.execute)?;
            let db = ZoteroDb::open(&context.config)?;
            let item = db.get_item(&args.key)?.summary;
            let params = json!({
                "itemKeys": [args.key],
            });
            mutation::preview_or_execute(
                &context.config,
                args.dry_run,
                MutationPlan::helper(
                    "trash_items",
                    params,
                    json!({
                        "target": item,
                        "destructive": true,
                        "zotero_semantics": "move to trash, not permanent delete",
                        "execute_command": format!("zcli write trash {} --execute", args.key),
                    }),
                ),
            )
        }
        WriteCommands::Metadata(args) => {
            mutation::require_intent(args.dry_run, args.execute)?;
            let fields = parse_metadata_patch(&args.set_fields, &args.clear_fields)?;
            local_api::validate_metadata_patch(&fields)?;
            let db = ZoteroDb::open(&context.config)?;
            let item = db.get_item(&args.key)?;
            let current = fields
                .keys()
                .map(|field| {
                    (
                        field.clone(),
                        Value::String(item.fields.get(field).cloned().unwrap_or_default()),
                    )
                })
                .collect::<serde_json::Map<String, Value>>();
            mutation::preview_or_execute(
                &context.config,
                args.dry_run,
                MutationPlan::local_api(
                    "update_metadata",
                    json!({
                        "itemKey": args.key,
                        "fields": fields.clone(),
                    }),
                    json!({
                        "target": item.summary,
                        "current": current,
                        "changes": fields,
                        "execute_command": format!(
                            "zcli write metadata {} <same --set/--clear arguments> --execute",
                            args.key
                        ),
                    }),
                ),
            )
        }
    }
}

fn parse_metadata_patch(
    set_fields: &[String],
    clear_fields: &[String],
) -> Result<serde_json::Map<String, Value>> {
    let mut fields = serde_json::Map::new();
    for assignment in set_fields {
        let (field, value) = assignment
            .split_once('=')
            .ok_or_else(|| anyhow!("invalid --set value {assignment:?}; expected FIELD=VALUE"))?;
        let field = field.trim();
        if field.is_empty() {
            return Err(anyhow!("metadata field names cannot be empty"));
        }
        fields.insert(field.to_string(), Value::String(value.to_string()));
    }
    for field in clear_fields {
        let field = field.trim();
        if field.is_empty() {
            return Err(anyhow!("metadata field names cannot be empty"));
        }
        fields.insert(field.to_string(), Value::String(String::new()));
    }
    if fields.is_empty() {
        return Err(anyhow!(
            "pass at least one --set FIELD=VALUE or --clear FIELD"
        ));
    }
    Ok(fields)
}

fn dispatch_import(context: &Context, command: &ImportCommands) -> Result<Value> {
    match command {
        ImportCommands::Arxiv(args) => {
            dispatch_import_identifiers(context, args, Some("arxiv"), "arxiv")
        }
        ImportCommands::Ids(args) => dispatch_import_identifiers(context, args, None, "ids"),
        ImportCommands::Pdf(args) => dispatch_import_pdfs(context, args),
        ImportCommands::Url(args) => dispatch_import_urls(context, args),
    }
}

fn dispatch_import_identifiers(
    context: &Context,
    args: &ImportIdentifiersArgs,
    forced_kind: Option<&str>,
    command_name: &str,
) -> Result<Value> {
    mutation::require_intent(args.dry_run, args.execute)?;
    let db = ZoteroDb::open(&context.config).ok();
    let plan = import_plan::identifiers(
        &args.identifiers,
        forced_kind,
        command_name,
        db.as_ref(),
        &PlanOptions {
            collections: &args.collections,
            tags: &args.tags,
            allow_duplicates: args.allow_duplicates,
        },
    );
    mutation::preview_or_execute(&context.config, args.dry_run, plan?)
}

fn dispatch_import_pdfs(context: &Context, args: &ImportPdfArgs) -> Result<Value> {
    mutation::require_intent(args.dry_run, args.execute)?;
    let db = ZoteroDb::open(&context.config).ok();
    let plan = import_plan::pdfs(
        &args.sources,
        !args.no_recognize,
        db.as_ref(),
        &PlanOptions {
            collections: &args.collections,
            tags: &args.tags,
            allow_duplicates: args.allow_duplicates,
        },
    );
    mutation::preview_or_execute(&context.config, args.dry_run, plan?)
}

fn dispatch_import_urls(context: &Context, args: &ImportUrlArgs) -> Result<Value> {
    mutation::require_intent(args.dry_run, args.execute)?;
    let db = ZoteroDb::open(&context.config).ok();
    let plan = import_plan::urls(
        &args.urls,
        db.as_ref(),
        &PlanOptions {
            collections: &args.collections,
            tags: &args.tags,
            allow_duplicates: args.allow_duplicates,
        },
    );
    mutation::preview_or_execute(&context.config, args.dry_run, plan?)
}

fn write_note_content(args: &WriteNoteArgs) -> Result<String> {
    match (&args.content, &args.file) {
        (Some(_), Some(_)) => Err(anyhow!("pass either --content or --file, not both")),
        (Some(content), None) => Ok(content.clone()),
        (None, Some(path)) => fs::read_to_string(path)
            .map_err(|err| anyhow!("failed to read note content from {}: {err}", path.display())),
        (None, None) => Err(anyhow!("pass --content <text> or --file <path>")),
    }
}

fn resolve_collection_key(db: &ZoteroDb, input: &str) -> Result<String> {
    let input = input.trim();
    let matches = db
        .list_collections()?
        .into_iter()
        .filter(|collection| {
            collection.key == input
                || collection.id.to_string() == input
                || collection.name.eq_ignore_ascii_case(input)
        })
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [collection] => Ok(collection.key.clone()),
        [] => Err(anyhow!("collection not found: {input}")),
        _ => Err(anyhow!("collection name is ambiguous: {input}")),
    }
}

fn dispatch_mirror(context: &Context, command: &MirrorCommands) -> Result<Value> {
    match command {
        MirrorCommands::Status => mirror::status(&context.config),
        MirrorCommands::Rebuild(args) => {
            let db = ZoteroDb::open(&context.config)?;
            mirror::rebuild(
                &context.config,
                &db,
                MirrorOptions {
                    dry_run: args.dry_run,
                    mode: args.mode,
                    limit: args.limit,
                    incremental: false,
                    cleanup_stale: !args.no_cleanup,
                    write_markdown: args.write_markdown,
                    markdown_max_chars: args.markdown_max_chars,
                },
            )
        }
        MirrorCommands::Sync(args) => {
            let db = ZoteroDb::open(&context.config)?;
            mirror::rebuild(
                &context.config,
                &db,
                MirrorOptions {
                    dry_run: args.dry_run,
                    mode: args.mode,
                    limit: args.limit,
                    incremental: true,
                    cleanup_stale: !args.no_cleanup,
                    write_markdown: args.write_markdown,
                    markdown_max_chars: args.markdown_max_chars,
                },
            )
        }
        MirrorCommands::Watch(args) => mirror::watch(
            &context.config,
            mirror::MirrorWatchOptions {
                interval_secs: args.interval,
                settle_ms: args.settle_ms,
                include_storage: args.include_storage,
                once: args.once,
                run_on_start: args.run_on_start,
                dry_run: args.dry_run,
                mode: args.mode,
                limit: args.limit,
                cleanup_stale: !args.no_cleanup,
                max_events: args.max_events,
                write_markdown: args.write_markdown,
                markdown_max_chars: args.markdown_max_chars,
            },
        ),
        MirrorCommands::DaemonInstall(args) => mirror_daemon_install(context, args),
    }
}

fn dispatch_recap(context: &Context, command: &RecapCommands) -> Result<Value> {
    match command {
        RecapCommands::Reading(args) => {
            let range = DateRange::parse(args.from.as_deref(), args.to.as_deref())?;
            dispatch_reading_recap(
                context,
                &range,
                args.item.as_deref(),
                args.include_lfz,
                args.no_lfz,
                args.why,
                "reading",
            )
        }
        RecapCommands::Today(args) => {
            let range = DateRange::parse(Some("today"), Some("today"))?;
            dispatch_reading_recap(context, &range, None, false, args.no_lfz, args.why, "today")
        }
        RecapCommands::Week(args) => {
            let today = Local::now().date_naive();
            let from = (today - Duration::days(6)).format("%Y-%m-%d").to_string();
            let to = today.format("%Y-%m-%d").to_string();
            let range = DateRange::parse(Some(&from), Some(&to))?;
            dispatch_reading_recap(context, &range, None, false, args.no_lfz, args.why, "week")
        }
        RecapCommands::Lfz(args) => {
            let range = DateRange::parse(args.from.as_deref(), args.to.as_deref())?;
            let db = ZoteroDb::open(&context.config)?;
            let item_id = resolve_item_filter(&db, args.item.as_deref())?;
            let reading = filter_reading(
                db.reading_recap(&range, context.config.state_dir.as_deref())?,
                item_id,
            );
            Ok(json!({
                "ok": true,
                "kind": "lfz",
                "range": range_json(&range),
                "reading": if args.details {
                    json!(reading)
                } else {
                    compact_reading(&reading, args.limit)
                },
                "item_filter": args.item.as_ref().map(|key| json!({
                    "key": key,
                    "id": item_id,
                })),
                "lfz": lfz::recap(
                    &context.config,
                    &db,
                    &range,
                    lfz::RecapOptions {
                        item_id,
                        compact: !args.details,
                        limit: args.limit,
                        full_text: args.full_text,
                        include_contexts: args.include_contexts,
                    },
                )?,
            }))
        }
    }
}

fn resolve_item_filter(db: &ZoteroDb, key: Option<&str>) -> Result<Option<i64>> {
    key.map(|key| {
        db.item_id_by_key(key)?
            .ok_or_else(|| anyhow!("Zotero item not found: {key}"))
    })
    .transpose()
}

fn dispatch_reading_recap(
    context: &Context,
    range: &DateRange,
    item_key: Option<&str>,
    request_lfz: bool,
    no_lfz: bool,
    why: bool,
    kind: &str,
) -> Result<Value> {
    if request_lfz && no_lfz {
        anyhow::bail!("--include-lfz and --no-lfz cannot be used together");
    }
    let db = ZoteroDb::open(&context.config)?;
    let item_id = resolve_item_filter(&db, item_key)?;
    let reading = filter_reading(
        db.reading_recap(range, context.config.state_dir.as_deref())?,
        item_id,
    );
    let entries = if why {
        json!(reading
            .iter()
            .map(|entry| {
                let mut value = serde_json::to_value(entry).unwrap_or_else(|_| json!({}));
                value["why"] = json!(provenance_reason(&entry.provenance));
                value
            })
            .collect::<Vec<_>>())
    } else {
        json!(reading)
    };
    let lfz_enabled = context.config.lfz.enabled.unwrap_or(false);
    let include_lfz = lfz_enabled && !no_lfz;
    let mut value = json!({
        "ok": true,
        "kind": kind,
        "range": range_json(range),
        "item_filter": item_key.map(|key| json!({
            "key": key,
            "id": item_id,
        })),
        "entries": entries,
        "lfz_policy": {
            "enabled_in_config": lfz_enabled,
            "requested_by_flag": request_lfz,
            "disabled_by_flag": no_lfz,
            "included": include_lfz,
            "unavailable_reason": if request_lfz && !lfz_enabled {
                Some("lfz_not_enabled_in_config")
            } else {
                None
            },
        },
    });
    if why {
        value["provenance_legend"] = provenance_legend();
    }
    if include_lfz {
        value["lfz"] = lfz::recap(
            &context.config,
            &db,
            range,
            lfz::RecapOptions {
                item_id,
                compact: true,
                limit: 8,
                full_text: false,
                include_contexts: false,
            },
        )?;
    }
    Ok(value)
}

fn filter_reading(
    mut entries: Vec<crate::zotero::ReadingEntry>,
    item_id: Option<i64>,
) -> Vec<crate::zotero::ReadingEntry> {
    if let Some(item_id) = item_id {
        entries.retain(|entry| entry.item.id == item_id);
    }
    entries
}

fn compact_reading(entries: &[crate::zotero::ReadingEntry], limit: usize) -> Value {
    let mut provenance = serde_json::Map::new();
    for entry in entries {
        let current = provenance
            .get(&entry.provenance)
            .and_then(Value::as_u64)
            .unwrap_or(0);
        provenance.insert(entry.provenance.clone(), json!(current + 1));
    }
    json!({
        "count": entries.len(),
        "provenance": provenance,
        "entries": entries.iter().take(limit).collect::<Vec<_>>(),
    })
}

fn dispatch_lfz(context: &Context, command: &LfzCommands) -> Result<Value> {
    match command {
        LfzCommands::Doctor => {
            let db = ZoteroDb::open(&context.config).ok();
            Ok(json!({
                "ok": true,
                "lfz": lfz::doctor(&context.config, db.as_ref())?,
            }))
        }
        LfzCommands::Turn(args) => {
            let db = ZoteroDb::open(&context.config)?;
            let mut value = json!({
                "ok": true,
                "lfz": lfz::turn(
                    &context.config,
                    &db,
                    &args.message_ref,
                    args.include_contexts,
                )?,
            });
            if let Some(budget) = &args.budget {
                let budget_tokens = parse_budget_tokens(budget)?;
                value["budget"] = budget_meta(budget_tokens);
                apply_lfz_turn_budget(&mut value["lfz"], budget_tokens, &args.message_ref);
            }
            Ok(value)
        }
        LfzCommands::Turns(args) => {
            let db = ZoteroDb::open(&context.config)?;
            let item_id = resolve_item_filter(&db, Some(&args.item))?
                .ok_or_else(|| anyhow!("Zotero item not found: {}", args.item))?;
            let range = DateRange::parse(Some("1970-01-01"), Some("2100-01-01"))?;
            Ok(json!({
                "ok": true,
                "item_key": args.item,
                "lfz": lfz::recap(
                    &context.config,
                    &db,
                    &range,
                    lfz::RecapOptions {
                        item_id: Some(item_id),
                        compact: true,
                        limit: args.limit,
                        full_text: false,
                        include_contexts: false,
                    },
                )?,
            }))
        }
    }
}

fn dispatch_inbox(context: &Context, command: &InboxCommands) -> Result<Value> {
    match command {
        InboxCommands::Status => Ok(inbox::status(&context.config)),
        InboxCommands::Fetch(args) | InboxCommands::Triage(args) => {
            inbox::fetch(inbox::FetchOptions {
                config: &context.config,
                source: args.source,
                query: args.query.as_deref(),
                handles: &args.handle,
                configured_x_handles: &context.config.inbox.x_handles,
                tweet_limit: args.tweet_limit,
                limit: args.limit,
                fallback_sort: args.fallback_sort,
                fallback_interval: &args.fallback_interval,
                topics: &args.topics,
                min_likes: args.min_likes,
                min_github_stars: args.min_github_stars,
                min_visits: args.min_visits,
                since: args.since.as_deref(),
                days: args.days,
                date_field: args.date_field,
                timeout: args.timeout,
                context: !args.no_context,
                code_overview: args.code_overview,
                show_seen: args.show_seen,
                show_existing: args.show_existing,
                seen_days: args.seen_days,
                cache_overview: !args.no_cache_overview,
                overview_ttl_days: args.overview_ttl_days,
                dry_run: args.dry_run,
                execute: args.execute,
            })
        }
        InboxCommands::Discussion(args) => inbox::discussion(inbox::DiscussionOptions {
            paper: &args.paper,
            handles: &args.handle,
            tweets: &args.tweet,
            days: args.days,
            search_limit: args.search_limit,
            limit: args.limit,
            reply_limit: args.reply_limit,
            max_pages: args.max_pages,
            expand: !args.no_expand,
        }),
        InboxCommands::Sources { command } => dispatch_inbox_sources(context, command),
    }
}

fn dispatch_inbox_sources(context: &Context, command: &InboxSourceCommands) -> Result<Value> {
    match command {
        InboxSourceCommands::X { command } => {
            let mut config = context.config.clone();
            let value = match command {
                XSourceCommands::List => inbox::x_handles(&config),
                XSourceCommands::Add(args) => {
                    let value = inbox::add_x_handle(&mut config, &args.handle)?;
                    config.save(&context.config_path, true)?;
                    value
                }
                XSourceCommands::Remove(args) => {
                    let value = inbox::remove_x_handle(&mut config, &args.handle)?;
                    config.save(&context.config_path, true)?;
                    value
                }
            };
            Ok(value)
        }
    }
}

fn dispatch_skill(context: &Context, command: &SkillCommands) -> Result<Value> {
    match command {
        SkillCommands::Doctor => skill::doctor(&context.config),
        SkillCommands::Install(args) => skill::install(
            SkillInstallOptions {
                target: args.target,
                dry_run: args.dry_run,
                copy: args.copy,
            },
            &context.config,
        ),
    }
}

fn dispatch_helper(context: &Context, command: &HelperCommands) -> Result<Value> {
    match command {
        HelperCommands::Doctor => helper::doctor(&context.config),
        HelperCommands::Package(args) => helper::package(
            HelperPackageOptions {
                output: args.output.clone(),
                dry_run: args.dry_run,
                force: args.force,
            },
            &context.config,
        ),
        HelperCommands::Install(args) => helper::install(
            HelperInstallOptions {
                dry_run: args.dry_run,
                execute: args.execute,
                profile: args.profile.clone(),
                force: args.force,
            },
            &context.config,
        ),
    }
}

fn dispatch_local_api(context: &Context, command: &LocalApiCommands) -> Result<Value> {
    match command {
        LocalApiCommands::Doctor => local_api::doctor(&context.config),
        LocalApiCommands::Authorize(args) => {
            local_api::authorize(&context.config, args.dry_run, args.execute)
        }
    }
}

fn dispatch_web_api(context: &Context, command: &WebApiCommands) -> Result<Value> {
    match command {
        WebApiCommands::Doctor => crate::web_api::doctor(&context.config),
    }
}

fn dispatch_queue(context: &Context, command: &QueueCommands) -> Result<Value> {
    let path = queue_path(&context.config);
    match command {
        QueueCommands::List => Ok(json!({
            "ok": true,
            "queue_path": path,
            "items": read_queue(&path)?,
        })),
        QueueCommands::Add(args) => {
            let db = ZoteroDb::open(&context.config)?;
            let item = db.get_item(&args.key)?.summary;
            let mut items = read_queue(&path)?;
            items.retain(|entry| {
                entry.get("key").and_then(Value::as_str) != Some(args.key.as_str())
            });
            items.push(json!({
                "key": args.key,
                "title": item.title,
                "note": args.note,
                "added_at": chrono::Utc::now().to_rfc3339(),
                "status": "todo",
            }));
            write_queue(&path, &items)?;
            Ok(json!({"ok": true, "queue_path": path, "items": items}))
        }
        QueueCommands::Done(args) => {
            let mut items = read_queue(&path)?;
            for item in &mut items {
                if item.get("key").and_then(Value::as_str) == Some(args.key.as_str()) {
                    item["status"] = json!("done");
                    item["done_at"] = json!(chrono::Utc::now().to_rfc3339());
                }
            }
            write_queue(&path, &items)?;
            Ok(json!({"ok": true, "queue_path": path, "items": items}))
        }
    }
}

fn dispatch_export(context: &Context, command: &ExportCommands) -> Result<Value> {
    match command {
        ExportCommands::Pack(args) => export_pack(context, args),
    }
}

fn dispatch_open_reveal(context: &Context, args: &OpenArgs, reveal: bool) -> Result<Value> {
    let db = ZoteroDb::open(&context.config)?;
    let item = db.get_item(&args.key)?;
    let target = item
        .attachments
        .iter()
        .find_map(|attachment| {
            attachment
                .resolved_path
                .as_ref()
                .filter(|path| path.exists())
        })
        .cloned();
    let Some(target) = target else {
        return Ok(json!({
            "ok": false,
            "item": item.summary,
            "reason": "no local attachment path found",
        }));
    };
    let command = if cfg!(target_os = "macos") {
        if reveal {
            vec![
                "open".to_string(),
                "-R".to_string(),
                target.display().to_string(),
            ]
        } else {
            vec!["open".to_string(), target.display().to_string()]
        }
    } else if reveal {
        vec![
            "xdg-open".to_string(),
            target
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .display()
                .to_string(),
        ]
    } else {
        vec!["xdg-open".to_string(), target.display().to_string()]
    };
    if !args.dry_run {
        let mut process = ProcessCommand::new(&command[0]);
        process.args(&command[1..]);
        process.spawn()?;
    }
    Ok(json!({
        "ok": true,
        "dry_run": args.dry_run,
        "action": if reveal { "reveal" } else { "open" },
        "item": item.summary,
        "target": target,
        "command": command,
    }))
}

fn mirror_daemon_install(context: &Context, args: &MirrorDaemonInstallArgs) -> Result<Value> {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let plist = home
        .join("Library")
        .join("LaunchAgents")
        .join("com.zotero-cli.mirror-watch.plist");
    let mut command = vec![
        "zcli".to_string(),
        "--config".to_string(),
        context.config_path.display().to_string(),
        "mirror".to_string(),
        "watch".to_string(),
        "--interval".to_string(),
        args.interval.to_string(),
        "--run-on-start".to_string(),
    ];
    if args.write_markdown {
        command.push("--write-markdown".to_string());
    }
    if !args.execute {
        return Ok(json!({
            "ok": true,
            "dry_run": true,
            "plist_path": plist,
            "command": command,
            "execute_command": "zcli mirror daemon-install --execute",
        }));
    }
    if let Some(parent) = plist.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&plist, launchd_plist(&command))?;
    Ok(json!({
        "ok": true,
        "dry_run": args.dry_run,
        "plist_path": plist,
        "command": command,
        "message": "launchd plist written; load it with launchctl bootstrap gui/$(id -u) <plist>",
    }))
}

fn export_pack(context: &Context, args: &ExportPackArgs) -> Result<Value> {
    let db = ZoteroDb::open(&context.config)?;
    let item = db.get_item(&args.key)?;
    let root = args
        .output
        .clone()
        .unwrap_or_else(|| PathBuf::from(format!("{}-zcli-pack", args.key)));
    let files = vec![
        root.join("metadata.json"),
        root.join("paper.md"),
        root.join("notes.json"),
        root.join("annotations.json"),
        root.join("bibtex.bib"),
        root.join("PACK.md"),
    ];
    if args.dry_run {
        return Ok(json!({
            "ok": true,
            "dry_run": true,
            "target": format!("{:?}", args.for_agent).to_ascii_lowercase(),
            "item": item.summary,
            "output": root,
            "files": files,
        }));
    }
    fs::create_dir_all(&root)?;
    let markdown = db.markdown_for_item(&context.config, &args.key, 240_000, true)?;
    fs::write(
        root.join("metadata.json"),
        serde_json::to_string_pretty(&item)?,
    )?;
    fs::write(root.join("paper.md"), markdown.markdown)?;
    fs::write(
        root.join("notes.json"),
        serde_json::to_string_pretty(&db.notes_for_item(item.summary.id)?)?,
    )?;
    fs::write(
        root.join("annotations.json"),
        serde_json::to_string_pretty(&db.annotations_for_item(item.summary.id)?)?,
    )?;
    fs::write(root.join("bibtex.bib"), db.bibtex(&args.key)?)?;
    fs::write(
        root.join("PACK.md"),
        format!(
            "# zcli pack\n\n- key: `{}`\n- target: `{:?}`\n- paper: `paper.md`\n- metadata: `metadata.json`\n",
            args.key, args.for_agent
        ),
    )?;
    Ok(json!({
        "ok": true,
        "dry_run": false,
        "target": format!("{:?}", args.for_agent).to_ascii_lowercase(),
        "item": item.summary,
        "output": root,
        "files": files,
    }))
}

fn queue_path(config: &Config) -> PathBuf {
    config
        .state_dir
        .clone()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("queue.json")
}

fn read_queue(path: &Path) -> Result<Vec<Value>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&raw).unwrap_or_default())
}

fn write_queue(path: &Path, items: &[Value]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(items)?)?;
    Ok(())
}

fn launchd_plist(command: &[String]) -> String {
    let args = command
        .iter()
        .map(|arg| format!("        <string>{}</string>", xml_escape(arg)))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.zotero-cli.mirror-watch</string>
    <key>ProgramArguments</key>
    <array>
{args}
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
</dict>
</plist>
"#
    )
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn parse_budget_tokens(value: &str) -> Result<usize> {
    let raw = value.trim().to_lowercase();
    let (number, multiplier) = if let Some(rest) = raw.strip_suffix('k') {
        (rest, 1_000_f64)
    } else if let Some(rest) = raw.strip_suffix('m') {
        (rest, 1_000_000_f64)
    } else {
        (raw.as_str(), 1_f64)
    };
    let parsed = number
        .parse::<f64>()
        .map_err(|_| anyhow!("invalid budget: {value}"))?;
    Ok((parsed * multiplier).max(1.0) as usize)
}

fn estimated_tokens(chars: usize) -> usize {
    chars.div_ceil(4)
}

fn budget_meta(tokens: usize) -> Value {
    json!({
        "tokens": tokens,
        "approx_chars": tokens.saturating_mul(4),
    })
}

fn clip_text<F>(text: &str, max_chars: usize, fetch_command: F) -> (String, Value)
where
    F: FnOnce() -> String,
{
    let chars = text.chars().count();
    let clipped = if chars > max_chars {
        text.chars().take(max_chars).collect::<String>()
    } else {
        text.to_string()
    };
    let clipped_chars = clipped.chars().count();
    (
        clipped,
        json!({
            "chars": chars,
            "estimated_tokens": estimated_tokens(chars),
            "included_chars": clipped_chars,
            "included_estimated_tokens": estimated_tokens(clipped_chars),
            "truncated": chars > clipped_chars,
            "fetch_command": fetch_command(),
        }),
    )
}

fn apply_lfz_turn_budget(value: &mut Value, budget_tokens: usize, message_ref: &str) {
    let max_chars = budget_tokens.saturating_mul(4);
    let per_field = (max_chars / 3).max(1000);
    if let Some(text) = value
        .get_mut("question")
        .and_then(|q| q.get_mut("text"))
        .and_then(|value| value.as_str())
        .map(str::to_string)
    {
        let (clipped, meta) = clip_text(&text, per_field, || {
            format!("zcli lfz turn {message_ref} --format json")
        });
        value["question"]["text"] = json!(clipped);
        value["question"]["budget_meta"] = meta;
    }
    if let Some(answers) = value.get_mut("answers").and_then(Value::as_array_mut) {
        for answer in answers {
            if let Some(text) = answer
                .get("text")
                .and_then(Value::as_str)
                .map(str::to_string)
            {
                let (clipped, meta) = clip_text(&text, per_field, || {
                    format!("zcli lfz turn {message_ref} --format json")
                });
                answer["text"] = json!(clipped);
                answer["budget_meta"] = meta;
            }
        }
    }
    if let Some(finals) = value.get_mut("agent_finals").and_then(Value::as_array_mut) {
        for final_run in finals {
            if let Some(text) = final_run
                .get("final_text")
                .and_then(Value::as_str)
                .map(str::to_string)
            {
                let (clipped, meta) = clip_text(&text, per_field, || {
                    format!("zcli lfz turn {message_ref} --format json")
                });
                final_run["final_text"] = json!(clipped);
                final_run["budget_meta"] = meta;
            }
        }
    }
}

fn provenance_reason(provenance: &str) -> &'static str {
    match provenance {
        "cli_read_log" => "zcli previously read or extracted this item",
        "annotation" => "an annotation changed in the requested date range",
        "note" => "a note changed in the requested date range",
        "metadata_modified" => {
            "Zotero metadata changed; this is a weak touched-paper signal, not definite reading"
        }
        _ => "unknown provenance",
    }
}

fn provenance_legend() -> Value {
    json!({
        "cli_read_log": provenance_reason("cli_read_log"),
        "annotation": provenance_reason("annotation"),
        "note": provenance_reason("note"),
        "metadata_modified": provenance_reason("metadata_modified"),
    })
}

fn apply_web_api_args(config: &mut WebApiConfig, args: &WebApiArgs) -> Result<()> {
    if args.enable {
        config.enabled = true;
    }
    if args.disable {
        config.enabled = false;
    }
    if let Some(library_type) = args.library_type {
        config.library_type = match library_type {
            WebApiLibraryType::User => "user",
            WebApiLibraryType::Group => "group",
        }
        .to_string();
    }
    if let Some(library_id) = &args.library_id {
        config.library_id = Some(library_id.clone());
    }
    if let Some(base_url) = &args.base_url {
        config.base_url = base_url.trim_end_matches('/').to_string();
    }
    if let Some(env_name) = &args.api_key_env {
        config.api_key_env = Some(env_name.clone());
        config.api_key = None;
    }
    if args.clear_stored_key {
        config.api_key = None;
    }
    if args.api_key_stdin {
        let mut key = String::new();
        io::stdin().read_to_string(&mut key)?;
        let key = key.trim();
        if key.is_empty() {
            return Err(anyhow!("stdin did not contain a Zotero API key"));
        }
        config.api_key = Some(key.to_string());
    }
    Ok(())
}

fn web_api_status(config: &WebApiConfig) -> Value {
    let env_key_present = config
        .api_key_env
        .as_deref()
        .and_then(|name| env::var_os(name))
        .is_some();
    json!({
        "enabled": config.enabled,
        "base_url": config.base_url,
        "library_type": config.library_type,
        "library_id": config.library_id,
        "library_id_help_url": API_LIBRARY_ID_HELP_URL,
        "api_key_url": API_KEY_URL,
        "api_key_env": config.api_key_env,
        "api_key_present": config.api_key.as_deref().map(|s| !s.is_empty()).unwrap_or(false) || env_key_present,
        "stored_api_key": config.api_key.as_ref().map(|_| "<redacted>"),
        "network_used_by_core_commands": false,
    })
}

fn redacted_config(config: &Config) -> Value {
    json!({
        "zotero_db_path": config.zotero_db_path,
        "zotero_storage_path": config.zotero_storage_path,
        "mirror_root": config.mirror_root,
        "cache_dir": config.cache_dir,
        "state_dir": config.state_dir,
        "web_api": web_api_status(&config.web_api),
        "helper": {
            "enabled": config.helper.enabled,
            "endpoint": config.helper.endpoint,
            "token_path": config.helper.token_path,
        },
        "lfz": config.lfz,
        "inbox": {
            "x_handles": config.inbox.x_handles,
        },
        "risk": {
            "high_risk_auth_enabled": config.risk.high_risk_auth_enabled,
            "alphaxiv_auth_enabled": config.risk.alphaxiv_auth_enabled,
        },
    })
}

fn range_json(range: &DateRange) -> Value {
    json!({
        "from": range.from.to_rfc3339(),
        "to": range.to.to_rfc3339(),
    })
}
