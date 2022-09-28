use aria2_ws::Client;
use futures::{future::BoxFuture, FutureExt};
use log::{debug, warn};
use snafu::ResultExt;
use std::time::{Duration, Instant};
use tokio::{
    process::{Child, Command},
    time::timeout,
};

use crate::{error, get_available_port, WaitGroup};

pub use reqwest::header::HeaderMap;

use super::Task;

pub struct Aria2Downloader {
    client: Client,
    child: Child,
    waitgroup: WaitGroup,
}

impl Drop for Aria2Downloader {
    fn drop(&mut self) {
        let r = self.child.start_kill();
        debug!("tried to kill aria2: {:?}", r);
    }
}

impl Aria2Downloader {
    pub async fn new(aria2_path: &str) -> crate::Result<Self> {
        let token = "bowerbird";
        let ra = 30311..30400;
        let port = get_available_port(ra.clone()).ok_or_else(|| {
            error::NoAvaliablePort {
                message: format!("{:?}", ra),
            }
            .build()
        })?;
        let mut child = Command::new(aria2_path)
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
            child,
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

    pub async fn add_task(&self, task: Task) -> crate::Result<()> {
        let hooks = task.hooks.map(|hooks| aria2_ws::TaskHooks {
            on_complete: Some(self.map_hook(hooks.on_success)),
            on_error: Some(self.map_hook(hooks.on_error)),
        });
        self.client
            .add_uri(vec![task.url], task.options, None, hooks)
            .await
            .context(error::Aria2)?;
        self.waitgroup.add(1);
        Ok(())
    }

    pub async fn wait_shutdown(self) {
        self.waitgroup.clone().await;
        let r = self.client.force_shutdown().await;
        debug!("tried to force shutdown aria2: {:?}", r);
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
