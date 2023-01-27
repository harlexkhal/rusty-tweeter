#![warn(bad_style)]
#![warn(unused)]
#![warn(unused_extern_crates)]
#![warn(unused_import_braces)]
#![warn(unused_qualifications)]
#![warn(unused_results)]

use oauth_client::{ RequestBuilder, DefaultRequestBuilder, Token };
use serde::{ Deserialize, Serialize };
use rand::Rng;
use std::{ thread, time::{ Duration, Instant }, env };
use chrono::{ Timelike, Utc };
use std::{ borrow::Cow, collections::HashMap, str };
use thiserror::Error;
use reqwest;

use base64::encode;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    #[error("OAuth error: {0}")] Oauth(#[from] oauth_client::Error),
    #[error("JSON error: {0}")] Json(#[from] serde_json::Error),
    #[error("decode string error: {0}")] FromUtf8(#[from] str::Utf8Error),
}

mod api_twitter_oauth {
    pub const REQUEST_TOKEN: &str = "https://api.twitter.com/oauth/request_token";
    pub const AUTHORIZE: &str = "https://api.twitter.com/oauth/authorize";
    pub const ACCESS_TOKEN: &str = "https://api.twitter.com/oauth/access_token";
}

mod api_twitter_soft {
    pub const UPDATE_STATUS: &str = "https://api.twitter.com/1.1/statuses/update.json";
    pub const HOME_TIMELINE: &str = "https://api.twitter.com/1.1/statuses/home_timeline.json";
    pub const UPLOAD_MEDIA: &str = "https://upload.twitter.com/1.1/media/upload.json";
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Tweet {
    pub created_at: String,
    pub text: String,
}

impl Tweet {
    pub fn parse_timeline(json_str: impl AsRef<str>) -> Result<Vec<Tweet>> {
        let tweets = serde_json::from_str(json_str.as_ref())?;
        Ok(tweets)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Media {
    pub media_id: u64,
    pub media_id_string: String,
}

impl Media {
    pub fn parse_media(json_str: impl AsRef<str>) -> Result<Media> {
        let info = serde_json::from_str(json_str.as_ref())?;
        Ok(info)
    }
}

fn split_query(query: &str) -> HashMap<Cow<'_, str>, Cow<'_, str>> {
    let mut param = HashMap::new();
    for q in query.split('&') {
        let (k, v) = q.split_once('=').unwrap();
        let _ = param.insert(k.into(), v.into());
    }
    param
}

pub fn get_request_token<RB>(
    consumer: &Token<'_>,
    client: &RB::ClientBuilder
)
    -> Result<Token<'static>>
    where RB: RequestBuilder, RB::ReturnValue: AsRef<[u8]>
{
    let bytes: RB::ReturnValue = oauth_client::get::<RB>(
        api_twitter_oauth::REQUEST_TOKEN,
        consumer,
        None,
        None,
        client
    )?;
    let resp = str::from_utf8(bytes.as_ref())?;
    let param = split_query(resp);
    let token = Token::new(
        param.get("oauth_token").unwrap().to_string(),
        param.get("oauth_token_secret").unwrap().to_string()
    );
    Ok(token)
}

pub fn get_authorize_url(request: &Token<'_>) -> String {
    format!("{}?oauth_token={}", api_twitter_oauth::AUTHORIZE, request.key)
}

pub fn get_access_token<RB>(
    consumer: &Token<'_>,
    request: &Token<'_>,
    pin: &str,
    client: &RB::ClientBuilder
)
    -> Result<Token<'static>>
    where RB: RequestBuilder, RB::ReturnValue: AsRef<[u8]>
{
    let mut param = HashMap::new();
    let _ = param.insert("oauth_verifier".into(), pin.into());
    let bytes = oauth_client::get::<RB>(
        api_twitter_oauth::ACCESS_TOKEN,
        consumer,
        Some(request),
        Some(&param),
        client
    )?;
    let resp = str::from_utf8(bytes.as_ref())?;
    let param = split_query(resp);
    let token = Token::new(
        param.get("oauth_token").unwrap().to_string(),
        param.get("oauth_token_secret").unwrap().to_string()
    );
    Ok(token)
}

pub fn update_status<RB>(
    consumer: &Token<'_>,
    access: &Token<'_>,
    status: &str,
    base64_media: &str,
    client: &RB::ClientBuilder
)
    -> Result<()>
    where RB: RequestBuilder, RB::ReturnValue: AsRef<[u8]>
{
    let mut media_param = HashMap::new();
    let _ = media_param.insert("media".into(), base64_media.into());
    let response = oauth_client::post::<RB>(
        api_twitter_soft::UPLOAD_MEDIA,
        consumer,
        Some(access),
        Some(&media_param),
        client
    )?;

    let res = str::from_utf8(response.as_ref())?;
    let ts = Media::parse_media(&res).unwrap();

    let mut param = HashMap::new();
    let _ = param.insert("status".into(), status.into());
    let _ = param.insert("media_ids".into(), ts.media_id_string.into());
    let _ = oauth_client::post::<RB>(
        api_twitter_soft::UPDATE_STATUS,
        consumer,
        Some(access),
        Some(&param),
        client
    )?;

    Ok(())
}

pub fn get_last_tweets<RB>(
    consumer: &Token<'_>,
    access: &Token<'_>,
    client: &RB::ClientBuilder
)
    -> Result<Vec<Tweet>>
    where RB: RequestBuilder, RB::ReturnValue: AsRef<[u8]>
{
    let bytes = oauth_client::get::<RB>(
        api_twitter_soft::HOME_TIMELINE,
        consumer,
        Some(access),
        None,
        client
    )?;
    let last_tweets_json = str::from_utf8(bytes.as_ref())?;
    let ts = Tweet::parse_timeline(&last_tweets_json)?;
    Ok(ts)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Meme {
    title: String,
    url: String,
    subreddit: String,
}

fn tweet_meme<'a>(status: &'a str, media: &'a str) {
    let consumer_key = env::var("CONSUMER_KEY").unwrap();
    let consumer_secret = env::var("CONSUMER_SECRET").unwrap();
    let access_key = env::var("ACCESS_KEY").unwrap();
    let access_secret = env::var("ACCESS_SECRET").unwrap();

    let consumer = Token::new(consumer_key, consumer_secret);
    let access = Token::new(access_key, access_secret);

    let _ = update_status::<DefaultRequestBuilder>(&consumer, &access, &status, &media, &());
}

#[tokio::main]
async fn main() {
    // 30 mins interval
    let wait_time = Duration::from_millis(1800000);
    loop {
        // for logging events
        let now = Utc::now();
        let (is_pm, hour) = now.hour12();
        let time = format!("{:02}:{:02}:{:02}{} - ", hour, now.minute(), now.second(), if is_pm {
            "PM"
        } else {
            "AM"
        });

        let start = Instant::now();
        let runtime = start.elapsed();

        if let Some(remaining) = wait_time.checked_sub(runtime) {
            let mut rng = rand::thread_rng();
            let random: usize = rng.gen_range(0..4);
            const TOPICS: &'static [&'static str] = &[
                "programming_memes",
                "anime_memes",
                "linux_memes",
                "programminghumor",
            ];

            let client = reqwest::Client::new();
            let response = client
                .get(format!("https://meme-api.com/gimme/{query}", query = TOPICS[random]))
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .header(reqwest::header::ACCEPT, "application/json")
                .send().await
                .unwrap();

            match response.status() {
                reqwest::StatusCode::OK => {
                    match response.json::<Meme>().await {
                        Ok(parsed) => {
                            println!("{} {:?}", time, parsed);

                            let media_response = reqwest::Client
                                ::new()
                                .get(parsed.url)
                                .send().await
                                .unwrap()
                                .bytes().await
                                .unwrap();
                            let base64_str = encode(media_response);

                            thread
                                ::spawn(move || {
                                    tweet_meme(&parsed.title, &base64_str);
                                })
                                .join()
                                .expect("Thread panicked");
                        }

                        Err(_) => println!("{} response did not match for trivia!", time),
                    }
                }
                reqwest::StatusCode::UNAUTHORIZED => {
                    println!("{} unauthorized", time);
                }
                other => {
                    panic!("{} uh oh! Something unexpected happened: {:?}", time, other);
                }
            }

            thread::sleep(remaining);
        }
    }
}

// Threaded code...sample in rust. i basically do not need threads for now.
/*
use std::{
    thread,
    time::{Duration, Instant},
};

fn main() {
    let scheduler = thread::spawn(|| {
        let wait_time = Duration::from_millis(5000);

        // Make this an infinite loop
        // Or some control path to exit the loop
        loop {
            let start = Instant::now();
            eprintln!("Scheduler starting at {:?}", start);

            let thread_a = thread::spawn(a);
           // let thread_b = thread::spawn(b);

            thread_a.join().expect("Thread A panicked");
             thread_b.join().expect("Thread B panicked");

            let runtime = start.elapsed();

            if let Some(remaining) = wait_time.checked_sub(runtime) {
                eprintln!(
                    "schedule slice has time left over; sleeping for {:?}",
                    remaining
                );
                thread::sleep(remaining);
            }
        }
    });

    scheduler.join().expect("Scheduler panicked");
}

fn a() {
    eprintln!("a");
}
fn b() {
    eprintln!("b");
}
*/
