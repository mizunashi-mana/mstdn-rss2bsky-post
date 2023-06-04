use clap::{Parser, Subcommand};
use std::error::Error;
use atrium_api::com::atproto;
use atrium_api::app::bsky;
use chrono::Utc;
use std::marker::Sync;
use file_lock::FileLock;
use std::io::{Write, BufReader, BufRead};
use std::fs::{File};
use std::collections::HashSet;

mod xrpc_client;
use xrpc_client::{XrpcHttpClient, XrpcReqwestClient};

mod richtext;
use richtext::{RichTextSegment};

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

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Run {
        #[arg(long)]
        feed_url: String,

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
        Commands::Run { feed_url, atproto_identifier, atproto_password, .. } => {
            command_run(
                feed_url.to_string(),
                cli.xrpc_host.to_string(),
                atproto_identifier.to_string(),
                atproto_password.to_string(),
                cli.filelock_path.to_string(),
                cli.db_path.to_string(),
            )
        }
    }.await?;

    Ok(())
}

async fn command_run(
    feed_url: String,
    xrpc_host: String,
    atproto_identifier: String,
    atproto_password: String,
    filelock_path: String,
    db_path: String,
) -> Result<(), Box<dyn Error>> {
    use atproto::server::create_session;
    use create_session::CreateSession;

    let reqwest_client = reqwest::Client::new();

    let channel = fetch_channel(&reqwest_client, feed_url).await?;

    let mut client = XrpcReqwestClient::new(
        xrpc_host,
        reqwest_client,
    );
    let session = client.create_session(create_session::Input {
        identifier: atproto_identifier,
        password: atproto_password,
    }).await?;
    client.set_session(session.access_jwt, session.did);

    post_items(&client, &channel, &filelock_path, &db_path).await?;

    write_done_links_on_db(&db_path, &channel).await?;

    Ok(())
}

async fn fetch_channel(client: &reqwest::Client, url: String) -> Result<rss::Channel, Box<dyn Error>> {
    let request = client.get(url).send().await?;
    let content_bytes = request.bytes().await?;
    let channel = rss::Channel::read_from(&content_bytes[..])?;
    Ok(channel)
}

async fn post_items<Client>(
    client: &Client,
    channel: &rss::Channel,
    filelock_path: &str,
    db_path: &str,
) -> Result<(), Box<dyn Error>>
    where Client: XrpcHttpClient + atproto::repo::create_record::CreateRecord + Sync
{
    let options = file_lock::FileOptions::new()
        .write(true)
        .create(true)
        .append(true)
        ;
    let mut filelock = FileLock::lock(filelock_path, false, options)
        .map_err(|err| format!("Failed to get lock: {err}"))?;
    write!(filelock.file, "{}", Utc::now().to_rfc3339())?;

    let mut done_links: HashSet<String> = HashSet::new();
    for done_link in BufReader::new(File::open(db_path)?).lines() {
        done_links.insert(done_link?);
    }

    for item in &channel.items {
        if let Some(item_post) = post_item(client, &item, &done_links).await? {
            println!(
                "orig_link={}, cid={}, uri={}",
                item_post.orig_link,
                item_post.bsky_post.cid,
                item_post.bsky_post.uri,
            );
        }
    }

    Ok(())
}

async fn write_done_links_on_db(db_path: &str, channel: &rss::Channel) -> Result<(), Box<dyn Error>> {
    println!("{:?}, {:?}", db_path, channel);
    Ok(())
}

#[derive(Debug)]
struct ItemPost {
    orig_link: String,
    bsky_post: BskyPost,
}

async fn post_item<Client>(
    client: &Client,
    item: &rss::Item,
    done_links: &HashSet<String>,
) -> Result<Option<ItemPost>, Box<dyn Error>>
    where Client: XrpcHttpClient + atproto::repo::create_record::CreateRecord + Sync
{
    use bsky::richtext::facet;

    let description = match &item.description {
        Some(content) => {
            content
        }
        None => {
            Err(Box::<dyn Error>::from("Failed to get any descriptions of the given RSS item."))?
        }
    };
    let item_link = match &item.link {
        Some(content) => {
            content
        }
        None => {
            Err(Box::<dyn Error>::from("Failed to get any links of the given RSS item."))?
        }
    };

    if done_links.contains(item_link) {
        return Ok(None);
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

                let byte_start = text.len() as i32;

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

                let byte_end = text.len() as i32;

                facets.push(facet::Main {
                    index: facet::ByteSlice {
                        byte_start,
                        byte_end,
                    },
                    features: vec![
                        facet::MainFeaturesItem::Link(Box::new(facet::Link {
                            uri: link,
                        })),
                    ],
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
    content.push_str("[マストドン投稿から]:");

    {
        let byte_start = content.len() as i32;
        content.push_str(&item_link);
        let byte_end = content.len() as i32;
        facets.push(facet::Main {
            index: facet::ByteSlice {
                byte_start,
                byte_end,
            },
            features: vec![
                facet::MainFeaturesItem::Link(Box::new(facet::Link {
                    uri: item_link.to_string(),
                })),
            ],
        });
    }

    let result = post_to_bsky(client, content, facets).await?;

    Ok(Some(ItemPost {
        orig_link: item_link.to_string(),
        bsky_post: result,
    }))
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
    where Client: XrpcHttpClient + atproto::repo::create_record::CreateRecord + Sync
{
    use atproto::repo::create_record;
    use bsky::feed::post;
    use atrium_api::records::Record;

    let target_did = match client.current_did() {
        Some(did) => {
            did
        }
        None => {
            Err(Box::<dyn Error>::from("Expected an authenticated session of the given client."))?
        }
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
