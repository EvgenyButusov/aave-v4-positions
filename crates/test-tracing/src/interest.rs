use tracing::level_filters::LevelFilter;
use tracing_subscriber::layer::SubscriberExt as _;

/// Call before installing a scoped subscriber. Idempotent.
///
/// With one dispatcher registered, `tracing-core` 0.1.36 takes a callsite's interest from the
/// thread that first reaches it (tokio-rs/tracing#3611). A sibling case with no subscriber then
/// caches `never`, and the capturing case reads an empty log. A global default that wants
/// everything is a second dispatcher, and every thread's default.
pub fn interesting() {
    static ONCE: std::sync::OnceLock<()> = std::sync::OnceLock::new();

    ONCE.get_or_init(|| {
        let _ = tracing::subscriber::set_global_default(
            tracing_subscriber::registry().with(LevelFilter::TRACE),
        );
    });
}
