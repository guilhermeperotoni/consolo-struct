//! Reserva exclusiva do download até a aplicação/descarte, com arquivo temporário próprio.
use super::{error, net, UpdateError};
use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const DOWNLOAD_LOCK: &str = ".consolostruct-update.lock";
pub const INSTALL_LOCK: &str = ".consolostruct-install.lock";
static NEXT_FILE: AtomicU64 = AtomicU64::new(0);

pub struct ProcessLock {
    _file: File,
}

fn open_lock(path: &Path) -> Result<File, UpdateError> {
    if std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(error("permission", "A trava de atualização não pode ser um link simbólico."));
    }
    OpenOptions::new().read(true).write(true).create(true).truncate(false)
        .open(path).map_err(|e| net::io_error(e).into())
}

fn try_lock(file: &File) -> Result<(), UpdateError> {
    file.try_lock().map_err(|e| match e {
        TryLockError::WouldBlock => error("busy", "Outra cópia do ConsoloStruct já está baixando ou instalando uma atualização."),
        TryLockError::Error(e) => net::io_error(e).into(),
    })
}

impl ProcessLock {
    pub fn acquire(dir: &Path) -> Result<Self, UpdateError> {
        let file = open_lock(&dir.join(DOWNLOAD_LOCK))?;
        try_lock(&file)?;
        // No Windows, o processo auxiliar mantém esta segunda trava depois que
        // o aplicativo fecha. A ordem é sempre não bloqueante; não há deadlock.
        let installing = open_lock(&dir.join(INSTALL_LOCK))?;
        try_lock(&installing)?;
        drop(installing);
        // Não remover o arquivo de trava: remover o inode permitiria outra trava
        // independente. O SO libera a reserva ao fechar o handle ou terminar o processo.
        Ok(Self { _file: file })
    }
}

pub struct Transaction {
    pub path: PathBuf,
    #[cfg(windows)]
    pub install_lock: PathBuf,
    _lock: ProcessLock,
    cleanup: bool,
}

impl Transaction {
    pub fn begin(dir: &Path, file_name: &str) -> Result<Self, UpdateError> {
        std::fs::create_dir_all(dir).map_err(net::io_error)?;
        let lock = ProcessLock::acquire(dir)?;
        let suffix = if file_name.ends_with(".exe") { "exe" } else { "part" };
        for _ in 0..32 {
            let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
            let sequence = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
            let path = dir.join(format!(".consolostruct-{}-{stamp}-{sequence}.{suffix}", std::process::id()));
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)] {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match options.open(&path) {
                Ok(file) => {
                    drop(file);
                    return Ok(Self { path, #[cfg(windows)] install_lock: dir.join(INSTALL_LOCK), _lock: lock, cleanup: true });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(net::io_error(e).into()),
            }
        }
        Err(error("io", "Não foi possível reservar um arquivo exclusivo para a atualização."))
    }

    #[cfg(windows)]
    pub fn transfer_cleanup(&mut self) {
        // O auxiliar só confirma a transferência depois de abrir/conferir o pacote
        // e manter a trava da instalação. Ele apaga o pacote ao terminar o NSIS.
        self.cleanup = false;
    }
}

impl Drop for Transaction {
    fn drop(&mut self) {
        if self.cleanup { let _ = std::fs::remove_file(&self.path); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn temp(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("consolostruct-transaction-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn ready_package_keeps_the_lock_and_discard_releases_it() {
        let dir = temp("ready");
        let transaction = Transaction::begin(&dir, "update.AppImage").unwrap();
        std::fs::write(&transaction.path, b"verified package").unwrap();
        let path = transaction.path.clone();
        assert_eq!(ProcessLock::acquire(&dir).err().unwrap().kind, "busy");
        assert_eq!(std::fs::read(&path).unwrap(), b"verified package");
        drop(transaction);
        assert!(!path.exists());
        assert!(ProcessLock::acquire(&dir).is_ok());
    }

    #[test]
    fn retries_have_distinct_paths_and_do_not_touch_other_files() {
        let dir = temp("unique");
        let unrelated = dir.join("update.exe");
        std::fs::write(&unrelated, b"do not change").unwrap();
        let first = Transaction::begin(&dir, "update.exe").unwrap();
        let first_path = first.path.clone();
        drop(first);
        let second = Transaction::begin(&dir, "update.exe").unwrap();
        assert_ne!(first_path, second.path);
        assert_eq!(second.path.extension().unwrap(), "exe");
        assert_eq!(std::fs::read(unrelated).unwrap(), b"do not change");
    }

    #[test]
    fn installer_guard_blocks_a_new_download_after_application_exit() {
        let dir = temp("handoff");
        let transaction = Transaction::begin(&dir, "update.exe").unwrap();
        let installer = open_lock(&dir.join(INSTALL_LOCK)).unwrap();
        installer.try_lock().unwrap();
        drop(transaction);
        assert_eq!(ProcessLock::acquire(&dir).err().unwrap().kind, "busy");
        drop(installer);
        assert!(ProcessLock::acquire(&dir).is_ok());
    }

    #[test]
    fn interrupted_download_discards_its_own_file_and_releases_the_reservation() {
        let dir = temp("cancel");
        let transaction = Transaction::begin(&dir, "update.AppImage").unwrap();
        let path = transaction.path.clone();
        let result = super::super::net::write_verified(
            &b"download"[..], &path, 8, &"0".repeat(64),
            &std::sync::atomic::AtomicBool::new(true), &mut |_, _| {}
        );
        assert!(matches!(result, Err(super::super::net::NetError::Cancelled)));
        drop(transaction);
        assert!(!path.exists());
        assert!(Transaction::begin(&dir, "update.AppImage").is_ok());
    }

    #[test]
    fn child_lock_probe() {
        let Some(dir) = std::env::var_os("CONSOLO_LOCK_TEST_DIR") else { return; };
        let dir = PathBuf::from(dir);
        let _guard = ProcessLock::acquire(&dir).unwrap();
        std::fs::write(dir.join("child-ready"), b"ok").unwrap();
        std::thread::sleep(std::time::Duration::from_secs(15));
    }

    #[test]
    fn kernel_lock_is_released_after_a_process_is_terminated() {
        use std::process::{Command, Stdio};
        use std::time::{Duration, Instant};
        let dir = temp("process-exit");
        let ready = dir.join("child-ready");
        let _ = std::fs::remove_file(&ready);
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "update::transaction::tests::child_lock_probe", "--nocapture"])
            .env("CONSOLO_LOCK_TEST_DIR", &dir)
            .stdout(Stdio::null()).stderr(Stdio::inherit()).spawn().unwrap();
        let start = Instant::now();
        while !ready.exists() {
            if start.elapsed() > Duration::from_secs(10) {
                let _ = child.kill();
                let _ = child.wait();
                panic!("child did not acquire the lock");
            }
            assert!(child.try_wait().unwrap().is_none());
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(ProcessLock::acquire(&dir).err().unwrap().kind, "busy");
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(ProcessLock::acquire(&dir).is_ok());
        // O arquivo continua presente; não é necessário apagar uma trava "velha".
        assert!(dir.join(DOWNLOAD_LOCK).is_file());
    }
}
