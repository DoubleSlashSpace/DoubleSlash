//! Client-initiated DDDNS updates. Credentials never enter the inventory.

use std::net::{Ipv4Addr, Ipv6Addr};
use std::time::Duration;

use reqwest::header::{HeaderValue, AUTHORIZATION};
use reqwest::{Client, StatusCode, Url};
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use super::network::PublicAddresses;
use crate::connection_fallback::dddns_name;

const DEFAULT_UPDATE_URL: &str = "https://users.doubleslash.space/v1/update";
const MAX_REPLY_BYTES: usize = 4096;

#[derive(Default, Deserialize)]
#[serde(transparent)]
pub struct UpdateToken(String);

impl Drop for UpdateToken {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

#[derive(Default, Deserialize)]
pub struct UpdateRequest {
    #[serde(default)]
    dddns_token: UpdateToken,
    #[serde(default)]
    dddns_update_url: String,
}

pub(super) struct UpdatePlan {
    url: Url,
    name: String,
}

impl UpdateRequest {
    pub(super) fn plan(&self, hostname: &str, lookup: &str) -> Result<Option<UpdatePlan>, String> {
        let official = dddns_name(hostname).is_some();
        if hostname.is_empty() || !(lookup == "dddns" || (lookup.is_empty() && official)) {
            return Ok(None);
        }
        let name = hostname.split('.').next().unwrap_or_default();
        if name.len() < 3 || name.starts_with("xn--") {
            return Err(
                "A DDDNS name needs a label of at least three characters, without xn--".into(),
            );
        }
        let token = &self.dddns_token.0;
        if token.is_empty() {
            return Err("Enter the DDDNS update token for this hostname".into());
        }
        if token.len() > 512 || !token.bytes().all(|b| b.is_ascii_graphic()) {
            return Err(
                "The DDDNS update token must contain no spaces or control characters".into(),
            );
        }
        let raw_url = match self.dddns_update_url.trim() {
            "" if official => DEFAULT_UPDATE_URL,
            "" => return Err("Enter this DDDNS provider's HTTPS update URL".into()),
            url => url,
        };
        let url = Url::parse(raw_url).map_err(|_| "Enter a valid HTTPS DDDNS update URL")?;
        if url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(
                "The DDDNS update URL must use HTTPS without credentials, query or fragment".into(),
            );
        }
        if official && url.as_str() != DEFAULT_UPDATE_URL {
            return Err(
                "DoubleSlash DDDNS updates use https://users.doubleslash.space/v1/update".into(),
            );
        }
        Ok(Some(UpdatePlan {
            url,
            name: name.to_owned(),
        }))
    }
}

#[derive(Serialize)]
struct UpdateBody<'a> {
    name: &'a str,
    // Never omit these: omission substitutes the caller IP or keeps an old
    // family. This caller is the desktop, and the hostname may be moving.
    ipv4: Option<Ipv4Addr>,
    ipv6: Option<Ipv6Addr>,
    port: u16,
}

impl UpdatePlan {
    pub(super) fn key(&self) -> String {
        format!("{}#{}", self.url, self.name)
    }

    fn body(
        &self,
        mut addresses: PublicAddresses,
        port: u16,
        signaling_ipv6: Option<bool>,
    ) -> Result<UpdateBody<'_>, String> {
        if signaling_ipv6 == Some(false) {
            addresses.ipv6 = None;
        }
        if port == 0 || (addresses.ipv4.is_none() && addresses.ipv6.is_none()) {
            return Err(
                "No usable signaling address and port are available for the DDDNS update".into(),
            );
        }
        Ok(UpdateBody {
            name: &self.name,
            ipv4: addresses.ipv4,
            ipv6: addresses.ipv6,
            port,
        })
    }

    pub(super) async fn publish(
        &self,
        request: &UpdateRequest,
        addresses: PublicAddresses,
        port: u16,
        signaling_ipv6: Option<bool>,
    ) -> Result<(), String> {
        let body = self.body(addresses, port, signaling_ipv6)?;
        let client = Client::builder()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|_| "Could not initialize the DDDNS update connection")?;
        post_update(&client, &self.url, &request.dddns_token, &body).await
    }
}

#[derive(Deserialize)]
struct UpdateReply {
    #[serde(rename = "changed")]
    _changed: bool,
    record: UpdatedRecord,
}

#[derive(Deserialize)]
struct UpdatedRecord {
    v: u8,
    name: String,
    status: String,
    ipv4: Option<Ipv4Addr>,
    ipv6: Option<Ipv6Addr>,
    port: u16,
}

async fn post_update(
    client: &Client,
    url: &Url,
    token: &UpdateToken,
    body: &UpdateBody<'_>,
) -> Result<(), String> {
    let bearer = zeroize::Zeroizing::new(format!("Bearer {}", token.0));
    let mut authorization = HeaderValue::from_str(&bearer)
        .map_err(|_| "The DDDNS update token cannot be used as a credential")?;
    authorization.set_sensitive(true);
    let mut response = client
        .post(url.clone())
        .header(AUTHORIZATION, authorization)
        .json(body)
        .send()
        .await
        .map_err(|_| "Could not confirm the DDDNS update: connection failed or timed out")?;
    match response.status() {
        status if status.is_success() => {}
        StatusCode::UNAUTHORIZED => {
            return Err("DDDNS rejected the update token; it may be invalid or revoked".into())
        }
        StatusCode::FORBIDDEN => {
            return Err("The DDDNS token is not authorized for this hostname".into())
        }
        StatusCode::CONFLICT => {
            return Err("This DDDNS name is not active or in its grace period".into())
        }
        StatusCode::BAD_REQUEST => {
            return Err("DDDNS rejected the hostname, addresses or signaling port".into())
        }
        StatusCode::TOO_MANY_REQUESTS => {
            return Err("DDDNS is rate limiting updates; try again later".into())
        }
        status if status.is_redirection() => {
            return Err("The DDDNS update endpoint redirected; the token was not forwarded".into())
        }
        _ => {
            return Err("Could not confirm the DDDNS update: the service returned an error".into())
        }
    }
    // Do not surface arbitrary response bodies: a faulty service may echo a
    // credential. Only a bounded, matching record establishes success.
    let mut bytes = zeroize::Zeroizing::new(Vec::new());
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "Could not read the DDDNS update confirmation")?
    {
        if bytes.len() + chunk.len() > MAX_REPLY_BYTES {
            return Err("The DDDNS update confirmation is too large".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let reply: UpdateReply = serde_json::from_slice(&bytes)
        .map_err(|_| "DDDNS did not return a valid update confirmation")?;
    let record = reply.record;
    if record.v != 1
        || record.name != body.name
        || !matches!(record.status.as_str(), "active" | "grace")
        || record.ipv4 != body.ipv4
        || record.ipv6 != body.ipv6
        || record.port != body.port
    {
        return Err(
            "The DDDNS update confirmation does not match this server's addresses and port".into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::{anyhow, ensure, Result};
    use serde_json::{json, Value};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    const TOKEN: &str = "ddt_test_only_not_a_real_credential";

    fn request(url: &str) -> Result<UpdateRequest> {
        Ok(serde_json::from_value(
            json!({ "dddns_token": TOKEN, "dddns_update_url": url }),
        )?)
    }

    #[test]
    fn token_and_https_destination_are_required_only_for_dddns() -> Result<()> {
        let empty = UpdateRequest::default();
        ensure!(empty
            .plan("relay.example.com", "dns")
            .map_err(anyhow::Error::msg)?
            .is_none());
        ensure!(empty.plan("", "").map_err(anyhow::Error::msg)?.is_none());
        ensure!(empty.plan("node.doubleslash.space", "").is_err());
        let update = request("")?;
        let plan = update
            .plan("node.doubleslash.space", "")
            .map_err(anyhow::Error::msg)?
            .ok_or_else(|| anyhow!("missing plan"))?;
        ensure!(plan.url.as_str() == DEFAULT_UPDATE_URL);
        ensure!(plan.name == "node");
        ensure!(request("https://other.example/v1/update")?
            .plan("node.doubleslash.space", "")
            .is_err());
        ensure!(update.plan("node.names.example", "dddns").is_err());
        for url in [
            "http://users.example/v1/update",
            "https://user:pass@users.example/v1/update",
            "https://users.example/v1/update?token=secret",
            "https://users.example/v1/update#secret",
        ] {
            ensure!(request(url)?.plan("node.names.example", "dddns").is_err());
        }
        ensure!(request("https://users.example/v1/update")?
            .plan("node.names.example", "dddns")
            .map_err(anyhow::Error::msg)?
            .is_some());
        let malformed: UpdateRequest =
            serde_json::from_value(json!({"dddns_token": "bad\r\nheader"}))?;
        ensure!(malformed.plan("node.doubleslash.space", "").is_err());
        Ok(())
    }

    #[test]
    fn payload_clears_absent_or_unusable_families_instead_of_using_the_laptop_ip() -> Result<()> {
        let update = request("")?;
        let plan = update
            .plan("node.doubleslash.space", "")
            .map_err(anyhow::Error::msg)?
            .ok_or_else(|| anyhow!("missing plan"))?;
        let addresses = PublicAddresses {
            ipv4: Some("9.9.9.9".parse()?),
            ipv6: Some("2606:4700::1234".parse()?),
        };
        let body = plan
            .body(addresses, 41000, Some(false))
            .map_err(anyhow::Error::msg)?;
        let json = serde_json::to_value(body)?;
        ensure!(json == json!({"name":"node", "ipv4":"9.9.9.9", "ipv6":null, "port":41000}));
        ensure!(plan
            .body(PublicAddresses::default(), 41000, Some(true))
            .is_err());
        Ok(())
    }

    async fn read_request(stream: &mut TcpStream) -> Result<(String, Value)> {
        let mut bytes = Vec::new();
        let (end, len) = loop {
            let mut chunk = [0u8; 1024];
            let n = stream.read(&mut chunk).await?;
            ensure!(n > 0, "request ended early");
            bytes.extend_from_slice(&chunk[..n]);
            ensure!(bytes.len() < 16 * 1024, "oversized test request");
            if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                let header = std::str::from_utf8(&bytes[..end])?;
                let len = header
                    .lines()
                    .filter_map(|l| l.split_once(':'))
                    .find(|(key, _)| key.eq_ignore_ascii_case("content-length"))
                    .ok_or_else(|| anyhow!("missing body length"))?
                    .1
                    .trim()
                    .parse::<usize>()?;
                break (end + 4, len);
            }
        };
        while bytes.len() < end + len {
            let mut chunk = [0u8; 1024];
            let n = stream.read(&mut chunk).await?;
            ensure!(n > 0, "body ended early");
            bytes.extend_from_slice(&chunk[..n]);
        }
        Ok((
            String::from_utf8(bytes[..end].to_vec())?,
            serde_json::from_slice(&bytes[end..end + len])?,
        ))
    }

    enum Reply {
        Echo,
        Raw(u16, String),
        Redirect(String),
    }

    type FixtureTask = tokio::task::JoinHandle<Result<Vec<(String, Value)>>>;

    async fn fixture(replies: Vec<Reply>) -> Result<(Url, FixtureTask)> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let url = Url::parse(&format!("http://{}/v1/update", listener.local_addr()?))?;
        let task = tokio::spawn(async move {
            let mut requests = Vec::new();
            for reply in replies {
                let (mut stream, _) = listener.accept().await?;
                let (header, body) = read_request(&mut stream).await?;
                let (status, extra, response) = match reply {
                    Reply::Echo => (
                        200,
                        String::new(),
                        json!({"changed":false, "record": {
                            "v":1, "name":body["name"], "status":"active", "ipv4":body["ipv4"],
                            "ipv6":body["ipv6"], "port":body["port"]
                        }})
                        .to_string(),
                    ),
                    Reply::Raw(status, text) => (status, String::new(), text),
                    Reply::Redirect(location) => {
                        (307, format!("Location: {location}\r\n"), String::new())
                    }
                };
                let response = format!("HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n{response}", response.len());
                stream.write_all(response.as_bytes()).await?;
                stream.shutdown().await?;
                requests.push((header, body));
            }
            Ok(requests)
        });
        Ok((url, task))
    }

    fn fixture_client() -> Result<Client> {
        // Only this private test transport uses loopback HTTP. Production
        // validates HTTPS endpoints and also enables reqwest's https_only.
        Ok(Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(3))
            .build()?)
    }

    #[tokio::test]
    async fn moving_the_same_name_posts_the_new_server_and_clears_the_old_family() -> Result<()> {
        let (url, task) = fixture(vec![Reply::Echo, Reply::Echo]).await?;
        let update = request("")?;
        let client = fixture_client()?;
        let first = UpdateBody {
            name: "node",
            ipv4: Some("8.8.4.4".parse()?),
            ipv6: Some("2606:4700::1234".parse()?),
            port: 34935,
        };
        let second = UpdateBody {
            name: "node",
            ipv4: Some("9.9.9.9".parse()?),
            ipv6: None,
            port: 41000,
        };
        post_update(&client, &url, &update.dddns_token, &first)
            .await
            .map_err(anyhow::Error::msg)?;
        post_update(&client, &url, &update.dddns_token, &second)
            .await
            .map_err(anyhow::Error::msg)?;
        let requests = task.await??;
        for (header, body) in &requests {
            ensure!(header.starts_with("POST /v1/update HTTP/1.1"));
            ensure!(header
                .to_ascii_lowercase()
                .contains(&format!("authorization: bearer {TOKEN}")));
            ensure!(!body.to_string().contains(TOKEN));
        }
        ensure!(
            requests[1].1 == json!({"name":"node", "ipv4":"9.9.9.9", "ipv6":null, "port":41000})
        );
        Ok(())
    }

    #[tokio::test]
    async fn rejects_unconfirmed_updates_without_echoing_credentials_or_following_redirects(
    ) -> Result<()> {
        let destination = TcpListener::bind("127.0.0.1:0").await?;
        let redirect = format!("http://{}/leak", destination.local_addr()?);
        let mismatch = json!({"changed":true, "record":{"v":1,"name":"other","status":"active","ipv4":"9.9.9.9","port":41000}}).to_string();
        let replies = vec![
            Reply::Raw(401, TOKEN.into()),
            Reply::Raw(403, TOKEN.into()),
            Reply::Redirect(redirect),
            Reply::Raw(200, "x".repeat(MAX_REPLY_BYTES + 1)),
            Reply::Raw(200, mismatch),
        ];
        let (url, task) = fixture(replies).await?;
        let client = fixture_client()?;
        let update = request("")?;
        let body = UpdateBody {
            name: "node",
            ipv4: Some("9.9.9.9".parse()?),
            ipv6: None,
            port: 41000,
        };
        for _ in 0..5 {
            let error = post_update(&client, &url, &update.dddns_token, &body)
                .await
                .err()
                .ok_or_else(|| anyhow!("unexpected success"))?;
            ensure!(!error.contains(TOKEN));
        }
        ensure!(task.await??.len() == 5);
        ensure!(
            tokio::time::timeout(Duration::from_millis(50), destination.accept())
                .await
                .is_err()
        );
        Ok(())
    }
}
