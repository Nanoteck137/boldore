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
    anilist::{self, fetch_anilist_metadata},
    ChapterMetadata, MangaMetadata,
};

#[derive(Serialize, Deserialize, Debug)]
struct Mangapill {
    id: usize,
}

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

    #[arg(short, long, default_value = "1")]
    threads: usize,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Fetch,

    AddManga {
        query: String,
        #[arg(short, long)]
        add_to_current: bool,
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

fn fetch_mangapill(mangapill_id: usize) -> Manga {
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

    println!("Fetching manga: {}", mangapill_id);
    let manga = fetch_manga(mangapill_id);

    // for chapter in manga.chapters.iter_mut() {
    //     println!("Fetching chapter: {}", chapter.index);
    //     fetch_chapter_data(chapter);
    //     std::thread::sleep(Duration::from_millis(50));
    // }

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

    manga
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
            "image/gif" => "gif",
            _ => panic!("Unknown Content-Type '{}'", content_type),
        };

        work.dest.set_extension(ext);
        let mut file = File::create(&work.dest).unwrap();
        res.copy_to(&mut file).unwrap();
    }
}

fn fetch_chapters(
    paths: &Paths,
    manga: &mut Manga,
    missing_chapters: &[usize],
    thread_count: usize,
) {
    if !paths.chapters_dir.is_dir() {
        std::fs::create_dir_all(&paths.chapters_dir).unwrap();
    }

    let mut thread_jobs = VecDeque::new();
    for &chapter_index in missing_chapters {
        let chapter =
            manga.chapters.iter_mut().find(|i| i.index == chapter_index);

        if let Some(mut chapter) = chapter {
            let mut chapter_dest = paths.chapters_dir.clone();
            chapter_dest.push(chapter.index.to_string());

            std::fs::create_dir_all(&chapter_dest).unwrap();

            if chapter.pages.is_none() {
                println!("Fetching {}", chapter.index);
                fetch_chapter_data(&mut chapter);
                std::thread::sleep(Duration::from_millis(50));
            }

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
    println!("Using {} threads", thread_count);

    let queue = Arc::new(Mutex::new(thread_jobs));

    let mut threads = Vec::new();

    for tid in 0..thread_count {
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
    chapters_dir: PathBuf,
    manga_metadata: PathBuf,
}

fn create_paths(manga_dir: &PathBuf) -> Paths {
    let mut chapters_dir = manga_dir.clone();
    chapters_dir.push("chapters");

    let mut manga_metadata = manga_dir.clone();
    manga_metadata.push("manga.json");

    Paths {
        chapters_dir,
        manga_metadata,
    }
}

fn main() {
    let args = Args::parse();
    println!("Args: {:#?}", args);

    let base = if let Some(dir) = args.dir {
        dir
    } else {
        std::env::current_dir().unwrap()
    };

    match args.command {
        Commands::Fetch => {
            for path in base.read_dir().unwrap() {
                let path = path.unwrap();
                let path = path.path();

                let mut mangapill_file = path.clone();
                mangapill_file.push("mangapill.json");

                if !mangapill_file.is_file() {
                    continue;
                }

                let s = std::fs::read_to_string(&mangapill_file).unwrap();
                let mangapill = serde_json::from_str::<Mangapill>(&s).unwrap();

                println!("Fetch {:?}", path);
                let paths = create_paths(&path);

                let s =
                    std::fs::read_to_string(&paths.manga_metadata).unwrap();
                let mut metadata =
                    serde_json::from_str::<MangaMetadata>(&s).unwrap();

                let mut manga = fetch_mangapill(mangapill.id);

                let missing_chapters = {
                    let mut missing = Vec::new();
                    for chapter in manga.chapters.iter() {
                        let res = metadata
                            .chapters
                            .iter()
                            .find(|i| i.index == chapter.index);
                        if res.is_none() {
                            missing.push(chapter.index);
                        }
                    }

                    for missing in missing.iter() {
                        let mut path = paths.chapters_dir.clone();
                        path.push(missing.to_string());

                        if path.is_dir() {
                            panic!("Chapter '{}' is not declared in 'manga.json' but exists as an directory", missing);
                        }
                    }

                    missing
                };

                println!("Missing: {:?}", missing_chapters);
                fetch_chapters(
                    &paths,
                    &mut manga,
                    &missing_chapters,
                    args.threads,
                );

                let mut chapters = Vec::new();

                for chapter in manga.chapters.iter() {
                    let mut dir = paths.chapters_dir.clone();
                    dir.push(chapter.index.to_string());

                    let mut dest = dir.clone();
                    dest.push("name.txt");

                    std::fs::write(dest, &chapter.name).unwrap();

                    let pages = swadloon::get_sorted_pages(&dir);
                    // let read = dir.read_dir().unwrap();
                    // let mut res = read
                    //     .map(|e| e.unwrap().path())
                    //     .map(|e| {
                    //         e.file_name()
                    //             .unwrap()
                    //             .to_str()
                    //             .unwrap()
                    //             .to_string()
                    //     })
                    //     .filter(|e| e.as_str() != "name.txt")
                    //     .map(|e| {
                    //         let (page_num, _) = e.split_once(".").unwrap();
                    //
                    //         (page_num.parse::<usize>().unwrap(), e)
                    //     })
                    //     .collect::<Vec<_>>();
                    // res.sort_by(|l, r| l.0.cmp(&r.0));
                    //
                    // let pages = res
                    //     .into_iter()
                    //     .map(|p| format!("{}.{}", p.0, p.1))
                    //     .collect::<Vec<_>>();

                    chapters.push(ChapterMetadata {
                        index: chapter.index,
                        name: chapter.name.clone(),
                        pages,
                    });
                }

                metadata.chapters = chapters;

                let s = serde_json::to_string_pretty(&metadata).unwrap();
                write_to_file(&paths.manga_metadata, &s);
            }
        }

        Commands::AddManga {
            query,
            add_to_current,
        } => {
            // TODO(patrik): Filter out results where malId == null
            let results = anilist::query(&query);
            let anilist = user_pick_anilist(&results);

            let results = search(&query);
            let manga = user_pick_manga(&results);

            let name = sanitize_name(&manga.name);

            let dir = if add_to_current {
                std::env::current_dir().unwrap()
            } else {
                let mut dir = base.clone();
                dir.push(name);

                assert!(!dir.exists(), "Directory already exists: {:?}", dir);

                if !dir.is_dir() {
                    std::fs::create_dir_all(&dir).unwrap();
                }

                dir
            };

            let mut metadata_file = dir.clone();
            metadata_file.push("metadata.json");

            let mut manga_metadata_file = dir.clone();
            manga_metadata_file.push("manga.json");

            let mut mangapill_file = dir.clone();
            mangapill_file.push("mangapill.json");

            let mut images_dir = dir.clone();
            images_dir.push("images");

            if !images_dir.exists() {
                std::fs::create_dir_all(&images_dir).unwrap();
            }

            let metadata = fetch_anilist_metadata(anilist.mal_id.unwrap());
            let s = serde_json::to_string_pretty(&metadata).unwrap();
            write_to_file(&metadata_file, &s);

            let id = swadloon::gen_manga_id();
            let meta = swadloon::metadata_from_anilist(&dir, metadata, id);

            let s = serde_json::to_string_pretty(&meta).unwrap();
            write_to_file(&manga_metadata_file, &s);

            let mangapill = Mangapill { id: manga.id };

            let s = serde_json::to_string_pretty(&mangapill).unwrap();
            write_to_file(&mangapill_file, &s);
        }
    }
}
