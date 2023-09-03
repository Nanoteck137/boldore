use std::{
    collections::VecDeque,
    fs::File,
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use clap::{Parser, Subcommand};
use regex::Regex;
use reqwest::blocking::Client;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use swadloon::{
    anilist::{self, fetch_anilist_metadata, Metadata},
    ChapterEntry,
};

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

#[derive(Debug)]
struct SearchResult {
    id: usize,
    name: String,
}

fn search(query: &str) -> Vec<SearchResult> {
    // let s = std::fs::read_to_string("search.html").unwrap();
    let query = urlencoding::encode(query);
    let s = fetch_html(&format!(
        "https://mangapill.com/search?q={}&type=manga&status=",
        query
    ));
    let document = Html::parse_document(&s);

    let selector = Selector::parse("body > div.container.py-3 > div.my-3.grid.justify-end.gap-3.grid-cols-2.md\\:grid-cols-3.lg\\:grid-cols-5 > div").unwrap();

    let a_selector = Selector::parse("a").unwrap();
    let name_selector = Selector::parse("a > div").unwrap();

    let mut res = Vec::new();

    for sel in document.select(&selector) {
        let a = sel.select(&a_selector).nth(1).unwrap();
        let name = sel.select(&name_selector).nth(0).unwrap();
        let name = name.first_child().unwrap().value();
        let href = a.value().attrs().find(|i| i.0 == "href").unwrap().1;
        let id = href.split("/").nth(2).unwrap();

        res.push(SearchResult {
            id: id.parse::<usize>().unwrap(),
            name: name.as_text().unwrap().to_string(),
        });
    }

    res
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

    Search {
        query: String,
    },
}

fn user_pick_anilist(
    list: &[anilist::SearchResult],
) -> &anilist::SearchResult {
    for (index, result) in list.iter().enumerate() {
        print!("{:2} ", index + 1);

        print!(
            "{} ",
            result
                .title
                .english
                .as_ref()
                .unwrap_or(&result.title.romaji)
        );

        // let title = result.get("title").unwrap();
        // if let Some(english) = title.get("english") {
        //     if !english.is_null() {
        //         let title = english.as_str().unwrap();
        //         print!("{} - ", title);
        //     }
        // }
        //
        // if let Some(romaji) = title.get("romaji") {
        //     if !romaji.is_null() {
        //         let title = romaji.as_str().unwrap();
        //         print!("{} - ", title);
        //     }
        // }
        //
        // if let Some(native) = title.get("native") {
        //     if !native.is_null() {
        //         let title = native.as_str().unwrap();
        //         print!("{} ", title);
        //     }
        // }
        println!();
    }

    print!("Choose entry: ");
    std::io::stdout().flush().unwrap();

    let mut buf = String::new();
    std::io::stdin().read_line(&mut buf).unwrap();

    let entry = buf.trim().parse::<usize>().unwrap();
    let result = &list[entry - 1];

    result
}

fn user_pick_manga(list: &[SearchResult]) -> &SearchResult {
    for (index, manga) in list.iter().enumerate() {
        println!("{} - {}", index + 1, manga.name);
    }

    print!("Choose entry: ");
    std::io::stdout().flush().unwrap();

    let mut buf = String::new();
    std::io::stdin().read_line(&mut buf).unwrap();

    let entry = buf.trim().parse::<usize>().unwrap();
    let result = &list[entry - 1];

    result
}

fn fetch(
    mal_id: usize,
    mangapill_id: usize,
    fetch_anilist: bool,
) -> (Option<Metadata>, Manga) {
    // let mut manga_dir = base.as_ref().to_path_buf();
    // manga_dir.push(mal_id.to_string());
    //
    // let mut mangapill_file = manga_dir.clone();
    // mangapill_file.push("mangapill.json");
    //
    // let mut metadata_file = manga_dir.clone();
    // metadata_file.push("metadata.json");
    //
    // if manga_dir.exists() && !manga_dir.is_dir() {
    //     panic!("'{}' already exists on the filesystem", mal_id);
    // } else {
    //     std::fs::create_dir_all(&manga_dir).unwrap();
    // }

    // TODO(patrik): Check if anilist and malid matches
    // TODO(patrik): Refetch if user picks it
    let metadata = if fetch_anilist {
        println!("Fetching anilist metadata (MAL Id: {})", mal_id);
        Some(fetch_anilist_metadata(mal_id))
    } else {
        println!("Skipping fetching anilist metadata");
        None
    };

    println!("Fetching manga: {}", mangapill_id);
    let mut manga = fetch_manga(mangapill_id);
    for chapter in manga.chapters.iter_mut() {
        println!("Fetching chapter: {}", chapter.index);
        fetch_chapter_data(chapter);
        std::thread::sleep(Duration::from_millis(50));
    }

    // if mangapill_file.is_file() {
    //     let s = std::fs::read_to_string(&mangapill_file).unwrap();
    //     let current = serde_json::from_str::<Manga>(&s).unwrap();
    //     println!("Original: {:#?}", current);
    //
    //     if current.chapters.len() != manga.chapters.len() {
    //         let mut missing_chapters = Vec::new();
    //         for chapter in manga.chapters.iter() {
    //             let res = current.chapters.iter().find(|i| i.index == chapter.index);
    //             if res.is_none() {
    //                 missing_chapters.push(chapter.index);
    //             }
    //         }
    //
    //         println!("Missing Chapters: {:#?}", missing_chapters);
    //     }
    // }

    // let s = serde_json::to_string_pretty(&manga).unwrap();
    // write_to_file(mangapill_file, &s);

    (metadata, manga)
}

struct ThreadJob {
    referer: String,
    url: String,
    dest: PathBuf,
}

fn thread_worker(tid: usize, queue: Arc<Mutex<VecDeque<ThreadJob>>>) {
    let client = Client::new();

    'work_loop: loop {
        let mut work = {
            let mut lock = queue.lock().unwrap();
            if let Some(job) = lock.pop_front() {
                job
            } else {
                break 'work_loop;
            }
        };

        println!("{} working on '{}'", tid, work.url);

        let mut res = client
            .get(work.url)
            .header("Referer", &work.referer)
            .send()
            .unwrap();
        if !res.status().is_success() {
            // TODO(patrik): Add error queue
            panic!("Failed to download");
        }

        let content_type =
            res.headers().get("content-type").unwrap().to_str().unwrap();
        let ext = match content_type {
            "image/jpeg" => "jpeg",
            "image/png" => "png",
            _ => panic!("Unknown Content-Type '{}'", content_type),
        };

        work.dest.set_extension(ext);
        let mut file = File::create(&work.dest).unwrap();
        res.copy_to(&mut file).unwrap();
    }
}

fn fetch_chapters(paths: &Paths, manga: &Manga, missing_chapters: &[usize]) {
    if !paths.chapters_dir.is_dir() {
        std::fs::create_dir_all(&paths.chapters_dir).unwrap();
    }

    let mut thread_jobs = VecDeque::new();
    for &chapter_index in missing_chapters {
        let chapter = manga.chapters.iter().find(|i| i.index == chapter_index);

        if let Some(chapter) = chapter {
            let mut chapter_dest = paths.chapters_dir.clone();
            chapter_dest.push(chapter.index.to_string());
            std::fs::create_dir_all(&chapter_dest).unwrap();

            let pages = chapter.pages.as_ref().unwrap();
            for (index, page) in pages.iter().enumerate() {
                std::io::stdout().flush().unwrap();

                let mut filepath = chapter_dest.clone();
                filepath.push(index.to_string());

                thread_jobs.push_back(ThreadJob {
                    referer: chapter.url.clone(),
                    url: page.clone(),
                    dest: filepath,
                });
            }
        } else {
            println!("Unkown chapter index: {}", chapter_index);
        }
    }

    println!("Thread Jobs: {}", thread_jobs.len());

    let queue = Arc::new(Mutex::new(thread_jobs));

    const THREAD_COUNT: usize = 4;

    let mut threads = Vec::new();

    for tid in 0..THREAD_COUNT {
        let queue_handle = queue.clone();
        let handle = std::thread::spawn(move || {
            thread_worker(tid, queue_handle);
        });

        threads.push(handle);
    }

    for (index, handle) in threads.into_iter().enumerate() {
        handle.join().unwrap();
        println!("{} finished", index);
    }
}

struct Paths {
    metadata_file: PathBuf,
    chapters_dir: PathBuf,
    chapters_metadata: PathBuf,
}

fn create_paths(base: &PathBuf, mal_id: usize) -> Paths {
    let mut manga_dir = base.clone();
    manga_dir.push(mal_id.to_string());

    let mut metadata_file = manga_dir.clone();
    metadata_file.push("metadata.json");

    let mut chapters_dir = manga_dir.clone();
    chapters_dir.push("chapters");

    let mut chapters_metadata = manga_dir.clone();
    chapters_metadata.push("chapters.json");

    Paths {
        metadata_file,
        chapters_dir,
        chapters_metadata,
    }
}

fn main() {
    let args = Args::parse();
    println!("Args: {:#?}", args);

    let base = if let Some(dir) = args.dir {
        dir
    } else {
        PathBuf::new()
    };

    match args.command {
        Commands::Fetch {
            mal_id,
            mangapill_id,
        } => {
            let paths = create_paths(&base, mal_id);
            let (metadata, manga) =
                fetch(mal_id, mangapill_id, !paths.metadata_file.is_file());

            if let Some(metadata) = metadata {
                let s = serde_json::to_string_pretty(&metadata).unwrap();
                write_to_file(&paths.metadata_file, &s);
            }

            let mut chapters = Vec::new();

            if paths.chapters_dir.is_dir() {
                for chapter in paths.chapters_dir.read_dir().unwrap() {
                    let chapter = chapter.unwrap();
                    let chapter = chapter.path();

                    let index = chapter
                        .file_stem()
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .parse::<usize>()
                        .unwrap();

                    chapters.push((index, chapter));
                }
            }

            let mut missing_chapters = Vec::new();

            for chapter in manga.chapters.iter() {
                let res = chapters.iter().find(|i| i.0 == chapter.index);
                if res.is_none() {
                    missing_chapters.push(chapter.index);
                }
            }

            println!("Chapters to fetch: {}", missing_chapters.len());
            fetch_chapters(&paths, &manga, &missing_chapters);

            let mut chapters = Vec::new();

            for chapter in manga.chapters.iter() {
                chapters.push(ChapterEntry {
                    index: chapter.index,
                    name: chapter.name.clone(),
                    page_count: chapter.pages.as_ref().map(|i| i.len()).unwrap_or(0),
                });
            }

            let s = serde_json::to_string_pretty(&chapters).unwrap();
            write_to_file(&paths.chapters_metadata, &s);

        }

        Commands::Search { query } => {
            // TODO(patrik): Filter out results where malId == null
            // let results = anilist::query(&query);
            // let anilist = user_pick_anilist(&results);
            //
            // let results = search(&query);
            // let manga = user_pick_manga(&results);
            //
            // let manga = fetch(&base, anilist.mal_id.unwrap(), manga.id);
            // download(&base, &manga);
        }
    }
}
