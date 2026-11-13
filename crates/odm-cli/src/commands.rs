//! Command definitions for the odm CLI client.
use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "odm", about = "Open Download Manager CLI", version)]
pub struct Cli {
    #[arg(long, global = true, help = "Path to custom config directory")]
    pub config_dir: Option<PathBuf>,

    #[arg(
        long,
        global = true,
        env = "ODM_SERVER",
        default_value = "http://127.0.0.1:6820",
        help = "Daemon server URL"
    )]
    pub server: String,

    #[arg(
        long,
        global = true,
        env = "ODM_TOKEN",
        help = "API token (defaults to reading config file)"
    )]
    pub token: Option<String>,

    #[arg(long, global = true, help = "Format output as JSON")]
    pub json: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(clap::Args, Debug, Clone)]
pub struct AddArgs {
    #[arg(
        required_unless_present = "torrent_file",
        help = "One or more URLs to download"
    )]
    pub urls: Vec<String>,

    #[arg(short = 'd', long = "dir", help = "Target download directory")]
    pub dir: Option<String>,

    #[arg(
        short = 'o',
        long = "output",
        help = "Custom filename (only for single URL)"
    )]
    pub output: Option<String>,

    #[arg(short = 's', long = "segments", help = "Number of segments (1-32)")]
    pub segments: Option<u32>,

    #[arg(short = 'l', long = "limit", help = "Speed limit in bytes/sec")]
    pub limit: Option<u64>,

    #[arg(short = 'q', long = "queue", help = "Target queue name")]
    pub queue: Option<String>,

    #[arg(short = 'H', long = "header", help = "Custom header 'Key: Value'")]
    pub headers: Vec<String>,

    #[arg(
        long = "checksum",
        help = "Checksum in algo:digest format (e.g. sha256:...)"
    )]
    pub checksum: Option<String>,

    #[arg(long = "mirror", help = "Mirror download URL")]
    pub mirror: Vec<String>,

    #[arg(long = "metalink", help = "Path to .metalink or .meta4 file")]
    pub metalink: Option<String>,

    #[arg(long = "category", help = "Category name")]
    pub category: Option<String>,

    #[arg(long = "torrent-file", help = "Path to a .torrent file to download")]
    pub torrent_file: Option<String>,

    #[arg(
        long = "select-files",
        help = "Comma-separated indices of files to download (e.g. 0,1,3)"
    )]
    pub select_files: Option<String>,

    #[arg(
        long = "seed-ratio",
        help = "Target upload/seed ratio override (e.g. 2.0)"
    )]
    pub seed_ratio: Option<f64>,

    #[arg(
        long,
        help = "Quality or format identifier (e.g. 720p, 1080p, format ID)"
    )]
    pub quality: Option<String>,

    #[arg(
        long,
        help = "Download with yt-dlp instead of picking a backend by URL"
    )]
    pub ytdlp: bool,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    #[command(about = "Add one or more downloads")]
    Add(Box<AddArgs>),

    #[command(about = "List downloads")]
    List {
        #[arg(long, help = "Filter by state")]
        state: Option<String>,

        #[arg(long, help = "Filter by queue")]
        queue: Option<String>,
    },

    #[command(about = "Show detailed info for a download")]
    Info {
        #[arg(required = true, help = "Job ID")]
        id: i64,
    },

    #[command(about = "Pause a download")]
    Pause {
        #[arg(required = true, help = "Job ID")]
        id: i64,
    },

    #[command(about = "Resume a download")]
    Resume {
        #[arg(required = true, help = "Job ID")]
        id: i64,
    },

    #[command(about = "Cancel a download")]
    Cancel {
        #[arg(required = true, help = "Job ID")]
        id: i64,
    },

    #[command(about = "Retry a failed download")]
    Retry {
        #[arg(required = true, help = "Job ID")]
        id: i64,
    },

    #[command(about = "Remove a download")]
    Remove {
        #[arg(required = true, help = "Job ID")]
        id: i64,

        #[arg(long, help = "Also delete downloaded file from disk")]
        delete_file: bool,
    },

    #[command(subcommand, about = "Manage download queues")]
    Queue(QueueCommand),

    #[command(subcommand, about = "Manage download categories")]
    Category(CategoryCommand),

    #[command(subcommand, about = "Daemon management commands")]
    Daemon(DaemonCommand),

    #[command(about = "Crawl and extract downloadable links from a website")]
    Grab {
        #[arg(help = "Starting URL to crawl")]
        url: String,

        #[arg(long, default_value = "1", help = "Maximum link hop depth")]
        depth: u32,

        #[arg(
            long = "ext",
            value_delimiter = ' ',
            help = "Filter files by extension(s)"
        )]
        extensions: Vec<String>,

        #[arg(long, help = "Regex pattern to filter file URLs")]
        pattern: Option<String>,

        #[arg(long, help = "Ignore robots.txt restrictions")]
        no_robots: bool,

        #[arg(long, default_value = "50", help = "Maximum HTML pages to visit")]
        max_pages: usize,

        #[arg(long, help = "Enqueue found files as active downloads")]
        add: bool,
    },

    #[command(about = "Manage and view installed plugins")]
    Plugin {
        #[command(subcommand)]
        subcommand: PluginCommands,
    },
}

#[derive(Subcommand, Debug)]
pub enum PluginCommands {
    #[command(about = "List installed plugins")]
    List,
}

#[derive(Subcommand, Debug)]
pub enum QueueCommand {
    #[command(about = "List queues")]
    List,

    #[command(about = "Create a new queue")]
    Create {
        #[arg(required = true)]
        name: String,

        #[arg(short = 'c', long = "concurrency", default_value = "3")]
        concurrency: u32,
    },

    #[command(about = "Set queue concurrency limit")]
    SetLimit {
        #[arg(required = true)]
        name: String,

        #[arg(required = true)]
        concurrency: u32,
    },

    #[command(about = "Configure queue schedule")]
    Schedule {
        #[arg(required = true)]
        name: String,

        #[arg(long = "start", required = true, help = "Start time HH:MM")]
        start: String,

        #[arg(long = "stop", help = "Stop time HH:MM")]
        stop: Option<String>,

        #[arg(long = "days", help = "Active days comma-separated (e.g. mon,tue,wed)")]
        days: Option<String>,
    },

    #[command(about = "Delete a queue")]
    Delete {
        #[arg(required = true)]
        name: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum CategoryCommand {
    #[command(about = "List categories")]
    List,

    #[command(about = "Create or update a category")]
    Set {
        #[arg(required = true)]
        name: String,

        #[arg(long = "dir", help = "Target download directory")]
        dir: Option<String>,

        #[arg(long = "exts", help = "File extensions comma-separated")]
        exts: Option<String>,
    },

    #[command(about = "Delete a category")]
    Delete {
        #[arg(required = true)]
        name: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum DaemonCommand {
    #[command(about = "Check daemon status and connectivity")]
    Status,
}
