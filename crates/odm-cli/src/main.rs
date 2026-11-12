//! Command-line client for the odm download manager daemon.
#![forbid(unsafe_code)]

mod client;
mod commands;
mod format;

use clap::Parser;
use commands::{CategoryCommand, Cli, Command, DaemonCommand, QueueCommand};
use odm_api::{AddDownloadRequest, ChecksumAlgorithm, ChecksumDto, JobOptionsDto};

use crate::client::DaemonClient;
use crate::format::{
    print_categories_table, print_download_detail, print_downloads_table, print_queues_table,
};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let json = cli.json;
    let client = DaemonClient::new(cli.server, cli.token, cli.config_dir);

    let result = run_command(cli.command, &client, json).await;

    if let Err(err) = result {
        if json {
            println!(
                r#"{{"error":{}}}"#,
                serde_json::to_string(&err).unwrap_or_default()
            );
        } else {
            eprintln!("Error: {err}");
        }
        std::process::exit(1);
    }
}

async fn run_command(command: Command, client: &DaemonClient, json: bool) -> Result<(), String> {
    match command {
        Command::Daemon(DaemonCommand::Status) => match client.health().await {
            Ok(health) => {
                if json {
                    println!(
                        r#"{{"status":"ok","version":"{}","server":"{}"}}"#,
                        health.version,
                        client.server_url()
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
                        r#"{{"status":"offline","server":"{}","error":{}}}"#,
                        client.server_url(),
                        serde_json::to_string(&err).unwrap_or_default()
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

        Command::Add {
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
        } => {
            let mut final_urls = Vec::new();
            let mut metalink_checksum = None;
            let mut all_mirrors = mirror;

            if let Some(metalink_path) = metalink {
                let xml = std::fs::read_to_string(&metalink_path)
                    .map_err(|e| format!("Failed to read metalink file '{metalink_path}': {e}"))?;
                let spec = odm_core::metalink::parse_metalink(&xml)
                    .map_err(|e| format!("Failed to parse metalink: {e}"))?;
                if let Some(first_url) = spec.urls.first() {
                    final_urls.push(first_url.clone());
                    for m in spec.urls.iter().skip(1) {
                        all_mirrors.push(m.clone());
                    }
                }
                if let Some(ref cs) = spec.checksum {
                    let algorithm = match cs.algorithm {
                        odm_core::checksum::ChecksumAlgorithm::Sha256 => ChecksumAlgorithm::Sha256,
                        odm_core::checksum::ChecksumAlgorithm::Sha1 => ChecksumAlgorithm::Sha1,
                        odm_core::checksum::ChecksumAlgorithm::Md5 => ChecksumAlgorithm::Md5,
                    };
                    metalink_checksum = Some(ChecksumDto {
                        algorithm,
                        digest: cs.expected.clone(),
                    });
                }
            }

            for url in urls {
                match odm_core::batch::expand_batch_url(&url) {
                    Ok(expanded) => final_urls.extend(expanded),
                    Err(_) => final_urls.push(url),
                }
            }

            if final_urls.is_empty() {
                return Err("No URLs specified".to_string());
            }

            let parsed_checksum = if let Some(ref cs) = checksum {
                Some(
                    cs.parse::<ChecksumDto>()
                        .map_err(|e| format!("Invalid checksum: {e}"))?,
                )
            } else {
                metalink_checksum
            };

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

            let mirrors_opt = if all_mirrors.is_empty() {
                None
            } else {
                Some(all_mirrors)
            };

            let options = if segments.is_some()
                || limit.is_some()
                || parsed_headers.is_some()
                || parsed_checksum.is_some()
                || mirrors_opt.is_some()
            {
                Some(JobOptionsDto {
                    segments,
                    speed_limit: limit,
                    headers: parsed_headers,
                    user_agent: None,
                    referer: None,
                    cookies: None,
                    checksum: parsed_checksum,
                    mirrors: mirrors_opt,
                    ..Default::default()
                })
            } else {
                None
            };

            let single = final_urls.len() == 1;
            for url in final_urls {
                let filename = if single { output.clone() } else { None };
                let req = AddDownloadRequest {
                    url: url.clone(),
                    target_dir: dir.clone(),
                    filename,
                    queue: queue.clone(),
                    category: category.clone(),
                    options: options.clone(),
                };

                let resp = client.add_download(&req).await?;
                if json {
                    println!(
                        r#"{{"id":{},"url":{}}}"#,
                        resp.id,
                        serde_json::to_string(&url).unwrap_or_default()
                    );
                } else {
                    println!("Added download #{} ({url})", resp.id);
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
                println!(r#"{{"status":"paused","id":{id}}}"#);
            } else {
                println!("Paused download #{id}");
            }
            Ok(())
        }

        Command::Resume { id } => {
            client.resume_download(id).await?;
            if json {
                println!(r#"{{"status":"resumed","id":{id}}}"#);
            } else {
                println!("Resumed download #{id}");
            }
            Ok(())
        }

        Command::Cancel { id } => {
            client.cancel_download(id).await?;
            if json {
                println!(r#"{{"status":"cancelled","id":{id}}}"#);
            } else {
                println!("Cancelled download #{id}");
            }
            Ok(())
        }

        Command::Retry { id } => {
            client.retry_download(id).await?;
            if json {
                println!(r#"{{"status":"retrying","id":{id}}}"#);
            } else {
                println!("Retried download #{id}");
            }
            Ok(())
        }

        Command::Remove { id, delete_file } => {
            client.delete_download(id, delete_file).await?;
            if json {
                println!(r#"{{"status":"removed","id":{id}}}"#);
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
                        r#"{{"status":"deleted","name":{}}}"#,
                        serde_json::to_string(&name).unwrap_or_default()
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
                        r#"{{"status":"deleted","category":{}}}"#,
                        serde_json::to_string(&name).unwrap_or_default()
                    );
                } else {
                    println!("Deleted category '{name}'");
                }
                Ok(())
            }
        },
    }
}
