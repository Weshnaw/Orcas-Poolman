use std::{
    fs::{self, File},
    io::BufReader,
    path::PathBuf,
    rc::Rc,
    sync::mpsc,
};

use derive_more::{Debug, Display, Error, From};
use directories::ProjectDirs;
use notify::{
    Event, EventKind, RecursiveMode, Watcher,
    event::{CreateKind, ModifyKind},
};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[derive(Debug, Display, Error, From)]
enum Error {
    Notify(notify::Error),
    Fs(std::io::Error),
    Serde(serde_json::Error),
    NoOrcaFolder,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    // TODO: look for cli arg
    let proj_dirs = ProjectDirs::from("", "", "OrcaSlicer").ok_or(Error::NoOrcaFolder)?;
    let orca_cfg = proj_dirs.config_dir();
    // If on windows
    let orca_cfg = orca_cfg.parent().unwrap_or(orca_cfg);

    let filaments = orca_cfg.join("user\\default\\filament");

    info!("Path found to Filaments: {:?}", filaments);

    for file in fs::read_dir(&filaments)? {
        let file = file?;
        let file = file.path();

        reconcile_file(&file).await;
    }

    info!("Initializing filament.json watcher");
    let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
    let mut watcher = notify::recommended_watcher(tx)?;

    watcher.watch(&filaments, RecursiveMode::NonRecursive)?;
    for res in rx {
        match res {
            // Reconcile on Create or Modify events
            Ok(event) => {
                debug!(?event);
                match event.kind {
                    EventKind::Create(CreateKind::File | CreateKind::Any)
                    | EventKind::Modify(
                        ModifyKind::Any | ModifyKind::Data(_) | ModifyKind::Other,
                    ) => {
                        for path in event.paths {
                            reconcile_file(&path).await;
                        }
                    }
                    _ => {} // skip
                }
            }
            Err(e) => warn!("watch error: {:?}", e),
        }
    }

    Ok(())
}

#[derive(Serialize, Deserialize, Debug)]
struct FilamentConfig {
    #[serde(deserialize_with = "deserialize_singleton_array")]
    nozzle_temperature: Option<Rc<str>>,
}

fn deserialize_singleton_array<'de, D>(deserializer: D) -> Result<Option<Rc<str>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    // Deserialize as an optional Vec<String>
    let opt: Option<Vec<String>> = Option::deserialize(deserializer)?;

    Ok(opt.and_then(|mut v| {
        if v.is_empty() {
            None
        } else {
            Some(Rc::<str>::from(v.remove(0)))
        }
    }))
}

fn parse_file(path: &PathBuf) -> Result<FilamentConfig, Error> {
    Ok(serde_json::from_reader(BufReader::new(File::open(path)?))?)
}

async fn reconcile_file(path: &PathBuf) {
    if path.extension().is_some_and(|ext| ext != "json") || path.is_dir() {
        debug!("invalid file: {:?}", path);
        return;
    }

    match parse_file(path) {
        Ok(config) => {
            todo!("handle config: {:?}", config)
        }
        Err(e) => warn!("file parsing error: {:?}, err: {:?}", path, e),
    }
}
