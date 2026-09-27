//! Link previews: fetching a page's card metadata and image.

use super::*;

#[cfg(test)]
#[path = "links_tests.rs"]
mod tests;

/// A link preview fetched from a URL.
pub(super) struct LinkPreview {
    pub(super) url: String,
    pub(super) title: Option<String>,
    pub(super) description: Option<String>,
    pub(super) thumbnail: Option<Vec<u8>>,
    /// `og:site_name`, shown above the title.
    pub(super) site: Option<String>,
    /// The page's `theme-color`, for the embed's side bar.
    pub(super) color: Option<String>,
}

/// The first http(s) URL in a piece of text.
pub(super) fn first_url(text: &str) -> Option<String> {
    let start = text.find("http://").or_else(|| text.find("https://"))?;
    let rest = &text[start..];
    let end = rest
        .find(|c: char| c.is_whitespace() || c == '<' || c == '>')
        .unwrap_or(rest.len());
    Some(rest[..end].to_string())
}

/// Fetches a page's Open Graph / Twitter card metadata and its image, the way
/// Discord's embeds do.
///
/// Blocking; call it from `spawn_blocking`. Discord's crawler user agent is what
/// sites with rich embeds (fxtwitter, fixupx, YouTube…) answer with full cards.
pub(super) fn fetch_link_preview(url: &str) -> Option<LinkPreview> {
    let config = ureq::Agent::config_builder()
        .max_redirects(5)
        .timeout_global(Some(Duration::from_secs(10)))
        .user_agent("Mozilla/5.0 (compatible; Discordbot/2.0; +https://discordapp.com)")
        .build();
    let agent = ureq::Agent::with_parts(
        config,
        ureq::unversioned::transport::DefaultConnector::new(),
        PublicResolver(ureq::unversioned::resolver::DefaultResolver::default()),
    );
    fetch_preview_with(&agent, url)
}

fn fetch_preview_with(agent: &ureq::Agent, url: &str) -> Option<LinkPreview> {
    // A proxy may resolve the target itself, bypassing our address filter.
    if agent.config().proxy().is_some() {
        log::debug!("link preview skipped: proxy target addresses cannot be verified");
        return None;
    }
    let mut response = fetch_public(agent, url)?;
    let html = response.body_mut().with_config().limit(3 << 20).read_to_string().ok()?;
    let meta = meta_tags(&html);
    let get = |keys: &[&str]| keys.iter().find_map(|k| meta.get(*k)).filter(|v| !v.trim().is_empty()).cloned();

    let title = get(&["og:title", "twitter:title"]).or_else(|| html_title(&html));
    let image = get(&["og:image", "og:image:url", "twitter:image", "twitter:image:src"])
        .filter(|src| src.starts_with("http"));
    let thumbnail = image.and_then(|src| {
        let mut response = fetch_public(agent, &src)?;
        let bytes = response.body_mut().with_config().limit(8 << 20).read_to_vec().ok()?;
        link_thumbnail(&bytes)
    });
    let color = get(&["theme-color"]).filter(|c| {
        let c = c.trim();
        (c.starts_with('#') && c.len() <= 9 && c[1..].chars().all(|d| d.is_ascii_hexdigit())) || c.starts_with("rgb")
    });

    Some(LinkPreview {
        url: url.to_string(),
        title,
        description: get(&["og:description", "twitter:description", "description"]),
        thumbnail,
        site: get(&["og:site_name", "application-name"]),
        color,
    })
}

/// Resolves like the default resolver but keeps only public addresses, so the
/// address that was checked is the one connected to (no DNS-rebinding window).
#[derive(Debug, Default)]
struct PublicResolver<R>(R);

impl<R: ureq::unversioned::resolver::Resolver> ureq::unversioned::resolver::Resolver for PublicResolver<R> {
    fn resolve(
        &self,
        uri: &ureq::http::Uri,
        config: &ureq::config::Config,
        timeout: ureq::unversioned::transport::NextTimeout,
    ) -> Result<ureq::unversioned::resolver::ResolvedSocketAddrs, ureq::Error> {
        let resolved = self.0.resolve(uri, config, timeout)?;
        let mut public = self.0.empty();
        for addr in resolved.iter().filter(|a| is_public_ip(a.ip())) {
            public.push(*addr);
        }
        if public.is_empty() {
            log::warn!("link preview: {} has no public address, skipped", uri.host().unwrap_or("?"));
            return Err(ureq::Error::HostNotFound);
        }
        Ok(public)
    }
}

/// GETs `url` over http(s), following up to five redirects. The agent's
/// resolver keeps it on the public internet, redirects included.
fn fetch_public(agent: &ureq::Agent, url: &str) -> Option<ureq::http::Response<ureq::Body>> {
    let uri: ureq::http::Uri = url.parse().ok()?;
    if !matches!(uri.scheme_str(), Some("http" | "https")) {
        return None;
    }
    agent.get(url).call().ok()
}

/// Whether an address is on the public internet: not loopback, private,
/// link-local, carrier-grade NAT, documentation, multicast or unspecified.
pub(super) fn is_public_ip(ip: std::net::IpAddr) -> bool {
    use std::net::{IpAddr, Ipv4Addr};
    let public_v4 = |ip: Ipv4Addr| {
        let [a, b, c, d] = ip.octets();
        !(ip.is_private()
            || ip.is_loopback()
            || ip.is_link_local()
            || ip.is_unspecified()
            || ip.is_broadcast()
            || ip.is_multicast()
            || ip.is_documentation()
            || a == 0 || a >= 240
            || (a == 100 && (b & 0xc0) == 64)
            || (a == 192 && b == 0 && c == 0 && !matches!(d, 9 | 10))
            || (a == 192 && b == 88 && c == 99)
            || (a == 198 && matches!(b, 18 | 19)))
    };
    match ip {
        IpAddr::V4(ip) => public_v4(ip),
        IpAddr::V6(ip) => match ip.to_ipv4_mapped() {
            Some(v4) => public_v4(v4),
            None => {
                let [a, b, c, d, e, f, g, h] = ip.segments();
                if [a, b, c, d, e, f] == [0x64, 0xff9b, 0, 0, 0, 0] {
                    return public_v4(Ipv4Addr::from((u32::from(g) << 16) | u32::from(h)));
                }
                // Public unicast, excluding IANA special-purpose ranges and 6to4.
                if a & 0xe000 != 0x2000 || a == 0x2002 || (a == 0x3fff && b < 0x1000)
                    || (a == 0x2001 && b == 0xdb8) {
                    return false;
                }
                if a == 0x2001 && b < 0x200 {
                    return b == 3 || (b == 4 && c == 0x112)
                        || matches!(b & 0xfff0, 0x20 | 0x30)
                        || (b == 1 && [c, d, e, f, g] == [0; 5] && matches!(h, 1..=3));
                }
                true
            }
        },
    }
}

/// `<meta>` contents keyed by their lower-cased `property` or `name`; the first wins.
pub(super) fn meta_tags(html: &str) -> std::collections::HashMap<String, String> {
    let lower = html.to_ascii_lowercase();
    let mut out = std::collections::HashMap::new();
    let mut at = 0;
    while let Some(found) = lower[at..].find("<meta") {
        let start = at + found;
        let Some(len) = lower[start..].find('>') else { break };
        let tag = &html[start..start + len];
        at = start + len;
        let key = tag_attr(tag, "property").or_else(|| tag_attr(tag, "name"));
        if let (Some(key), Some(content)) = (key, tag_attr(tag, "content")) {
            out.entry(key.to_ascii_lowercase()).or_insert_with(|| decode_entities(content));
        }
    }
    out
}

/// An attribute's value in one HTML tag, quoted either way or bare.
pub(super) fn tag_attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let lower = tag.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let skip_space = |mut j: usize| {
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        j
    };
    let mut from = 0;
    while let Some(found) = lower[from..].find(name) {
        let i = from + found;
        from = i + name.len();
        if i == 0 || !bytes[i - 1].is_ascii_whitespace() {
            continue;
        }
        let j = skip_space(from);
        if bytes.get(j) != Some(&b'=') {
            continue;
        }
        let j = skip_space(j + 1);
        return match *bytes.get(j)? {
            quote @ (b'"' | b'\'') => {
                let len = tag[j + 1..].find(quote as char)?;
                Some(&tag[j + 1..j + 1 + len])
            }
            _ => {
                let len = tag[j..]
                    .find(|c: char| c.is_ascii_whitespace() || c == '/')
                    .unwrap_or(tag.len() - j);
                Some(&tag[j..j + len])
            }
        };
    }
    None
}

/// The document's `<title>`, when there is no card title.
pub(super) fn html_title(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let open = lower.find("<title")?;
    let start = open + lower[open..].find('>')? + 1;
    let end = start + lower[start..].find("</title")?;
    Some(decode_entities(html[start..end].trim())).filter(|t| !t.is_empty())
}

/// Decodes the HTML entities that show up in meta tags.
pub(super) fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let Some(semi) = rest[..rest.len().min(10)].find(';') else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..semi];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some('\u{a0}'),
            _ => entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// A link's image as the JPEG a message carries: any format, at most 512 px.
fn link_thumbnail(bytes: &[u8]) -> Option<Vec<u8>> {
    let image = image::load_from_memory(bytes).ok()?.thumbnail(512, 512);
    let mut out = Vec::new();
    image::DynamicImage::ImageRgb8(image.to_rgb8())
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Jpeg)
        .ok()?;
    Some(out)
}
