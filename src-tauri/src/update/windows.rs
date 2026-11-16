//! Handoff do NSIS: o auxiliar mantém uma trava do SO até o instalador terminar.
//! O script é fixo e embutido; caminhos não são interpolados em comandos de shell.
use super::{error, net, UpdateError};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Deserialize;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

struct StatusFile(PathBuf);
impl Drop for StatusFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[derive(Deserialize)]
struct Status {
    #[serde(default)]
    ready: bool,
    error: Option<String>,
    kind: Option<String>,
}

fn status(path: &Path) -> Option<Result<(), UpdateError>> {
    let raw = std::fs::read_to_string(path).ok()?;
    let status: Status = serde_json::from_str(raw.trim_start_matches('\u{feff}')).ok()?;
    if status.ready {
        Some(Ok(()))
    } else {
        status
            .error
            .map(|message| Err(error(status.kind.as_deref().unwrap_or("io"), message)))
    }
}

pub fn launch(
    installer: &Path,
    dir: &Path,
    lock: &Path,
    size: u64,
    sha256: &str,
) -> Result<Child, UpdateError> {
    if !installer.is_absolute() || !dir.is_absolute() || dir.to_string_lossy().contains('"') {
        return Err(error("state", "Pasta de instalação inválida."));
    }
    let system = std::env::var_os("SystemRoot")
        .ok_or_else(|| error("state", "Pasta do Windows indisponível."))?;
    let shell = PathBuf::from(system).join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let marker = StatusFile(installer.with_extension("status.json"));
    std::fs::File::create_new(&marker.0).map_err(net::io_error)?;
    let script: Vec<u8> = include_str!("windows-install.ps1")
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    let mut child = Command::new(shell)
        .args(["-NoProfile", "-NonInteractive", "-EncodedCommand"])
        .arg(STANDARD.encode(script))
        .env("CONSOLO_UPDATE_PACKAGE", installer)
        .env("CONSOLO_UPDATE_DIRECTORY", dir)
        .env("CONSOLO_UPDATE_LOCK", lock)
        .env("CONSOLO_UPDATE_STATUS", &marker.0)
        .env("CONSOLO_UPDATE_SIZE", size.to_string())
        .env("CONSOLO_UPDATE_SHA256", sha256)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(0x08000000) // CREATE_NO_WINDOW para o auxiliar, não para o NSIS.
        .spawn()
        .map_err(net::io_error)?;
    // Não liberar a reserva nem encerrar o aplicativo antes do ACK: o auxiliar já
    // deve possuir a trava, o pacote aberto/conferido e o processo NSIS iniciado.
    loop {
        if let Some(result) = status(&marker.0) {
            if let Err(e) = result {
                let _ = child.wait();
                return Err(e);
            }
            return Ok(child);
        }
        if child.try_wait().map_err(net::io_error)?.is_some() {
            return match status(&marker.0) {
                Some(Ok(())) => Ok(child),
                Some(Err(e)) => Err(e),
                None => Err(error("io", "O auxiliar de instalação terminou sem confirmar o início. O aplicativo continua aberto.")),
            };
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(test)]
mod tests {
    use super::super::transaction::{ProcessLock, Transaction};
    use super::*;
    use std::time::Instant;

    fn temp(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "consolostruct-windows-{name}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        path.canonicalize().unwrap()
    }
    fn wait_until(mut f: impl FnMut() -> bool) {
        let start = Instant::now();
        while !f() {
            assert!(
                start.elapsed() < Duration::from_secs(15),
                "native child timed out"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn guardian_holds_the_lock_through_the_native_installer_and_cleans_up() {
        let dir = temp("guardian");
        let mut transaction = Transaction::begin(&dir, "update.exe").unwrap();
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/update-installer.rs");
        assert!(Command::new("rustc")
            .arg(fixture)
            .arg("-o")
            .arg(&transaction.path)
            .status()
            .unwrap()
            .success());
        let (size, sha) = net::file_sha256(&transaction.path).unwrap();
        // Caminhos com espaços e caracteres de shell continuam sendo dados.
        let target = dir.join("install dir & unicode ç");
        std::fs::create_dir_all(&target).unwrap();
        let package = transaction.path.clone();
        let mut child = launch(&package, &target, &transaction.install_lock, size, &sha).unwrap();
        transaction.transfer_cleanup();
        drop(transaction); // Simula o fechamento do aplicativo de origem.
        assert_eq!(ProcessLock::acquire(&dir).err().unwrap().kind, "busy");
        wait_until(|| target.join("started.json").is_file());
        let arguments = std::fs::read_to_string(target.join("started.json")).unwrap();
        assert!(arguments.contains("/P /R /UPDATE /D="));
        assert!(arguments.contains("install dir & unicode ç"));
        std::fs::write(target.join("finish"), b"ok").unwrap();
        wait_until(|| child.try_wait().unwrap().is_some());
        assert!(ProcessLock::acquire(&dir).is_ok());
        assert!(!package.exists());
    }

    #[test]
    fn guardian_rejects_changed_bytes_without_starting_the_installer() {
        let dir = temp("integrity");
        let transaction = Transaction::begin(&dir, "update.exe").unwrap();
        std::fs::write(&transaction.path, b"not the verified installer").unwrap();
        let err = launch(
            &transaction.path,
            &dir,
            &transaction.install_lock,
            12,
            &"0".repeat(64),
        )
        .err()
        .unwrap();
        assert_eq!(err.kind, "integrity");
        assert!(!dir.join("started.json").exists());
        assert!(transaction.path.exists());
    }
}
