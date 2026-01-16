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

        handle_file(&file).await;
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
                            handle_file(&path).await;
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

async fn handle_file(path: &PathBuf) {
    if path.extension().is_some_and(|ext| ext != "json") || path.is_dir() {
        debug!("invalid file: {:?}", path);
        return;
    }

    match parse_file(path) {
        Ok(config) => {
            reconcile_config(config).await;
        }
        Err(e) => warn!("file parsing error: {:?}, err: {:?}", path, e),
    }
}

// Priority List:
// Allow option to ignore OpenPrintTag values in case folks do not want to go through the hassle of adding OpenPrintTag to the extra fields, maybe in the future I will support other filament tags
// OpenPrintTag value this should be from the manufacturer so these values should take precident (we do not touch these values)
// Spoolman == Local (tie breaker is most recent last_modified unless local is specifically marked as 
//                         spoolman_force_pull (forces the local to change with current spoolman) or 
//                         spoolman_force_push (forces spoolman to update to the current local)) 
//             (store changes in local file somewhere)
// the file name should be: (Manufacturer) Material - Name @Printer // can overwrite this on a per printer basis 
// Exta data is stored in orca_poolman in json format, it can also contain any overrides for OpenPrintTag data
// We will only update Fields such as Extruder Temp once if they do not exist, but we rely on the orca_poolman for the reconcilation of bed/extruder temp
// the data in orca_poolman will consist of a map of printers, and a overrides object for more generic data like min/max temp, usually OpenPrintTag overrides
// for e.x. 
// {
//   'printers': { "Voron": {...}, "Prusa": {...}}
//   'overrides': {...}
// }
// the orca slicer filament_notes will contain json data for the spoolman_id (optional str), printer_id (optional str),  spoolman_force_push/pull (optional bool)
// spoolman_force_push/pull will be removed after reconcilation
// if the printer_id cannot be determined (@... is not present, printer_id is not present, or compatible_printers) then reconcilation does not occur and an error message is populated in the notes
// if an error field is present AT_ALL then we do not proceed with reconcilation
// add reconcilation_status (updated_spoolman, updated_local, etc) to the filament_notes, this is ignored in terms of the actual spoolman_reconcilation
// add dryrun (optional bool) which if present and true prevents the service from making any changes and outputs the desired changes to a new entry desired_spoolman, desired_local
// last_modified just cause I don't want to deal with OS modified
async fn reconcile_config(config: FilamentConfig) {
    todo!("handle config: {:?}", config)
}
