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

#[derive(Subcommand, Debug)]
pub enum Command {
    #[command(about = "Add one or more downloads")]
    Add {
        #[arg(required = true, help = "One or more URLs to download")]
        urls: Vec<String>,

        #[arg(short = 'd', long = "dir", help = "Target download directory")]
        dir: Option<String>,

        #[arg(
            short = 'o',
            long = "output",
            help = "Custom filename (only for single URL)"
        )]
        output: Option<String>,

        #[arg(short = 's', long = "segments", help = "Number of segments (1-32)")]
        segments: Option<u32>,

        #[arg(short = 'l', long = "limit", help = "Speed limit in bytes/sec")]
        limit: Option<u64>,

        #[arg(short = 'q', long = "queue", help = "Target queue name")]
        queue: Option<String>,

        #[arg(short = 'H', long = "header", help = "Custom header 'Key: Value'")]
        headers: Vec<String>,
    },

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

    #[command(subcommand, about = "Daemon management commands")]
    Daemon(DaemonCommand),
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

    #[command(about = "Delete a queue")]
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
