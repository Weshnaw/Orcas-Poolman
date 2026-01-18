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
    // defaults to: (Manufacturer) Material - Name @Printer
    filament_settings_id: Option<Rc<str>>,
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

    // TODO: Inherits doesa a lot of heavy lifting, so idealy I would somehow check these values
    inherits: Option<Rc<str>>,

    // We only really need to specify fields that have a 1-1 with the spoolman tagging
    #[serde(flatten, default)]
    extra_fields: HashMap<Rc<str>, serde_json::Value>,
}

#[derive(Serialize, Deserialize, Debug, Default)]
struct ConfigNotes {
    spoolman_id: Option<u64>,
    // if the printer_id is not present the service will attempt to guess based on the compatible_printers, or inherits ; if it is unable to it will throw an error
    // setting a printer_id that does not exist will create a new entry in the PoolmanData
    printer_id: Option<Rc<str>>,
    // there should be a flag to set wether the force_push/pull should be reset to false after reconcilation 
    spoolman_force_push: Option<bool>,
    spoolman_force_pull: Option<bool>,
    dry_run: Option<bool>,
    last_modified: Option<u64>,
    reconcilation_status: Option<ReconcilationStatus>,

    // debug will be a running list, i.e. unless the user manually deletes entries (or maybe a settings to only hold x logs) it will only be appended to after reconcilation
    #[serde(default)]
    debug: Vec<DebugEntry>,

    // if error is present at all then the service will not proceed with reconcilations, maybe could add a severity enum, but lets try the simple version first
    #[serde(default)]
    errors: Vec<ErrorEntry>,
}

#[derive(Serialize, Deserialize, Debug)]
struct DebugEntry {
    data: DebugData,
    timestampe: u64,
}

#[derive(Serialize, Deserialize, Debug)]
enum DebugData {
    Reconcilation(ReconcilationStatus),
    Generic(Rc<str>),
}

#[derive(Serialize, Deserialize, Debug)]
struct ErrorEntry {
    message: Rc<str>,
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
struct PoolmanData {
    #[serde(default)]
    printers: HashMap<Rc<str>, FilamentConfig>,
    // Allows overriding the data from OpenPrintTags, or other static properties that you wish to ignore
    overrides: HashMap<Rc<str>, Rc<str>>,
}

// reconcilation prefers spoolman for data that should be static (like manufactuer, colour, etc), 
//     then the entry with the latest last_modified date, if one not present it always prefers the one with it 
async fn reconcile_config(config: LocalFilamentConfig) {
    todo!("handle config: {:#?}", config);
}
