// Downloads from GitHub, and only from GitHub: the rules WhatsApp's "Install" uses to
// fetch GOWA (whatsapp/install.rs, whatsapp/gateway.rs). Same rules as the Mac's
// installer, and as upstream's update.rs, which this will be shared with if Coucou
// ever downloads its own updates.

use reqwest::Url;

/// Only GitHub serves the download (its assets redirect to githubusercontent.com).
pub fn allowed_host(host: Option<&str>) -> bool {
    let Some(h) = host.map(str::to_ascii_lowercase) else { return false };
    h == "github.com" || h == "api.github.com" || h.ends_with(".githubusercontent.com")
}

pub fn allowed_url(url: &str) -> bool {
    Url::parse(url).is_ok_and(|u| u.scheme() == "https" && allowed_host(u.host_str()))
}

/// "sha256:E823…" → the 64 lowercase hex digits; anything else → None.
pub fn parse_digest(digest: &str) -> Option<String> {
    let hex = digest.strip_prefix("sha256:")?;
    (hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit())).then(|| hex.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_github_may_serve_the_download() {
        for host in ["github.com", "api.github.com", "objects.githubusercontent.com", "release-assets.githubusercontent.com", "GitHub.com"] {
            assert!(allowed_host(Some(host)), "{host}");
        }
        for host in ["evil.com", "github.com.evil.com", "githubusercontent.com.evil.com", "notgithub.com", "githubusercontent.com", ""] {
            assert!(!allowed_host(Some(host)), "{host}");
        }
        assert!(!allowed_host(None));
        assert!(allowed_url("https://github.com/aldinokemal/go-whatsapp-web-multidevice/releases/download/x/y.zip"));
        assert!(!allowed_url("http://github.com/x"));
        assert!(!allowed_url("file:///C:/Windows/notepad.exe"));
        assert!(!allowed_url("not a url"));
    }

    #[test]
    fn digests_are_sha256_hex_only() {
        let hex = "e8234a092e8fe1bf6eed0395e6db13052be8fe47413949c2b7fa368467342337";
        assert_eq!(parse_digest(&format!("sha256:{hex}")).as_deref(), Some(hex));
        assert_eq!(parse_digest(&format!("sha256:{}", hex.to_uppercase())).as_deref(), Some(hex));
        assert_eq!(parse_digest(hex), None);
        assert_eq!(parse_digest(&format!("sha512:{hex}")), None);
        assert_eq!(parse_digest(&format!("sha256:{}", &hex[1..])), None);
        assert_eq!(parse_digest(&format!("sha256:{}z", &hex[1..])), None);
        assert_eq!(parse_digest(""), None);
    }
}
