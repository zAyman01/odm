//! Output formatting for tabular text and human-readable reports.
use odm_api::{DownloadDetailDto, ListDownloadsResponse, ListQueuesResponse};

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
