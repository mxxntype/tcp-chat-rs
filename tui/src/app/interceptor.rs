use tcp_chat::{auth::AuthenticatedRequest, proto::AuthPair};
use tonic::{Request, Status};

#[derive(Debug)]
pub struct Interceptor {
    auth_pair: AuthPair,
}

impl Interceptor {
    pub const fn new(auth_pair: AuthPair) -> Self {
        Self { auth_pair }
    }
}

impl tonic::service::Interceptor for Interceptor {
    fn call(&mut self, mut request: Request<()>) -> Result<Request<()>, Status> {
        request
            .add_auth_pair(self.auth_pair.clone())
            .expect("The stored AuthPair is invalid!");
        Ok(request)
    }
}
