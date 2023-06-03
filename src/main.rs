use clap::{Parser, Subcommand};
use std::error::Error;
use atrium_api::com::atproto;
use atrium_api::app::bsky;
use chrono::Utc;
use std::marker::Sync;

mod xrpc_client;
use xrpc_client::{XrpcHttpClient, XrpcReqwestClient};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Turn debugging information on
    #[arg(short, long, action = clap::ArgAction::Count)]
    debug: u8,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Run,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let channel = fetch_channel(String::from("https://mstdn.mizunashi.work/@mizunashi_mana.rss")).await?;
    for item in channel.items {
        post_item(item).await?;
    };

    use atproto::server::create_session;
    use create_session::CreateSession;

    let mut client = XrpcReqwestClient::new(
        String::from("https://bsky.social"),
        reqwest::Client::new(),
    );
    let session = client.create_session(create_session::Input {
        identifier: String::from("identifier"),
        password: String::from("password"),
    }).await?;
    client.set_session(session.access_jwt, session.did);
    post_to_bsky(&client, String::from("atproto.createRecord API test")).await?;
    Ok(())
}

async fn fetch_channel(url: String) -> Result<rss::Channel, Box<dyn Error>> {
    let request = reqwest::get(url).await?;
    let content_bytes = request.bytes().await?;
    let channel = rss::Channel::read_from(&content_bytes[..])?;
    Ok(channel)
}

async fn post_item(item: rss::Item) -> Result<(), Box<dyn Error>> {
    println!("{:?}", item);
    Ok(())
}

async fn post_to_bsky<Client>(client: &Client, text: String) -> Result<(), Box<dyn Error>>
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
            facets: None,
            reply: None,
            text: text,
        })),
        repo: String::from(target_did),
        rkey: None,
        swap_commit: None,
        validate: None,
    };

    let result = client.create_record(input).await?;
    println!("{:?}", result);
    Ok(())
}
