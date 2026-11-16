//! Upgrade controlado com pacotes oficiais, exclusivamente em host descartável de CI.
//! Não é compilado no aplicativo e não executa instalação numa sessão local comum.
use super::*;
use std::fs;
use std::process::Command;
use std::time::Instant;

fn wait_until(label: &str, mut ready: impl FnMut() -> bool) {
    let start = Instant::now();
    while !ready() {
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "timeout: {label}"
        );
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn pinned_offer(payload: &serde_json::Value, version: &str) -> Offer {
    // Seleciona somente uma release já autenticada pelo envelope. Os metadados
    // individuais continuam sujeitos à assinatura de pacote normal do produto.
    let mut subset = payload.clone();
    let app = subset["apps"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|a| a["id"] == PRODUCT_ID)
        .unwrap();
    app["releases"]
        .as_array_mut()
        .unwrap()
        .retain(|r| r["version"] == version);
    let Selection::Available(offer) = catalog::select(
        &subset,
        &official_trust(),
        &Version::parse("0.0.0").unwrap(),
        target(),
    )
    .unwrap() else {
        panic!("official signed release {version} is required for the controlled test")
    };
    assert_eq!(offer.version.to_string(), version);
    offer
}

fn download(client: &Client, offer: &Offer, path: &Path) {
    client
        .download(
            &offer.asset.url,
            DOWNLOAD_HOSTS,
            path,
            offer.asset.size,
            &offer.asset.sha256,
            &AtomicBool::new(false),
            &mut |_, _| {},
        )
        .unwrap();
}

struct Env(Vec<(String, Option<std::ffi::OsString>)>);
impl Env {
    fn set(&mut self, key: &str, value: impl AsRef<std::ffi::OsStr>) {
        self.0.push((key.into(), std::env::var_os(key)));
        std::env::set_var(key, value);
    }
}
impl Drop for Env {
    fn drop(&mut self) {
        for (key, value) in self.0.drain(..).rev() {
            match value {
                Some(v) => std::env::set_var(key, v),
                None => std::env::remove_var(key),
            }
        }
    }
}

#[cfg(windows)]
fn ps(script: &str, exe: &Path) -> String {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let output = Command::new(
        PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32/WindowsPowerShell/v1.0/powershell.exe"),
    )
    .args(["-NoProfile", "-NonInteractive", "-EncodedCommand"])
    .arg(STANDARD.encode(bytes))
    .env("CONSOLO_SMOKE_EXE", exe)
    .output()
    .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

#[cfg(windows)]
fn file_version(exe: &Path) -> String {
    ps("if (Test-Path -LiteralPath $env:CONSOLO_SMOKE_EXE) { [System.Diagnostics.FileVersionInfo]::GetVersionInfo($env:CONSOLO_SMOKE_EXE).ProductVersion }", exe)
}

#[cfg(windows)]
fn visible_process(exe: &Path) -> bool {
    !ps("Get-Process -Name consolostruct -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $env:CONSOLO_SMOKE_EXE -and $_.MainWindowHandle -ne 0 } | ForEach-Object { $_.Id }", exe).is_empty()
}

#[cfg(target_os = "linux")]
fn window(version: &str) -> Option<String> {
    let pattern = format!("ConsoloStruct.*{}", version.replace('.', "[.]"));
    let output = Command::new("xdotool")
        .args(["search", "--onlyvisible", "--name", &pattern])
        .output()
        .unwrap();
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .map(str::to_owned)
}

#[test]
#[ignore = "installs official packages on a disposable CI host only"]
fn official_upgrade() {
    if std::env::var("CONSOLO_NATIVE_UPGRADE_CI").as_deref() != Ok("1")
        || std::env::var("CI").as_deref() != Ok("true")
    {
        eprintln!("SKIPPED: requires explicit opt-in on a disposable CI host");
        return;
    }
    let root = std::env::temp_dir().join(format!(
        "consolostruct-native-upgrade-{}-{}",
        std::process::id(),
        now_secs()
    ));
    fs::create_dir_all(&root).unwrap();
    #[cfg(windows)]
    let root = PathBuf::from(
        root.canonicalize()
            .unwrap()
            .to_string_lossy()
            .trim_start_matches("\\\\?\\")
            .to_owned(),
    );
    let client = Client::new("ConsoloStruct-native-upgrade-CI");
    let bytes = client
        .fetch_small(CATALOG_URL, CATALOG_HOSTS, CHECK_TIMEOUT)
        .unwrap();
    let payload = catalog::open_envelope(&bytes, &official_trust()).unwrap();
    let old = pinned_offer(&payload, "0.7.0");
    let new = pinned_offer(&payload, "0.7.1");
    let old_package = root.join(&old.file_name);
    download(&client, &old, &old_package);
    let install_dir = root.join("installed");
    fs::create_dir_all(&install_dir).unwrap();
    let cache = root.join("updater-cache");
    let mut transaction = Transaction::begin(&cache, &new.file_name).unwrap();
    download(&client, &new, &transaction.path);
    let project = root.join("preserved-project.lume.json");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/casos-validacao.lume.json"),
        &project,
    )
    .unwrap();
    let project_before = net::file_sha256(&project).unwrap();
    let mut environment = Env(Vec::new());
    let mut sentinels = Vec::new();

    #[cfg(windows)]
    {
        // Isola o perfil do WebView2 e verifica também as pastas reais de dados do app
        // do usuário descartável do runner (não há dados pessoais neste host).
        environment.set("WEBVIEW2_USER_DATA_FOLDER", root.join("webview-data"));
        for key in ["LOCALAPPDATA", "APPDATA"] {
            if let Some(base) = std::env::var_os(key) {
                let dir = PathBuf::from(base).join("org.engilume.lumeconsolo");
                fs::create_dir_all(&dir).unwrap();
                let sentinel = dir.join("native-upgrade-ci-project.lume.json");
                fs::copy(&project, &sentinel).unwrap();
                sentinels.push(sentinel);
            }
        }
        use std::os::windows::process::CommandExt;
        let status = Command::new(&old_package)
            .args(["/S", "/CURRENTUSER", "/NS"])
            .raw_arg(format!("/D={}", install_dir.display()))
            .status()
            .unwrap();
        assert!(status.success(), "old official NSIS installation failed");
        let exe = install_dir.join(old.asset.executable.as_ref().unwrap());
        assert!(file_version(&exe).starts_with("0.7.0"));
        let mut running = Command::new(&exe).spawn().unwrap();
        wait_until("old NSIS application window", || visible_process(&exe));
        assert!(matches!(
            install::detect(&Probe {
                os: "windows",
                debug: false,
                exe: &exe,
                appimage: None,
                product_id: PRODUCT_ID
            }),
            InstallMode::WindowsNsis { .. }
        ));
        apply(
            &new,
            &mut transaction,
            &InstallMode::WindowsNsis {
                dir: install_dir.clone(),
            },
        )
        .unwrap();
        drop(transaction); // O aplicativo de origem fecha; o auxiliar protege o NSIS.
        wait_until("NSIS completion", || ProcessLock::acquire(&cache).is_ok());
        wait_until("new NSIS application window", || {
            file_version(&exe).starts_with("0.7.1") && visible_process(&exe)
        });
        assert!(
            running.try_wait().unwrap().is_some(),
            "old executable was not closed"
        );
        println!("NATIVE_UPGRADE_SMOKE platform=windows kind=nsis from=0.7.0 to=0.7.1 executed_version={} window=visible", file_version(&exe));
        ps("Get-Process -Name consolostruct -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $env:CONSOLO_SMOKE_EXE } | Stop-Process -Force", &exe);
    }

    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::PermissionsExt;
        environment.set("XDG_DATA_HOME", root.join("user-data"));
        environment.set("XDG_CONFIG_HOME", root.join("user-config"));
        environment.set("XDG_CACHE_HOME", root.join("user-cache"));
        environment.set("APPIMAGE_EXTRACT_AND_RUN", "1");
        let data = root.join("user-data/org.engilume.lumeconsolo");
        fs::create_dir_all(&data).unwrap();
        let sentinel = data.join("native-upgrade-ci-project.lume.json");
        fs::copy(&project, &sentinel).unwrap();
        sentinels.push(sentinel);
        let image = install_dir.join("consolostruct.AppImage");
        fs::copy(&old_package, &image).unwrap();
        fs::set_permissions(&image, fs::Permissions::from_mode(0o755)).unwrap();
        let mut running = Command::new(&image).spawn().unwrap();
        wait_until("old AppImage window", || window("0.7.0").is_some());
        let old_window = window("0.7.0").unwrap();
        // Reproduz o ambiente herdado da instância executada pelo AppImage.
        let pid = Command::new("xdotool")
            .args(["getwindowpid", &old_window])
            .output()
            .unwrap();
        let pid = String::from_utf8_lossy(&pid.stdout).trim().to_owned();
        let env_bytes = fs::read(format!("/proc/{pid}/environ")).unwrap();
        let mut runtime = Env(Vec::new());
        for entry in env_bytes.split(|b| *b == 0) {
            let Some((key, value)) = std::str::from_utf8(entry)
                .ok()
                .and_then(|s| s.split_once('='))
            else {
                continue;
            };
            if [
                "APPIMAGE",
                "APPDIR",
                "ARGV0",
                "OWD",
                "LD_LIBRARY_PATH",
                "XDG_DATA_DIRS",
                "GSETTINGS_SCHEMA_DIR",
                "GTK_PATH",
            ]
            .contains(&key)
            {
                runtime.set(key, value);
            }
        }
        assert!(matches!(
            install::detect(&Probe {
                os: "linux",
                debug: false,
                exe: Path::new("/tmp/.mount/usr/bin/consolostruct"),
                appimage: Some(&image),
                product_id: PRODUCT_ID
            }),
            InstallMode::LinuxAppImage { .. }
        ));
        apply(
            &new,
            &mut transaction,
            &InstallMode::LinuxAppImage {
                path: image.clone(),
            },
        )
        .unwrap();
        drop(runtime);
        // Simula o fechamento da janela de origem após o retorno de update_apply.
        Command::new("xdotool")
            .args(["windowclose", &old_window])
            .status()
            .unwrap();
        drop(transaction);
        wait_until("new AppImage window", || window("0.7.1").is_some());
        assert_eq!(
            net::file_sha256(&image).unwrap(),
            (new.asset.size, new.asset.sha256.clone())
        );
        assert!(ProcessLock::acquire(&cache).is_ok());
        println!("NATIVE_UPGRADE_SMOKE platform=linux kind=appimage from=0.7.0 to=0.7.1 executed_version=0.7.1 window=visible");
        if let Some(w) = window("0.7.1") {
            let _ = Command::new("xdotool").args(["windowclose", &w]).status();
        }
        let _ = running.kill();
        let _ = running.wait();
    }
    assert_eq!(net::file_sha256(&project).unwrap(), project_before);
    for sentinel in sentinels {
        assert_eq!(net::file_sha256(&sentinel).unwrap(), project_before);
        let _ = fs::remove_file(sentinel);
    }
    println!("NATIVE_UPGRADE_DATA project_and_app_data=preserved");
    drop(environment);
    let _ = fs::remove_dir_all(root);
}
