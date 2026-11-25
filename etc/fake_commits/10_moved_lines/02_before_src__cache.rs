fn evict(cache: &mut Cache) {
    cache.remove_expired_entries(current_time);
    cache.shrink_to_fit_the_configured_capacity();
    cache.flush_dirty_entries_to_the_disk();
    cache.record_the_statistics_of_this_eviction();
}
