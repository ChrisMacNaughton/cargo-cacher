#[macro_use]
extern crate clap;
#[macro_use]
extern crate iron;
#[macro_use]
extern crate log;
extern crate logger;
#[macro_use]
extern crate router;
extern crate simple_logger;

use std::fs::File;
use std::io::prelude::*;
use std::io;
use std::time::Duration;
use std::path::PathBuf;
use std::process::{Command, ExitStatus};
use std::str::FromStr;
use std::thread;

mod index_sync;
mod git;

use clap::{Arg, App};

// Iron Stuff
use iron::status;
use iron::prelude::*;

use logger::Logger;

use router::Router;

#[derive(Clone, Debug)]
pub struct Config {
    index_path: String,
    upstream: String,
    index: String,
    port: u16,
    refresh_rate: u64,
    /// hours to keep the files around
    cache_timeout: u64,
    log_level: log::LogLevel,
}

fn main() {
    let (logger_before, logger_after) = Logger::new(None);

    let matches = App::new("cargo-cacher")
        .version(crate_version!())
        .arg(Arg::with_name("debug")
            .short("d")
            .multiple(true)
            .help("Sets the level of debugging information"))
        .arg(Arg::with_name("git")
            .short("g")
            .required(false)
            .takes_value(true)
            .help("Upstream git index (Default: \
                   https://github.com/rust-lang/crates.io-index.git)"))
        .arg(Arg::with_name("index")
            .short("i")
            .required(false)
            .takes_value(true)
            .help("Path to store the indexes (git and fiels) at (Default: ./index)"))
        .arg(Arg::with_name("upstream")
            .short("u")
            .required(false)
            .takes_value(true)
            .help("Upstream Crate source (Default: https://crates.io/api/v1/crates/)"))
        .arg(Arg::with_name("port")
            .short("p")
            .required(false)
            .takes_value(true)
            .help("Output file to put compiled crushmap into (Default: 8080)"))
        .arg(Arg::with_name("refresh")
            .short("r")
            .required(false)
            .takes_value(true)
            .help("Refresh rate for the git index (Default: 600)"))
        .arg(Arg::with_name("timeout")
            .short("t")
            .required(false)
            .takes_value(true)
            .help("How long, in hours, to keep cached crates around (Default: 168 / 7 days)"))
        .arg(Arg::with_name("prefetch")
            .short("f")
            .takes_value(true)
            .required(false)
            .help("Path with a list of crate_name=version to pre-fetch"))
        .get_matches();

    let log_level = match matches.occurrences_of("debug") {
        0 => log::LogLevel::Warn,
        1 => log::LogLevel::Info,
        2 => log::LogLevel::Debug,
        3 | _ => log::LogLevel::Trace,
    };
    let config = Config {
        index_path: matches.value_of("index").unwrap_or("./index").into(),
        upstream: matches.value_of("upstream").unwrap_or("https://crates.io/api/v1/crates/").into(),
        index: matches.value_of("git")
            .unwrap_or("https://github.com/rust-lang/crates.io-index.git")
            .into(),
        port: u16::from_str(matches.value_of("port")
                .unwrap_or("8080"))
            .unwrap_or(8080),
        refresh_rate: u64::from_str(matches.value_of("refresh")
                .unwrap_or("600"))
            .unwrap_or(600),
        cache_timeout: u64::from_str(matches.value_of("refresh")
                .unwrap_or("168"))
            .unwrap_or(168),
        log_level: log_level,
    };

    simple_logger::init_with_level(config.log_level).unwrap();
    info!("Configuration: {:?}", config);

    let _ = std::fs::create_dir_all(PathBuf::from(format!("{}/{}", config.index_path, "crates")));
    let _ = std::fs::create_dir_all(PathBuf::from(format!("{}/{}", config.index_path, "index")));

    let mut git_index: String = config.index_path.clone();
    git_index.push_str("/index");
    index_sync::init_sync(PathBuf::from(git_index),
                          &config.index,
                          config.port,
                          Duration::from_secs(config.refresh_rate));
    if let Some(prefetch) = matches.value_of("prefetch").map(|r| r.to_string()) {
        let config = config.clone();
        pre_fetch(prefetch, config);
    }

    // web server to handle DL requests

    let host = format!("0.0.0.0:{}", config.port);

    let router = router!(
        download: get "api/v1/crates/:crate_name/:crate_version/download" => {
            let config = config.clone();
            move |request: &mut Request|
                fetch_download(request, &config.clone())
        },
        head: get "index/*" => {
            let config = config.clone();
            move |request: &mut Request|
                git::git(request, &config.clone())
        },
        index: get "index/**/*" => {
            let config = config.clone();
            move |request: &mut Request|
                git::git(request, &config.clone())
        },
        head: post "index/*" => {
            let config = config.clone();
            move |request: &mut Request|
                git::git(request, &config.clone())
        },
        index: post "index/**/*" => {
            let config = config.clone();
            move |request: &mut Request|
                git::git(request, &config.clone())
        },
        root: any "/" => log,
        query: any "/*" => log,
    );
    let mut chain = Chain::new(router);

    chain.link_before(logger_before);
    chain.link_after(logger_after);

    println!("Listening on {}", host);
    // Iron::new(chain).http(host).unwrap();
    Iron::new(chain).http(&host[..]).unwrap();
}

pub fn log(req: &mut Request) -> IronResult<Response> {
    info!("Whoops! {:?}", req);
    Ok(Response::with((status::Ok, "Ok")))
}

fn fetch_download(req: &mut Request, config: &Config) -> IronResult<Response> {
    let ref crate_name = req.extensions
        .get::<Router>()
        .unwrap()
        .find("crate_name")
        .unwrap();
    let ref crate_version = req.extensions
        .get::<Router>()
        .unwrap()
        .find("crate_version")
        .unwrap();
    debug!("Downloading: {}:{}", crate_name, crate_version);
    trace!("Raw request: {:?}", req);
    let path = PathBuf::from(format!("{}/crates/{}/{}",
                                     config.index_path,
                                     crate_name,
                                     crate_version));
    if path.exists() {
        debug!("path {:?} exists!", path);
        Ok(Response::with((status::Ok, path)))
    } else {
        debug!("path {:?} doesn't exist!", path);

        match fetch(&path,
                    &config.upstream,
                    &config.index_path,
                    &crate_name,
                    &crate_version) {
            Ok(_) => Ok(Response::with((status::Ok, path))),
            Err(e) => {
                error!("{:?}", e);
                return Ok(Response::with((status::ServiceUnavailable,
                                          "Couldn't fetch from Crates.io")));
            }
        }
    }

    // Ok(Response::with((status::Ok, "Ok")))
}

fn fetch(path: &PathBuf,
         upstream: &str,
         index_path: &str,
         crate_name: &str,
         crate_version: &str)
         -> Result<ExitStatus, io::Error> {
    info!("Fetching {}(v: {})", crate_name, crate_version);
    let url = format!("{}/{}/{}/download", upstream, crate_name, crate_version);
    let _ = std::fs::create_dir_all(PathBuf::from(format!("{}/crates/{}", index_path, crate_name)));
    Command::new("curl").arg("-o").arg(&path) // Save to disk
                         .arg("-L") // Follow redirects
                         .arg("-s") // Quietly!
                         // .current_dir(path)
                         .arg(url)
                         .status()
}

fn pre_fetch(prefetch_path: String, config: Config) {
    thread::spawn(move || {
        debug!("Prefetching file at {}!", prefetch_path);
        if let Ok(f) = File::open(prefetch_path) {
            let reader = io::BufReader::new(f);
            for line in reader.lines().filter(|l| l.is_ok()).map(|l| l.unwrap()) {
                let mut split = line.split("=");
                if let Some(crate_name) = split.next() {
                    if let Some(crate_version) = split.next() {
                        let path = PathBuf::from(format!("{}/crates/{}/{}",
                                                         config.index_path,
                                                         crate_name,
                                                         crate_version));
                        if path.exists() {
                            debug!("{}:{} is already fetched", crate_name, crate_version);
                        } else {
                            let _ = fetch(&path,
                                          &config.upstream,
                                          &config.index_path,
                                          &crate_name,
                                          &crate_version);
                        }
                    }
                }
            }
        }
    });
}
