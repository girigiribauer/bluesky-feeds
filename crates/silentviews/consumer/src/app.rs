use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use jetstream::{App, PostEvent};
use sqlx::SqlitePool;

use crate::pipeline::{self, Models, Rejection};
use crate::{candidate_images, CandidateImage};

type Job = (
    Vec<Vec<u8>>,
    tokio::sync::oneshot::Sender<Result<(), Rejection>>,
);

pub struct SilentViews {
    pool: SqlitePool,
    jobs: mpsc::Sender<Job>,
    http: reqwest::Client,
    cdn_base: Option<String>,
}

impl SilentViews {
    pub fn new(
        pool: SqlitePool,
        models_dir: &Path,
        cdn_base: Option<String>,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            pool,
            jobs: start_judge_thread(models_dir.to_path_buf())?,
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()?,
            cdn_base,
        })
    }

    fn url(&self, url: &str) -> String {
        match &self.cdn_base {
            Some(base) => url.replacen(crate::CDN_BASE, base, 1),
            None => url.to_string(),
        }
    }

    async fn judge(&self, images: &[CandidateImage]) -> anyhow::Result<Result<(), Rejection>> {
        let smalls = self
            .fetch_all(images.iter().map(|i| self.url(&i.small_url)))
            .await?;
        let first = tokio::task::spawn_blocking(move || pipeline::stage1(&smalls)).await?;
        if first.is_err() {
            return Ok(first);
        }
        let larges = self
            .fetch_all(images.iter().map(|i| self.url(&i.large_url)))
            .await?;
        let (reply, result) = tokio::sync::oneshot::channel();
        self.jobs
            .send((larges, reply))
            .map_err(|_| anyhow::anyhow!("judge thread has stopped"))?;
        Ok(result.await?)
    }

    async fn fetch_all(&self, urls: impl Iterator<Item = String>) -> anyhow::Result<Vec<Vec<u8>>> {
        let mut out = Vec::new();
        for url in urls {
            let bytes = self
                .http
                .get(url)
                .send()
                .await?
                .error_for_status()?
                .bytes()
                .await?;
            out.push(bytes.to_vec());
        }
        Ok(out)
    }
}

fn start_judge_thread(models_dir: PathBuf) -> anyhow::Result<mpsc::Sender<Job>> {
    let (jobs, inbox) = mpsc::channel::<Job>();
    let (ready, loaded) = mpsc::channel();
    std::thread::Builder::new()
        .name("silentviews-judge".into())
        .spawn(move || {
            let models = match Models::load(&models_dir) {
                Ok(models) => {
                    let _ = ready.send(Ok(()));
                    models
                }
                Err(e) => {
                    let _ = ready.send(Err(e));
                    return;
                }
            };
            for (larges, reply) in inbox {
                let _ = reply.send(pipeline::stage2(&models, &larges));
            }
        })?;
    loaded.recv()??;
    Ok(jobs)
}

pub async fn store(pool: &SqlitePool, post: &PostEvent) -> anyhow::Result<()> {
    sqlx::query("INSERT OR REPLACE INTO silentviews_posts (uri, cid, indexed_at) VALUES (?, ?, ?)")
        .bind(post.uri())
        .bind(&post.cid)
        .bind(post.indexed_at_us())
        .execute(pool)
        .await?;
    Ok(())
}

#[async_trait::async_trait]
impl App for SilentViews {
    fn name(&self) -> &'static str {
        "silentviews"
    }

    async fn migrate(&self) -> anyhow::Result<()> {
        crate::migrate(&self.pool).await
    }

    fn sieve(&self, post: &PostEvent) -> bool {
        candidate_images(post).is_some()
    }

    async fn process(&self, post: &PostEvent) {
        let Some(images) = candidate_images(post) else {
            return;
        };
        let started = std::time::Instant::now();
        match self.judge(&images).await {
            Ok(Ok(())) => match store(&self.pool, post).await {
                Ok(()) => tracing::info!(
                    "PASS: images={}, t={:.0}ms, uri={}",
                    images.len(),
                    started.elapsed().as_secs_f64() * 1000.0,
                    post.uri()
                ),
                Err(e) => tracing::error!("Failed to store silentviews post: {e}"),
            },
            Ok(Err(rejection)) => tracing::debug!(
                "reject {rejection:?}: t={:.0}ms, uri={}",
                started.elapsed().as_secs_f64() * 1000.0,
                post.uri()
            ),
            Err(e) => tracing::warn!("Could not judge {}: {e}", post.uri()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn post() -> PostEvent {
        let json = json!({
            "did": "did:plc:z72i7hdynmk6r22z27h6tvur",
            "time_us": 1_700_000_000_000_000i64,
            "kind": "commit",
            "commit": {
                "operation": "create", "rev": "3l3temxelsm2a", "rkey": "3l3temxelsm2a",
                "collection": "app.bsky.feed.post",
                "cid": "bafyreibvjvcv745gig4mvqs4hctx4zfkono4rjejm2ta6gtyzkqxfjeily",
                "record": { "$type": "app.bsky.feed.post", "text": "", "createdAt": "2023-11-14T22:13:19.000Z" },
            },
        })
        .to_string();
        match jetstream::event::parse_event(&json, &mut Default::default()) {
            Ok(Some(jetstream::Event::Post(p))) => *p,
            _ => panic!("投稿として読めなかった"),
        }
    }

    /// 同じ投稿が編集されて版（cid）が変わったら、新しい版で上書きし、1件のままにすること
    #[tokio::test]
    async fn test_store_replaces_edited_post() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .unwrap();
        crate::migrate(&pool).await.unwrap();
        let first = post();
        let mut edited = post();
        edited.cid = "bafyreiedited".to_string();

        store(&pool, &first).await.unwrap();
        store(&pool, &edited).await.unwrap();

        let rows: Vec<(String,)> = sqlx::query_as("SELECT cid FROM silentviews_posts")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(rows, vec![("bafyreiedited".to_string(),)]);
    }

    /// 通った投稿は、住所・版・並び順の時刻を表に入れ、同じ投稿を2回入れても1件のままにすること
    #[tokio::test]
    async fn test_store_inserts_post_once() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .unwrap();
        crate::migrate(&pool).await.unwrap();
        let p = post();

        store(&pool, &p).await.unwrap();
        store(&pool, &p).await.unwrap();

        let rows: Vec<(String, String, i64)> =
            sqlx::query_as("SELECT uri, cid, indexed_at FROM silentviews_posts")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(rows, vec![(p.uri(), p.cid.clone(), 1_699_999_999_000_000)]);
    }

    /// モデルが読み込めないときは、Silent Views 自体を作れないこと（判定せずに動き出さないため）
    #[tokio::test]
    async fn test_silentviews_cannot_start_without_models() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .unwrap();

        assert!(SilentViews::new(pool, Path::new("/nonexistent"), None).is_err());
    }
}
