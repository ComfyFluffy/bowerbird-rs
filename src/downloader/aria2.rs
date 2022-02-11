use std::time::Duration;

use aria2_ws::Client;
use futures::{future::BoxFuture, FutureExt};
pub use reqwest::header::HeaderMap;
use snafu::ResultExt;
use tokio::{
    process::{Child, Command},
    time::timeout,
};

use crate::{
    error,
    log::warning,
    utils::{get_available_port, WaitGroup},
};

use super::Task;

pub struct Aria2Downloader {
    client: Client,
    child: Child,
    waitgroup: WaitGroup,
}

impl Drop for Aria2Downloader {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

impl Aria2Downloader {
    pub async fn new(aria2_path: &str) -> crate::Result<Self> {
        let token = "bowerbird";
        let ra = 30311..30400;
        let port = get_available_port(ra.clone()).ok_or(
            error::NoAvaliablePort {
                message: format!("{:?}", ra),
            }
            .build(),
        )?;
        let mut child = Command::new(aria2_path)
            .args(&[
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
        match timeout(Duration::from_millis(100), child.wait()).await {
            Ok(r) => {
                return error::Aria2StartUpExit {
                    status: r.context(error::Aria2StartUpIo)?,
                }
                .fail();
            } // aria2 exited unexpectedly
            Err(_) => {} // aria2 continues to run
        };
        let client = Client::connect(&format!("ws://127.0.0.1:{}/jsonrpc", port), Some(token))
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
                if let Err(err) = hook.await {
                    warning!("error on hook: {}", err);
                }
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
        let _ = self.client.force_shutdown().await;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
