fn schedule(queue: &mut Queue) {
    queue.push_back(first_pending_request);
    queue.retain(|request| request.is_valid());
    queue.sort_by_key(|request| request.priority);
    queue.truncate(MAXIMUM_QUEUE_LENGTH);
}
