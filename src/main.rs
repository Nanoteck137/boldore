use std::{
    fs::File,
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};

use chrono::DateTime;
use clap::{Parser, Subcommand};
use regex::Regex;
use reqwest::blocking::Client;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Serialize, Deserialize, Clone, Debug)]
struct Chapter {
    index: usize,
    name: String,
    url: String,
    pages: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct Manga {
    chapters: Vec<Chapter>,
}

fn fetch_html(url: &str) -> String {
    let client = Client::new();

    let res = client.get(url).send().unwrap();
    res.text().unwrap()
}

fn fetch_manga(id: usize) -> Manga {
    let s = fetch_html(&format!("https://mangapill.com/manga/{}", id));
    let document = Html::parse_document(&s);

    let selector = Selector::parse("#chapters").unwrap();
    let chapters_selector = Selector::parse("a").unwrap();

    let mut chapters = Vec::new();

    let sel = document.select(&selector).next().unwrap();
    for sel in sel.select(&chapters_selector) {
        let href = sel.value().attrs().find(|i| i.0 == "href").unwrap().1;
        let name = sel.text().next().unwrap();

        chapters.push(Chapter {
            index: 0,
            name: name.to_string(),
            url: format!("https://mangapill.com{}", href.to_string()),
            pages: None,
        });
    }

    chapters.reverse();

    for (index, chapter) in chapters.iter_mut().enumerate() {
        chapter.index = index + 1;
    }

    chapters.sort_by(|l, r| l.index.cmp(&r.index));

    Manga { chapters }
}

fn fetch_chapter_data(chapter: &mut Chapter) {
    // let s = std::fs::read_to_string("chapter.html").unwrap();
    let s = fetch_html(&chapter.url);
    let document = Html::parse_document(&s);

    let selector = Selector::parse("chapter-page img").unwrap();

    let mut pages = Vec::new();

    for sel in document.select(&selector) {
        let href = sel.value().attrs().find(|i| i.0 == "data-src").unwrap().1;
        pages.push(href.to_string());
    }

    chapter.pages = Some(pages);
}

// NOTE(patrik): https://anilist.co/graphiql
const QUERY: &str = "
query ($id: Int) {
  Media(idMal: $id) {
    id
    description(asHtml: true)
    type
    format
    status(version: 2)
    genres
    title {
      romaji
      english
      native
    }
    volumes
    chapters
    coverImage {
      medium
      extraLarge
      large
      color
    }
    bannerImage
    startDate {
      year
      month
      day
    }
    endDate {
      year
      month
      day
    }
  }
}
";

fn fetch_anilist_metadata(mal_id: usize) -> serde_json::Value {
    let client = Client::new();

    let json = json!({
        "query": QUERY,
        "variables": {
            "id": mal_id
        }
    });

    let res = client
        .post("https://graphql.anilist.co/")
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .body(json.to_string())
        .send()
        .unwrap();

    let headers = res.headers();
    println!("Headers: {:#?}", headers);

    let date = headers.get("date").unwrap();
    let date = date.to_str().unwrap();
    let date = DateTime::parse_from_rfc2822(date).unwrap();
    println!("Date: {:?}", date);

    let limit = headers.get("x-ratelimit-limit").unwrap();
    let limit = limit.to_str().unwrap();
    let limit = limit.parse::<usize>().unwrap();
    println!("Limit: {}", limit);

    let remaining = headers.get("x-ratelimit-remaining").unwrap();
    let remaining = remaining.to_str().unwrap();
    let remaining = remaining.parse::<usize>().unwrap();
    println!("Remaining: {}", remaining);

    if !res.status().is_success() {
        panic!("Request Error");
    }

    let j = res.json::<serde_json::Value>().unwrap();

    j.get("data").unwrap().get("Media").unwrap().clone()
}

fn write_to_file<P>(filepath: P, content: &str)
where
    P: AsRef<Path>,
{
    let mut file = File::create(filepath).unwrap();
    file.write_all(content.as_bytes()).unwrap();
}

// Same as: https://github.com/metafates/mangal/blob/main/util/util.go
pub fn sanitize_name(name: &str) -> String {
    let rep = [
        (Regex::new(r#"[\\/<>:;"'|?!*{}#%&^+,~\s]"#).unwrap(), "_"),
        (Regex::new(r#"__+"#).unwrap(), "_"),
        (Regex::new(r#"^[_\-.]+|[_\-.]+$"#).unwrap(), ""),
    ];

    let mut name = name.to_string();

    for i in rep {
        name = i.0.replace_all(&name, i.1).to_string();
    }

    name
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long, value_name = "DIR")]
    dir: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Fetch {
        #[arg(short, long, value_name = "ID")]
        mal_id: usize,

        #[arg(short = 'p', long, value_name = "ID")]
        mangapill_id: usize,
    },
}

fn main() {
    let args = Args::parse();
    println!("Args: {:#?}", args);

    match args.command {
        Commands::Fetch {
            mal_id,
            mangapill_id,
        } => {
            let mut path = if let Some(dir) = args.dir {
                dir
            } else {
                PathBuf::new()
            };
            path.push(mal_id.to_string());

            let mut chapters_file = path.clone();
            chapters_file.push("chapters.json");

            let mut metadata_file = path.clone();
            chapters_file.push("metadata.json");

            if path.exists() && !path.is_dir() {
                panic!("'{}' already exists on the filesystem", mal_id);
            } else {
                std::fs::create_dir_all(&path).unwrap();
            }

            panic!();

            println!("Fetching anilist metadata (MAL Id: {})", mal_id);
            let metadata = fetch_anilist_metadata(mal_id);
            let s = serde_json::to_string_pretty(&metadata).unwrap();
            write_to_file(metadata_file, &s);

            println!("Fetching manga: {}", mangapill_id);
            let mut manga = fetch_manga(mangapill_id);
            for chapter in manga.chapters.iter_mut() {
                println!("Fetching chapter: {}", chapter.index);
                fetch_chapter_data(chapter);
                std::thread::sleep(Duration::from_millis(50));
            }

            let s = serde_json::to_string_pretty(&manga.chapters).unwrap();
            write_to_file(chapters_file, &s);
        }
    }
}
