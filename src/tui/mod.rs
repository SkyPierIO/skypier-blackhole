mod logs;
mod ui;

use crate::loader::{self, SourceSummary};
use crate::{BlocklistManager, Config, DnsServer, Result, RuntimeMetrics, UpdateScheduler};
use chrono::{DateTime, Local, Utc};
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use futures::StreamExt;
use logs::{LogBuffer, TuiLogLayer};
use ratatui::DefaultTerminal;
use std::collections::VecDeque;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use tokio::signal::unix::{signal, SignalKind};
use tokio::task::JoinHandle;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

/// Number of log lines kept in RAM for the activity panel
const LOG_CAPACITY: usize = 500;

/// Run the DNS daemon with the interactive dashboard attached
pub async fn run(config_path: &str) -> Result<()> {
    let config = Config::load(config_path)?;

    // Install log capture before anything logs: stdout belongs to the TUI,
    // so tracing events are rendered inside the activity panel instead.
    let log_buffer: LogBuffer = Arc::new(Mutex::new(VecDeque::new()));
    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(&config.logging.log_level))
        .or_else(|_| EnvFilter::try_new("info"))?;
    tracing_subscriber::registry()
        .with(filter)
        .with(TuiLogLayer::new(Arc::clone(&log_buffer), LOG_CAPACITY))
        .init();

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        "Starting Skypier Blackhole (TUI mode)"
    );

    // Blocklist + initial load
    let blocklist = Arc::new(BlocklistManager::new());
    let sources = loader::load_blocklist(&config, &blocklist).await?;

    // Update scheduler
    let config = Arc::new(config);
    let mut scheduler = UpdateScheduler::new(Arc::clone(&config), Arc::clone(&blocklist)).await?;
    if let Err(e) = scheduler.start().await {
        tracing::warn!("Failed to start update scheduler: {}", e);
    }
    let scheduler = Arc::new(scheduler);

    // DNS server with in-RAM metrics attached
    let metrics = Arc::new(RuntimeMetrics::new());
    let server = DnsServer::new((*config).clone(), Arc::clone(&blocklist))?
        .with_metrics(Arc::clone(&metrics));
    let server_task = tokio::spawn(async move { server.start().await });

    scheduler.spawn_startup_refresh();

    let mut app = App {
        config: Arc::clone(&config),
        config_path: config_path.to_string(),
        blocklist,
        scheduler,
        metrics,
        logs: log_buffer,
        sources,
        last_update: None,
        cache_mtime: None,
        total_domains: 0,
        next_run: None,
        input: None,
        updating: Arc::new(AtomicBool::new(false)),
    };
    app.refresh_summary();

    let terminal = ratatui::init();
    let result = app.run(terminal, server_task).await;
    ratatui::restore();
    result
}

/// Which action an input popup is collecting a domain for
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InputAction {
    Add,
    Remove,
}

#[derive(Debug)]
pub(crate) struct InputState {
    pub action: InputAction,
    pub buffer: String,
}

pub(crate) struct App {
    pub config: Arc<Config>,
    pub config_path: String,
    pub blocklist: Arc<BlocklistManager>,
    pub scheduler: Arc<UpdateScheduler>,
    pub metrics: Arc<RuntimeMetrics>,
    pub logs: LogBuffer,
    pub sources: Vec<SourceSummary>,
    pub last_update: Option<DateTime<Local>>,
    pub total_domains: usize,
    pub next_run: Option<DateTime<Utc>>,
    pub input: Option<InputState>,
    pub updating: Arc<AtomicBool>,
    /// Last observed mtime of the remote cache, to detect background updates
    cache_mtime: Option<SystemTime>,
}

impl App {
    async fn run(
        &mut self,
        mut terminal: DefaultTerminal,
        mut server_task: JoinHandle<Result<()>>,
    ) -> Result<()> {
        let mut events = EventStream::new();
        let mut tick = tokio::time::interval(Duration::from_millis(250));
        let mut sigterm = signal(SignalKind::terminate())?;

        loop {
            self.total_domains = self.blocklist.count().await;
            self.next_run = if self.config.updater.enabled {
                self.scheduler.next_run().await
            } else {
                None
            };
            self.detect_cache_change();

            terminal.draw(|frame| ui::draw(frame, self))?;

            tokio::select! {
                _ = tick.tick() => {}
                _ = sigterm.recv() => break,
                result = &mut server_task => {
                    return match result {
                        Ok(Ok(())) => Err(anyhow::anyhow!("DNS server stopped unexpectedly")),
                        Ok(Err(e)) => Err(e.context("DNS server error")),
                        Err(e) => Err(anyhow::anyhow!("DNS server task panicked: {e}")),
                    };
                }
                maybe_event = events.next() => {
                    match maybe_event {
                        Some(Ok(Event::Key(key))) if key.kind == KeyEventKind::Press => {
                            if self.handle_key(key).await {
                                break;
                            }
                        }
                        Some(Ok(_)) => {}
                        Some(Err(e)) => tracing::warn!(error = %e, "Terminal input error"),
                        None => break,
                    }
                }
            }
        }

        tracing::info!("Shutting down");
        Ok(())
    }

    /// Handle a key press; returns true when the app should quit
    async fn handle_key(&mut self, key: KeyEvent) -> bool {
        // Ctrl+C always quits (raw mode swallows the signal)
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return true;
        }

        if self.input.is_some() {
            match key.code {
                KeyCode::Esc => self.input = None,
                KeyCode::Enter => {
                    let state = self.input.take().expect("input mode checked above");
                    let domain = state.buffer.trim().to_string();
                    if !domain.is_empty() {
                        match state.action {
                            InputAction::Add => self.add_domain(domain).await,
                            InputAction::Remove => self.remove_domain(domain).await,
                        }
                    }
                }
                KeyCode::Backspace => {
                    if let Some(input) = &mut self.input {
                        input.buffer.pop();
                    }
                }
                KeyCode::Char(c) => {
                    if let Some(input) = &mut self.input {
                        input.buffer.push(c);
                    }
                }
                _ => {}
            }
            return false;
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return true,
            KeyCode::Char('a') => {
                self.input = Some(InputState {
                    action: InputAction::Add,
                    buffer: String::new(),
                })
            }
            KeyCode::Char('d') => {
                self.input = Some(InputState {
                    action: InputAction::Remove,
                    buffer: String::new(),
                })
            }
            KeyCode::Char('u') => self.trigger_update(),
            KeyCode::Char('r') => self.reload().await,
            _ => {}
        }
        false
    }

    /// Append a domain to the custom list and activate it immediately
    async fn add_domain(&mut self, domain: String) {
        let path = self.config.blocklist.custom_list.clone();
        let write_result = (|| -> Result<()> {
            if let Some(parent) = Path::new(&path).parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)?;
            writeln!(file, "{}", domain)?;
            Ok(())
        })();

        match write_result {
            Ok(()) => {
                if let Err(e) = self.blocklist.add_domain(domain.clone()).await {
                    tracing::error!(error = %e, "Failed to activate domain");
                    return;
                }
                tracing::info!(domain = %domain, "Domain added to custom blocklist");
                self.refresh_summary();
            }
            Err(e) => tracing::error!(error = %e, path = %path, "Failed to write custom blocklist"),
        }
    }

    /// Remove a domain from the custom list and deactivate it immediately
    async fn remove_domain(&mut self, domain: String) {
        let path = self.config.blocklist.custom_list.clone();
        let remove_result = (|| -> Result<bool> {
            let content = std::fs::read_to_string(&path)?;
            let kept: Vec<&str> = content
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty() && *line != domain)
                .collect();
            let removed = kept.len() != content.lines().filter(|l| !l.trim().is_empty()).count();
            if removed {
                std::fs::write(&path, kept.join("\n") + "\n")?;
            }
            Ok(removed)
        })();

        match remove_result {
            Ok(true) => {
                if let Err(e) = self.blocklist.remove_domain(&domain).await {
                    tracing::error!(error = %e, "Failed to deactivate domain");
                    return;
                }
                tracing::info!(domain = %domain, "Domain removed from custom blocklist");
                self.refresh_summary();
            }
            Ok(false) => tracing::warn!(domain = %domain, "Domain not found in custom blocklist"),
            Err(e) => {
                tracing::error!(error = %e, path = %path, "Failed to update custom blocklist")
            }
        }
    }

    /// Kick off a remote blocklist update in the background
    fn trigger_update(&self) {
        if self.config.blocklist.remote_lists.is_empty() {
            tracing::warn!("No remote blocklist sources configured");
            return;
        }
        if self.updating.swap(true, Ordering::SeqCst) {
            tracing::warn!("An update is already running");
            return;
        }

        let scheduler = Arc::clone(&self.scheduler);
        let updating = Arc::clone(&self.updating);
        tokio::spawn(async move {
            match scheduler.trigger_manual_update().await {
                Ok(count) => tracing::info!(domains = count, "Manual update completed"),
                Err(e) => tracing::error!(error = %e, "Manual update failed"),
            }
            updating.store(false, Ordering::SeqCst);
        });
    }

    /// Full reload of the blocklist from all files on disk
    async fn reload(&mut self) {
        tracing::info!("Reloading blocklists from disk");
        if let Err(e) = self.blocklist.clear().await {
            tracing::error!(error = %e, "Failed to clear blocklist");
            return;
        }
        match loader::load_blocklist(&self.config, &self.blocklist).await {
            Ok(sources) => self.sources = sources,
            Err(e) => tracing::error!(error = %e, "Failed to reload blocklists"),
        }
        self.refresh_summary();
    }

    /// Re-read source files and the remote cache mtime for the summary panel
    fn refresh_summary(&mut self) {
        self.sources = loader::summarize_sources(&self.config);
        self.cache_mtime = std::fs::metadata(loader::remote_cache_path(&self.config))
            .and_then(|m| m.modified())
            .ok();
        self.last_update = self.cache_mtime.map(DateTime::<Local>::from);
    }

    /// Refresh the summary when a background update rewrote the remote cache
    fn detect_cache_change(&mut self) {
        let mtime = std::fs::metadata(loader::remote_cache_path(&self.config))
            .and_then(|m| m.modified())
            .ok();
        if mtime != self.cache_mtime {
            self.refresh_summary();
        }
    }
}
