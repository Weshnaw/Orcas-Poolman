use std::{
    collections::HashMap,
    fs::{self, File},
    io::BufReader,
    path::PathBuf,
    rc::Rc,
    sync::mpsc,
};

use derive_more::{Debug, Deref, DerefMut, Display, Error, From, Into};
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

#[derive(Debug, Deref, DerefMut, From, Into, Default)]
struct ConfigField(Option<Rc<str>>);

impl Serialize for ConfigField {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match &self.0 {
            Some(field) => serializer.collect_seq([field]),
            None => serializer.serialize_none(),
        }
    }
}

impl<'de> Deserialize<'de> for ConfigField {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let field: Option<Vec<Rc<str>>> = Option::deserialize(deserializer)?;
        let field: Option<Rc<str>> = field.and_then(|mut v| {
            if v.is_empty() {
                None
            } else {
                Some(v.remove(0))
            }
        });
        Ok(Self(field))
    }
}

fn parse_file(path: &PathBuf) -> Result<LocalFilamentConfig, Error> {
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

#[derive(Serialize, Deserialize, Debug, Default)]
struct LocalFilamentConfig {
    #[serde(flatten)]
    config: FilamentConfig,
    filament_notes: ConfigNotesField,
}

#[derive(Serialize, Deserialize, Debug, Default)]
struct FilamentConfig {
    name: Option<Rc<str>>,
    default_filament_colour: ConfigField,
    filament_vendor: ConfigField,
    // Spoolman Properties:
    //Material
    //Density
    //Diameter
    nozzle_temperature: ConfigField,
    //Bed Temperature

    // Open Print Tag Properties:
    // Min/Max temperatures
    // Chamber Temperatures

    // TODO: Inherits doesa a lot of heavy lifting, so idealy I would somehow chuck these values
    inherits: Option<Rc<str>>,

    // We only really need to specify fields that have a 1-1 with the spoolman tagging
    #[serde(flatten, default)]
    extra_fields: HashMap<Rc<str>, serde_json::Value>,
}

#[derive(Serialize, Deserialize, Debug, Default)]
struct ConfigNotes {
    spoolman_id: Option<u64>,
    printer_id: Option<Rc<str>>,
    spoolman_force_push: Option<bool>,
    spoolman_force_pull: Option<bool>,
    dry_run: Option<bool>,
    last_modified: Option<u64>,
    reconcilation_status: Option<ReconcilationStatus>,

    // TODO: consider using serde_json::Value instead of converting the data to strings
    #[serde(default)]
    debug: Vec<Rc<str>>,
    #[serde(default)]
    errors: Vec<Rc<str>>,
}

// Uses serde-diff to get the actually changed differences
#[derive(Serialize, Deserialize, Debug, Default)]
enum ReconcilationStatus {
    UpdateSpoolman(Rc<str>),
    UpdatedLocal(Rc<str>),
    UpdatedBoth(Rc<str>, Rc<str>),
    #[default]
    Noop,
}

#[derive(Debug, Deref, DerefMut, From, Into, Default)]
struct ConfigNotesField(ConfigNotes);
impl Serialize for ConfigNotesField {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_seq([&self.0])
    }
}

impl<'de> Deserialize<'de> for ConfigNotesField {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let field: Option<Vec<Rc<str>>> = Option::deserialize(deserializer)?;
        let field = field
            .and_then(|mut v| {
                if v.is_empty() {
                    None
                } else {
                    let notes = v.remove(0);
                    let notes: ConfigNotes = serde_json::from_str(&notes).ok()?;
                    Some(notes)
                }
            })
            .unwrap_or_default();
        Ok(Self(field))
    }
}

#[derive(Serialize, Deserialize, Debug, Default)]
struct PoolManData {
    #[serde(default)]
    printers: HashMap<Rc<str>, FilamentConfig>,
    overrides: HashMap<Rc<str>, Rc<str>>,
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
async fn reconcile_config(config: LocalFilamentConfig) {
    todo!("handle config: {:#?}", config);
}
