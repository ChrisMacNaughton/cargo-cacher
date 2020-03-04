use std::env;
use std::fs::{self, File};
// use std::fs::File;
use std::io;
use std::io::prelude::*;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::thread;

use cargo::core::Workspace;
use cargo::Config as CargoConfig;
use scoped_threadpool::Pool;
use serde_json;
use walkdir::WalkDir;

use super::Config;

#[derive(Debug, Deserialize)]
pub struct Package {
    name: String,
    vers: String,
}

pub fn fetch(
    path: &PathBuf,
    upstream: &str,
    index_path: &str,
    crate_name: &str,
    crate_version: &str,
) -> Result<ExitStatus, io::Error> {
    debug!("Fetching {}(v: {})", crate_name, crate_version);
    let url = format!(
        "{}{}/{}-{}.crate",
        upstream, crate_name, crate_name, crate_version
    );
    trace!("Fetching from {}", url);
    let _ = fs::create_dir_all(PathBuf::from(format!(
        "{}/crates/{}",
        index_path, crate_name
    )));
    Command::new("curl")
        .arg("-o")
        .arg(&path) // Save to disk
        .arg("-L") // Follow redirects
        .arg("-s") // Quietly!
        .arg(url)
        .status()
}

pub fn size(path: &PathBuf) -> u64 {
    match fs::metadata(path) {
        Ok(metadata) => metadata.len(),
        _ => 0,
    }
}

fn try_fetch(config: &Config, crate_name: &str, crate_version: &str) {
    let path = PathBuf::from(format!(
        "{}/crates/{}/{}",
        config.index_path, crate_name, crate_version
    ));
    if path.exists() {
        trace!("{}:{} is already fetched", crate_name, crate_version);
    } else {
        match fetch(
            &path,
            &config.upstream,
            &config.index_path,
            &crate_name,
            &crate_version,
        ) {
            Ok(_) => {}
            Err(e) => error!("Couldn't fetch {}/{}: {:?}", crate_name, crate_version, e),
        }
    }
}

pub fn pre_fetch(config: &Config) {
    fetch_all(&config);
    let config = config.clone();
    if let Some(_) = config.prefetch_path {
        let prefetch_path = config.prefetch_path.clone().unwrap();
        let prefetch_parts: Vec<&str> = prefetch_path.split(".").collect();
        let prefetch_ext = prefetch_parts[prefetch_parts.len() - 1];
        if prefetch_ext.eq("lock") {
            thread::spawn(move || fetch_lock(&config));
            return;
        }
        thread::spawn(move || {
            debug!("Prefetching file at {}!", prefetch_path);
            if let Ok(f) = File::open(prefetch_path) {
                let reader = io::BufReader::new(f);
                for line in reader.lines().filter(|l| l.is_ok()).map(|l| l.unwrap()) {
                    let mut split = line.split("=");
                    if let Some(crate_name) = split.next() {
                        if let Some(crate_version) = split.next() {
                            try_fetch(&config, crate_name, crate_version);
                        }
                    }
                }
            }
        });
    }
}

pub fn fetch_all(config: &Config) {
    if !config.all {
        return;
    }
    let config = config.clone();
    thread::spawn(move || {
        let mut pool = Pool::new(config.threads);
        debug!("Spawned batch fetch thread");
        let mut git_path = config.index_path.clone();
        git_path.push_str("/index");
        pool.scoped(|scope| {
            for entry in WalkDir::new(git_path)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|f| !f.path().to_str().unwrap().contains(".git"))
                .filter(|f| f.file_type().is_file())
                .filter(|f| f.file_name() != "config.json")
            {
                trace!("Found crate info file at {:?}", entry.path());

                let config = config.clone();
                scope.execute(move || {
                    if let Ok(f) = File::open(entry.path()) {
                        let reader = io::BufReader::new(f);
                        for line in reader.lines().filter_map(|l| l.ok()) {
                            match serde_json::from_str::<Package>(&line) {
                                // match json::decode::<Package>(&line) {
                                Ok(package) => {
                                    trace!("Found package: {:?}", package);

                                    try_fetch(&config, &package.name, &package.vers);
                                }
                                Err(e) => warn!(
                                    "Had a problem with \"{}\" / {:?}: {:?}",
                                    line,
                                    entry.path(),
                                    e
                                ),
                            };
                        }
                    }
                });
            }
        });

        debug!("Finished background fetch all");
    });
}

fn fetch_lock(config: &Config) {
    let cargo_config = CargoConfig::default().unwrap();
    let lockfile = match config.prefetch_path {
        Some(ref file) => file,
        None => return,
    };
    let lockfile = Path::new(lockfile);
    let manifest = lockfile.parent().unwrap().join("Cargo.toml");
    let manifest = env::current_dir().unwrap().join(&manifest);
    let ws = Workspace::new(&manifest, &cargo_config).expect("Cannot find Cargo.toml");

    let (_, resolve) = cargo::ops::resolve_ws(&ws).expect("failed to load pkg lockfile");

    for id in resolve.iter() {
        let name = id.name().as_str();
        let version = id.version().to_string();
        trace!("Resolved package: {} v{}", name, version);
        try_fetch(config, name, &version);
    }
}
