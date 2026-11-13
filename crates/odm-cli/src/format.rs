//! Output formatting for tabular text and human-readable reports.
use odm_api::{
    DownloadDetailDto, GrabberResponse, ListDownloadsResponse, ListQueuesResponse,
    PluginListResponse,
};

pub fn format_size(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;
    const GIB: u64 = 1024 * MIB;

    if bytes >= GIB {
        format!("{:.2} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.2} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.2} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}

pub fn print_downloads_table(list: &ListDownloadsResponse) {
    if list.downloads.is_empty() {
        println!("No downloads found.");
        return;
    }

    println!(
        "{:<6} {:<12} {:<10} {:<12} {:<10} FILENAME",
        "ID", "STATUS", "PROGRESS", "DOWNLOADED", "QUEUE"
    );
    println!("{}", "-".repeat(70));

    for item in &list.downloads {
        let size_str = format_size(item.downloaded_bytes);
        let pct_str = format!("{:.1}%", item.progress_pct);
        println!(
            "{:<6} {:<12} {:<10} {:<12} {:<10} {}",
            item.id, item.state, pct_str, size_str, item.queue, item.filename
        );
    }
}

pub fn print_download_detail(detail: &DownloadDetailDto) {
    let item = &detail.item;
    println!("Download #{}", item.id);
    println!("  URL:        {}", item.url);
    println!("  Filename:   {}", item.filename);
    println!("  Directory:  {}", item.target_dir);
    println!("  Status:     {}", item.state);
    println!("  Queue:      {}", item.queue);
    println!(
        "  Progress:   {:.1}% ({} / {})",
        item.progress_pct,
        format_size(item.downloaded_bytes),
        item.total_bytes
            .map(format_size)
            .unwrap_or_else(|| "unknown".to_string())
    );

    if let Some(ref err) = item.error {
        println!("  Error:      {err}");
    }

    if !detail.segments.is_empty() {
        println!("  Segments ({}):", detail.segments.len());
        for s in &detail.segments {
            println!(
                "    [{}] bytes {}-{} (downloaded: {}){}",
                s.index,
                s.start,
                s.end,
                format_size(s.downloaded),
                if s.is_complete { " [done]" } else { "" }
            );
        }
    }
}

pub fn print_queues_table(list: &ListQueuesResponse) {
    if list.queues.is_empty() {
        println!("No queues found.");
        return;
    }

    println!("{:<6} {:<20} {:<15}", "ID", "NAME", "MAX CONCURRENT");
    println!("{}", "-".repeat(45));

    for q in &list.queues {
        println!("{:<6} {:<20} {:<15}", q.id, q.name, q.max_concurrent);
    }
}

pub fn print_categories_table(list: &odm_api::ListCategoriesResponse) {
    if list.categories.is_empty() {
        println!("No categories found.");
        return;
    }

    println!("{:<15} {:<25} EXTENSIONS", "NAME", "DIRECTORY");
    println!("{}", "-".repeat(60));

    for c in &list.categories {
        let exts = c.extensions.join(", ");
        println!("{:<15} {:<25} {}", c.name, c.directory, exts);
    }
}

pub fn print_grabber_table(resp: &GrabberResponse) {
    if resp.files.is_empty() {
        println!("No files found (crawled {} pages).", resp.pages_crawled);
        return;
    }

    println!(
        "{:<40} {:<25} {:<30} {:<6}",
        "URL", "FILENAME", "FOUND ON", "DEPTH"
    );
    println!("{}", "-".repeat(105));

    for file in &resp.files {
        println!(
            "{:<40} {:<25} {:<30} {:<6}",
            file.url, file.filename, file.found_on, file.depth
        );
    }
    println!(
        "\nFound {} file(s) across {} page(s).",
        resp.files.len(),
        resp.pages_crawled
    );
}

pub fn print_plugins_table(list: &PluginListResponse) {
    println!("Security notice: Plugins run with full user permissions without sandboxing.\n");
    if list.plugins.is_empty() {
        println!("No plugins found.");
        return;
    }

    println!(
        "{:<20} {:<10} {:<12} {:<30} COMMAND",
        "NAME", "VERSION", "ROLE", "PATTERNS"
    );
    println!("{}", "-".repeat(85));

    for p in &list.plugins {
        let patterns = p.url_patterns.join(", ");
        let cmd = p.command.join(" ");
        println!(
            "{:<20} {:<10} {:<12} {:<30} {}",
            p.name, p.version, p.role, patterns, cmd
        );
    }
}
