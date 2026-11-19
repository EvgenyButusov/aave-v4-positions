use std::time::Duration;

use url::Url;

use crate::config::Config;

#[test]
fn names_each_provider_by_host_in_the_order_tried_so_no_key_is_logged() {
    let config = Config {
        preferred: Url::parse("https://eth.example/v2/the-providers-key").unwrap(),
        fallbacks: vec![Url::parse("http://localhost:8545/?key=secret").unwrap()],
        timeout: Duration::from_secs(1),
    };

    assert_eq!(config.hosts(), ["eth.example", "localhost:8545"]);
}
