use bytes::{BufMut, BytesMut};

use std::{
    collections::BTreeMap,
    ffi::OsString,
    io::SeekFrom,
    path::{Path, PathBuf},
    pin::Pin,
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering::SeqCst},
        Arc, Mutex, RwLock,
    },
    task::{Context, Poll},
    time::{Duration, Instant},
};

use futures::{future::BoxFuture, task::AtomicWaker, Future};
use lazy_static::lazy_static;
use regex::Regex;
use reqwest::{Method, Url};
use snafu::{ErrorCompat, ResultExt};
use tokio::{
    fs::{self, File},
    io::{AsyncSeekExt, AsyncWriteExt},
    spawn,
    sync::{mpsc, Semaphore},
    task::JoinHandle,
};

use crate::{
    error,
    log::{debug, error, info, warning},
};

lazy_static! {
    static ref RE_CONTENT_DISPOSITION: Regex =
        Regex::new(r#"^attachment; filename="(.*)"$"#).unwrap();
}

#[derive(Debug)]
struct WaitGroupInner {
    num: AtomicUsize,
    waker: AtomicWaker,
}

/// A Golang-like waitgroup to wait for all tasks to complete.
#[derive(Debug, Clone)]
struct WaitGroup(Arc<WaitGroupInner>);

impl WaitGroup {
    // Inspired by an example in https://docs.rs/futures/0.3.17/futures/task/struct.AtomicWaker.html
    pub fn new() -> Self {
        Self(Arc::new(WaitGroupInner {
            num: AtomicUsize::new(0),
            waker: AtomicWaker::new(),
        }))
    }

    pub fn add(&self, n: usize) {
        self.0.num.fetch_add(n, SeqCst);
    }

    pub fn done(&self) {
        if self.0.num.fetch_sub(1, SeqCst) <= 1 {
            self.0.waker.wake();
        }
    }
}

impl Future for WaitGroup {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.0.num.load(SeqCst) == 0 {
            return Poll::Ready(());
        }
        self.0.waker.register(cx.waker());
        if self.0.num.load(SeqCst) == 0 {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

#[derive(Debug)]
pub struct Downloader {
    pub client: reqwest::Client,

    tasks_finished: Arc<RwLock<BTreeMap<u64, Task>>>,
    tasks_pending: Arc<RwLock<BTreeMap<u64, Task>>>,
    tasks_running: Arc<Mutex<BTreeMap<u64, JoinHandle<()>>>>,
    // Use mutex for sender to ensure ordering.
    task_sender: Arc<tokio::sync::Mutex<mpsc::Sender<Task>>>,
    semaphore: Arc<Semaphore>,
    waitgroup: WaitGroup,
    main_handle: Option<JoinHandle<()>>,
}

impl Drop for Downloader {
    fn drop(&mut self) {
        self.semaphore.close();
        if let Some(task_handle) = self.main_handle.take() {
            task_handle.abort();
        }
        for (_, h) in self.tasks_running.lock().unwrap().iter() {
            h.abort();
        }
    }
}

// Create a new type to impl Debug for the closure,
// so that we can derive(Debug) for Task
struct RequestBuilder(
    Box<dyn Fn(&reqwest::Client) -> crate::Result<reqwest::Request> + Send + Sync>,
);
impl std::fmt::Debug for RequestBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "RequestBuilder")
    }
}

pub type BoxError = Box<dyn std::error::Error + Send + Sync>;
pub type ClosureFuture =
    Box<dyn FnOnce(&Task) -> BoxFuture<'static, Result<(), BoxError>> + Send + Sync>;
#[derive(Default)]
pub struct TaskHooks {
    pub on_success: Option<ClosureFuture>,
    pub on_error: Option<ClosureFuture>,
}
fn print_some<T>(t: &Option<T>) -> &str {
    if t.is_some() {
        "Some(..)"
    } else {
        "None"
    }
}
impl std::fmt::Debug for TaskHooks {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("TaskHooks")
            .field("on_success", &print_some(&self.on_success))
            .field("on_error", &print_some(&self.on_error))
            .finish()
    }
}

#[derive(Debug)]
pub struct Task {
    // Get unique ID for each task.
    id: u64,

    request_builder: RequestBuilder,

    pub hooks: Option<TaskHooks>,
    pub options: TaskOptions,
    pub status: TaskStatus,
    pub hook_status: TaskStatus,

    pub file_size: Option<u64>,
    pub url: Url,
}

static TASK_ID: AtomicU64 = AtomicU64::new(0);

impl Task {
    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn build_request(&self, client: &reqwest::Client) -> crate::Result<reqwest::Request> {
        (self.request_builder.0)(client)
    }

    pub fn new(
        request_builder: Box<
            dyn Fn(&reqwest::Client) -> crate::Result<reqwest::Request> + Send + Sync,
        >,
        url: Url,
        options: TaskOptions,
        hooks: Option<TaskHooks>,
    ) -> Task {
        Task {
            request_builder: RequestBuilder(request_builder),
            options,
            hook_status: if hooks.is_none() {
                TaskStatus::Skipped
            } else {
                TaskStatus::Pending
            },
            hooks,
            id: TASK_ID.fetch_add(1, SeqCst),
            file_size: None,
            status: TaskStatus::default(),
            url,
        }
    }
}

/// Options of the task. If `path` is None, the downloader will try to get
/// filename from headers or url, than join `dir` with the filename.
///
/// If both `path` and `dir` are none, it will cause an error while downloading.
#[derive(Debug, Clone)]
pub struct TaskOptions {
    pub path: Option<PathBuf>,
    pub dir: Option<PathBuf>,
    /// If `true`, the downloader will skip the task.
    /// If `path` is not set, the downloader will still try to `HEAD` to get the filename.
    pub skip_exists: bool,
    /// Max retries in last 1 minutes.
    ///
    /// Tasks are automatically retried on network error.
    /// When a try fail, the downloader will check for the tries in last minutes,
    /// if the tries reach the `retries`, the task will fail.
    pub retries: usize,
}

impl Default for TaskOptions {
    fn default() -> Self {
        Self {
            dir: None,
            path: None,
            retries: 5,
            skip_exists: true,
        }
    }
}

#[derive(Debug)]
pub enum TaskStatus {
    Pending,
    Running,
    Error(error::Error),
    Success,
    Skipped,
}

impl Default for TaskStatus {
    fn default() -> TaskStatus {
        TaskStatus::Pending
    }
}

macro_rules! exec_hook {
    ($t:ident, $task:ident) => {
        if let Some(ref mut hooks) = $task.hooks {
            if let Some(hook) = hooks.$t.take() {
                if let Err(e) = hook(&$task).await {
                    let err = error::Error::DownloadHook {
                        hook: stringify!($t),
                        source: e,
                    };
                    error!(err, "Downloader: Task {}", $task.id());
                    $task.hook_status = TaskStatus::Error(err);
                }
            }
        }
    };
}

impl Downloader {
    pub fn new(client: reqwest::Client, threads: usize) -> Downloader {
        let (task_sender, mut task_receiver) = mpsc::channel::<Task>(1);

        let mut downloader = Downloader {
            client: client.clone(),
            tasks_finished: Arc::new(RwLock::new(BTreeMap::new())),
            tasks_pending: Arc::new(RwLock::new(BTreeMap::new())),
            tasks_running: Arc::new(Mutex::new(BTreeMap::new())),
            task_sender: Arc::new(tokio::sync::Mutex::new(task_sender)),
            semaphore: Arc::new(Semaphore::new(threads)),
            waitgroup: WaitGroup::new(),
            main_handle: None,
        };

        let tasks_pending = Arc::clone(&downloader.tasks_pending);
        let tasks_finished = Arc::clone(&downloader.tasks_finished);
        let tasks_running = Arc::clone(&downloader.tasks_running);
        let sem = Arc::clone(&downloader.semaphore);
        let client = downloader.client.clone();
        let waitgroup = downloader.waitgroup.clone();
        let task_sender = downloader.task_sender.clone();

        downloader.main_handle = Some(spawn(async move {
            while let Some(mut task) = task_receiver.recv().await {
                match sem.clone().try_acquire_owned() {
                    Ok(permit) => {
                        let task_sender = task_sender.clone();
                        let tasks_finished = Arc::clone(&tasks_finished);
                        let tasks_pending = Arc::clone(&tasks_pending);
                        let client = client.clone();
                        let waitgroup = waitgroup.clone();
                        let tasks_running_cloned = Arc::clone(&tasks_running);
                        let task_id = task.id();

                        let handle = spawn(async move {
                            let permit = permit;
                            task.status = TaskStatus::Running;
                            match Self::download(client, &mut task).await {
                                Err(e) => {
                                    error!(e, "Downloader: Task {}", task.id());
                                    task.status = TaskStatus::Error(e);
                                    exec_hook!(on_error, task);
                                }
                                Ok(status) => {
                                    match status {
                                        TaskStatus::Skipped => {
                                            debug!(
                                                "Downloader: Task {} skipped: {}",
                                                task.id(),
                                                task.options
                                                    .path
                                                    .as_ref()
                                                    .unwrap()
                                                    .to_string_lossy()
                                            );
                                        }
                                        TaskStatus::Success => {
                                            info!(
                                                "Downloader: Task {} finished: {}",
                                                task.id(),
                                                task.options
                                                    .path
                                                    .as_ref()
                                                    .unwrap()
                                                    .to_string_lossy()
                                            );
                                            exec_hook!(on_success, task);
                                        }
                                        _ => {}
                                    }
                                    task.status = status;
                                }
                            }

                            tasks_finished.write().unwrap().insert(task.id, task);
                            // We need not to care about the sending result here.
                            #[allow(unused_must_use)]
                            if let Some((_, task)) = {
                                let task = tasks_pending.write().unwrap().pop_first();
                                task
                            } {
                                let locked_sender = task_sender.lock().await;
                                // Lock the task_sender here to ensure that the task from tasks_pending
                                // will be execute in the next iteration.
                                drop(permit);
                                // Drop the permit before sending the task to make
                                // next try_acquire_owned() success.
                                locked_sender.send(task).await;
                            }

                            tasks_running_cloned.lock().unwrap().remove(&task_id);

                            waitgroup.done();
                        });

                        tasks_running.lock().unwrap().insert(task_id, handle);
                    }
                    Err(tokio::sync::TryAcquireError::Closed) => {
                        break;
                    }
                    Err(tokio::sync::TryAcquireError::NoPermits) => {
                        tasks_pending.write().unwrap().insert(task.id, task);
                    }
                }
            }
        }));

        downloader
    }

    pub async fn send(&self, tasks: Vec<Task>) {
        if tasks.is_empty() {
            return;
        }
        self.waitgroup.add(tasks.len());
        for task in tasks {
            self.task_sender.lock().await.send(task).await.unwrap();
        }
    }

    /// Wait for all sent tasks to finish.
    pub async fn wait(mut self) {
        (&mut self.waitgroup).await;
    }

    async fn download(client: reqwest::Client, task: &mut Task) -> crate::Result<TaskStatus> {
        if let Some(p) = &task.options.path {
            if p.is_relative() {
                return error::DownloadPathNotAbsolute {
                    message: format!("{:?}", task),
                }
                .fail();
            }
            if p.exists() && task.options.skip_exists {
                return Ok(TaskStatus::Skipped);
            }
        }
        info!("Downloader: Starting task {}: {}", task.id(), task.url);

        let mut request = task.build_request(&client)?;
        task.url = request.url().clone();

        fn part_from_str(mut p: OsString) -> PathBuf {
            p.push(".part");
            PathBuf::from(p)
        }

        let path = match &task.options.path {
            Some(p) => p.clone(),
            None => match &task.options.dir {
                Some(dir) => {
                    let resp = client
                        .execute(
                            client
                                .request(Method::HEAD, request.url().clone())
                                .headers(request.headers().clone())
                                .build()
                                .context(error::DownloadHTTP)?,
                        )
                        .await
                        .context(error::DownloadHTTP)?;

                    let path_from_header = if resp.status().is_success() {
                        match resp.headers().get(reqwest::header::CONTENT_DISPOSITION) {
                            Some(h) => match h.to_str() {
                                Ok(h) => match RE_CONTENT_DISPOSITION.captures(h) {
                                    Some(c) => Some(PathBuf::from(sanitize_filename::sanitize(
                                        c.get(1).unwrap().as_str(),
                                    ))),
                                    None => None,
                                },
                                Err(_) => None,
                            },
                            None => None,
                        }
                    } else {
                        None
                    };
                    if path_from_header.is_none() {
                        let path = dir.join(match Path::new(resp.url().path()).file_name() {
                            Some(p) => sanitize_filename::sanitize(p.to_string_lossy()),
                            None => {
                                return error::DownloadPathNotSet {
                                    message: format!("{:?}", task),
                                }
                                .fail();
                            }
                        });
                        path
                    } else {
                        let path_from_header = path_from_header.unwrap();
                        let path = dir.join(&path_from_header);
                        path
                    }
                }
                None => {
                    return error::DownloadPathNotSet {
                        message: format!("{:?}", task),
                    }
                    .fail();
                }
            },
        };
        task.options.path = Some(path.clone());
        if path.exists() && task.options.skip_exists {
            return Ok(TaskStatus::Skipped);
        }

        if let Some(p) = path.parent() {
            fs::create_dir_all(p).await.context(error::DownloadIO)?;
        }

        let path_part = part_from_str(path.as_os_str().to_owned());

        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .open(&path_part)
            .await
            .context(error::DownloadIO)?;
        let mut retries_last_min = vec![Instant::now()];
        let mut tries = 1;
        loop {
            match Downloader::download_single_try(&client, task, &mut file, request).await {
                Ok(()) => {
                    drop(file);
                    fs::rename(path_part, path)
                        .await
                        .context(error::DownloadIO)?;
                    return Ok(TaskStatus::Success);
                }
                Err(error::Error::DownloadHTTP { source, backtrace }) => {
                    retries_last_min = retries_last_min
                        .drain_filter(|i| i.elapsed() <= Duration::from_secs(60))
                        .collect();

                    if retries_last_min.len() > task.options.retries {
                        return Err(error::Error::DownloadHTTP { source, backtrace });
                    }

                    warning!(
                        "Downloader: tries {}: HTTP error in task {}: {}",
                        tries,
                        task.id(),
                        source
                    );
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    request = task.build_request(&client)?;
                    retries_last_min.push(Instant::now());
                    tries += 1;
                }
                Err(e) => {
                    return Err(e);
                }
            }
        }
    }

    async fn download_single_try(
        client: &reqwest::Client,
        task: &mut Task,
        file: &mut File,
        mut request: reqwest::Request,
    ) -> crate::Result<()> {
        let mut downloaded_len = file
            .seek(SeekFrom::End(0))
            .await
            .context(error::DownloadIO)?;

        if downloaded_len > 0 {
            request.headers_mut().insert(
                "Range",
                format!("bytes={}-", downloaded_len).parse().unwrap(),
            );
        }

        let mut resp = client.execute(request).await.context(error::DownloadHTTP)?;
        if !resp.status().is_success() {
            let mut response = BytesMut::with_capacity(4096);
            while let Ok(Some(chunk)) = resp.chunk().await {
                response.put(chunk);
                if response.len() > 1024 * 100 {
                    break;
                }
            }
            return error::DownloadHTTPStatus {
                status: resp.status(),
                response: String::from_utf8_lossy(&response).clone(),
            }
            .fail();
        }

        if let Some(v) = resp.headers().get(reqwest::header::CONTENT_RANGE) {
            if let Ok(v) = v.to_str() {
                if let Some(i) = v.find("/") {
                    if let Ok(size) = v[i + 1..].parse::<u64>() {
                        task.file_size = Some(size);
                    }
                }
            }
        }

        while let Some(chunk) = resp.chunk().await.context(error::DownloadHTTP)? {
            file.write_all(&chunk).await.context(error::DownloadIO)?;
            downloaded_len += chunk.len() as u64;
        }
        task.file_size = Some(downloaded_len);

        Ok(())
    }
}
