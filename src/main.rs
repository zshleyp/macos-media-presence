use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use discord_rich_presence::{
    DiscordIpc, DiscordIpcClient,
    activity::{self, Activity, ActivityType, Assets, StatusDisplayType, Timestamps},
};
use platform_dirs::AppDirs;
use reqwest::{Client, multipart};
use serde::Deserialize;
use std::{
    fs,
    io::{BufRead, BufReader},
    path::Path,
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};
use std::{path::PathBuf, sync::Arc};
use tokio::sync::Mutex;

async fn post(
    path: &str,
    client: &reqwest::Client,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let file_bytes = fs::read(path)?;
    let part = multipart::Part::bytes(file_bytes).file_name(path.to_string());

    let form = multipart::Form::new().part("file", part);

    let res = client
        //if you have another server you wanna use you can use that as well.
        //i dont implement caching since timpfiles deletes files after an hour.
        .post("https://tmpfiles.org/api/v1/upload")
        .multipart(form)
        .send()
        .await?;

    let body = res.text().await?;

    let response: Response = serde_json::from_str(&body)?;

    let url = response.data.url;

    Ok(url)
}

#[derive(Deserialize, Debug)]
struct Streamed {
    payload: MediaInfo,
}

#[derive(Deserialize, Debug, Default, Clone)]
struct MediaInfo {
    artworkData: Option<String>,
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
    playing: Option<bool>,
    timestampEpochMicros: Option<u64>,
    elapsedTimeMicros: Option<u64>,
    durationMicros: Option<u64>,
    url: Option<String>,
    bundleIdentifier: Option<String>,
    artworkMimeType: Option<String>,
}

#[derive(Deserialize, Debug)]
struct ResponseData {
    url: Option<String>,
}

#[derive(Deserialize)]
struct Response {
    data: ResponseData,
}

fn write_image(bytes: Option<&String>, img_type: Option<&String>, config_path: &PathBuf) -> String {
    if bytes.is_none() || img_type.is_none() {
        return "placeholder".to_string();
    }

    let decoded = STANDARD.decode(bytes.unwrap()).unwrap();

    let kind: Vec<&str> = img_type.unwrap().split("/").collect();

    let path = format!("{}/cover.{}", config_path.to_str().unwrap(), kind[1]);

    fs::write(&path, decoded).unwrap();

    return path;
}

async fn get_stream(
    mut media: MediaInfo,
    discord_client: Arc<Mutex<DiscordIpcClient>>,
    client: &Client,
    path: PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut stream = Command::new("/opt/homebrew/bin/media-control")
        .arg("stream")
        .arg("--micros")
        .arg("--no-diff")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    let exe = std::env::current_exe().unwrap();

    let resource_path = exe
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("Resources")
        .join("placeholder");

    let placeholder_image_bytes = fs::read_to_string(if resource_path.exists() {
        resource_path.to_str().unwrap_or("assets/placeholder")
    } else {
        "assets/placeholder"
    })
    .unwrap();

    {
        let stdout = stream.stdout.take().unwrap();
        let stdout_reader = BufReader::new(stdout);

        for line in stdout_reader.lines() {
            let pre_format = line.unwrap();
            let data: Streamed = serde_json::from_str(&pre_format).unwrap();

            let mut text: MediaInfo = data.payload;

            let url_clone = media.url.clone();

            if text.title.is_none() {
                println!("no data!");
                discord_client.lock().await.clear_activity()?;
                continue;
            };

            if text.bundleIdentifier.as_ref().unwrap() != "com.foobar2000.mac" {
                println!("not foobar2000");
                continue;
            }

            text.album = Some(text.album.unwrap_or(
                if text.title.is_none() && media.album.as_ref().is_some() {
                    media.album.as_ref().unwrap().to_string()
                } else {
                    "".to_string()
                },
            ));

            let update_image = if text.artworkData.is_none()
                || media.artworkData == text.artworkData
                || text.artworkData.as_ref().is_some_and(|img| {
                    //placeholder stores the raw bytes of the placeholder image. we do not want to
                    //upload that.
                    img.to_string() == placeholder_image_bytes
                }) {
                false
            } else {
                true
            };

            media = text;
            media.url = url_clone;

            if update_presence(&mut media, &discord_client, update_image, client, &path)
                .await
                .is_err()
            {
                println!("bruh ok");
            }
        }
    }
    stream.wait().unwrap();

    Ok(())
}

async fn update_presence(
    media: &mut MediaInfo,
    presence_client: &Arc<Mutex<DiscordIpcClient>>,
    update_image: bool,
    client: &Client,
    config_path: &PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    let path: String;

    path = write_image(
        media.artworkData.as_ref(),
        media.artworkMimeType.as_ref(),
        config_path,
    );
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();

    let timestamp = media.timestampEpochMicros.unwrap_or(now) as i64;

    let elapsed = media.elapsedTimeMicros.unwrap_or(0) as i64;

    let timestamp_start = timestamp - elapsed;

    let timestamp_end = timestamp_start + media.durationMicros.unwrap() as i64;

    let mut url: String;

    let title = media
        .title
        .clone()
        .unwrap_or("Loading title...".to_string());

    let artist = media
        .artist
        .clone()
        .unwrap_or("Loading artist...".to_string());

    let mut album = media.album.as_ref().unwrap().to_string();

    if title == album {
        album = "".to_string();
    }

    if path == "placeholder".to_string() {
        url = path;
    } else if update_image {
        url = post(&path, client)
            .await?
            .unwrap_or("placeholder".to_string());

        media.url = Some(url.clone());
    } else {
        url = media.url.clone().unwrap_or("placeholder".to_string());
    }

    if url != "placeholder".to_string() {
        url.insert_str(20, "dl/");
    }

    let timestamps: Timestamps;
    let assets: Assets;

    if media.playing.unwrap_or(true) {
        timestamps = Timestamps::new()
            .start(timestamp_start / 1_000_000)
            .end(timestamp_end / 1_000_000);
        assets = Assets::new()
            .large_image(&url)
            .large_text(album)
            .small_image("foobar2000")
            .small_text("foobar2000")
            .small_url("https://www.foobar2000.org/");
    } else {
        timestamps = Timestamps::new().start(now as i64);
        assets = Assets::new()
            .large_image(&url)
            .large_text(album)
            .small_image("pause")
            .small_text("Paused");
    }

    let activity = Activity::new()
        .details(title)
        .name("foobar2000")
        .state(artist)
        .activity_type(ActivityType::Listening)
        .timestamps(timestamps)
        .status_display_type(StatusDisplayType::State)
        .assets(assets);

    let mut presence_client_locked = presence_client.lock().await;
    if presence_client_locked
        .set_activity(activity.clone())
        .is_err()
    {
        loop {
            if presence_client_locked.reconnect().is_ok() {
                println!("reconnect successful");

                presence_client_locked.set_activity(activity.clone())?;

                break;
            }

            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app_dirs = AppDirs::new(Some("com.zshleyp.macos-presence"), false).unwrap();

    fs::create_dir_all(&app_dirs.config_dir).unwrap();

    let request_client = reqwest::Client::new();

    let mut client = DiscordIpcClient::new("1478993515869507664");
    client.connect()?;

    let client = Arc::new(Mutex::new(client));

    let stream_client = client.clone();

    let media = MediaInfo::default();

    tokio::spawn(async move {
        get_stream(media, stream_client, &request_client, app_dirs.config_dir)
            .await
            .unwrap();
    })
    .await?;

    Ok(())
}
