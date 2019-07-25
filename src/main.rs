#[macro_use]
extern crate clap;
extern crate iron;
#[macro_use]
extern crate log;
extern crate logger;
#[macro_use]
extern crate router;
extern crate rusqlite;
extern crate scoped_threadpool;
extern crate serde;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate serde_json;
extern crate simple_logger;
extern crate walkdir;

use std::env;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::mpsc::SyncSender;
use std::sync::Mutex;

mod crates;
mod git;
mod index_sync;
mod stats;

use clap::{App, Arg};

// Iron Stuff
use iron::prelude::*;
use iron::status;
use iron::AfterMiddleware;
use logger::Logger;
use router::Router;

use iron::mime::{Mime, SubLevel, TopLevel};

use crates::{fetch, pre_fetch, size};
use stats::Database;

#[derive(Clone, Debug)]
pub struct Config {
    all: bool,
    prefetch_path: Option<String>,
    index_path: String,
    crate_path: String,
    git_index_path: String,
    upstream: String,
    index: String,
    extern_url: String,
    port: u16,
    refresh_rate: u64,
    threads: u32,
    log_level: log::Level,
}

impl Config {
    pub fn init() -> Config {
        let matches = App::new("cargo-cacher")
            .version(crate_version!())
            .about(
                r#"Cargo-cacher is a caching proxy for Cargo, Rust's package manager.

    The cacher can be used easily by setting your $HOME/.cargo/config to:

    `
    [source]

    [source.crates-io]
    replace-with = "mirror"

    [source.mirror]
    registry = "http://localhost:8080/index"
    `"#,
            )
            .arg(
                Arg::with_name("debug")
                    .short("d")
                    .multiple(true)
                    .help("Sets the level of debugging information"),
            )
            .arg(
                Arg::with_name("git")
                    .short("g")
                    .required(false)
                    .takes_value(true)
                    .help(
                        "Upstream git index (Default: \
                         https://github.com/rust-lang/crates.io-index.git)",
                    ),
            )
            .arg(
                Arg::with_name("index")
                    .long("index")
                    .short("i")
                    .required(false)
                    .takes_value(true)
                    .help("Path to store the indexes (git and crates) at (Default: $HOME/.crates)"),
            )
            .arg(
                Arg::with_name("upstream")
                    .long("upstream")
                    .short("u")
                    .required(false)
                    .takes_value(true)
                    .help("Upstream Crate source (Default: https://static.crates.io/crates/)"),
            )
            .arg(
                Arg::with_name("port")
                    .long("port")
                    .short("p")
                    .required(false)
                    .takes_value(true)
                    .help("Port to listen on (Default: 8080)"),
            )
            .arg(
                Arg::with_name("extern-url")
                    .long("eurl")
                    .short("e")
                    .required(false)
                    .takes_value(true)
                    .help("Externally reachable URL (Default: http://localhost:8080)")
            )
            .arg(
                Arg::with_name("refresh")
                    .short("r")
                    .required(false)
                    .takes_value(true)
                    .help("Refresh rate for the git index (Default: 600)"),
            )
            .arg(
                Arg::with_name("prefetch")
                    .short("f")
                    .takes_value(true)
                    .required(false)
                    .help("Path with a list of crate_name=version to pre-fetch"),
            )
            .arg(
                Arg::with_name("threads")
                    .short("t")
                    .help("How many threads to use to fetch crates in the background")
                    .takes_value(true),
            )
            .arg(
                Arg::with_name("all")
                    .long("all")
                    .short("a")
                    .help("Prefetch entire Cargo index"),
            )
            .get_matches();

        let log_level = match matches.occurrences_of("debug") {
            0 => log::Level::Warn,
            1 => log::Level::Info,
            2 => log::Level::Debug,
            3 | _ => log::Level::Trace,
        };
        let default_crate_path = format!("{}/.crates", dirs::home_dir().unwrap().to_str().unwrap());
        let index_path: String = matches
            .value_of("index")
            .unwrap_or(&default_crate_path)
            .into();

        let mut crate_path = index_path.clone();
        crate_path.push_str("/crates");
        let mut git_index: String = index_path.clone();
        git_index.push_str("/index");
        let port = u16::from_str(matches.value_of("port")
                    .unwrap_or("8080"))
                .unwrap_or(8080);
        Config {
            all: matches.is_present("all"),
            prefetch_path: matches.value_of("prefetch").map(|r| r.to_string()),
            index_path: index_path,
            crate_path: crate_path,
            git_index_path: git_index,
            upstream: matches
                .value_of("upstream")
                .unwrap_or("https://static.crates.io/crates/")
                .into(),
            index: matches
                .value_of("git")
                .unwrap_or("https://github.com/rust-lang/crates.io-index.git")
                .into(),
            port: u16::from_str(matches.value_of("port").unwrap_or("8080")).unwrap_or(8080),
            extern_url: matches.value_of("extern-url")
                .map(Into::into)
                .unwrap_or(format!("http://localhost:{}", port)),
            refresh_rate: u64::from_str(matches.value_of("refresh").unwrap_or("600"))
                .unwrap_or(600),
            threads: u32::from_str(matches.value_of("threads").unwrap_or("16")).unwrap_or(16),
            log_level: log_level,
        }
    }
}

#[derive(Clone, Debug)]
pub struct CargoRequest {
    /// crate name, ex: cargo-cacher
    name: String,
    /// major.minor.patch
    version: String,
    /// Cache hit?
    hit: bool,
    /// Filesize in bytes
    size: i64,
}

fn main() {
    let config = Config::init();

    simple_logger::init_with_level(config.log_level).unwrap();
    info!("Configuration: {:?}", config);

    setup_filesystem(&config);

    index_sync::init_sync(PathBuf::from(&config.git_index_path), &config);

    pre_fetch(&config);
    let collector = stats::stat_collector();
    server(&config, collector)
}

fn setup_filesystem(config: &Config) {
    let _ = std::fs::create_dir_all(&config.crate_path);
    let _ = std::fs::create_dir_all(&config.git_index_path);
}

struct CorsMiddleware;

impl AfterMiddleware for CorsMiddleware {
    fn after(&self, _req: &mut Request, mut res: Response) -> IronResult<Response> {
        res.headers
            .set(iron::headers::AccessControlAllowOrigin::Any);
        Ok(res)
    }
}

fn server(config: &Config, stats: SyncSender<CargoRequest>) {
    // web server to handle DL requests
    let host = format!(":::{}", config.port);
    let router = router!(
        stats_json: get "/stats.json" => {
                move |_request: &mut Request|
                    stats_json()
        },
        stats: get "/stats" => {
            move |_request: &mut Request|
                stats_view()
        },
        download: get "api/v1/crates/:crate_name/:crate_version/download" => {
            let config = config.clone();
            let stats = Mutex::new(stats.clone());
            move |request: &mut Request|
                fetch_download(request, &config, &stats)
        },
        head: get "index/*" => {
            let config = config.clone();
            move |request: &mut Request|
                git::git(request, &config)
        },
        index: get "index/**/*" => {
            let config = config.clone();
            move |request: &mut Request|
                git::git(request, &config)
        },
        head: post "index/*" => {
            let config = config.clone();
            move |request: &mut Request|
                git::git(request, &config)
        },
        index: post "index/**/*" => {
            let config = config.clone();
            move |request: &mut Request|
                git::git(request, &config)
        },
        root: any "/" => log,
        query: any "/*" => log,
    );
    let mut chain = Chain::new(router);
    let (logger_before, logger_after) = Logger::new(None);
    chain.link_before(logger_before);
    chain.link_after(logger_after);

    chain.link_after(CorsMiddleware);
    println!("Listening on {}", host);
    // Iron::new(chain).http(host).unwrap();
    Iron::new(chain).http(&host[..]).unwrap();
}

pub fn log(req: &mut Request) -> IronResult<Response> {
    info!("Whoops! {:?}", req);
    Ok(Response::with((status::Ok, "Ok")))
}

fn fetch_download(
    req: &mut Request,
    config: &Config,
    stats: &Mutex<SyncSender<CargoRequest>>,
) -> IronResult<Response> {
    let stats = stats.lock().unwrap();
    let ref crate_name = req
        .extensions
        .get::<Router>()
        .unwrap()
        .find("crate_name")
        .unwrap();
    let ref crate_version = req
        .extensions
        .get::<Router>()
        .unwrap()
        .find("crate_version")
        .unwrap();
    debug!("Downloading: {}:{}", crate_name, crate_version);
    trace!("Raw request: {:?}", req);
    let path = PathBuf::from(format!(
        "{}/crates/{}/{}",
        config.index_path, crate_name, crate_version
    ));
    if path.exists() {
        debug!("path {:?} exists!", path);
        let _ = stats.send(CargoRequest {
            name: crate_name.to_string(),
            version: crate_version.to_string(),
            hit: true,
            size: size(&path) as i64,
        });
        Ok(Response::with((status::Ok, path)))
    } else {
        debug!("path {:?} doesn't exist!", path);

        match fetch(
            &path,
            &config.upstream,
            &config.index_path,
            &crate_name,
            &crate_version,
        ) {
            Ok(_) => {
                let _ = stats.send(CargoRequest {
                    name: crate_name.to_string(),
                    version: crate_version.to_string(),
                    hit: false,
                    size: size(&path) as i64,
                });
                Ok(Response::with((status::Ok, path)))
            }
            Err(e) => {
                error!("{:?}", e);
                return Ok(Response::with((
                    status::ServiceUnavailable,
                    "Couldn't fetch from Crates.io",
                )));
            }
        }
    }

    // Ok(Response::with((status::Ok, "Ok")))
}

fn stats_view() -> IronResult<Response> {
    let db = Database::new(None::<&str>);
    let stats = db.stats();
    Ok(Response::with((
        status::Ok,
        format!(
            include_str!("stats.html"),
            stats.downloads, stats.hits, stats.misses, stats.bandwidth_saved
        ),
        Mime(TopLevel::Text, SubLevel::Html, vec![]),
    )))
}

fn stats_json() -> IronResult<Response> {
    let db = Database::new(None::<&str>);
    let stats = db.stats();
    Ok(Response::with((
        status::Ok,
        stats.as_json(),
        Mime(TopLevel::Text, SubLevel::Json, vec![]),
    )))
}
