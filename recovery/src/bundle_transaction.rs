// SPDX-License-Identifier: BSD-3-Clause
// Power-loss-safe filesystem transaction used by Recovery bundle installs.

use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

pub const TRANSACTION_DIR: &str = ".thistle-bundle-transaction";
const STAGE_DIR: &str = "stage";
const BACKUP_DIR: &str = "backup";
const JOURNAL_FILE: &str = "journal";
const JOURNAL_TMP: &str = "journal.tmp";
const JOURNAL_VERSION: &str = "THISTLE_BUNDLE_V1";
const FILES_ACTIVE_MARKER: &str = "files-active";
const FIRMWARE_WRITTEN_MARKER: &str = "firmware-written";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Prepared,
    FilesActive,
    FirmwareWritten,
}

impl Phase {
    fn as_str(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::FilesActive => "files_active",
            Self::FirmwareWritten => "firmware_written",
        }
    }

    fn parse(value: &str) -> io::Result<Self> {
        match value {
            "prepared" => Ok(Self::Prepared),
            "files_active" => Ok(Self::FilesActive),
            "firmware_written" => Ok(Self::FirmwareWritten),
            _ => Err(invalid_data("unknown bundle transaction phase")),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryAction {
    None,
    Rollback,
    Finalize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Boundary {
    StageFile(usize),
    WritePreparedJournal,
    BackupFile(usize),
    ActivateFile(usize),
    WriteFilesActiveJournal,
    WriteFirmware,
    WriteFirmwareJournal,
    SelectBootPartition,
}

#[derive(Debug)]
pub struct BundleFile {
    relative_path: PathBuf,
    contents: BundleContents,
}

#[derive(Debug)]
enum BundleContents {
    Bytes(Vec<u8>),
    File(PathBuf),
}

impl BundleFile {
    pub fn new(relative_path: impl Into<PathBuf>, data: Vec<u8>) -> io::Result<Self> {
        let relative_path = relative_path.into();
        validate_relative_path(&relative_path)?;
        Ok(Self {
            relative_path,
            contents: BundleContents::Bytes(data),
        })
    }

    pub fn from_file(
        relative_path: impl Into<PathBuf>,
        source_path: impl Into<PathBuf>,
    ) -> io::Result<Self> {
        let relative_path = relative_path.into();
        validate_relative_path(&relative_path)?;
        Ok(Self {
            relative_path,
            contents: BundleContents::File(source_path.into()),
        })
    }
}

#[derive(Debug)]
struct JournalEntry {
    had_original: bool,
    relative_path: PathBuf,
}

#[derive(Debug)]
struct Journal {
    phase: Phase,
    entries: Vec<JournalEntry>,
}

pub fn transaction_exists(root: &Path) -> bool {
    journal_path(root).exists()
}

pub fn recovery_action(
    root: &Path,
    ota_image_is_selected: bool,
    ota_image_is_valid: bool,
) -> io::Result<RecoveryAction> {
    if !transaction_exists(root) {
        return Ok(RecoveryAction::None);
    }
    let journal = read_journal(root)?;
    if journal.phase == Phase::FirmwareWritten && ota_image_is_selected && ota_image_is_valid {
        Ok(RecoveryAction::Finalize)
    } else {
        Ok(RecoveryAction::Rollback)
    }
}

/// Install already-downloaded, already-verified files as one reversible generation.
/// The successful path deliberately leaves the journal and backups in place until
/// the new firmware confirms a healthy boot.
pub fn install<F, S, B>(
    root: &Path,
    files: &[BundleFile],
    mut write_firmware: F,
    mut select_boot_partition: S,
    mut boundary: B,
) -> io::Result<()>
where
    F: FnMut() -> io::Result<()>,
    S: FnMut() -> io::Result<()>,
    B: FnMut(Boundary) -> io::Result<()>,
{
    if transaction_exists(root) {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "an unfinished bundle transaction already exists",
        ));
    }

    let tx_root = transaction_root(root);
    if tx_root.exists() {
        fs::remove_dir_all(&tx_root)?;
    }
    fs::create_dir_all(tx_root.join(STAGE_DIR))?;
    fs::create_dir_all(tx_root.join(BACKUP_DIR))?;

    let mut entries = Vec::with_capacity(files.len());
    let mut unique_paths = HashSet::new();
    let prepare_result = (|| {
        for (index, file) in files.iter().enumerate() {
            if !unique_paths.insert(file.relative_path.clone()) {
                return Err(invalid_input("duplicate bundle destination"));
            }
            boundary(Boundary::StageFile(index))?;
            let staged = tx_root.join(STAGE_DIR).join(&file.relative_path);
            stage_contents(&staged, &file.contents)?;
            entries.push(JournalEntry {
                had_original: root.join(&file.relative_path).exists(),
                relative_path: file.relative_path.clone(),
            });
        }
        boundary(Boundary::WritePreparedJournal)?;
        write_journal(
            root,
            &Journal {
                phase: Phase::Prepared,
                entries,
            },
        )
    })();

    if let Err(error) = prepare_result {
        let _ = fs::remove_dir_all(&tx_root);
        return Err(error);
    }

    let operation_result = (|| {
        let mut journal = read_journal(root)?;
        for (index, entry) in journal.entries.iter().enumerate() {
            let destination = root.join(&entry.relative_path);
            let staged = tx_root.join(STAGE_DIR).join(&entry.relative_path);
            let backup = tx_root.join(BACKUP_DIR).join(&entry.relative_path);

            if entry.had_original {
                boundary(Boundary::BackupFile(index))?;
                ensure_parent(&backup)?;
                fs::rename(&destination, &backup)?;
            }

            boundary(Boundary::ActivateFile(index))?;
            ensure_parent(&destination)?;
            fs::rename(&staged, &destination)?;
        }

        boundary(Boundary::WriteFilesActiveJournal)?;
        journal.phase = Phase::FilesActive;
        write_phase(root, journal.phase)?;

        boundary(Boundary::WriteFirmware)?;
        write_firmware()?;

        boundary(Boundary::WriteFirmwareJournal)?;
        journal.phase = Phase::FirmwareWritten;
        write_phase(root, journal.phase)?;

        boundary(Boundary::SelectBootPartition)?;
        select_boot_partition()
    })();

    if let Err(error) = operation_result {
        let rollback_error = rollback(root).err();
        return match rollback_error {
            Some(rollback_error) => Err(io::Error::new(
                io::ErrorKind::Other,
                format!("{error}; rollback also failed: {rollback_error}"),
            )),
            None => Err(error),
        };
    }

    Ok(())
}

pub fn rollback(root: &Path) -> io::Result<()> {
    if !transaction_exists(root) {
        return Ok(());
    }
    let journal = read_journal(root)?;
    let tx_root = transaction_root(root);

    for entry in journal.entries.iter().rev() {
        let destination = root.join(&entry.relative_path);
        let staged = tx_root.join(STAGE_DIR).join(&entry.relative_path);
        let backup = tx_root.join(BACKUP_DIR).join(&entry.relative_path);

        if entry.had_original {
            if backup.exists() {
                remove_file_if_present(&destination)?;
                ensure_parent(&destination)?;
                fs::rename(&backup, &destination)?;
            }
        } else if !staged.exists() {
            remove_file_if_present(&destination)?;
        }
    }

    fs::remove_dir_all(tx_root)
}

pub fn finalize(root: &Path) -> io::Result<()> {
    let tx_root = transaction_root(root);
    if tx_root.exists() {
        fs::remove_dir_all(tx_root)?;
    }
    Ok(())
}

fn transaction_root(root: &Path) -> PathBuf {
    root.join(TRANSACTION_DIR)
}

fn journal_path(root: &Path) -> PathBuf {
    transaction_root(root).join(JOURNAL_FILE)
}

fn write_journal(root: &Path, journal: &Journal) -> io::Result<()> {
    if journal.phase != Phase::Prepared || journal_path(root).exists() {
        return Err(invalid_input("the bundle journal may only be created once"));
    }
    let tx_root = transaction_root(root);
    fs::create_dir_all(&tx_root)?;
    let tmp = tx_root.join(JOURNAL_TMP);
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&tmp)?;
    writeln!(file, "{JOURNAL_VERSION}")?;
    writeln!(file, "phase={}", Phase::Prepared.as_str())?;
    for entry in &journal.entries {
        let path = entry
            .relative_path
            .to_str()
            .ok_or_else(|| invalid_input("bundle path is not UTF-8"))?;
        writeln!(file, "{}\t{}", u8::from(entry.had_original), path)?;
    }
    file.sync_all()?;
    drop(file);
    fs::rename(tmp, journal_path(root))?;
    Ok(())
}

fn read_journal(root: &Path) -> io::Result<Journal> {
    let contents = fs::read_to_string(journal_path(root))?;
    let mut lines = contents.lines();
    if lines.next() != Some(JOURNAL_VERSION) {
        return Err(invalid_data("unsupported bundle transaction journal"));
    }
    let phase_line = lines
        .next()
        .and_then(|line| line.strip_prefix("phase="))
        .ok_or_else(|| invalid_data("bundle transaction journal has no phase"))?;
    if Phase::parse(phase_line)? != Phase::Prepared {
        return Err(invalid_data("bundle journal must begin in prepared phase"));
    }
    let tx_root = transaction_root(root);
    let phase = if tx_root.join(FIRMWARE_WRITTEN_MARKER).exists() {
        Phase::FirmwareWritten
    } else if tx_root.join(FILES_ACTIVE_MARKER).exists() {
        Phase::FilesActive
    } else {
        Phase::Prepared
    };
    let mut entries = Vec::new();
    let mut unique_paths = HashSet::new();
    for line in lines {
        let (existed, path) = line
            .split_once('\t')
            .ok_or_else(|| invalid_data("malformed bundle transaction entry"))?;
        let relative_path = PathBuf::from(path);
        validate_relative_path(&relative_path)?;
        if !unique_paths.insert(relative_path.clone()) {
            return Err(invalid_data("duplicate path in bundle transaction journal"));
        }
        entries.push(JournalEntry {
            had_original: match existed {
                "0" => false,
                "1" => true,
                _ => return Err(invalid_data("invalid original-file marker")),
            },
            relative_path,
        });
    }
    if entries.is_empty() {
        return Err(invalid_data("empty bundle transaction journal"));
    }
    Ok(Journal { phase, entries })
}

fn write_phase(root: &Path, phase: Phase) -> io::Result<()> {
    let marker = match phase {
        Phase::Prepared => return Ok(()),
        Phase::FilesActive => FILES_ACTIVE_MARKER,
        Phase::FirmwareWritten => FIRMWARE_WRITTEN_MARKER,
    };
    write_synced(
        &transaction_root(root).join(marker),
        phase.as_str().as_bytes(),
    )
}

fn validate_relative_path(path: &Path) -> io::Result<()> {
    if path.as_os_str().is_empty() || path.is_absolute() {
        return Err(invalid_input("bundle destination must be relative"));
    }
    for component in path.components() {
        if !matches!(component, Component::Normal(_)) {
            return Err(invalid_input(
                "bundle destination contains unsafe components",
            ));
        }
    }
    let text = path
        .to_str()
        .ok_or_else(|| invalid_input("bundle destination is not UTF-8"))?;
    if text.contains('\t') || text.contains('\n') || text.contains('\r') {
        return Err(invalid_input(
            "bundle destination contains control characters",
        ));
    }
    Ok(())
}

fn write_synced(path: &Path, data: &[u8]) -> io::Result<()> {
    ensure_parent(path)?;
    let mut file = File::create(path)?;
    file.write_all(data)?;
    file.sync_all()
}

fn stage_contents(path: &Path, contents: &BundleContents) -> io::Result<()> {
    match contents {
        BundleContents::Bytes(data) => write_synced(path, data),
        BundleContents::File(source) => {
            ensure_parent(path)?;
            fs::copy(source, path)?;
            OpenOptions::new().write(true).open(path)?.sync_all()
        }
    }
}

fn ensure_parent(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn remove_file_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn invalid_input(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn invalid_data(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    struct TestRoot(PathBuf);

    impl TestRoot {
        fn new() -> Self {
            let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "thistle-bundle-transaction-{}-{id}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn files() -> Vec<BundleFile> {
        vec![
            BundleFile::new("drivers/display.drv.elf", b"new-driver".to_vec()).unwrap(),
            BundleFile::new("wm/default.wm.elf", b"new-wm".to_vec()).unwrap(),
        ]
    }

    fn seed_originals(root: &Path) {
        write_synced(&root.join("drivers/display.drv.elf"), b"old-driver").unwrap();
    }

    fn assert_original_generation(root: &Path) {
        assert_eq!(
            fs::read(root.join("drivers/display.drv.elf")).unwrap(),
            b"old-driver"
        );
        assert!(!root.join("wm/default.wm.elf").exists());
        assert!(!transaction_root(root).exists());
    }

    #[test]
    fn success_orders_boot_selection_last_and_preserves_rollback_state() {
        let root = TestRoot::new();
        seed_originals(&root.0);
        let events = RefCell::new(Vec::new());
        install(
            &root.0,
            &files(),
            || {
                events.borrow_mut().push("firmware");
                Ok(())
            },
            || {
                events.borrow_mut().push("boot");
                Ok(())
            },
            |_| Ok(()),
        )
        .unwrap();

        assert_eq!(*events.borrow(), ["firmware", "boot"]);
        assert_eq!(
            fs::read(root.0.join("drivers/display.drv.elf")).unwrap(),
            b"new-driver"
        );
        assert_eq!(
            fs::read(root.0.join("wm/default.wm.elf")).unwrap(),
            b"new-wm"
        );
        assert_eq!(
            recovery_action(&root.0, true, false).unwrap(),
            RecoveryAction::Rollback
        );
        assert_eq!(
            recovery_action(&root.0, false, true).unwrap(),
            RecoveryAction::Rollback
        );
        assert_eq!(
            recovery_action(&root.0, true, true).unwrap(),
            RecoveryAction::Finalize
        );
        finalize(&root.0).unwrap();
        assert!(!transaction_exists(&root.0));
    }

    #[test]
    fn every_activation_boundary_rolls_back_on_failure() {
        let boundaries = [
            Boundary::StageFile(0),
            Boundary::StageFile(1),
            Boundary::WritePreparedJournal,
            Boundary::BackupFile(0),
            Boundary::ActivateFile(0),
            Boundary::ActivateFile(1),
            Boundary::WriteFilesActiveJournal,
            Boundary::WriteFirmware,
            Boundary::WriteFirmwareJournal,
            Boundary::SelectBootPartition,
        ];

        for failed_boundary in boundaries {
            let root = TestRoot::new();
            seed_originals(&root.0);
            let result = install(
                &root.0,
                &files(),
                || Ok(()),
                || Ok(()),
                |boundary| {
                    if boundary == failed_boundary {
                        Err(io::Error::new(io::ErrorKind::Other, "injected failure"))
                    } else {
                        Ok(())
                    }
                },
            );
            assert!(
                result.is_err(),
                "boundary {failed_boundary:?} unexpectedly succeeded"
            );
            assert_original_generation(&root.0);
        }
    }

    #[test]
    fn recovery_restores_a_power_loss_after_each_file_move() {
        for stop_after in 0..=2 {
            let root = TestRoot::new();
            seed_originals(&root.0);
            let tx_root = transaction_root(&root.0);
            fs::create_dir_all(tx_root.join(STAGE_DIR)).unwrap();
            fs::create_dir_all(tx_root.join(BACKUP_DIR)).unwrap();
            let test_files = files();
            let entries: Vec<_> = test_files
                .iter()
                .map(|file| {
                    write_synced(
                        &tx_root.join(STAGE_DIR).join(&file.relative_path),
                        match &file.contents {
                            BundleContents::Bytes(data) => data,
                            BundleContents::File(_) => unreachable!(),
                        },
                    )
                    .unwrap();
                    JournalEntry {
                        had_original: root.0.join(&file.relative_path).exists(),
                        relative_path: file.relative_path.clone(),
                    }
                })
                .collect();
            write_journal(
                &root.0,
                &Journal {
                    phase: Phase::Prepared,
                    entries,
                },
            )
            .unwrap();

            for entry in read_journal(&root.0)
                .unwrap()
                .entries
                .iter()
                .take(stop_after)
            {
                let destination = root.0.join(&entry.relative_path);
                let staged = tx_root.join(STAGE_DIR).join(&entry.relative_path);
                let backup = tx_root.join(BACKUP_DIR).join(&entry.relative_path);
                if entry.had_original {
                    ensure_parent(&backup).unwrap();
                    fs::rename(&destination, backup).unwrap();
                }
                ensure_parent(&destination).unwrap();
                fs::rename(staged, destination).unwrap();
            }

            rollback(&root.0).unwrap();
            assert_original_generation(&root.0);
        }
    }

    #[test]
    fn journal_rejects_path_traversal() {
        assert!(BundleFile::new("../outside", vec![]).is_err());
        assert!(BundleFile::new("/absolute", vec![]).is_err());
    }

    #[test]
    fn file_backed_artifacts_are_copied_into_the_transaction_stage() {
        let root = TestRoot::new();
        let source = root.0.join("downloaded-driver");
        write_synced(&source, b"verified-driver").unwrap();
        let file = BundleFile::from_file("drivers/display.drv.elf", &source).unwrap();

        install(&root.0, &[file], || Ok(()), || Ok(()), |_| Ok(())).unwrap();

        assert_eq!(
            fs::read(root.0.join("drivers/display.drv.elf")).unwrap(),
            b"verified-driver"
        );
    }
}
