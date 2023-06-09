use async_trait::async_trait;
use atrium_api::xrpc;
use std::error::Error;

pub struct XrpcReqwestClient {
    client: reqwest::Client,
    access_jwt: Option<String>,
    access_did: Option<String>,
    host: String,
    dry_run: bool,
}

impl XrpcReqwestClient {
    pub fn new(host: String, client: reqwest::Client, dry_run: bool) -> Self {
        Self {
            host,
            access_jwt: None,
            access_did: None,
            client,
            dry_run,
        }
    }
}

#[async_trait]
pub trait XrpcHttpClient: xrpc::HttpClient + xrpc::XrpcClient {
    fn set_session(&mut self, jwt: String, did: String) -> ();
    fn current_did(&self) -> Option<&str>;
    async fn get_remote_content(&self, url: &str) -> Result<bytes::Bytes, Box<dyn Error>>;
}

#[async_trait]
impl xrpc::HttpClient for XrpcReqwestClient {
    async fn send(
        &self,
        req: xrpc::http::Request<Vec<u8>>,
    ) -> Result<xrpc::http::Response<Vec<u8>>, Box<dyn Error>> {
        let res = if self.dry_run {
            Err(format!("Enabled dry run mode."))?
        } else {
            self.client.execute(req.try_into()?).await?
        };
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

#[async_trait]
impl XrpcHttpClient for XrpcReqwestClient {
    fn current_did(&self) -> Option<&str> {
        self.access_did.as_deref()
    }

    fn set_session(&mut self, jwt: String, did: String) {
        self.access_jwt = Some(jwt);
        self.access_did = Some(did);
    }

    async fn get_remote_content(&self, url: &str) -> Result<bytes::Bytes, Box<dyn Error>> {
        let res = if self.dry_run {
            Err(format!("Enabled dry run mode."))?
        } else {
            let req = reqwest::Request::new(
                reqwest::Method::GET,
                reqwest::Url::parse(url)?,
            );
            self.client.execute(req).await?
        };
        let status = res.status();
        if status == 200 {
            res.bytes().await
                .map_err(|err| err.into())
        } else {
            let res_text = res.text().await;
            Err(format!("Respond not ok: status={}, body={:?}", status, res_text))?
        }
    }
}

atrium_api::impl_traits!(XrpcReqwestClient);
