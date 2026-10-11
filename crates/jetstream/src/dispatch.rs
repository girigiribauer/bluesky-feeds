use crate::{App, Event, PostDelete, PostEvent};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::Instrument;

pub(crate) struct Progress(std::sync::Mutex<ProgressState>);

struct ProgressState {
    dispatched_us: i64,
    in_flight: BTreeMap<i64, usize>,
}

impl Progress {
    pub(crate) fn new(dispatched_us: i64) -> Self {
        Progress(std::sync::Mutex::new(ProgressState {
            dispatched_us,
            in_flight: BTreeMap::new(),
        }))
    }

    fn start(&self, time_us: i64) {
        *self.0.lock().unwrap().in_flight.entry(time_us).or_insert(0) += 1;
    }

    fn finish(&self, time_us: i64) {
        let mut state = self.0.lock().unwrap();
        if let Some(count) = state.in_flight.get_mut(&time_us) {
            *count -= 1;
            if *count == 0 {
                state.in_flight.remove(&time_us);
            }
        }
    }

    fn dispatched(&self, time_us: i64) {
        let mut state = self.0.lock().unwrap();
        state.dispatched_us = state.dispatched_us.max(time_us);
    }

    pub(crate) fn dispatched_us(&self) -> i64 {
        self.0.lock().unwrap().dispatched_us
    }

    pub(crate) fn safe_cursor_us(&self) -> i64 {
        let state = self.0.lock().unwrap();
        match state.in_flight.keys().next() {
            Some(&oldest) => (oldest - 1).min(state.dispatched_us),
            None => state.dispatched_us,
        }
    }
}

#[derive(Clone)]
enum Job {
    Post(Arc<PostEvent>),
    Delete(Arc<PostDelete>),
}

impl Job {
    fn time_us(&self) -> i64 {
        match self {
            Job::Post(post) => post.time_us,
            Job::Delete(delete) => delete.time_us,
        }
    }
}

struct Lane {
    app: Arc<dyn App>,
    tx: mpsc::Sender<Job>,
}

pub(crate) struct Dispatcher {
    lanes: Vec<Lane>,
    capacity: usize,
    progress: Arc<Progress>,
}

impl Dispatcher {
    pub(crate) fn start(apps: Vec<Arc<dyn App>>, capacity: usize, progress: Arc<Progress>) -> Self {
        let lanes = apps
            .into_iter()
            .map(|app| {
                let (tx, rx) = mpsc::channel(capacity);
                let rx = Arc::new(Mutex::new(rx));
                for _ in 0..app.workers().max(1) {
                    tokio::spawn(run_worker(app.clone(), rx.clone(), progress.clone()));
                }
                Lane { app, tx }
            })
            .collect();

        Dispatcher {
            lanes,
            capacity,
            progress,
        }
    }

    pub(crate) async fn dispatch(&self, event: Event) {
        let time_us = event.time_us();
        let job = match event {
            Event::Post(post) => Job::Post(Arc::from(post)),
            Event::PostDelete(delete) => Job::Delete(Arc::new(delete)),
        };

        for lane in &self.lanes {
            let wanted = match &job {
                Job::Post(post) => lane.app.sieve(post),
                Job::Delete(delete) => lane.app.sieve_delete(delete),
            };
            if !wanted {
                continue;
            }

            self.progress.start(time_us);
            if lane.tx.send(job.clone()).await.is_err() {
                self.progress.finish(time_us);
            }
        }

        self.progress.dispatched(time_us);
    }

    pub(crate) fn queue_report(&self) -> String {
        self.lanes
            .iter()
            .map(|lane| {
                format!(
                    "{}={}/{}",
                    lane.app.name(),
                    self.capacity - lane.tx.capacity(),
                    self.capacity
                )
            })
            .collect::<Vec<_>>()
            .join(",")
    }
}

async fn run_worker(
    app: Arc<dyn App>,
    rx: Arc<Mutex<mpsc::Receiver<Job>>>,
    progress: Arc<Progress>,
) {
    loop {
        let Some(job) = rx.lock().await.recv().await else {
            return;
        };
        let time_us = job.time_us();
        let span = tracing::info_span!("consumer", feed = %app.name());

        let app_for_job = app.clone();
        let result = tokio::spawn(
            async move {
                match job {
                    Job::Post(post) => app_for_job.process(&post).await,
                    Job::Delete(delete) => app_for_job.process_delete(&delete).await,
                }
            }
            .instrument(span.clone()),
        )
        .await;

        if let Err(e) = result {
            span.in_scope(|| tracing::error!("Processing stopped abnormally: {}", e));
        }
        progress.finish(time_us);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::parse_event;
    use crate::local_jetstream::{delete_json, post_json};
    use crate::SkipStats;
    use std::time::Duration;
    use tokio::sync::Semaphore;

    fn event(json: &str) -> Event {
        parse_event(json, &mut SkipStats::default())
            .unwrap()
            .unwrap()
    }

    fn post(text: &str, time_us: i64) -> Event {
        event(&post_json(text, time_us))
    }

    #[derive(Clone)]
    struct TestApp {
        name: &'static str,
        keyword: &'static str,
        workers: usize,
        wants_deletes: bool,
        gate: Option<Arc<Semaphore>>,
        started: Arc<std::sync::Mutex<Vec<String>>>,
        processed: Arc<std::sync::Mutex<Vec<String>>>,
        deleted: Arc<std::sync::Mutex<Vec<String>>>,
    }

    impl TestApp {
        fn new(name: &'static str, keyword: &'static str) -> Self {
            TestApp {
                name,
                keyword,
                workers: 1,
                wants_deletes: false,
                gate: None,
                started: Arc::default(),
                processed: Arc::default(),
                deleted: Arc::default(),
            }
        }

        fn gated(mut self, gate: &Arc<Semaphore>) -> Self {
            self.gate = Some(gate.clone());
            self
        }

        fn processed(&self) -> Vec<String> {
            self.processed.lock().unwrap().clone()
        }

        fn started(&self) -> Vec<String> {
            self.started.lock().unwrap().clone()
        }
    }

    #[async_trait::async_trait]
    impl App for TestApp {
        fn name(&self) -> &'static str {
            self.name
        }

        fn workers(&self) -> usize {
            self.workers
        }

        async fn migrate(&self) -> anyhow::Result<()> {
            Ok(())
        }

        fn sieve(&self, post: &PostEvent) -> bool {
            post.record.text.contains(self.keyword)
        }

        async fn process(&self, post: &PostEvent) {
            let text = post.record.text.clone();
            self.started.lock().unwrap().push(text.clone());
            if let Some(gate) = &self.gate {
                gate.acquire().await.unwrap().forget();
            }
            if text.contains("panic") {
                panic!("テスト用の異常終了");
            }
            tracing::info!("processed {}", text);
            self.processed.lock().unwrap().push(text);
        }

        fn sieve_delete(&self, _delete: &PostDelete) -> bool {
            self.wants_deletes
        }

        async fn process_delete(&self, delete: &PostDelete) {
            self.deleted.lock().unwrap().push(delete.uri());
        }
    }

    fn start(apps: Vec<TestApp>, capacity: usize) -> (Dispatcher, Arc<Progress>) {
        let progress = Arc::new(Progress::new(0));
        let apps = apps
            .into_iter()
            .map(|app| Arc::new(app) as Arc<dyn App>)
            .collect();
        (
            Dispatcher::start(apps, capacity, progress.clone()),
            progress,
        )
    }

    async fn wait_until(mut condition: impl FnMut() -> bool) {
        for _ in 0..200 {
            if condition() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("待っても条件を満たさなかった");
    }

    /// 処理中の投稿が無ければ、振り分け済みの位置がそのままカーソルにしてよい位置になること
    #[test]
    fn test_safe_cursor_is_dispatched_position_when_nothing_in_flight() {
        let progress = Progress::new(0);

        progress.start(100);
        progress.finish(100);
        progress.dispatched(100);

        assert_eq!(progress.safe_cursor_us(), 100);
    }

    /// 処理中の投稿があれば、その中で一番古いものの手前がカーソルにしてよい位置になること
    #[test]
    fn test_safe_cursor_stops_before_oldest_in_flight() {
        let progress = Progress::new(0);

        for time_us in [100, 200, 300] {
            progress.start(time_us);
            progress.dispatched(time_us);
        }
        progress.finish(300);

        assert_eq!(progress.safe_cursor_us(), 99);
    }

    /// 新しい投稿が先に終わっても、古い投稿が終わるまではカーソルにしてよい位置が進まないこと
    #[test]
    fn test_safe_cursor_waits_for_older_job_even_if_newer_finished() {
        let progress = Progress::new(0);

        progress.start(100);
        progress.dispatched(100);
        progress.start(200);
        progress.dispatched(200);
        progress.finish(200);
        assert_eq!(progress.safe_cursor_us(), 99);

        progress.finish(100);
        assert_eq!(progress.safe_cursor_us(), 200);
    }

    /// 同じ投稿を複数のアプリが処理しているときは、全部終わるまでカーソルにしてよい位置が進まないこと
    #[test]
    fn test_safe_cursor_waits_until_every_app_finishes_same_post() {
        let progress = Progress::new(0);

        progress.start(100);
        progress.start(100);
        progress.dispatched(100);
        progress.finish(100);
        assert_eq!(progress.safe_cursor_us(), 99);

        progress.finish(100);
        assert_eq!(progress.safe_cursor_us(), 100);
    }

    /// ふるいを通った投稿だけが、そのアプリの本処理に渡ること
    #[tokio::test]
    async fn test_only_posts_passing_sieve_are_processed() {
        let app = TestApp::new("hello", "hello");
        let (dispatcher, progress) = start(vec![app.clone()], 10);

        dispatcher.dispatch(post("hello world", 100)).await;
        dispatcher.dispatch(post("goodbye", 200)).await;
        wait_until(|| progress.safe_cursor_us() == 200).await;

        assert_eq!(app.processed(), vec!["hello world"]);
    }

    /// 1件の投稿が複数のアプリのふるいを通れば、それぞれのアプリで処理されること
    #[tokio::test]
    async fn test_post_passing_several_sieves_is_processed_by_each_app() {
        let hello = TestApp::new("hello", "hello");
        let world = TestApp::new("world", "world");
        let (dispatcher, progress) = start(vec![hello.clone(), world.clone()], 10);

        dispatcher.dispatch(post("hello world", 100)).await;
        wait_until(|| progress.safe_cursor_us() == 100).await;

        assert_eq!(hello.processed(), vec!["hello world"]);
        assert_eq!(world.processed(), vec!["hello world"]);
    }

    /// 本処理が遅いアプリがあっても、待ち行列に空きがある間は、他のアプリの処理が止まらないこと
    #[tokio::test]
    async fn test_slow_app_does_not_block_other_apps() {
        let gate = Arc::new(Semaphore::new(0));
        let slow = TestApp::new("slow", "post").gated(&gate);
        let fast = TestApp::new("fast", "post");
        let (dispatcher, progress) = start(vec![slow.clone(), fast.clone()], 10);

        for (i, time_us) in [100, 200, 300].into_iter().enumerate() {
            dispatcher
                .dispatch(post(&format!("post {i}"), time_us))
                .await;
        }
        wait_until(|| fast.processed().len() == 3).await;

        assert!(slow.processed().is_empty());
        assert_eq!(progress.safe_cursor_us(), 99);

        gate.add_permits(3);
        wait_until(|| progress.safe_cursor_us() == 300).await;
        assert_eq!(slow.processed().len(), 3);
    }

    /// 待ち行列が埋まったら振り分けが待ち、空きが出たら再開して、1件も取りこぼさないこと
    #[tokio::test]
    async fn test_dispatch_waits_when_queue_is_full_and_loses_nothing() {
        let gate = Arc::new(Semaphore::new(0));
        let app = TestApp::new("app", "post").gated(&gate);
        let (dispatcher, progress) = start(vec![app.clone()], 1);

        let dispatching = tokio::spawn(async move {
            for (i, time_us) in [100, 200, 300, 400].into_iter().enumerate() {
                dispatcher
                    .dispatch(post(&format!("post {i}"), time_us))
                    .await;
            }
        });
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(!dispatching.is_finished());

        gate.add_permits(4);
        dispatching.await.unwrap();
        wait_until(|| progress.safe_cursor_us() == 400).await;
        assert_eq!(
            app.processed(),
            vec!["post 0", "post 1", "post 2", "post 3"]
        );
    }

    /// 作業係を複数にしたアプリは、複数の投稿を同時に処理すること
    #[tokio::test]
    async fn test_app_with_several_workers_processes_posts_concurrently() {
        let gate = Arc::new(Semaphore::new(0));
        let mut app = TestApp::new("app", "post").gated(&gate);
        app.workers = 2;
        let (dispatcher, progress) = start(vec![app.clone()], 10);

        dispatcher.dispatch(post("post 0", 100)).await;
        dispatcher.dispatch(post("post 1", 200)).await;
        wait_until(|| app.started().len() == 2).await;

        gate.add_permits(2);
        wait_until(|| progress.safe_cursor_us() == 200).await;
    }

    /// 本処理が途中で異常終了しても、カーソルにしてよい位置が止まらず、次の投稿も処理されること
    #[tokio::test]
    async fn test_abnormal_end_of_processing_does_not_stall_cursor() {
        let app = TestApp::new("app", "post");
        let (dispatcher, progress) = start(vec![app.clone()], 10);

        dispatcher.dispatch(post("post panic", 100)).await;
        dispatcher.dispatch(post("post next", 200)).await;
        wait_until(|| progress.safe_cursor_us() == 200).await;

        assert_eq!(app.processed(), vec!["post next"]);
    }

    /// 削除用のふるいを用意していないアプリには、削除の通知が渡らないこと
    #[tokio::test]
    async fn test_delete_is_not_queued_for_app_without_delete_sieve() {
        let app = TestApp::new("app", "post");
        let (dispatcher, progress) = start(vec![app.clone()], 10);

        dispatcher.dispatch(event(&delete_json(100))).await;

        assert_eq!(progress.safe_cursor_us(), 100);
        assert_eq!(dispatcher.queue_report(), "app=0/10");
        assert!(app.deleted.lock().unwrap().is_empty());
    }

    /// 削除用のふるいを通った削除の通知は、そのアプリの削除時の処理に渡ること
    #[tokio::test]
    async fn test_delete_passing_delete_sieve_is_processed() {
        let mut app = TestApp::new("app", "post");
        app.wants_deletes = true;
        let (dispatcher, progress) = start(vec![app.clone()], 10);

        dispatcher.dispatch(event(&delete_json(100))).await;
        wait_until(|| progress.safe_cursor_us() == 100).await;

        assert_eq!(app.deleted.lock().unwrap().len(), 1);
    }

    /// 待ち行列の埋まり具合を「アプリ名=入っている数/上限」の形で並べること
    #[tokio::test]
    async fn test_queue_report_shows_usage_per_app() {
        let gate = Arc::new(Semaphore::new(0));
        let slow = TestApp::new("slow", "post").gated(&gate);
        let idle = TestApp::new("idle", "nothing");
        let (dispatcher, _progress) = start(vec![slow.clone(), idle], 10);

        for time_us in [100, 200, 300] {
            dispatcher.dispatch(post("post", time_us)).await;
        }
        wait_until(|| slow.started().len() == 1).await;

        assert_eq!(dispatcher.queue_report(), "slow=2/10,idle=0/10");
        gate.add_permits(3);
    }

    thread_local! {
        static LOGS: std::cell::RefCell<Vec<u8>> = const { std::cell::RefCell::new(Vec::new()) };
    }

    struct ThreadLocalWriter;

    impl std::io::Write for ThreadLocalWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            LOGS.with(|logs| logs.borrow_mut().extend_from_slice(buf));
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn start_capturing_logs() {
        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(|| {
            let subscriber = tracing_subscriber::fmt()
                .with_writer(|| ThreadLocalWriter)
                .with_ansi(false)
                .finish();
            tracing::subscriber::set_global_default(subscriber).unwrap();
        });
        LOGS.with(|logs| logs.borrow_mut().clear());
    }

    fn captured_logs() -> String {
        LOGS.with(|logs| String::from_utf8(logs.borrow().clone()).unwrap())
    }

    /// 本処理の中で出たログに、アプリ名の印（consumer{feed=アプリ名}）が付くこと
    #[tokio::test]
    async fn test_logs_during_processing_are_tagged_with_app_name() {
        start_capturing_logs();
        let app = TestApp::new("helloworld", "hello");
        let (dispatcher, progress) = start(vec![app], 10);

        dispatcher.dispatch(post("hello world", 100)).await;
        wait_until(|| progress.safe_cursor_us() == 100).await;

        let text = captured_logs();
        let line = text
            .lines()
            .find(|line| line.contains("processed hello world"))
            .unwrap_or_else(|| panic!("「processed hello world」を含むログが無い:\n{text}"));
        assert!(line.contains("consumer{feed=helloworld}"), "{line}");
    }
}
