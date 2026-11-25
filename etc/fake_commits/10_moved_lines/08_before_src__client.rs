pub struct Client {
    transport: Transport,
}

impl Client {
    pub fn send(&self, request: Request) -> Response {
        let retry_delay = Duration::from_millis(250);
        let maximum_attempts = 3;
        let timeout = Duration::from_secs(30);
        self.transport.send(request, retry_delay, maximum_attempts, timeout)
    }
}
