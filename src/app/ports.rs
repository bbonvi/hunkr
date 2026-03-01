use std::{path::Path, sync::Arc, time::Instant};

use chrono::{DateTime, Utc};

use crate::{comments::CommentStore, config::AppConfig, git_data::GitService, store::StateStore};

/// Clock abstraction used by app workflows for deterministic tests.
pub trait AppClock: Send + Sync {
    fn now_utc(&self) -> DateTime<Utc>;
    fn now_instant(&self) -> Instant;
}

/// Dependency injection interface for app bootstrap wiring.
pub trait AppBootstrapPorts: Send + Sync {
    fn open_current_git(&self) -> anyhow::Result<GitService>;
    fn load_config(&self) -> anyhow::Result<AppConfig>;
    fn state_store_for_repo(&self, repo_root: &Path) -> StateStore;
    fn open_comment_store(&self, store_root: &Path, branch: &str) -> anyhow::Result<CommentStore>;
    fn clock(&self) -> Arc<dyn AppClock>;
    fn runtime_ports(&self) -> Arc<dyn AppRuntimePorts> {
        Arc::new(SystemRuntimePorts)
    }
}

/// Runtime dependency ports used after initial bootstrap.
pub trait AppRuntimePorts: Send + Sync {
    fn open_git_at(&self, path: &Path) -> anyhow::Result<GitService>;
    fn open_comment_store(&self, store_root: &Path, branch: &str) -> anyhow::Result<CommentStore>;
}

/// System clock implementation for production runtime.
pub struct SystemClock;

impl AppClock for SystemClock {
    fn now_utc(&self) -> DateTime<Utc> {
        Utc::now()
    }

    fn now_instant(&self) -> Instant {
        Instant::now()
    }
}

/// Default bootstrap provider backed by concrete infrastructure adapters.
pub struct SystemBootstrapPorts;
pub struct SystemRuntimePorts;

impl AppRuntimePorts for SystemRuntimePorts {
    fn open_git_at(&self, path: &Path) -> anyhow::Result<GitService> {
        GitService::open_at(path)
    }

    fn open_comment_store(&self, store_root: &Path, branch: &str) -> anyhow::Result<CommentStore> {
        CommentStore::new(store_root, branch)
    }
}

impl AppBootstrapPorts for SystemBootstrapPorts {
    fn open_current_git(&self) -> anyhow::Result<GitService> {
        GitService::open_current()
    }

    fn load_config(&self) -> anyhow::Result<AppConfig> {
        AppConfig::load()
    }

    fn state_store_for_repo(&self, repo_root: &Path) -> StateStore {
        StateStore::for_project(repo_root)
    }

    fn open_comment_store(&self, store_root: &Path, branch: &str) -> anyhow::Result<CommentStore> {
        CommentStore::new(store_root, branch)
    }

    fn clock(&self) -> Arc<dyn AppClock> {
        Arc::new(SystemClock)
    }
}
