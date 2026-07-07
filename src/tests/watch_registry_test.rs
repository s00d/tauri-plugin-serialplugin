#[cfg(test)]
mod tests {
    use crate::watch_registry;

    #[test]
    fn register_rejects_duplicate_path() {
        watch_registry::unregister(1);
        watch_registry::unregister(2);
        watch_registry::register(1, "/dev/a".into()).unwrap();
        assert!(watch_registry::register(2, "/dev/a".into()).is_err());
        watch_registry::unregister(1);
    }

    #[test]
    fn paths_for_port_returns_channel_ids() {
        watch_registry::unregister(10);
        watch_registry::register(10, "/dev/b".into()).unwrap();
        let ids = watch_registry::paths_for_port("/dev/b");
        assert!(ids.contains(&10));
        watch_registry::unregister(10);
    }

    #[test]
    fn unregister_after_thread_exit_allows_reregister() {
        watch_registry::unregister(20);
        watch_registry::register(20, "/dev/c".into()).unwrap();
        watch_registry::unregister(20);
        assert!(watch_registry::register(20, "/dev/c".into()).is_ok());
        watch_registry::unregister(20);
    }

    #[test]
    fn unregister_is_idempotent() {
        watch_registry::unregister(999);
        watch_registry::unregister(999);
    }
}
