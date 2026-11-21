use anyhow::{Context as _, Result, bail};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::process::{Child, Command};

pub const WEBHOOK_SECRET: &str = "replay-secret";
const DEFAULT_WEBHOOK_PATH: &str = "/api/github/webhooks";
const TEST_PRIVATE_KEY: &str = include_str!("../test-key.pem");
const PROBOT_RUN_SCRIPT: &str = "node_modules/probot/bin/probot-run.js";
const PASSED_THROUGH_VARIABLES: [&str; 3] = ["PATH", "HOME", "TMPDIR"];

#[derive(Debug, Clone)]
pub enum AppProgram {
    NodeOriginal { repository_root: PathBuf },
    Binary { path: PathBuf },
}

impl AppProgram {
    pub fn node_original() -> Result<AppProgram> {
        let repository_root = std::env::current_dir()?;
        if !repository_root.join(PROBOT_RUN_SCRIPT).exists() {
            bail!(
                "{PROBOT_RUN_SCRIPT} not found: run replay from the repository root after npm install"
            );
        }
        Ok(AppProgram::NodeOriginal { repository_root })
    }

    pub fn binary(path: &Path) -> Result<AppProgram> {
        let path = path
            .canonicalize()
            .with_context(|| format!("app binary {} not found", path.display()))?;
        Ok(AppProgram::Binary { path })
    }

    pub fn label(&self) -> String {
        match self {
            AppProgram::NodeOriginal { .. } => "node".to_owned(),
            AppProgram::Binary { path } => path.display().to_string(),
        }
    }

    fn command(&self) -> Command {
        match self {
            AppProgram::NodeOriginal { repository_root } => {
                let mut command = Command::new("node");
                command
                    .arg(PROBOT_RUN_SCRIPT)
                    .arg("./index.js")
                    .current_dir(repository_root);
                command
            }
            AppProgram::Binary { path } => Command::new(path),
        }
    }
}

pub fn free_port() -> Result<u16> {
    Ok(std::net::TcpListener::bind("127.0.0.1:0")?
        .local_addr()?
        .port())
}

pub struct AppProcess {
    child: Child,
    pub port: u16,
    pub case_env: BTreeMap<String, String>,
    pub webhook_path: String,
    pub started_at: Instant,
    log_path: PathBuf,
}

impl AppProcess {
    pub fn start(
        program: &AppProgram,
        mock_port: u16,
        case_env: &BTreeMap<String, String>,
        log_path: &Path,
    ) -> Result<AppProcess> {
        let port = free_port()?;
        if let Some(log_directory) = log_path.parent() {
            std::fs::create_dir_all(log_directory)?;
        }
        let log_file = std::fs::File::create(log_path)
            .with_context(|| format!("creating {}", log_path.display()))?;
        let mut command = program.command();
        command.env_clear();
        for variable in PASSED_THROUGH_VARIABLES {
            if let Some(value) = std::env::var_os(variable) {
                command.env(variable, value);
            }
        }
        command
            .env("PORT", port.to_string())
            .env("APP_ID", "1")
            .env("PRIVATE_KEY", TEST_PRIVATE_KEY)
            .env("WEBHOOK_SECRET", WEBHOOK_SECRET)
            .env("GHE_HOST", format!("127.0.0.1:{mock_port}"))
            .env("GHE_PROTOCOL", "http")
            .env("LOG_LEVEL", "fatal")
            .env("NODE_ENV", "production")
            .envs(case_env)
            .stdin(Stdio::null())
            .stdout(log_file.try_clone()?)
            .stderr(log_file)
            .kill_on_drop(true);
        let started_at = Instant::now();
        let child = command
            .spawn()
            .with_context(|| format!("starting {}", program.label()))?;
        Ok(AppProcess {
            child,
            port,
            case_env: case_env.clone(),
            webhook_path: case_env
                .get("WEBHOOK_PATH")
                .cloned()
                .unwrap_or_else(|| DEFAULT_WEBHOOK_PATH.to_owned()),
            started_at,
            log_path: log_path.to_path_buf(),
        })
    }

    pub fn process_id(&self) -> Option<u32> {
        self.child.id()
    }

    pub fn ensure_running(&mut self) -> Result<()> {
        if let Some(exit_status) = self.child.try_wait()? {
            bail!(
                "app exited with {exit_status}; see {}",
                self.log_path.display()
            );
        }
        Ok(())
    }

    pub async fn wait_until_listening(&mut self, timeout: Duration) -> Result<()> {
        let deadline = Instant::now() + timeout;
        loop {
            self.ensure_running()?;
            if tokio::net::TcpStream::connect(("127.0.0.1", self.port))
                .await
                .is_ok()
            {
                return Ok(());
            }
            if Instant::now() >= deadline {
                bail!(
                    "app did not listen on port {} within {timeout:?}; see {}",
                    self.port,
                    self.log_path.display()
                );
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }

    pub async fn stop(mut self) {
        if let Err(error) = self.child.kill().await {
            eprintln!("could not stop app: {error}");
        }
    }
}
