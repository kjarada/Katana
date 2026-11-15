//! A person's settings: the window's own file, in QSettings' place (the plan's section 4.11).
//!
//! The C++ keeps a person's state in QSettings - the online data keys (`online/keys/<provider>`),
//! the appearance (`toolBarNames`, `toolBarIcons`, `textSize`), the start screen
//! (`start/showAtStart`, `start/coordinateSystem`), recent scripts, the project folder, the GDAL
//! toolbox's pipeline algorithms, the import placement's choices, the window's place and layout. On
//! Windows that is the registry, with the layout as Qt's binary blob, which the Rust window cannot
//! read; so it keeps a file of its own: JSON through katana_core::json, in the per-user
//! configuration folder (`%APPDATA%\Katana\settings.json` on Windows,
//! `$XDG_CONFIG_HOME/katana/settings.json` elsewhere, else `~/.config/katana/settings.json`).
//!
//! In memory in a test and in a headless run, which reads and writes no per-user place, as the
//! C++'s headless window keeps no layout and no settings. Keys are QSettings' paths ("start/
//! showAtStart"); each window's agent keeps the C++'s key for what it stores.
//!
//! Rejected: reading QSettings (the registry's binary layout on Windows); eframe's persistence (ron
//! and serde beside the port's one JSON).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use katana_core::error::make_error;
use katana_core::json::Json;
use katana_core::path_text::{environment_variable, path_from_utf8, path_to_utf8};
use katana_core::{ErrorCode, Result, Status};

/// One setting's value.
#[derive(Clone, Debug, PartialEq)]
pub enum SettingValue {
    Text(String),
    Number(f64),
    Flag(bool),
    List(Vec<String>),
    /// Opaque bytes - a saved layout - kept in the file as hexadecimal.
    Bytes(Vec<u8>),
}

/// The settings, by key, and the file they are kept in (none: in memory).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UserSettings {
    values: BTreeMap<String, SettingValue>,
    file: Option<PathBuf>,
}

fn path_text(path: &Path) -> String {
    String::from_utf8_lossy(&path_to_utf8(path)).into_owned()
}

fn hex_of(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push_str(&format!("{byte:02x}"));
    }
    text
}

fn bytes_of_hex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&text[at..at + 2], 16).ok())
        .collect()
}

impl UserSettings {
    /// Settings kept in memory only: a test's and a headless run's.
    pub fn in_memory() -> Self {
        Self::default()
    }

    /// The person's settings file: read when it is there, empty when it is not. Refused when it is
    /// there and cannot be read as settings.
    pub fn open(file: PathBuf) -> Result<Self> {
        let mut settings = match std::fs::read(&file) {
            Ok(bytes) => {
                Self::from_json(&bytes).map_err(|error| error.with_context(path_text(&file)))?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(error) => {
                return Err(make_error(
                    ErrorCode::FileImportFailure,
                    format!("the settings file could not be read: {error}"),
                    path_text(&file),
                ));
            }
        };
        settings.file = Some(file);
        Ok(settings)
    }

    /// Where the person's settings file is (see the module comment); empty when no folder can be
    /// told.
    pub fn default_path() -> PathBuf {
        #[cfg(windows)]
        {
            let app_data = environment_variable(Some("APPDATA"));
            if !app_data.is_empty() {
                return path_from_utf8(&app_data)
                    .join("Katana")
                    .join("settings.json");
            }
            PathBuf::new()
        }
        #[cfg(not(windows))]
        {
            // The XDG base directory specification: a value that is not an absolute path is
            // invalid and must be ignored.
            let xdg = environment_variable(Some("XDG_CONFIG_HOME"));
            if !xdg.is_empty() {
                let base = path_from_utf8(&xdg);
                if base.is_absolute() {
                    return base.join("katana").join("settings.json");
                }
            }
            let home = environment_variable(Some("HOME"));
            if home.is_empty() {
                return PathBuf::new();
            }
            path_from_utf8(&home)
                .join(".config")
                .join("katana")
                .join("settings.json")
        }
    }

    /// The file the settings are kept in; None in memory.
    pub fn file(&self) -> Option<&Path> {
        self.file.as_deref()
    }

    pub fn value(&self, key: &str) -> Option<&SettingValue> {
        self.values.get(key)
    }

    pub fn contains(&self, key: &str) -> bool {
        self.values.contains_key(key)
    }

    /// The text at `key`; None when there is none or it is not text.
    pub fn text(&self, key: &str) -> Option<&str> {
        match self.values.get(key) {
            Some(SettingValue::Text(text)) => Some(text),
            _ => None,
        }
    }

    pub fn number(&self, key: &str) -> Option<f64> {
        match self.values.get(key) {
            Some(SettingValue::Number(number)) => Some(*number),
            _ => None,
        }
    }

    pub fn flag(&self, key: &str) -> Option<bool> {
        match self.values.get(key) {
            Some(SettingValue::Flag(flag)) => Some(*flag),
            _ => None,
        }
    }

    pub fn list(&self, key: &str) -> Option<&[String]> {
        match self.values.get(key) {
            Some(SettingValue::List(list)) => Some(list),
            _ => None,
        }
    }

    pub fn bytes(&self, key: &str) -> Option<&[u8]> {
        match self.values.get(key) {
            Some(SettingValue::Bytes(bytes)) => Some(bytes),
            _ => None,
        }
    }

    pub fn set(&mut self, key: &str, value: SettingValue) {
        self.values.insert(String::from(key), value);
    }

    pub fn set_text(&mut self, key: &str, text: &str) {
        self.set(key, SettingValue::Text(String::from(text)));
    }

    pub fn set_number(&mut self, key: &str, number: f64) {
        self.set(key, SettingValue::Number(number));
    }

    pub fn set_flag(&mut self, key: &str, flag: bool) {
        self.set(key, SettingValue::Flag(flag));
    }

    pub fn set_list(&mut self, key: &str, list: Vec<String>) {
        self.set(key, SettingValue::List(list));
    }

    pub fn set_bytes(&mut self, key: &str, bytes: Vec<u8>) {
        self.set(key, SettingValue::Bytes(bytes));
    }

    /// QSettings::remove of one key. Whether there was one.
    pub fn remove(&mut self, key: &str) -> bool {
        self.values.remove(key).is_some()
    }

    /// The keys directly under `group` ("online/keys"), without the group: QSettings::childKeys
    /// inside beginGroup.
    pub fn child_keys(&self, group: &str) -> Vec<String> {
        let prefix = format!("{group}/");
        self.values
            .keys()
            .filter_map(|key| key.strip_prefix(&prefix))
            .filter(|rest| !rest.contains('/'))
            .map(String::from)
            .collect()
    }

    /// The settings as their file's text: one JSON object, the keys in order.
    pub fn to_json(&self) -> String {
        let members = self.values.iter().map(|(key, value)| {
            let json = match value {
                SettingValue::Text(text) => Json::String(text.clone()),
                SettingValue::Number(number) => Json::NumberFloat(*number),
                SettingValue::Flag(flag) => Json::Boolean(*flag),
                SettingValue::List(list) => {
                    Json::Array(list.iter().map(|item| Json::String(item.clone())).collect())
                }
                SettingValue::Bytes(bytes) => {
                    Json::object_of([("bytes", Json::String(hex_of(bytes)))])
                }
            };
            (key.clone(), json)
        });
        Json::object_of(members).dump_with(2, ' ', false)
    }

    /// Settings read from their file's text. A value of a kind no setting has is passed over, as
    /// QSettings passes over what it cannot read.
    pub fn from_json(text: &[u8]) -> Result<Self> {
        let json = Json::parse(text).map_err(|error| {
            make_error(
                ErrorCode::ParseFailure,
                "the settings are not JSON",
                format!("{error}"),
            )
        })?;
        if !json.is_object() {
            return Err(make_error(
                ErrorCode::ParseFailure,
                "the settings are not a JSON object",
                String::new(),
            ));
        }
        let mut values = BTreeMap::new();
        for (key, item) in json.items() {
            let value = match item {
                Json::String(text) => Some(SettingValue::Text(text.clone())),
                Json::Boolean(flag) => Some(SettingValue::Flag(*flag)),
                Json::NumberFloat(number) => Some(SettingValue::Number(*number)),
                Json::NumberInteger(number) => Some(SettingValue::Number(*number as f64)),
                Json::NumberUnsigned(number) => Some(SettingValue::Number(*number as f64)),
                Json::Array(items) => Some(SettingValue::List(
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(String::from))
                        .collect(),
                )),
                Json::Object(_) => item
                    .find("bytes")
                    .and_then(Json::as_str)
                    .and_then(bytes_of_hex)
                    .map(SettingValue::Bytes),
                Json::Null => None,
            };
            if let Some(value) = value {
                values.insert(key.into_owned(), value);
            }
        }
        Ok(Self { values, file: None })
    }

    /// Writes the settings to their file, making its folder; nothing in memory.
    pub fn save(&self) -> Status {
        let Some(file) = &self.file else {
            return Ok(());
        };
        let written = file
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(file, self.to_json()));
        written.map_err(|error| {
            make_error(
                ErrorCode::FileExportFailure,
                format!("the settings file could not be written: {error}"),
                path_text(file),
            )
        })
    }
}
