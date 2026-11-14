//! Command-line client for the odm download manager daemon.
#![forbid(unsafe_code)]

mod client;
mod commands;
mod format;

use clap::Parser;
use commands::{
    AddArgs, CategoryCommand, Cli, Command, DaemonCommand, PluginCommands, QueueCommand,
};
use odm_api::{AddDownloadRequest, ChecksumAlgorithm, ChecksumDto, GrabberRequest, JobOptionsDto};

use crate::client::DaemonClient;
use crate::format::{
    print_categories_table, print_download_detail, print_downloads_table, print_grabber_table,
    print_plugins_table, print_queues_table,
};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let json = cli.json;
    let client = DaemonClient::new(cli.server, cli.token, cli.config_dir);

    let result = run_command(cli.command, &client, json).await;

    if let Err(err) = result {
        if json {
            println!("{}", serde_json::json!({ "error": err }));
        } else {
            eprintln!("Error: {err}");
        }
        std::process::exit(1);
    }
}

/// One download the `add` command will request.
struct AddItem {
    url: String,
    /// A name the source dictates, such as a metalink file name.
    filename: Option<String>,
    mirrors: Vec<String>,
    checksum: Option<ChecksumDto>,
}

impl AddItem {
    fn plain(url: String) -> Self {
        Self {
            url,
            filename: None,
            mirrors: Vec::new(),
            checksum: None,
        }
    }
}

async fn run_command(command: Command, client: &DaemonClient, json: bool) -> Result<(), String> {
    match command {
        Command::Daemon(DaemonCommand::Status) => match client.health().await {
            Ok(health) => {
                if json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "status": "ok",
                            "version": health.version,
                            "server": client.server_url(),
                        })
                    );
                } else {
                    println!(
                        "odm-daemon is running (version: {}, status: ok) at {}",
                        health.version,
                        client.server_url()
                    );
                }
                Ok(())
            }
            Err(err) => {
                if json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "status": "offline",
                            "server": client.server_url(),
                            "error": err,
                        })
                    );
                } else {
                    eprintln!(
                        "odm-daemon is offline or unreachable at {} ({err})",
                        client.server_url()
                    );
                }
                Err("daemon offline".to_string())
            }
        },

        Command::Add(args) => {
            let AddArgs {
                urls,
                dir,
                output,
                segments,
                limit,
                queue,
                headers,
                checksum,
                mirror,
                metalink,
                category,
                torrent_file,
                select_files,
                seed_ratio,
                quality,
                ytdlp,
            } = *args;
            let mut items: Vec<AddItem> = Vec::new();

            if let Some(torrent_path) = torrent_file {
                // The daemon runs elsewhere, so a relative path would point nowhere.
                let absolute = std::path::absolute(&torrent_path)
                    .map_err(|e| format!("Cannot resolve torrent file '{torrent_path}': {e}"))?;
                items.push(AddItem::plain(absolute.to_string_lossy().into_owned()));
            }

            if let Some(metalink_path) = metalink {
                let xml = std::fs::read_to_string(&metalink_path)
                    .map_err(|e| format!("Failed to read metalink file '{metalink_path}': {e}"))?;
                let specs = odm_core::metalink::parse_metalink(&xml)
                    .map_err(|e| format!("Failed to parse metalink: {e}"))?;
                for spec in specs {
                    let mut file_urls = spec.urls.into_iter();
                    let Some(url) = file_urls.next() else {
                        continue;
                    };
                    items.push(AddItem {
                        url,
                        filename: Some(spec.filename),
                        mirrors: file_urls.collect(),
                        checksum: spec.checksum.map(|cs| ChecksumDto {
                            algorithm: match cs.algorithm {
                                odm_core::checksum::ChecksumAlgorithm::Sha256 => {
                                    ChecksumAlgorithm::Sha256
                                }
                                odm_core::checksum::ChecksumAlgorithm::Sha1 => {
                                    ChecksumAlgorithm::Sha1
                                }
                                odm_core::checksum::ChecksumAlgorithm::Md5 => {
                                    ChecksumAlgorithm::Md5
                                }
                            },
                            digest: cs.expected,
                        }),
                    });
                }
            }

            for url in urls {
                match odm_core::batch::expand_batch_url(&url) {
                    Ok(expanded) => items.extend(expanded.into_iter().map(AddItem::plain)),
                    Err(_) => items.push(AddItem::plain(url)),
                }
            }

            if items.is_empty() {
                return Err("No URLs specified".to_string());
            }

            // --mirror and --checksum describe one file, so they need exactly one.
            if (!mirror.is_empty() || checksum.is_some()) && items.len() != 1 {
                return Err(format!(
                    "--mirror and --checksum apply to a single URL, but {} were given; add them one at a time",
                    items.len()
                ));
            }
            if let Some(only) = items.first_mut() {
                only.mirrors.extend(mirror);
                if let Some(cs) = checksum {
                    only.checksum = Some(
                        cs.parse::<ChecksumDto>()
                            .map_err(|e| format!("Invalid checksum: {e}"))?,
                    );
                }
            }

            let parsed_headers = if headers.is_empty() {
                None
            } else {
                let mut h_vec = Vec::new();
                for h in headers {
                    if let Some((k, v)) = h.split_once(':') {
                        h_vec.push((k.trim().to_string(), v.trim().to_string()));
                    }
                }
                Some(h_vec)
            };

            let parsed_selected_files = if let Some(sf) = select_files {
                let mut indices = Vec::new();
                for part in sf.split(',') {
                    let trimmed = part.trim();
                    if !trimmed.is_empty() {
                        let idx: u32 = trimmed
                            .parse()
                            .map_err(|e| format!("Invalid file index '{trimmed}': {e}"))?;
                        indices.push(idx);
                    }
                }
                Some(indices)
            } else {
                None
            };

            let shared_options = JobOptionsDto {
                segments,
                speed_limit: limit,
                headers: parsed_headers,
                selected_files: parsed_selected_files,
                seed_ratio,
                quality,
                ..JobOptionsDto::default()
            };

            let single = items.len() == 1;
            for item in items {
                let filename = item
                    .filename
                    .clone()
                    .or_else(|| if single { output.clone() } else { None });
                let options = JobOptionsDto {
                    checksum: item.checksum.clone(),
                    mirrors: (!item.mirrors.is_empty()).then(|| item.mirrors.clone()),
                    ..shared_options.clone()
                };
                let req = AddDownloadRequest {
                    url: item.url.clone(),
                    target_dir: dir.clone(),
                    filename,
                    queue: queue.clone(),
                    category: category.clone(),
                    options: (options != JobOptionsDto::default()).then_some(options),
                    backend: ytdlp.then(|| "ytdlp".to_string()),
                };

                let resp = client.add_download(&req).await?;
                if json {
                    println!("{}", serde_json::json!({ "id": resp.id, "url": item.url }));
                } else {
                    println!("Added download #{} ({})", resp.id, item.url);
                }
            }
            Ok(())
        }

        Command::List { state, queue } => {
            let list = client
                .list_downloads(state.as_deref(), queue.as_deref())
                .await?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&list).map_err(|e| e.to_string())?
                );
            } else {
                print_downloads_table(&list);
            }
            Ok(())
        }

        Command::Info { id } => {
            let detail = client.get_download(id).await?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&detail).map_err(|e| e.to_string())?
                );
            } else {
                print_download_detail(&detail);
            }
            Ok(())
        }

        Command::Pause { id } => {
            client.pause_download(id).await?;
            if json {
                println!("{}", serde_json::json!({ "status": "paused", "id": id }));
            } else {
                println!("Paused download #{id}");
            }
            Ok(())
        }

        Command::Resume { id } => {
            client.resume_download(id).await?;
            if json {
                println!("{}", serde_json::json!({ "status": "resumed", "id": id }));
            } else {
                println!("Resumed download #{id}");
            }
            Ok(())
        }

        Command::Cancel { id } => {
            client.cancel_download(id).await?;
            if json {
                println!("{}", serde_json::json!({ "status": "cancelled", "id": id }));
            } else {
                println!("Cancelled download #{id}");
            }
            Ok(())
        }

        Command::Retry { id } => {
            client.retry_download(id).await?;
            if json {
                println!("{}", serde_json::json!({ "status": "retrying", "id": id }));
            } else {
                println!("Retried download #{id}");
            }
            Ok(())
        }

        Command::Remove { id, delete_file } => {
            client.delete_download(id, delete_file).await?;
            if json {
                println!("{}", serde_json::json!({ "status": "removed", "id": id }));
            } else {
                println!("Removed download #{id}");
            }
            Ok(())
        }

        Command::Queue(queue_cmd) => match queue_cmd {
            QueueCommand::List => {
                let list = client.list_queues().await?;
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&list).map_err(|e| e.to_string())?
                    );
                } else {
                    print_queues_table(&list);
                }
                Ok(())
            }
            QueueCommand::Create { name, concurrency } => {
                let q = client.create_queue(&name, concurrency).await?;
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&q).map_err(|e| e.to_string())?
                    );
                } else {
                    println!(
                        "Created queue '{}' with max concurrency {}",
                        q.name, q.max_concurrent
                    );
                }
                Ok(())
            }
            QueueCommand::SetLimit { name, concurrency } => {
                let q = client.set_queue_limit(&name, concurrency).await?;
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&q).map_err(|e| e.to_string())?
                    );
                } else {
                    println!(
                        "Updated queue '{}' max concurrency to {}",
                        q.name, q.max_concurrent
                    );
                }
                Ok(())
            }
            QueueCommand::Schedule {
                name,
                start,
                stop,
                days,
            } => {
                let days_vec = days.map(|d| {
                    d.split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<String>>()
                });
                let q = client
                    .update_queue_schedule(&name, &start, stop.as_deref(), days_vec.as_deref())
                    .await?;
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&q).map_err(|e| e.to_string())?
                    );
                } else {
                    println!(
                        "Updated schedule for queue '{}': start={}, stop={:?}, days={:?}",
                        q.name,
                        q.schedule_start.unwrap_or_default(),
                        q.schedule_stop,
                        q.schedule_days.unwrap_or_default()
                    );
                }
                Ok(())
            }
            QueueCommand::Delete { name } => {
                client.delete_queue(&name).await?;
                if json {
                    println!(
                        "{}",
                        serde_json::json!({ "status": "deleted", "name": name })
                    );
                } else {
                    println!("Deleted queue '{name}'");
                }
                Ok(())
            }
        },

        Command::Category(category_cmd) => match category_cmd {
            CategoryCommand::List => {
                let list = client.list_categories().await?;
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&list).map_err(|e| e.to_string())?
                    );
                } else {
                    print_categories_table(&list);
                }
                Ok(())
            }
            CategoryCommand::Set { name, dir, exts } => {
                let exts_vec = exts.map(|e| {
                    e.split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<String>>()
                });
                let cat = client
                    .set_category(&name, dir.as_deref(), exts_vec.as_deref())
                    .await?;
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&cat).map_err(|e| e.to_string())?
                    );
                } else {
                    println!("Set category '{}'", cat.name);
                }
                Ok(())
            }
            CategoryCommand::Delete { name } => {
                client.delete_category(&name).await?;
                if json {
                    println!(
                        "{}",
                        serde_json::json!({ "status": "deleted", "category": name })
                    );
                } else {
                    println!("Deleted category '{name}'");
                }
                Ok(())
            }
        },

        Command::Grab {
            url,
            depth,
            extensions,
            pattern,
            no_robots,
            max_pages,
            add,
        } => {
            let extensions_opt = if extensions.is_empty() {
                None
            } else {
                let mut flat = Vec::new();
                for e in extensions {
                    for part in e.split(',') {
                        let trimmed = part.trim().trim_start_matches('.');
                        if !trimmed.is_empty() {
                            flat.push(trimmed.to_string());
                        }
                    }
                }
                if flat.is_empty() { None } else { Some(flat) }
            };

            let req = GrabberRequest {
                url,
                depth: Some(depth),
                extensions: extensions_opt,
                pattern,
                respect_robots: Some(!no_robots),
                max_pages: Some(max_pages),
            };

            let resp = client.grab(&req).await?;

            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&resp).map_err(|e| e.to_string())?
                );
            } else {
                print_grabber_table(&resp);
            }

            if add {
                for file in &resp.files {
                    let add_req = AddDownloadRequest {
                        url: file.url.clone(),
                        target_dir: None,
                        filename: Some(file.filename.clone()),
                        queue: None,
                        category: None,
                        options: None,
                        backend: None,
                    };
                    let add_resp = client.add_download(&add_req).await?;
                    if !json {
                        println!("Added download #{} ({})", add_resp.id, file.url);
                    }
                }
            }

            Ok(())
        }

        Command::Plugin { subcommand } => match subcommand {
            PluginCommands::List => {
                let list = client.list_plugins().await?;
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&list).map_err(|e| e.to_string())?
                    );
                } else {
                    print_plugins_table(&list);
                }
                Ok(())
            }
        },
    }
}
