use async_trait::async_trait;
use atrium_api::xrpc;
use std::error::Error;

pub struct XrpcReqwestClient {
    client: reqwest::Client,
    access_jwt: Option<String>,
    access_did: Option<String>,
    host: String,
}

impl XrpcReqwestClient {
    pub fn new(host: String, client: reqwest::Client) -> Self {
        Self {
            host: host,
            access_jwt: None,
            access_did: None,
            client: client,
        }
    }
}

pub trait XrpcHttpClient: xrpc::HttpClient + xrpc::XrpcClient {
    fn set_session(&mut self, jwt: String, did: String) -> ();
    fn current_did(&self) -> Option<&str>;
}

#[async_trait]
impl xrpc::HttpClient for XrpcReqwestClient {
    async fn send(
        &self,
        req: xrpc::http::Request<Vec<u8>>,
    ) -> Result<xrpc::http::Response<Vec<u8>>, Box<dyn Error>> {
        let res = self.client.execute(req.try_into()?).await?;
        let mut builder = xrpc::http::Response::builder().status(res.status());
        for (k, v) in res.headers() {
            builder = builder.header(k, v);
        }
        builder
            .body(res.bytes().await?.to_vec())
            .map_err(Into::into)
    }
}

impl xrpc::XrpcClient for XrpcReqwestClient {
    fn host(&self) -> &str {
        &self.host
    }

    fn auth(&self) -> Option<&str> {
        self.access_jwt.as_deref()
    }
}

impl XrpcHttpClient for XrpcReqwestClient {
    fn current_did(&self) -> Option<&str> {
        self.access_did.as_deref()
    }

    fn set_session(&mut self, jwt: String, did: String) {
        self.access_jwt = Some(jwt);
        self.access_did = Some(did);
    }
}

atrium_api::impl_traits!(XrpcReqwestClient);
