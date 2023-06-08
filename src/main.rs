use atrium_api::app::bsky;
use atrium_api::com::atproto;
use chrono::Utc;
use clap::{Parser, Subcommand};
use file_lock::FileLock;
use std::collections::HashSet;
use std::error::Error;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::marker::Sync;
use std::collections::VecDeque;

mod xrpc_client;
use xrpc_client::{XrpcHttpClient, XrpcReqwestClient};

mod richtext;
use richtext::RichTextSegment;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Turn debugging information on
    #[arg(short, long, action = clap::ArgAction::Count)]
    debug: u8,

    #[arg(long, default_value_t = String::from("https://bsky.social"), env = "XRPC_HOST")]
    xrpc_host: String,

    #[arg(long)]
    filelock_path: String,

    #[arg(long)]
    db_path: String,

    #[arg(long, default_value_t = 50)]
    min_save_posts: usize,

    #[arg(long, default_value_t = false)]
    dry_run: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Run {
        #[arg(long)]
        feed_url: String,

        #[arg(long, default_value_t = String::from("[マストドン投稿から]:"))]
        original_link_prefix: String,

        #[arg(long, env = "ATPROTO_IDENTIFIER")]
        atproto_identifier: String,

        #[arg(long, env = "ATPROTO_PASSWORD")]
        atproto_password: String,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();

    match &cli.command {
        Commands::Run {
            feed_url,
            atproto_identifier,
            atproto_password,
            original_link_prefix,
            ..
        } => command_run(
            cli.dry_run,
            feed_url.to_string(),
            cli.xrpc_host.to_string(),
            atproto_identifier.to_string(),
            atproto_password.to_string(),
            original_link_prefix.to_string(),
            cli.filelock_path.to_string(),
            cli.db_path.to_string(),
            cli.min_save_posts,
        ),
    }
    .await?;

    Ok(())
}

async fn command_run(
    dry_run: bool,
    feed_url: String,
    xrpc_host: String,
    atproto_identifier: String,
    atproto_password: String,
    original_link_prefix: String,
    filelock_path: String,
    db_path: String,
    min_save_posts: usize,
) -> Result<(), Box<dyn Error>> {
    use atproto::server::create_session;
    use create_session::CreateSession;

    let reqwest_client = reqwest::Client::new();

    let items = fetch_items(dry_run, &reqwest_client, feed_url).await?;

    let mut client = XrpcReqwestClient::new(xrpc_host, reqwest_client, dry_run);
    if dry_run {
        println!("Dry run: authenticate by {atproto_identifier}");
    } else {
        let session = client
            .create_session(create_session::Input {
                identifier: atproto_identifier,
                password: atproto_password,
            })
            .await?;
        client.set_session(session.access_jwt, session.did);
    }

    post_items(
        dry_run,
        &client,
        &items,
        &original_link_prefix,
        &filelock_path,
        &db_path,
        min_save_posts,
    )
    .await?;

    Ok(())
}

async fn fetch_items(
    dry_run: bool,
    client: &reqwest::Client,
    feed_url: String
) -> Result<Vec<rss::Item>, Box<dyn Error>> {
    if dry_run {
        Ok(vec![])
    } else {
        let channel = fetch_channel(&client, feed_url).await?;
        Ok(channel.items)
    }
}

async fn fetch_channel(
    client: &reqwest::Client,
    url: String,
) -> Result<rss::Channel, Box<dyn Error>> {
    let request = client.get(url).send().await?;
    let content_bytes = request.bytes().await?;
    let channel = rss::Channel::read_from(&content_bytes[..])?;
    Ok(channel)
}

async fn post_items<Client>(
    dry_run: bool,
    client: &Client,
    items: &[rss::Item],
    original_link_prefix: &str,
    filelock_path: &str,
    db_path: &str,
    min_save_posts: usize,
) -> Result<(), Box<dyn Error>>
where
    Client: XrpcHttpClient + atproto::repo::create_record::CreateRecord + Sync,
{
    if dry_run {
        println!("Dry run: create DB file if not exists.");
    } else {
        let mut append_db_file = OpenOptions::new()
            .create(true)
            .write(true)
            .append(true)
            .open(db_path)
            .map_err(|err| format!("Failed to open DB: {err}"))?;
        append_db_file.write(&vec![])?;
    }

    if dry_run {
        println!("Dry run: lock and post items.");
    } else {
        let mut filelock = FileLock::lock(
            filelock_path,
            false,
            file_lock::FileOptions::new()
                .write(true)
                .create(true)
                .truncate(true),
        )
        .map_err(|err| format!("Failed to get lock: {err}"))?;
        writeln!(filelock.file, "{}", Utc::now().to_rfc3339())
            .map_err(|err| format!("Failed to write lock: {err}"))?;

        let mut links_for_save: Vec<String> = vec![];
        let done_links = {
            let mut done_links: HashSet<String> = HashSet::new();
            let mut done_links_for_save: VecDeque<String> = VecDeque::new();
            let db_file = OpenOptions::new()
                .read(true)
                .open(db_path)
                .map_err(|err| format!("Failed to open DB: {err}"))?;
            for done_link in BufReader::new(db_file).lines() {
                let done_link = done_link?;
                done_links.insert(done_link.to_string());
                done_links_for_save.push_back(done_link);
                if done_links_for_save.len() > min_save_posts {
                    done_links_for_save.pop_front();
                }
            }
            for done_link_for_save in done_links_for_save.iter_mut() {
                links_for_save.push(done_link_for_save.to_string());
            }
            done_links
        };

        {
            let mut append_db_file = OpenOptions::new()
                .create(true)
                .write(true)
                .append(true)
                .open(db_path)
                .map_err(|err| format!("Failed to open DB: {err}"))?;
            for item in items.iter().rev() {
                let item_post = post_item(client, &item, original_link_prefix, &done_links).await?;
                match item_post.bsky_post_opt {
                    None => {
                        println!(
                            "orig_link={}: Already posted to Bluesky.",
                            item_post.orig_link,
                        );
                    }
                    Some(bsky_post) => {
                        println!(
                            "orig_link={}: Posted to Bluesky: cid={}, uri={}",
                            item_post.orig_link, bsky_post.cid, bsky_post.uri,
                        );
                        writeln!(append_db_file, "{}", &item_post.orig_link)
                            .map_err(|err| format!("Failed to write DB: {err}"))?;
                        append_db_file
                            .flush()
                            .map_err(|err| format!("Failed to flush DB: {err}"))?;
                        links_for_save.push(item_post.orig_link);
                    }
                }
            }
        }

        {
            let mut write_db_file = OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(db_path)
                .map_err(|err| format!("Failed to open DB: {err}"))?;
            for link_for_save in links_for_save {
                writeln!(write_db_file, "{}", link_for_save)
                    .map_err(|err| format!("Failed to write DB: {err}"))?;
            }
        }
    }

    Ok(())
}

#[derive(Debug)]
struct ItemPost {
    orig_link: String,
    bsky_post_opt: Option<BskyPost>,
}

async fn post_item<Client>(
    client: &Client,
    item: &rss::Item,
    original_link_prefix: &str,
    done_links: &HashSet<String>,
) -> Result<ItemPost, Box<dyn Error>>
where
    Client: XrpcHttpClient + atproto::repo::create_record::CreateRecord + Sync,
{
    use bsky::richtext::facet;

    let description = match &item.description {
        Some(content) => content,
        None => Err(Box::<dyn Error>::from(
            "Failed to get any descriptions of the given RSS item.",
        ))?,
    };
    let item_link = match &item.link {
        Some(content) => content,
        None => Err(Box::<dyn Error>::from(
            "Failed to get any links of the given RSS item.",
        ))?,
    };

    if done_links.contains(item_link) {
        return Ok(ItemPost {
            orig_link: item_link.to_string(),
            bsky_post_opt: None,
        });
    }

    let mut content = String::from("");
    let limit_count = 200 - 3;
    let mut need_truncate = false;
    let mut content_count = 0;
    let mut facets: Vec<facet::Main> = vec![];
    for seg in richtext::from_html(description.as_str())? {
        match seg {
            RichTextSegment::PlainText { text } => {
                let text_count = text.chars().count();

                if content_count + text_count > limit_count {
                    for c in text.chars().take(limit_count) {
                        content.push(c);
                    }
                    need_truncate = true;
                    content_count += limit_count;
                } else {
                    content.push_str(&text);
                    content_count += text_count;
                }

                if need_truncate {
                    break;
                }
            }
            RichTextSegment::Link { text, link } => {
                let text_count = text.chars().count();

                let byte_start = content.len() as i32;

                if content_count + text_count > limit_count {
                    for c in text.chars().take(limit_count) {
                        content.push(c);
                    }
                    need_truncate = true;
                    content_count += limit_count;
                } else {
                    content.push_str(&text);
                    content_count += text_count;
                }

                let byte_end = content.len() as i32;

                facets.push(facet::Main {
                    index: facet::ByteSlice {
                        byte_start,
                        byte_end,
                    },
                    features: vec![facet::MainFeaturesItem::Link(Box::new(facet::Link {
                        uri: link,
                    }))],
                });

                if need_truncate {
                    break;
                }
            }
        }
    }

    if need_truncate {
        content.push_str("...");
    }
    content.push_str("\n");
    content.push_str(original_link_prefix);

    {
        let byte_start = content.len() as i32;
        content.push_str(&item_link);
        let byte_end = content.len() as i32;
        facets.push(facet::Main {
            index: facet::ByteSlice {
                byte_start,
                byte_end,
            },
            features: vec![facet::MainFeaturesItem::Link(Box::new(facet::Link {
                uri: item_link.to_string(),
            }))],
        });
    }

    let result = post_to_bsky(client, content, facets).await?;

    Ok(ItemPost {
        orig_link: item_link.to_string(),
        bsky_post_opt: Some(result),
    })
}

#[derive(Debug)]
struct BskyPost {
    cid: String,
    uri: String,
}

async fn post_to_bsky<Client>(
    client: &Client,
    text: String,
    facets: Vec<bsky::richtext::facet::Main>,
) -> Result<BskyPost, Box<dyn Error>>
where
    Client: XrpcHttpClient + atproto::repo::create_record::CreateRecord + Sync,
{
    use atproto::repo::create_record;
    use atrium_api::records::Record;
    use bsky::feed::post;

    let target_did = match client.current_did() {
        Some(did) => did,
        None => Err(Box::<dyn Error>::from(
            "Expected an authenticated session of the given client.",
        ))?,
    };

    let input = create_record::Input {
        collection: String::from("app.bsky.feed.post"),
        record: Record::AppBskyFeedPost(Box::new(post::Record {
            created_at: Utc::now().to_rfc3339(),
            embed: None,
            entities: None,
            facets: Some(facets),
            reply: None,
            text: text,
        })),
        repo: String::from(target_did),
        rkey: None,
        swap_commit: None,
        validate: None,
    };

    let result = client.create_record(input).await?;
    Ok(BskyPost {
        cid: result.cid,
        uri: result.uri,
    })
}
