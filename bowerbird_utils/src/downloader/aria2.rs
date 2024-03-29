use aria2_ws::Client;
use futures::{future::BoxFuture, FutureExt};
use log::{debug, warn};
use snafu::ResultExt;
use std::time::{Duration, Instant};
use tokio::{
    process::{Child, Command},
    time::timeout,
};

use crate::{error, get_available_port, Result, WaitGroup};

pub use reqwest::header::HeaderMap;

use super::Task;

pub struct Aria2Downloader {
    client: Client,
    /// Spawned aria2 process. Will be killed when dropped.
    _child: Child,
    waitgroup: WaitGroup,
}

impl Aria2Downloader {
    pub async fn new(aria2_path: &str) -> Result<Self> {
        let token = "bowerbird";
        let ra = 30311..30400;
        let port = get_available_port(ra.clone()).ok_or_else(|| {
            error::NoAvaliablePort {
                message: format!("{:?}", ra),
            }
            .build()
        })?;
        let mut child = Command::new(aria2_path)
            .kill_on_drop(true)
            .args([
                "--no-conf",
                "--auto-file-renaming=false",
                "--enable-rpc",
                "--rpc-listen-port",
                &port.to_string(),
                "--rpc-secret",
                token,
            ])
            .spawn()
            .context(error::Aria2StartUpIo)?;
        if let Ok(r) = timeout(Duration::from_millis(100), child.wait()).await {
            return error::Aria2EarlyExited {
                status: r.context(error::Aria2StartUpIo)?,
            }
            .fail();
        };
        let client = Client::connect(&format!("ws://127.0.0.1:{port}/jsonrpc"), Some(token))
            .await
            .context(error::Aria2)?;
        Ok(Self {
            client,
            _child: child,
            waitgroup: WaitGroup::new(),
        })
    }

    fn map_hook(&self, hook: Option<super::BoxFutureResult>) -> BoxFuture<'static, ()> {
        let waitgroup = self.waitgroup.clone();
        if let Some(hook) = hook {
            async move {
                let i = Instant::now();
                if let Err(err) = hook.await {
                    warn!("error on hook: {}", err);
                }
                debug!("hook took {:?}", i.elapsed());
                waitgroup.done();
            }
            .boxed()
        } else {
            async move { waitgroup.done() }.boxed()
        }
    }

    pub async fn add_task(&self, task: Task) -> Result<()> {
        let hooks = task.hooks.map(|hooks| aria2_ws::Callbacks {
            on_download_complete: Some(self.map_hook(hooks.on_success)),
            on_error: Some(self.map_hook(hooks.on_error)),
        });
        self.client
            .add_uri(vec![task.url], task.options, None, hooks)
            .await
            .context(error::Aria2)?;
        self.waitgroup.add(1);
        Ok(())
    }

    pub async fn wait_and_shutdown(self) {
        self.waitgroup.await;
        let _ = self.client.force_shutdown().await;
    }
}
