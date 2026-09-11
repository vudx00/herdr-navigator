//! Installation-local preferences only. Nothing read here authorizes an action.
use std::{collections::HashSet, fmt, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::model::{Entry, EntryAction, EntryIdentity};

const FILE: &str = "navigation-history.json";
const LOCK: &str = "navigation-history.lock";
const MAX_BYTES: usize = 1024 * 1024;
const MAX_RECORDS: usize = 4096;
const MAX_STRING: usize = 16 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProviderKey {
    owner: String,
    namespace: String,
    provider: String,
    item: String,
    action: String,
    operation: Operation,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Operation {
    Folder {
        path: String,
        template: Option<String>,
    },
    Project {
        path: String,
        template: String,
    },
    Attach {
        name: String,
        remote: Option<String>,
    },
    Remote {
        target: String,
    },
    Plugin {
        action: String,
    },
    Command,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Preference {
    Provider {
        key: ProviderKey,
    },
    // A removed unresolved record is a tombstone. Exact provenance prevents reimport.
    Unresolved {
        source: LegacySource,
        value: String,
        removed: bool,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
enum LegacySource {
    Pins,
    PreviousWorkspace,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Record {
    pub(crate) id: String,
    preference: Preference,
}
impl Record {
    fn visible(&self) -> bool {
        !matches!(
            self.preference,
            Preference::Unresolved { removed: true, .. }
        )
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u32,
    provider_owner: String,
    records: Vec<Record>,
}

#[derive(Debug)]
pub(crate) enum Error {
    NotCommitted(String),
    Uncertain(String),
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotCommitted(reason) => write!(f, "history unchanged: {reason}"),
            Self::Uncertain(reason) => write!(f, "history replacement occurred but durability is uncertain; reopen before another change: {reason}"),
        }
    }
}
impl std::error::Error for Error {}
impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::NotCommitted(error.to_string())
    }
}
fn invalid(reason: impl Into<String>) -> Error {
    Error::NotCommitted(reason.into())
}

/// The directory is frozen at construction. Failed reads never become empty state.
pub(crate) struct Store {
    directory: PathBuf,
    document: Result<Document, String>,
    write_failure: Option<String>,
}
impl Store {
    pub(crate) fn open(directory: PathBuf) -> Self {
        let directory = std::path::absolute(&directory);
        match directory {
            Ok(directory) => {
                let document = transact(&directory, |_| Ok(()), Fault::None)
                    .map_err(|error| error.to_string());
                Self {
                    directory,
                    document,
                    write_failure: None,
                }
            }
            Err(error) => Self {
                directory: PathBuf::new(),
                document: Err(error.to_string()),
                write_failure: None,
            },
        }
    }

    pub(crate) fn diagnostic(&self) -> Option<&str> {
        self.write_failure
            .as_deref()
            .or_else(|| self.document.as_ref().err().map(String::as_str))
    }

    pub(crate) fn records(&self) -> Result<Vec<&Record>, String> {
        self.document
            .as_ref()
            .map(|doc| doc.records.iter().filter(|r| r.visible()).collect())
            .map_err(Clone::clone)
    }

    pub(crate) fn key(
        &self,
        entry: &Entry,
        directory_template: Option<&str>,
    ) -> Result<Option<ProviderKey>, String> {
        let EntryIdentity::Provider {
            provider,
            item,
            action,
            ..
        } = entry.key()
        else {
            return Ok(None);
        };
        let doc = self.document.as_ref().map_err(Clone::clone)?;
        let path = || {
            entry
                .path
                .as_ref()
                .ok_or("provider path missing")?
                .process_path()?
                .to_str()
                .map(str::to_owned)
                .ok_or_else(|| "provider path is not UTF-8".to_string())
        };
        let operation = match &entry.action {
            EntryAction::FocusOrCreateDir => Operation::Folder {
                path: path()?,
                template: directory_template.map(str::to_owned),
            },
            EntryAction::OpenProject => Operation::Project {
                path: path()?,
                template: entry
                    .template_file
                    .as_ref()
                    .ok_or("project template identity missing")?
                    .process_path()?
                    .to_str()
                    .ok_or("project template path is not UTF-8")?
                    .into(),
            },
            EntryAction::AttachSession { name, remote } => Operation::Attach {
                name: name.clone(),
                remote: remote.clone(),
            },
            EntryAction::OpenRemote { target } => Operation::Remote {
                target: target.clone(),
            },
            EntryAction::InvokePluginAction { action } => Operation::Plugin {
                action: action.clone(),
            },
            EntryAction::RunCommand { .. } => Operation::Command,
            _ => return Err("provider has no durable preference identity".into()),
        };
        let (namespace, provider) = provider.preference_parts();
        Ok(Some(ProviderKey {
            owner: doc.provider_owner.clone(),
            namespace: namespace.into(),
            provider: provider.into(),
            item: item.clone(),
            action: action.clone(),
            operation,
        }))
    }

    pub(crate) fn contains(&self, key: &ProviderKey) -> bool {
        self.document.as_ref().is_ok_and(|doc| {
            doc.records.iter().any(|record|
            matches!(&record.preference, Preference::Provider { key: saved } if saved == key))
        })
    }

    pub(crate) fn set_pin(&mut self, key: ProviderKey, pinned: bool) -> Result<(), String> {
        self.change(|doc| {
            if key.owner != doc.provider_owner {
                return Err(invalid("provider preference owner changed"));
            }
            let existing = doc.records.iter().position(|record| {
                matches!(&record.preference, Preference::Provider { key: saved } if saved == &key)
            });
            match (existing, pinned) {
                (Some(index), false) => {
                    doc.records.remove(index);
                }
                (None, true) => doc.records.push(Record {
                    id: random_id()?,
                    preference: Preference::Provider { key },
                }),
                _ => {}
            }
            Ok(())
        })
    }

    pub(crate) fn remove(&mut self, id: &str) -> Result<(), String> {
        self.change(|doc| {
            let index = doc
                .records
                .iter()
                .position(|record| record.id == id && record.visible())
                .ok_or_else(|| invalid("record ID not found"))?;
            match &mut doc.records[index].preference {
                Preference::Unresolved { removed, .. } => *removed = true,
                Preference::Provider { .. } => {
                    doc.records.remove(index);
                }
            }
            Ok(())
        })
    }

    fn change(
        &mut self,
        change: impl FnOnce(&mut Document) -> Result<(), Error>,
    ) -> Result<(), String> {
        self.change_with_fault(change, Fault::None)
    }

    fn change_with_fault(
        &mut self,
        change: impl FnOnce(&mut Document) -> Result<(), Error>,
        fault: Fault,
    ) -> Result<(), String> {
        if let Some(diagnostic) = self.diagnostic() {
            return Err(diagnostic.into());
        }
        match transact(&self.directory, change, fault) {
            Ok(doc) => {
                self.document = Ok(doc);
                Ok(())
            }
            Err(error) => {
                let diagnostic = error.to_string();
                // Fail closed even for precommit errors. Reopening requires a fresh bounded read.
                self.write_failure = Some(diagnostic.clone());
                Err(diagnostic)
            }
        }
    }
}

fn valid_id(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
fn validate(doc: &Document) -> Result<(), Error> {
    if doc.version != 1 {
        return Err(invalid("unsupported history version"));
    }
    if !valid_id(&doc.provider_owner) || doc.records.len() > MAX_RECORDS {
        return Err(invalid("invalid owner or history record capacity exceeded"));
    }
    let mut ids = HashSet::new();
    let mut keys = HashSet::new();
    let mut legacy = HashSet::new();
    for record in &doc.records {
        if !valid_id(&record.id) || !ids.insert(&record.id) {
            return Err(invalid("invalid or duplicate history record ID"));
        }
        match &record.preference {
            Preference::Provider { key } => {
                if key.owner != doc.provider_owner
                    || !matches!(key.namespace.as_str(), "builtin" | "command")
                    || [&key.provider, &key.item, &key.action]
                        .iter()
                        .any(|s| s.trim().is_empty())
                    || !keys.insert(key)
                {
                    return Err(invalid("invalid or duplicate provider preference"));
                }
                let valid = match &key.operation {
                    Operation::Folder { path, template } => {
                        !path.is_empty() && template.as_ref().is_none_or(|s| !s.is_empty())
                    }
                    Operation::Project { path, template } => {
                        !path.is_empty() && !template.is_empty()
                    }
                    Operation::Attach { name, remote } => {
                        !name.is_empty() && remote.as_ref().is_none_or(|s| !s.is_empty())
                    }
                    Operation::Remote { target } => !target.is_empty(),
                    Operation::Plugin { action } => !action.is_empty(),
                    Operation::Command => true,
                };
                if !valid {
                    return Err(invalid("incomplete provider preference"));
                }
            }
            Preference::Unresolved { source, value, .. } => {
                if value.trim().is_empty() || !legacy.insert((source, value)) {
                    return Err(invalid("invalid or duplicate legacy provenance"));
                }
            }
        }
    }
    let value = serde_json::to_value(doc).map_err(|e| invalid(e.to_string()))?;
    fn strings(value: &serde_json::Value) -> bool {
        match value {
            serde_json::Value::String(s) => s.len() <= MAX_STRING,
            serde_json::Value::Array(items) => items.iter().all(strings),
            serde_json::Value::Object(items) => items
                .iter()
                .all(|(k, v)| k.len() <= MAX_STRING && strings(v)),
            _ => true,
        }
    }
    if !strings(&value) {
        return Err(invalid("history string capacity exceeded"));
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Fault {
    None,
    #[cfg(test)]
    BeforeRename,
    #[cfg(test)]
    AfterRename,
}

fn transact(
    directory: &std::path::Path,
    change: impl FnOnce(&mut Document) -> Result<(), Error>,
    fault: Fault,
) -> Result<Document, Error> {
    let dir = disk::Directory::open(directory)?;
    let _lock = dir.lock()?;
    let original = dir.read(FILE)?;
    let mut doc = match &original {
        Some(bytes) => serde_json::from_slice::<Document>(bytes)
            .map_err(|e| invalid(format!("invalid history: {e}")))?,
        None => Document {
            version: 1,
            provider_owner: random_id()?,
            records: Vec::new(),
        },
    };
    validate(&doc)?;
    let before = doc.clone();
    for (name, source) in [
        ("pinned-entries.json", LegacySource::Pins),
        ("jump-back-workspace", LegacySource::PreviousWorkspace),
    ] {
        let Some(bytes) = dir.read(name)? else {
            continue;
        };
        let values = match source {
            LegacySource::Pins => serde_json::from_slice::<Vec<String>>(&bytes)
                .map_err(|e| invalid(format!("legacy pins unresolved; invalid import: {e}")))?,
            LegacySource::PreviousWorkspace => vec![String::from_utf8(bytes)
                .map_err(|e| invalid(format!("legacy workspace unresolved: {e}")))?],
        };
        if values.len() > MAX_RECORDS
            || values
                .iter()
                .any(|v| v.trim().is_empty() || v.len() > MAX_STRING)
        {
            return Err(invalid("legacy import incomplete or capacity exceeded"));
        }
        for value in values {
            let imported = doc.records.iter().any(|record| {
                matches!(&record.preference, Preference::Unresolved { source: old_source, value: old_value, .. } if *old_source == source && old_value == &value)
            });
            if !imported {
                doc.records.push(Record {
                    id: random_id()?,
                    preference: Preference::Unresolved {
                        source,
                        value,
                        removed: false,
                    },
                });
            }
        }
    }
    change(&mut doc)?;
    validate(&doc)?;
    if original.is_none() || before != doc {
        let bytes = serde_json::to_vec_pretty(&doc).map_err(|e| invalid(e.to_string()))?;
        if bytes.len() > MAX_BYTES {
            return Err(invalid("history byte capacity exceeded"));
        }
        dir.replace(&bytes, fault)?;
    }
    Ok(doc)
}

fn random_id() -> Result<String, Error> {
    use std::io::Read;
    let mut bytes = [0u8; 16];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(unix)]
mod disk {
    use super::*;
    use std::{
        ffi::CString,
        fs::File,
        io::{self, Read, Write},
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::fs::MetadataExt,
        },
        path::{Component, Path},
        thread,
        time::{Duration, Instant},
    };

    pub(super) struct Directory(File);
    fn name(value: &str) -> Result<CString, Error> {
        CString::new(value).map_err(|_| invalid("NUL in history filename"))
    }
    fn openat(parent: &File, name: &std::ffi::CStr, flags: i32) -> io::Result<File> {
        // The returned descriptor is owned here. O_NOFOLLOW rejects symlink leaves.
        let fd = unsafe {
            libc::openat(
                parent.as_raw_fd(),
                name.as_ptr(),
                flags | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
                0o600,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { File::from_raw_fd(fd) })
    }
    fn private_regular(file: &File) -> Result<(), Error> {
        let meta = file.metadata()?;
        if !meta.is_file()
            || meta.nlink() != 1
            || meta.uid() != unsafe { libc::geteuid() }
            || meta.mode() & 0o022 != 0
        {
            return Err(invalid(
                "history file must be an owner-controlled regular file with one link",
            ));
        }
        Ok(())
    }
    impl Directory {
        pub(super) fn open(path: &Path) -> Result<Self, Error> {
            if !path.is_absolute() {
                return Err(invalid("history directory must be absolute"));
            }
            let mut directory = File::open("/")?;
            for part in path.components() {
                let part = match part {
                    Component::RootDir | Component::CurDir => continue,
                    Component::Normal(value) => {
                        use std::os::unix::ffi::OsStrExt;
                        CString::new(value.as_bytes())
                            .map_err(|_| invalid("NUL in history directory"))?
                    }
                    _ => return Err(invalid("unsafe history directory component")),
                };
                match openat(&directory, &part, libc::O_RDONLY | libc::O_DIRECTORY) {
                    Ok(next) => directory = next,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        let result =
                            unsafe { libc::mkdirat(directory.as_raw_fd(), part.as_ptr(), 0o700) };
                        if result != 0
                            && io::Error::last_os_error().kind() != io::ErrorKind::AlreadyExists
                        {
                            return Err(io::Error::last_os_error().into());
                        }
                        // Persist newly created path entries before reporting durable data below.
                        directory.sync_all()?;
                        directory = openat(&directory, &part, libc::O_RDONLY | libc::O_DIRECTORY)?;
                    }
                    Err(error) => return Err(error.into()),
                }
            }
            let meta = directory.metadata()?;
            if meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o022 != 0 {
                return Err(invalid("history directory is not owner-controlled"));
            }
            Ok(Self(directory))
        }
        pub(super) fn lock(&self) -> Result<File, Error> {
            let lock = openat(&self.0, &name(LOCK)?, libc::O_RDWR | libc::O_CREAT)?;
            private_regular(&lock)?;
            let start = Instant::now();
            loop {
                if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                    return Ok(lock);
                }
                let error = io::Error::last_os_error();
                if error.kind() != io::ErrorKind::WouldBlock {
                    return Err(error.into());
                }
                if start.elapsed() >= Duration::from_millis(250) {
                    return Err(invalid("history lock wait expired"));
                }
                thread::sleep(Duration::from_millis(5));
            }
        }
        pub(super) fn read(&self, filename: &str) -> Result<Option<Vec<u8>>, Error> {
            let file = match openat(&self.0, &name(filename)?, libc::O_RDONLY) {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(error.into()),
            };
            private_regular(&file)?;
            if file.metadata()?.len() > MAX_BYTES as u64 {
                return Err(invalid(format!("{filename} exceeds history byte capacity")));
            }
            let mut bytes = Vec::new();
            file.take((MAX_BYTES + 1) as u64).read_to_end(&mut bytes)?;
            if bytes.len() > MAX_BYTES {
                return Err(invalid("history grew beyond byte capacity"));
            }
            Ok(Some(bytes))
        }
        pub(super) fn replace(&self, bytes: &[u8], fault: Fault) -> Result<(), Error> {
            let temp = name(&format!(".navigation-history-{}.tmp", random_id()?))?;
            let mut file = openat(
                &self.0,
                &temp,
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
            )?;
            let result = (|| {
                file.write_all(bytes)?;
                file.flush()?;
                file.sync_all()?;
                #[cfg(test)]
                if fault == Fault::BeforeRename {
                    return Err(invalid("injected before rename"));
                }
                let _ = fault;
                if unsafe {
                    libc::renameat(
                        self.0.as_raw_fd(),
                        temp.as_ptr(),
                        self.0.as_raw_fd(),
                        name(FILE)?.as_ptr(),
                    )
                } != 0
                {
                    return Err(io::Error::last_os_error().into());
                }
                #[cfg(test)]
                if fault == Fault::AfterRename {
                    return Err(Error::Uncertain("injected after rename".into()));
                }
                self.0
                    .sync_all()
                    .map_err(|e| Error::Uncertain(e.to_string()))
            })();
            // Never unlink the stable lock. Only this writer's private temp is removed.
            unsafe {
                libc::unlinkat(self.0.as_raw_fd(), temp.as_ptr(), 0);
            }
            result
        }
    }
}

#[cfg(not(unix))]
mod disk {
    use super::*;
    pub(super) struct Directory;
    impl Directory {
        pub(super) fn open(_: &std::path::Path) -> Result<Self, Error> {
            Err(invalid("history requires Unix advisory locking"))
        }
        pub(super) fn lock(&self) -> Result<(), Error> {
            Err(invalid("history unsupported"))
        }
        pub(super) fn read(&self, _: &str) -> Result<Option<Vec<u8>>, Error> {
            Err(invalid("history unsupported"))
        }
        pub(super) fn replace(&self, _: &[u8], _: Fault) -> Result<(), Error> {
            Err(invalid("history unsupported"))
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::{symlink, MetadataExt, PermissionsExt},
        sync::{Arc, Barrier},
        thread,
    };

    fn write(path: impl AsRef<std::path::Path>, bytes: impl AsRef<[u8]>) -> std::io::Result<()> {
        use std::{io::Write, os::unix::fs::OpenOptionsExt};
        let mut file = fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(bytes.as_ref())
    }
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("navigator-history-{}", random_id().unwrap()));
            fs::create_dir(&path).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
            Self(path)
        }
        fn store(&self) -> Store {
            Store::open(self.0.clone())
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn provider(store: &Store, item: &str) -> ProviderKey {
        ProviderKey {
            owner: store.document.as_ref().unwrap().provider_owner.clone(),
            namespace: "command".into(),
            provider: "fixture".into(),
            item: item.into(),
            action: "open {id}".into(),
            operation: Operation::Command,
        }
    }
    fn fixture() -> Document {
        serde_json::from_str(include_str!(
            "../tests/fixtures/history/provider-pins-v1.json"
        ))
        .unwrap()
    }

    #[test]
    fn version_one_round_trips_without_rewriting_and_records_have_stable_ids() {
        let dir = Temp::new();
        let bytes = include_bytes!("../tests/fixtures/history/provider-pins-v1.json");
        write(dir.0.join(FILE), bytes).unwrap();
        let mut store = dir.store();
        assert!(store.diagnostic().is_none());
        assert_eq!(fs::read(dir.0.join(FILE)).unwrap(), bytes);
        let key = provider(&store, "work");
        assert!(store.contains(&key));
        store.set_pin(key, true).unwrap();
        assert_eq!(fs::read(dir.0.join(FILE)).unwrap(), bytes);
        assert_eq!(
            store.records().unwrap()[0].id,
            dir.store().records().unwrap()[0].id
        );
    }

    #[test]
    fn independent_stores_do_not_share_owners_but_a_copy_copies_preferences_only() {
        let a = Temp::new();
        let b = Temp::new();
        let mut first = a.store();
        let second = b.store();
        let key = provider(&first, "same");
        assert_ne!(key, provider(&second, "same"));
        first.set_pin(key.clone(), true).unwrap();
        assert!(!second.contains(&key));
        fs::copy(a.0.join(FILE), b.0.join(FILE)).unwrap();
        assert!(b.store().contains(&key));
    }

    #[test]
    fn legacy_fixtures_import_unresolved_once_preserve_bytes_and_remember_removal() {
        let cases: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/history/legacy-cases.json"))
                .unwrap();
        for case in cases.as_array().unwrap() {
            let dir = Temp::new();
            let bytes = case["pins"].as_str().unwrap().as_bytes();
            write(dir.0.join("pinned-entries.json"), bytes).unwrap();
            write(dir.0.join("jump-back-workspace"), b"w1\n").unwrap();
            let mut store = dir.store();
            if case["error"].as_bool() == Some(true) {
                assert!(store.diagnostic().is_some(), "{case}");
                assert!(!dir.0.join(FILE).exists());
            } else {
                let count = case["values"].as_array().unwrap().len() + 1;
                assert_eq!(store.records().unwrap().len(), count);
                assert!(store
                    .records()
                    .unwrap()
                    .iter()
                    .all(|r| matches!(r.preference, Preference::Unresolved { .. })));
                let before = fs::read(dir.0.join(FILE)).unwrap();
                assert_eq!(dir.store().records().unwrap().len(), count);
                assert_eq!(fs::read(dir.0.join(FILE)).unwrap(), before);
                let ids: Vec<_> = store
                    .records()
                    .unwrap()
                    .iter()
                    .map(|r| r.id.clone())
                    .collect();
                for id in ids {
                    store.remove(&id).unwrap();
                }
                assert!(store.records().unwrap().is_empty());
                assert!(dir.store().records().unwrap().is_empty());
                assert_eq!(dir.store().document.unwrap().records.len(), count);
            }
            assert_eq!(fs::read(dir.0.join("pinned-entries.json")).unwrap(), bytes);
            assert_eq!(
                fs::read(dir.0.join("jump-back-workspace")).unwrap(),
                b"w1\n"
            );
        }
    }

    #[test]
    fn changed_legacy_file_adds_only_new_provenance_without_resurrecting_removed_records() {
        let dir = Temp::new();
        write(dir.0.join("pinned-entries.json"), r#"["w1"]"#).unwrap();
        let mut store = dir.store();
        let id = store.records().unwrap()[0].id.clone();
        store.remove(&id).unwrap();
        write(dir.0.join("pinned-entries.json"), r#"["w1","w2"]"#).unwrap();
        let reopened = dir.store();
        assert_eq!(reopened.records().unwrap().len(), 1);
        assert!(
            matches!(&reopened.records().unwrap()[0].preference, Preference::Unresolved { value, .. } if value == "w2")
        );
    }

    #[test]
    fn invalid_future_duplicate_and_oversized_documents_disable_writes_without_replacement() {
        let mut future = fixture();
        future.version = 2;
        let mut duplicate_id = fixture();
        duplicate_id.records.push(duplicate_id.records[0].clone());
        let mut duplicate_key = duplicate_id.clone();
        duplicate_key.records[1].id = "3".repeat(32);
        let mut foreign = fixture();
        let Preference::Provider { key } = &mut foreign.records[0].preference else {
            panic!()
        };
        key.owner = "9".repeat(32);
        let mut long = fixture();
        let Preference::Provider { key } = &mut long.records[0].preference else {
            panic!()
        };
        key.item = "x".repeat(MAX_STRING + 1);
        let mut cases: Vec<Vec<u8>> = [future, duplicate_id, duplicate_key, foreign, long]
            .iter()
            .map(|d| serde_json::to_vec(d).unwrap())
            .collect();
        cases.extend([
            b"{".to_vec(),
            br#"{"version":"1"}"#.to_vec(),
            vec![b' '; MAX_BYTES + 1],
        ]);
        for bytes in cases {
            let dir = Temp::new();
            write(dir.0.join(FILE), &bytes).unwrap();
            let mut store = dir.store();
            assert!(store.records().is_err());
            assert!(store.remove("anything").is_err());
            assert_eq!(fs::read(dir.0.join(FILE)).unwrap(), bytes);
        }
    }

    #[test]
    fn invalid_legacy_input_blocks_changes_to_an_existing_valid_store() {
        for bytes in [b"[".to_vec(), vec![b' '; MAX_BYTES + 1]] {
            let dir = Temp::new();
            let mut store = dir.store();
            let key = provider(&store, "work");
            let before = fs::read(dir.0.join(FILE)).unwrap();
            write(dir.0.join("pinned-entries.json"), &bytes).unwrap();
            assert!(store.set_pin(key, true).is_err());
            assert_eq!(fs::read(dir.0.join(FILE)).unwrap(), before);
            assert_eq!(fs::read(dir.0.join("pinned-entries.json")).unwrap(), bytes);
        }
    }

    #[test]
    fn capacity_rejection_does_not_evict_or_overwrite_records() {
        let dir = Temp::new();
        let mut store = dir.store();
        let key = provider(&store, "new");
        let doc = store.document.as_mut().unwrap();
        doc.records = (0..MAX_RECORDS)
            .map(|i| Record {
                id: format!("{i:032x}"),
                preference: Preference::Unresolved {
                    source: LegacySource::Pins,
                    value: format!("w{i}"),
                    removed: true,
                },
            })
            .collect();
        let before = serde_json::to_vec(doc).unwrap();
        assert!(before.len() < MAX_BYTES);
        write(dir.0.join(FILE), &before).unwrap();
        assert!(store.set_pin(key, true).unwrap_err().contains("capacity"));
        assert_eq!(fs::read(dir.0.join(FILE)).unwrap(), before);
    }

    #[test]
    fn independent_writers_reload_under_the_stable_lock_without_lost_updates() {
        let dir = Temp::new();
        let mut first = dir.store();
        let mut second = dir.store();
        let a = provider(&first, "a");
        let b = provider(&second, "b");
        let lock_inode = fs::metadata(dir.0.join(LOCK)).unwrap().ino();
        let barrier = Arc::new(Barrier::new(2));
        let other = barrier.clone();
        thread::scope(|threads| {
            threads.spawn(|| {
                barrier.wait();
                first.set_pin(a.clone(), true).unwrap();
            });
            threads.spawn(|| {
                other.wait();
                second.set_pin(b.clone(), true).unwrap();
            });
        });
        let read = dir.store();
        assert!(read.contains(&a));
        assert!(read.contains(&b));
        assert_eq!(read.records().unwrap().len(), 2);
        assert_eq!(fs::metadata(dir.0.join(LOCK)).unwrap().ino(), lock_inode);
        first.set_pin(a.clone(), false).unwrap();
        assert!(!dir.store().contains(&a));
        assert!(dir.store().contains(&b));
    }

    #[test]
    fn bounded_lock_wait_leaves_state_and_lock_inode_untouched() {
        let dir = Temp::new();
        let mut store = dir.store();
        let key = provider(&store, "new");
        let before = fs::read(dir.0.join(FILE)).unwrap();
        let disk = disk::Directory::open(&dir.0).unwrap();
        let _held = disk.lock().unwrap();
        let now = std::time::Instant::now();
        assert!(store
            .set_pin(key, true)
            .unwrap_err()
            .contains("wait expired"));
        assert!(now.elapsed() < std::time::Duration::from_secs(2));
        assert_eq!(fs::read(dir.0.join(FILE)).unwrap(), before);
        assert!(dir.0.join(LOCK).exists());
    }

    #[test]
    fn failure_before_rename_preserves_old_bytes_and_after_rename_reports_uncertainty() {
        let dir = Temp::new();
        let store = dir.store();
        let key = provider(&store, "new");
        let before = fs::read(dir.0.join(FILE)).unwrap();
        for fault in [Fault::BeforeRename, Fault::AfterRename] {
            let result = transact(
                &dir.0,
                |doc| {
                    doc.records.push(Record {
                        id: random_id()?,
                        preference: Preference::Provider { key: key.clone() },
                    });
                    Ok(())
                },
                fault,
            );
            if fault == Fault::BeforeRename {
                assert!(matches!(result, Err(Error::NotCommitted(_))));
                assert_eq!(fs::read(dir.0.join(FILE)).unwrap(), before);
            } else {
                assert!(matches!(result, Err(Error::Uncertain(_))));
                assert!(dir.store().contains(&key));
            }
            assert_eq!(
                fs::read_dir(&dir.0).unwrap().count(),
                2,
                "no temp files remain"
            );
        }
        assert_eq!(
            fs::metadata(dir.0.join(FILE)).unwrap().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(dir.0.join(LOCK)).unwrap().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn corrupt_reload_never_falls_back_to_the_cached_document() {
        let dir = Temp::new();
        let mut store = dir.store();
        let key = provider(&store, "new");
        write(dir.0.join(FILE), b"broken").unwrap();
        assert!(store.set_pin(key.clone(), true).is_err());
        assert_eq!(fs::read(dir.0.join(FILE)).unwrap(), b"broken");
        fs::remove_file(dir.0.join(FILE)).unwrap();
        assert!(
            store.set_pin(key, true).is_err(),
            "a failed store needs explicit reopening"
        );
        assert!(!dir.0.join(FILE).exists());
    }

    #[test]
    fn symlinks_hardlinks_unsafe_permissions_and_nonregular_files_are_rejected() {
        let outside = Temp::new();
        write(outside.0.join("sentinel"), b"untouched").unwrap();
        for filename in [FILE, LOCK, "pinned-entries.json", "jump-back-workspace"] {
            let dir = Temp::new();
            symlink(outside.0.join("sentinel"), dir.0.join(filename)).unwrap();
            assert!(dir.store().diagnostic().is_some());
            assert_eq!(fs::read(outside.0.join("sentinel")).unwrap(), b"untouched");
        }
        let parent = Temp::new();
        symlink(&outside.0, parent.0.join("link")).unwrap();
        assert!(Store::open(parent.0.join("link/subdir"))
            .diagnostic()
            .is_some());
        assert!(!outside.0.join("subdir").exists());
        let dir = Temp::new();
        fs::hard_link(outside.0.join("sentinel"), dir.0.join(FILE)).unwrap();
        assert!(dir.store().diagnostic().is_some());
        fs::remove_file(dir.0.join(FILE)).unwrap();
        fs::create_dir(dir.0.join(FILE)).unwrap();
        assert!(dir.store().diagnostic().is_some());
        fs::remove_dir(dir.0.join(FILE)).unwrap();
        fs::set_permissions(&dir.0, fs::Permissions::from_mode(0o777)).unwrap();
        assert!(dir.store().diagnostic().is_some());
        fs::set_permissions(&dir.0, fs::Permissions::from_mode(0o700)).unwrap();
        let fifo = std::ffi::CString::new(dir.0.join(FILE).to_str().unwrap()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        assert!(dir.store().diagnostic().is_some());
    }

    #[test]
    fn namespace_item_and_configured_action_changes_never_inherit_a_pin() {
        let dir = Temp::new();
        let mut store = dir.store();
        let key = provider(&store, "work");
        store.set_pin(key.clone(), true).unwrap();
        let mut variants = Vec::new();
        let mut changed = key.clone();
        changed.namespace = "builtin".into();
        variants.push(changed);
        let mut changed = key.clone();
        changed.provider = "other".into();
        variants.push(changed);
        let mut changed = key.clone();
        changed.item = "other".into();
        variants.push(changed);
        let mut changed = key;
        changed.action = "other".into();
        variants.push(changed);
        for changed in variants {
            assert!(!store.contains(&changed));
        }
    }
    #[test]
    fn postrename_uncertainty_keeps_visible_snapshot_and_disables_retries() {
        let dir = Temp::new();
        let mut store = dir.store();
        let key = provider(&store, "work");
        let error = store
            .change_with_fault(
                |doc| {
                    doc.records.push(Record {
                        id: random_id()?,
                        preference: Preference::Provider { key: key.clone() },
                    });
                    Ok(())
                },
                Fault::AfterRename,
            )
            .unwrap_err();
        assert!(error.contains("durability is uncertain"));
        assert!(
            !store.contains(&key),
            "failed mutation does not publish a new mark"
        );
        let after = fs::read(dir.0.join(FILE)).unwrap();
        assert!(store.set_pin(key.clone(), false).is_err());
        assert_eq!(fs::read(dir.0.join(FILE)).unwrap(), after);
        assert!(
            dir.store().contains(&key),
            "explicit reopen observes the renamed document"
        );
    }

    #[test]
    fn byte_capacity_and_read_permission_errors_preserve_the_document() {
        let dir = Temp::new();
        let mut store = dir.store();
        let before = fs::read(dir.0.join(FILE)).unwrap();
        let result = store.change(|doc| {
            for i in 0..100 {
                doc.records.push(Record {
                    id: format!("{i:032x}"),
                    preference: Preference::Unresolved {
                        source: LegacySource::Pins,
                        value: format!("{i}{}", "x".repeat(MAX_STRING - 4)),
                        removed: false,
                    },
                });
            }
            Ok(())
        });
        assert!(result.unwrap_err().contains("byte capacity"));
        assert_eq!(fs::read(dir.0.join(FILE)).unwrap(), before);
        if unsafe { libc::geteuid() } != 0 {
            fs::set_permissions(dir.0.join(FILE), fs::Permissions::from_mode(0o000)).unwrap();
            assert!(dir.store().diagnostic().is_some());
            fs::set_permissions(dir.0.join(FILE), fs::Permissions::from_mode(0o600)).unwrap();
            assert_eq!(fs::read(dir.0.join(FILE)).unwrap(), before);
        }
    }
}
