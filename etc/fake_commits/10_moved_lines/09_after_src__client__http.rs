pub fn send_over_http(request: Request) -> Response {
    let retry_delay = Duration::from_millis(250);
    let maximum_attempts = 5;
    let timeout = Duration::from_secs(30);
    http::post(request, retry_delay, maximum_attempts, timeout)
}
