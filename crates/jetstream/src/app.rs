use crate::{PostDelete, PostEvent};

#[async_trait::async_trait]
pub trait App: Send + Sync + 'static {
    fn name(&self) -> &'static str;

    fn workers(&self) -> usize {
        1
    }

    async fn migrate(&self) -> anyhow::Result<()>;

    fn sieve(&self, post: &PostEvent) -> bool;

    async fn process(&self, post: &PostEvent);

    fn sieve_delete(&self, _delete: &PostDelete) -> bool {
        false
    }

    async fn process_delete(&self, _delete: &PostDelete) {}
}
