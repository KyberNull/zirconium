use std::sync::Arc;

use crate::RunnableResolver;

/// Per-language hook point for runnable detection.
pub trait ContextProvider: Send + Sync {
    /// A resolver for runnable queries that group captures with `@run_item`.
    /// For each group, the resolver picks which `@run` range and extra captures
    /// to surface. Without a resolver, grouped matches emit no runnables.
    fn runnable_resolver(&self) -> Option<Arc<dyn RunnableResolver>> {
        None
    }
}
