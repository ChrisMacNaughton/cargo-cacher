use std::io::prelude::*;
use std::fs::{File, remove_dir_all};
use std::path::PathBuf;
use std::process::{Command, Stdio, ExitStatus};
use std::thread::{self, sleep};

use super::Config;
use crates::fetch_all;

pub fn init_sync(git_path: PathBuf, config: &Config) {
    let config = config.clone();
    git_sync(&git_path, &config.index, &config.extern_url);
    thread::spawn(move || loop {
        sleep(config.refresh_interval);
        git_sync(&git_path, &config.index, &config.extern_url);
        if config.all {
            fetch_all(&config);
        }
    });
}

pub fn git_sync(git_path: &PathBuf, index_path: &str, extern_url: &str) {
    debug!("Syncing git repo at {} with {}, setting API url to {}",
           git_path.to_str().unwrap(),
           index_path,
           extern_url);
    let mut repo_path = git_path.clone();
    repo_path.push(".git");
    debug!("Repo path is {:?}", repo_path);
    if !repo_path.exists() {
        if Some(())
            .and_then(|_| Command::new("git")
                .arg("init")
                .arg("-qq")
                .arg(&git_path)
                .current_dir(git_path)
                .stderr(Stdio::null())
                .stdout(Stdio::null())
                .status()
                .ok()
            )
            .filter(ExitStatus::success)
            .and_then(|_| Command::new("git")
                .arg("config")
                .arg("commit.gpgsign")
                .arg("false")
                .stderr(Stdio::null())
                .stdout(Stdio::null())
                .current_dir(git_path)
                .status()
                .ok()
            )
            .filter(ExitStatus::success)
            .and_then(|_| Command::new("git")
                .arg("commit")
                .arg("--allow-empty")
                .arg("-mTEMP")
                .arg("-qq")
                .stderr(Stdio::null())
                .stdout(Stdio::null())
                .current_dir(git_path)
                .status()
                .ok()
            )
            .filter(ExitStatus::success)
            .is_none()
        {
            error!("Failed to prepare empty repository in: {:?}", git_path);
            remove_dir_all(repo_path).ok();
            return;
        }
    }
    let status =  {
        match Command::new("git")
            .arg("pull")
            .arg("-q")
            .arg("--rebase")
            .arg("-f")
            .arg(index_path)
            .arg("HEAD:master")
            .stderr(Stdio::null())
            .stdout(Stdio::null())
            .current_dir(git_path)
            .status() {
            Ok(s) => Some(s),
            Err(e) => {
                warn!("Error pulling: {:?}", e);
                return;
            }
        }
    };
    if let Some(status) = status {
        if status.success() {
            let mut config_path = git_path.clone();
            config_path.push("config.json");
            if let Ok(mut f) = File::create(config_path) {
                let config = format!(r#"{{
                    "dl": "{0}/api/v1/crates",
                    "api": "{0}/"
                }}"#, extern_url);
                let _ = f.write(&config.as_bytes());
                Command::new("git")
                    .arg("commit")
                    .arg("-q")
                    .arg("-a")
                    .arg("-m 'Updating config.json'")
                    .arg("--no-gpg-sign")
                    .stderr(Stdio::null())
                    .stdout(Stdio::null())
                    .current_dir(git_path)
                    .status()
                    .unwrap();
                let mut export = git_path.clone();
                export.push(".git");
                export.push("git-daemon-export-ok");
                let _ = File::create(export);
                trace!("Successfully synced");
            } else {
                warn!("\tHad a problem modifying the config.json")
            }
            return;
        } else {
            warn!("Command was not a success");
        }
    }
    warn!("Failed to update index");
}
