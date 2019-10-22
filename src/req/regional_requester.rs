use std::future::Future;
use std::sync::Arc;

use reqwest::{ Client, StatusCode, Url };
use tokio_timer::delay_for;

use crate::riot_api_config::RiotApiConfig;
use crate::consts::Region;
use crate::util::InsertOnlyCHashMap;

use super::RateLimit;
use super::RateLimitType;

pub struct RegionalRequester {



    /// Represents the app rate limit.
    app_rate_limit: RateLimit,
    /// Represents method rate limits.
    method_rate_limits: InsertOnlyCHashMap<&'static str, RateLimit>,
}

impl RegionalRequester {
    /// Request header name for the Riot API key.
    const RIOT_KEY_HEADER: &'static str = "X-Riot-Token";

    /// HttpStatus codes that are considered a success, but will return None.
    const NONE_STATUS_CODES: [u16; 3] = [ 204, 404, 422 ];


    pub fn new() -> Self {
        Self {
            app_rate_limit: RateLimit::new(RateLimitType::Application),
            method_rate_limits: InsertOnlyCHashMap::new(),
        }
    }

    pub fn get<'a, T: serde::de::DeserializeOwned>(self: Arc<Self>,
        config: &'a RiotApiConfig, client: &'a Client,
        method_id: &'static str, region: Region, path: String,
        query: Option<String>)
        -> impl Future<Output = Result<Option<T>, reqwest::Error>> + 'a
    {
        async move {
            let query = query.as_ref().map(|s| s.as_ref());

            // let ref config = RiotApiConfig::with_key("asdf");
            // let ref client = Client::new();

            let mut attempts: u8 = 0;
            for _ in 0..=config.retries {
                attempts += 1;

                let method_rate_limit: Arc<RateLimit> = self.method_rate_limits
                    .get_or_insert_with(method_id, || RateLimit::new(RateLimitType::Method));

                // Rate limiting.
                while let Some(delay) = RateLimit::get_both_or_delay(&self.app_rate_limit, &*method_rate_limit) {
                    delay_for(delay).await;
                }

                // Send request.
                let url_base = format!("https://{}.api.riotgames.com", region.platform);
                let mut url = Url::parse(&*url_base)
                    .unwrap_or_else(|_| panic!("Failed to parse url_base: \"{}\".", url_base));
                url.set_path(&*path);
                url.set_query(query);

                let result = client.get(url)
                    .header(Self::RIOT_KEY_HEADER, &*config.api_key)
                    .send()
                    .await;

                let response = match result {
                    Err(e) => return Err(e),
                    Ok(r) => r,
                };

                // Maybe update rate limits (based on response headers).
                self.app_rate_limit.on_response(&response);
                method_rate_limit.on_response(&response);

                // Handle response.
                let status = response.status();
                // Success, return None.
                if Self::is_none_status_code(&status) {
                    return Ok(None);
                }
                // Success, return a value.
                if status.is_success() {
                    let value = response.json::<T>().await;
                    return match value {
                        Err(e) => Err(e),
                        Ok(v) => Ok(Some(v)),
                    }
                }
                // Retryable.
                if StatusCode::TOO_MANY_REQUESTS == status || status.is_server_error() {
                    continue;
                }
                // Failure (non-retryable).
                if status.is_client_error() {
                    break;
                }
                panic!("NOT HANDLED: {}!", status);
            }
            // TODO: return error.
            panic!("FAILED AFTER {} ATTEMPTS!", attempts);
        }
    }

    fn is_none_status_code(status: &StatusCode) -> bool {
        Self::NONE_STATUS_CODES.contains(&status.as_u16())
    }
}

#[cfg(test)]
mod tests {
    // use super::*;
}
