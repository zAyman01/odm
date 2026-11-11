//! Command-line client for the odm download manager daemon.
#![forbid(unsafe_code)]

mod client;
mod commands;
mod format;

use clap::Parser;
use commands::{Cli, Command, DaemonCommand, QueueCommand};
use odm_api::{AddDownloadRequest, JobOptionsDto};

use crate::client::DaemonClient;
use crate::format::{print_download_detail, print_downloads_table, print_queues_table};

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
        } => {
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

            let options = if segments.is_some() || limit.is_some() || parsed_headers.is_some() {
                Some(JobOptionsDto {
                    segments,
                    speed_limit: limit,
                    headers: parsed_headers,
                    user_agent: None,
                    referer: None,
                    cookies: None,
                    checksum: None,
                    mirrors: None,
                })
            } else {
                None
            };

            let single = urls.len() == 1;
            for url in urls {
                let filename = if single { output.clone() } else { None };
                let req = AddDownloadRequest {
                    url: url.clone(),
                    target_dir: dir.clone(),
                    filename,
                    queue: queue.clone(),
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
    }
}
