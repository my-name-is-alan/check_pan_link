use rand::Rng;
use reqwest::RequestBuilder;

const PAN115_REFERER: &str = "https://115.com/";
const PAN115_ORIGIN: &str = "https://115.com";

pub(super) fn with_share_snap_headers(request: RequestBuilder) -> RequestBuilder {
    request
        .header("Referer", PAN115_REFERER)
        .header("Origin", PAN115_ORIGIN)
        .header("X-Forwarded-For", random_forwarded_ipv4())
}

fn random_forwarded_ipv4() -> String {
    let mut rng = rand::rng();

    loop {
        let first = rng.random_range(1..=223);
        if matches!(first, 10 | 127) {
            continue;
        }

        let second = rng.random_range(0..=255);
        if matches!((first, second), (172, 16..=31) | (192, 168) | (169, 254)) {
            continue;
        }

        let third: u8 = rng.random_range(0..=255);
        let fourth: u8 = rng.random_range(0..=255);
        return format!("{first}.{second}.{third}.{fourth}");
    }
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use super::*;

    #[test]
    fn random_forwarded_ipv4_uses_allowed_public_range() {
        for _ in 0..256 {
            let value = random_forwarded_ipv4();
            let address = value.parse::<Ipv4Addr>().unwrap();
            let [first, second, ..] = address.octets();

            assert!((1..=223).contains(&first), "{value}");
            assert!(
                !matches!(
                    (first, second),
                    (10, _) | (127, _) | (172, 16..=31) | (192, 168) | (169, 254)
                ),
                "{value}"
            );
        }
    }
}
