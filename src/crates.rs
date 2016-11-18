use std::fs::{self, File};
// use std::fs::File;
use std::io::prelude::*;
use std::io;
use std::path::PathBuf;
use std::process::{Command, ExitStatus};
use std::thread;

use rustc_serialize::json;
use scoped_threadpool::Pool;
use walkdir::WalkDir;

use super::Config;

#[derive(Debug, RustcDecodable)]
pub struct Package {
    name: String,
    vers: String,
}

pub fn fetch(path: &PathBuf,
             upstream: &str,
             index_path: &str,
             crate_name: &str,
             crate_version: &str)
             -> Result<ExitStatus, io::Error> {
    debug!("Fetching {}(v: {})", crate_name, crate_version);
    let url = format!("{}/{}/{}/download", upstream, crate_name, crate_version);
    let _ = fs::create_dir_all(PathBuf::from(format!("{}/crates/{}", index_path, crate_name)));
    Command::new("curl").arg("-o").arg(&path) // Save to disk
                         .arg("-L") // Follow redirects
                         .arg("-s") // Quietly!
                         // .current_dir(path)
                         .arg(url)
                         .status()
}

fn try_fetch(config: &Config, crate_name: &str, crate_version: &str) {
    let path = PathBuf::from(format!("{}/crates/{}/{}",
                                     config.index_path,
                                     crate_name,
                                     crate_version));
    if path.exists() {
        trace!("{}:{} is already fetched", crate_name, crate_version);
    } else {
        let _ = fetch(&path,
                      &config.upstream,
                      &config.index_path,
                      &crate_name,
                      &crate_version);
    }
}

pub fn pre_fetch(prefetch_path: String, config: Config) {
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


pub fn fetch_all(config: &Config) {
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
                .filter(|f| f.file_type().is_file()) {
                trace!("Found file at {:?}", entry.file_name());

                let config = config.clone();
                scope.execute(move || {
                    if let Ok(f) = File::open(entry.path()) {
                        let reader = io::BufReader::new(f);
                        for line in reader.lines().filter_map(|l| l.ok()) {
                            match json::decode::<Package>(&line) {
                                Ok(package) => {
                                    trace!("Found package: {:?}", package);

                                    try_fetch(&config, &package.name, &package.vers);
                                }
                                Err(e) => debug!("Had a problem with \"{}\": {:?}", line, e),
                            };

                        }

                    }
                });
            }

        });

        debug!("Finished background fetch all");
    });
}
