fn schedule(queue: &mut Queue) {
    queue.push_back(first_pending_request);
    queue.sort_by_key(|request| request.priority);
    queue.retain(|request| request.is_valid() && !request.is_expired());
    queue.truncate(MAXIMUM_QUEUE_LENGTH);
}
