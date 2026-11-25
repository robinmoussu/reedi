pub struct Client {
    transport: Transport,
}

impl Client {
    pub fn send(&self, request: Request) -> Response {
        self.transport.send(request)
    }
}
