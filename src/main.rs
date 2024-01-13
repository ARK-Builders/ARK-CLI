use std::env::current_dir;
use std::fs::{canonicalize, create_dir_all, metadata, File};
use std::io::prelude::*;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use arklib::app_id;
use arklib::id::ResourceId;
use arklib::index::ResourceIndex;
use arklib::pdf::PDFQuality;
use arklib::{
    modify, AtomicFile, APP_ID_FILE, ARK_FOLDER, FAVORITES_FILE,
    METADATA_STORAGE_FOLDER, PREVIEWS_STORAGE_FOLDER,
    PROPERTIES_STORAGE_FOLDER, SCORE_STORAGE_FILE, STATS_FOLDER,
    TAG_STORAGE_FILE, THUMBNAILS_STORAGE_FOLDER,
};
use clap::{Parser, Subcommand};
use fs_extra::dir::{self, CopyOptions};
use home::home_dir;
use std::io::{Result, Write};
use url::Url;
use walkdir::WalkDir;

#[derive(Parser, Debug)]
#[clap(name = "ark-cli")]
#[clap(about = "Manage ARK tag storages and indexes", long_about = None)]
struct Cli {
    #[clap(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    Backup {
        #[clap(parse(from_os_str))]
        roots_cfg: Option<PathBuf>,
    },

    Collisions {
        #[clap(parse(from_os_str))]
        root_dir: Option<PathBuf>,
    },

    Monitor {
        #[clap(parse(from_os_str))]
        root_dir: Option<PathBuf>,
        interval: Option<u64>,
    },

    Render {
        #[clap(parse(from_os_str))]
        path: Option<PathBuf>,
        quality: Option<String>,
    },

    #[clap(subcommand)]
    Link(Link),

    #[clap(subcommand)]
    File(FileCommand),
}

#[derive(Subcommand, Debug)]
enum FileCommand {
    Insert {
        #[clap(parse(from_os_str))]
        file_path: Option<PathBuf>,

        content: Option<String>,
    },

    List {
        #[clap(parse(from_os_str))]
        storage: Option<PathBuf>,

        #[clap(short, long)]
        all: bool,
    },
}

#[derive(Subcommand, Debug)]
enum Link {
    Create {
        #[clap(parse(from_os_str))]
        root_dir: Option<PathBuf>,

        url: Option<String>,
        title: Option<String>,
        desc: Option<String>,
    },

    Load {
        #[clap(parse(from_os_str))]
        root_dir: Option<PathBuf>,

        #[clap(parse(from_os_str))]
        file_path: Option<PathBuf>,

        id: Option<ResourceId>,
    },
}

const ARK_CONFIG: &str = ".config/ark";
const ARK_BACKUPS_PATH: &str = ".ark-backups";
const ROOTS_CFG_FILENAME: &str = "roots";

#[tokio::main]
async fn main() {
    env_logger::init();

    let args = Cli::parse();

    let temp_dir = std::env::temp_dir();
    let ark_dir = temp_dir.join(".ark");
    if !ark_dir.exists() {
        std::fs::create_dir(&ark_dir).unwrap();
    }
    println!("Loading app id at {}...", ark_dir.display());
    let _ = app_id::load(ark_dir).map_err(|e| {
        println!("Couldn't load app id: {}", e);
        std::process::exit(1);
    });

    match &args.command {
        Command::Backup { roots_cfg } => {
            let timestamp = timestamp().as_secs();
            let backup_dir = home_dir()
                .expect("Couldn't retrieve home directory!")
                .join(&ARK_BACKUPS_PATH)
                .join(&timestamp.to_string());

            if backup_dir.is_dir() {
                println!("Wait at least 1 second, please!");
                std::process::exit(0)
            }

            println!("Preparing backup:");
            let roots = discover_roots(roots_cfg);

            let (valid, invalid): (Vec<PathBuf>, Vec<PathBuf>) = roots
                .into_iter()
                .partition(|root| storages_exists(&root));

            if !invalid.is_empty() {
                println!("These folders don't contain any storages:");
                invalid
                    .into_iter()
                    .for_each(|root| println!("\t{}", root.display()));
            }

            if valid.is_empty() {
                println!("Nothing to backup. Bye!");
                std::process::exit(0)
            }

            create_dir_all(&backup_dir)
                .expect("Couldn't create backup directory!");

            let mut roots_cfg_backup =
                File::create(&backup_dir.join(&ROOTS_CFG_FILENAME))
                    .expect("Couldn't backup roots config!");

            valid.iter().for_each(|root| {
                writeln!(roots_cfg_backup, "{}", root.display())
                    .expect("Couldn't write to roots config backup!")
            });

            println!("Performing backups:");
            valid
                .into_iter()
                .enumerate()
                .for_each(|(i, root)| {
                    println!("\tRoot {}", root.display());
                    let storage_backup = backup_dir.join(&i.to_string());

                    let mut options = CopyOptions::new();
                    options.overwrite = true;
                    options.copy_inside = true;

                    let result = dir::copy(
                        root.join(&arklib::ARK_FOLDER),
                        storage_backup,
                        &options,
                    );

                    if let Err(e) = result {
                        println!("\t\tFailed to copy storages!\n\t\t{}", e);
                    }
                });

            println!("Backup created:\n\t{}", backup_dir.display());
        }

        Command::Collisions { root_dir } => monitor_index(&root_dir, None),

        Command::Monitor { root_dir, interval } => {
            let millis = interval.unwrap_or(1000);
            monitor_index(&root_dir, Some(millis))
        }

        Command::Render { path, quality } => {
            let filepath = path.to_owned().unwrap();
            let quality = match quality.to_owned().unwrap().as_str() {
                "high" => PDFQuality::High,
                "medium" => PDFQuality::Medium,
                "low" => PDFQuality::Low,
                _ => panic!("unknown render option"),
            };
            let buf = File::open(&filepath).unwrap();
            let dest_path = filepath.with_file_name(
                filepath
                    .file_stem()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_owned()
                    + ".png",
            );
            let img = arklib::pdf::render_preview_page(buf, quality);
            img.save(PathBuf::from(dest_path)).unwrap();
        }

        Command::Link(link) => match &link {
            Link::Create {
                root_dir,
                url,
                title,
                desc,
            } => {
                let root = provide_root(root_dir);

                let url = Url::parse(url.as_deref().unwrap());
                let link: arklib::link::Link = arklib::link::Link::new(
                    url.unwrap(),
                    title.to_owned().unwrap(),
                    desc.to_owned(),
                );

                let future = link.save(&root, true);

                println!("Saving link...");

                match future.await {
                    Ok(_) => {
                        println!("Link saved successfully!");
                        match provide_index(&root).store() {
                            Ok(_) => println!("Index stored successfully!"),
                            Err(e) => println!("Error: {}", e),
                        }
                    }
                    Err(e) => println!("Error: {}", e),
                }
            }

            Link::Load {
                root_dir,
                file_path,
                id,
            } => {
                let root = provide_root(root_dir);

                let path_from_index = id.map(|id| {
                    let index = provide_index(&root);
                    index.id2path[&id].as_path().to_path_buf()
                });
                let path_from_user = file_path;

                let path = match (path_from_user, path_from_index) {
                    (Some(path), Some(path2)) => {
                        if path.canonicalize().unwrap() != path2 {
                            println!("Path {:?} was requested.", path);
                            println!(
                                "But id {} maps to path {:?}",
                                id.unwrap(),
                                path2
                            );
                            panic!()
                        } else {
                            path.to_path_buf()
                        }
                    }
                    (Some(path), None) => path.to_path_buf(),
                    (None, Some(path)) => path,
                    (None, None) => {
                        println!("Provide a path or id for request.");
                        panic!()
                    }
                };

                let link = arklib::link::Link::load(root, path);
                println!("Link data:\n{:?}", link.unwrap());
            }
        },

        Command::File(file) => match &file {
            FileCommand::Insert { file_path, content } => {
                let file_path = file_path.as_ref().unwrap();
                let atomic_file = arklib::AtomicFile::new(file_path).unwrap();

                if let Some(content) = content {
                    modify(&atomic_file, |_| content.as_bytes().to_vec())
                        .unwrap();
                }
            }

            FileCommand::List { storage, all } => {
                let root = provide_root(&None);
                let storage = storage.as_ref().unwrap_or(&root);

                if !all {
                    let file_path = if storage.exists() {
                        Some(storage.clone())
                    } else {
                        match storage
                            .clone()
                            .into_os_string()
                            .into_string()
                            .unwrap()
                            .to_lowercase()
                            .as_str()
                        {
                            "favorites" => Some(
                                provide_root(&None)
                                    .join(ARK_FOLDER)
                                    .join(FAVORITES_FILE),
                            ),
                            "app-id" => Some(
                                provide_root(&None)
                                    .join(ARK_FOLDER)
                                    .join(APP_ID_FILE),
                            ),
                            "tags" => Some(
                                provide_root(&None)
                                    .join(ARK_FOLDER)
                                    .join(TAG_STORAGE_FILE),
                            ),
                            "scores" => Some(
                                provide_root(&None)
                                    .join(ARK_FOLDER)
                                    .join(SCORE_STORAGE_FILE),
                            ),

                            _ => None,
                        }
                    }
                    .expect("Could not find storage folder");

                    let file = AtomicFile::new(&file_path).unwrap();
                    if let Ok(file) = format_file(&file) {
                        println!("{}", file);
                    } else {
                        println!(
                            "FILE: {} is not a valid atomic file",
                            file_path.display()
                        );
                    }
                } else {
                    let file_path = if storage.exists() {
                        Some(storage.clone())
                    } else {
                        match storage
                            .clone()
                            .into_os_string()
                            .into_string()
                            .unwrap()
                            .to_lowercase()
                            .as_str()
                        {
                            "stats" => Some(
                                provide_root(&None)
                                    .join(ARK_FOLDER)
                                    .join(STATS_FOLDER),
                            ),
                            "properties" => Some(
                                provide_root(&None)
                                    .join(ARK_FOLDER)
                                    .join(PROPERTIES_STORAGE_FOLDER),
                            ),
                            "metadata" => Some(
                                provide_root(&None)
                                    .join(ARK_FOLDER)
                                    .join(METADATA_STORAGE_FOLDER),
                            ),
                            "previews" => Some(
                                provide_root(&None)
                                    .join(ARK_FOLDER)
                                    .join(PREVIEWS_STORAGE_FOLDER),
                            ),
                            "thumbnails" => Some(
                                provide_root(&None)
                                    .join(ARK_FOLDER)
                                    .join(THUMBNAILS_STORAGE_FOLDER),
                            ),
                            _ => None,
                        }
                    }
                    .expect("Could not find storage folder");

                    let files: Vec<AtomicFile> = WalkDir::new(file_path)
                        .into_iter()
                        .filter_entry(|e| e.file_type().is_dir())
                        .filter_map(|v| v.ok())
                        .filter_map(|e| match AtomicFile::new(e.path()) {
                            Ok(file) => Some(file),
                            Err(_) => None,
                        })
                        .collect();

                    for file in files {
                        if let Ok(file) = format_file(&file) {
                            println!("{}", file);
                        }
                    }
                }
            }
        },
    }
}

fn format_file(file: &AtomicFile) -> Result<String> {
    let current = file.load()?;
    let data = current.read_to_string()?;
    let mut split = current
        .path
        .file_name()
        .expect("Not a file")
        .to_str()
        .unwrap()
        .split("_");

    Ok(format!(
        "{}: [{} - {}]: {}",
        current.version,
        split.next().unwrap(),
        split.next().unwrap(),
        data
    ))
}

fn discover_roots(roots_cfg: &Option<PathBuf>) -> Vec<PathBuf> {
    if let Some(path) = roots_cfg {
        println!(
            "\tRoots config provided explicitly:\n\t\t{}",
            path.display()
        );
        let config = File::open(&path).expect("File doesn't exist!");

        parse_roots(config)
    } else {
        if let Ok(config) = File::open(&ARK_CONFIG) {
            println!(
                "\tRoots config was found automatically:\n\t\t{}",
                &ARK_CONFIG
            );

            parse_roots(config)
        } else {
            println!("\tRoots config wasn't found.");

            println!("Looking for a folder containing tag storage:");
            let path = canonicalize(
                current_dir().expect("Can't open current directory!"),
            )
            .expect("Couldn't canonicalize working directory!");

            let result = path.ancestors().find(|path| {
                println!("\t{}", path.display());
                storages_exists(path)
            });

            if let Some(root) = result {
                println!("Root folder found:\n\t{}", root.display());
                vec![root.to_path_buf()]
            } else {
                println!("Root folder wasn't found.");
                vec![]
            }
        }
    }
}

fn provide_root(root_dir: &Option<PathBuf>) -> PathBuf {
    if let Some(path) = root_dir {
        path.clone()
    } else {
        current_dir()
            .expect("Can't open current directory!")
            .clone()
    }
}

// Read-only structure
fn provide_index(root_dir: &PathBuf) -> ResourceIndex {
    let rwlock =
        arklib::provide_index(root_dir).expect("Failed to retrieve index");
    let index = &*rwlock.read().unwrap();
    index.clone()
}

fn monitor_index(root_dir: &Option<PathBuf>, interval: Option<u64>) {
    let dir_path = provide_root(root_dir);

    println!("Building index of folder {}", dir_path.display());
    let start = Instant::now();
    let dir_path = provide_root(root_dir);
    let result = arklib::provide_index(dir_path);
    let duration = start.elapsed();

    match result {
        Ok(rwlock) => {
            println!("Build succeeded in {:?}\n", duration);

            if let Some(millis) = interval {
                let mut index = rwlock.write().unwrap();
                loop {
                    let pause = Duration::from_millis(millis);
                    thread::sleep(pause);

                    let start = Instant::now();
                    match index.update_all() {
                        Err(msg) => println!("Oops! {}", msg),
                        Ok(diff) => {
                            index.store().expect("Could not store index");
                            let duration = start.elapsed();
                            println!("Updating succeeded in {:?}\n", duration);

                            if !diff.deleted.is_empty() {
                                println!("Deleted: {:?}", diff.deleted);
                            }
                            if !diff.added.is_empty() {
                                println!("Added: {:?}", diff.added);
                            }
                        }
                    }
                }
            } else {
                let index = rwlock.read().unwrap();

                println!("Here are {} entries in the index", index.size());

                for (key, count) in index.collisions.iter() {
                    println!("Id {:?} calculated {} times", key, count);
                }
            }
        }
        Err(err) => println!("Failure: {:?}", err),
    }
}

fn storages_exists(path: &Path) -> bool {
    let meta = metadata(path.join(&arklib::ARK_FOLDER));
    if let Ok(meta) = meta {
        return meta.is_dir();
    }

    false
}

fn parse_roots(config: File) -> Vec<PathBuf> {
    return BufReader::new(config)
        .lines()
        .filter_map(|line| match line {
            Ok(path) => Some(PathBuf::from(path)),
            Err(msg) => {
                println!("{:?}", msg);
                None
            }
        })
        .collect();
}

fn timestamp() -> Duration {
    let start = SystemTime::now();
    return start
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards!");
}
