use super::*;
use std::{collections::HashMap, sync::atomic::AtomicUsize};
use ureq::{Error, config::Config, http::Uri, unversioned::{
    resolver::{Resolver, ResolvedSocketAddrs},
    transport::{Buffers, ConnectionDetails, Connector, LazyBuffers, NextTimeout, Transport},
}};

#[derive(Debug, Default)]
struct Dns(AtomicUsize);

impl Resolver for Dns {
    fn resolve(&self, uri: &Uri, _: &Config, _: NextTimeout) -> Result<ResolvedSocketAddrs, Error> {
        let mut addresses = self.empty();
        let private = uri.host() == Some("private.test")
            || (uri.host() == Some("rebind.test") && self.0.fetch_add(1, Ordering::SeqCst) > 0);
        addresses.push("10.0.0.1:80".parse().unwrap());
        if !private { addresses.push("1.1.1.1:80".parse().unwrap()); }
        Ok(addresses)
    }
}

#[derive(Debug)]
struct Wire {
    replies: HashMap<String, Vec<u8>>,
    requested: Arc<Mutex<Vec<String>>>,
}

impl Connector for Wire {
    type Out = Reply;
    fn connect(&self, details: &ConnectionDetails, _: Option<()>) -> Result<Option<Reply>, Error> {
        assert!(matches!(details.uri.scheme_str(), Some("http" | "https")));
        assert_eq!(details.addrs.len(), 1);
        assert!(details.addrs.iter().all(|address| is_public_ip(address.ip())));
        let url = details.uri.to_string();
        self.requested.lock().unwrap().push(url.clone());
        Ok(Some(Reply { bytes: self.replies.get(&url).unwrap_or_else(|| panic!("unexpected request {url}")).clone(),
            buffers: LazyBuffers::new(4096, 4096) }))
    }
}

#[derive(Debug)]
struct Reply { bytes: Vec<u8>, buffers: LazyBuffers }

impl Transport for Reply {
    fn buffers(&mut self) -> &mut dyn Buffers { &mut self.buffers }
    fn transmit_output(&mut self, _: usize, _: NextTimeout) -> Result<(), Error> { Ok(()) }
    fn await_input(&mut self, _: NextTimeout) -> Result<bool, Error> {
        if self.bytes.is_empty() { return Ok(false); }
        let size = self.bytes.len();
        self.buffers.input_append_buf()[..size].copy_from_slice(&self.bytes);
        self.buffers.input_appended(size);
        self.bytes.clear();
        Ok(true)
    }
    fn is_open(&mut self) -> bool { !self.bytes.is_empty() }
}

fn agent(replies: Vec<(&str, String)>, proxy: Option<ureq::Proxy>) -> (ureq::Agent, Arc<Mutex<Vec<String>>>) {
    agent_bytes(replies.into_iter().map(|(url, response)| (url, response.into_bytes())).collect(), proxy)
}

fn agent_bytes(replies: Vec<(&str, Vec<u8>)>, proxy: Option<ureq::Proxy>) -> (ureq::Agent, Arc<Mutex<Vec<String>>>) {
    let requested = Arc::new(Mutex::new(Vec::new()));
    let config = ureq::Agent::config_builder().proxy(proxy).max_redirects(5).build();
    (ureq::Agent::with_parts(config, Wire {
        replies: replies.into_iter().map(|(url, reply)| (url.to_owned(), reply)).collect(),
        requested: Arc::clone(&requested),
    }, PublicResolver(Dns::default())), requested)
}

fn page(body: &str) -> String {
    format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
}

fn redirect(location: &str) -> String {
    format!("HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
}

#[test]
fn preview_rejects_special_addresses_including_translated_ipv4() {
    for address in ["0.1.2.3", "10.1.1.1", "127.1.2.3", "169.254.1.1", "172.31.255.255",
        "192.168.1.1", "100.64.0.1", "100.127.255.255", "192.0.0.8", "192.0.0.170",
        "192.88.99.1", "192.0.2.1", "198.18.0.1", "198.19.255.255", "198.51.100.1",
        "203.0.113.1", "224.0.0.1", "240.0.0.1", "255.255.255.255", "::", "::1", "::127.0.0.1",
        "::ffff:127.0.0.1", "::ffff:198.18.0.1", "64:ff9b::a00:1", "64:ff9b:1::1",
        "100::1", "100:0:0:1::1", "2001::1", "2001:2::1", "2001:db8::1", "2002:7f00:1::1",
        "3fff::1", "5f00::1", "fc00::1", "fd00::1", "fe80::1", "fec0::1", "ff02::1"] {
        assert!(!is_public_ip(address.parse().unwrap()), "{address}");
    }
    for address in ["1.1.1.1", "8.8.8.8", "100.63.255.255", "100.128.0.1", "192.0.0.9",
        "192.0.0.10", "198.17.255.255", "198.20.0.1", "::ffff:1.1.1.1", "64:ff9b::101:101",
        "2606:4700::1111", "2001:4860:4860::8888", "2001:3::1", "2001:4:112::1"] {
        assert!(is_public_ip(address.parse().unwrap()), "{address}");
    }
}

#[test]
fn redirects_recheck_addresses_and_reject_other_schemes() {
    for location in ["http://private.test/", "//private.test/", "file://public.test/secret", "ftp://public.test/"] {
        let (agent, requested) = agent(vec![("http://public.test/", redirect(location))], None);
        assert!(fetch_preview_with(&agent, "http://public.test/").is_none(), "{location}");
        assert_eq!(requested.lock().unwrap().len(), 1);
    }
    let (agent, requested) = agent(vec![("http://rebind.test/", redirect("/next"))], None);
    assert!(fetch_preview_with(&agent, "http://rebind.test/").is_none());
    assert_eq!(requested.lock().unwrap().len(), 1);
}

#[test]
fn relative_redirects_work_and_private_preview_images_are_skipped() {
    let (agent, requested) = agent(vec![
        ("http://public.test/path/start", redirect("../page")),
        ("http://public.test/page", page("<title>Kept</title><meta property='og:image' content='http://private.test/image.png'>")),
    ], None);
    let preview = fetch_preview_with(&agent, "http://public.test/path/start").unwrap();
    assert_eq!(preview.title.as_deref(), Some("Kept"));
    assert!(preview.thumbnail.is_none());
    assert_eq!(*requested.lock().unwrap(), ["http://public.test/path/start", "http://public.test/page"]);
}

#[test]
fn preview_images_use_the_same_redirect_policy() {
    let (agent, requested) = agent(vec![
        ("http://public.test/", page("<meta property='og:image' content='http://public.test/image'>")),
        ("http://public.test/image", redirect("//private.test/image")),
    ], None);
    assert!(fetch_preview_with(&agent, "http://public.test/").unwrap().thumbnail.is_none());
    assert_eq!(requested.lock().unwrap().len(), 2);
}

#[test]
fn unsupported_schemes_and_proxy_resolution_never_connect() {
    let (direct, requested) = agent(vec![], None);
    for url in ["file:///secret", "ftp://public.test/", "http://private.test/"] {
        assert!(fetch_preview_with(&direct, url).is_none());
    }
    assert!(requested.lock().unwrap().is_empty());
    let (proxied, requested) = agent(vec![], Some(ureq::Proxy::new("http://proxy.test:8080").unwrap()));
    assert!(fetch_preview_with(&proxied, "http://public.test/").is_none());
    assert!(requested.lock().unwrap().is_empty());
}

#[test]
fn redirect_loops_stop_after_five_hops() {
    let (agent, requested) = agent(vec![("http://public.test/", redirect("/"))], None);
    assert!(fetch_preview_with(&agent, "http://public.test/").is_none());
    assert_eq!(requested.lock().unwrap().len(), 6);
}

#[test]
fn artwork_fetch_reuses_public_resolution_and_decodes_the_image_before_caching() {
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(2, 2).write_to(&mut png, image::ImageFormat::Png).unwrap();
    let bytes = png.into_inner();
    let mut response = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", bytes.len()).into_bytes();
    response.extend(bytes);
    let (agent, requested) = agent_bytes(vec![("http://public.test/artwork", response)], None);
    let thumbnail = fetch_thumbnail_with(&agent, "http://public.test/artwork").unwrap();
    assert_eq!(image::guess_format(&thumbnail).unwrap(), image::ImageFormat::Jpeg);
    let decoded = image::load_from_memory(&thumbnail).unwrap();
    assert!((1..=512).contains(&decoded.width()) && (1..=512).contains(&decoded.height()));
    assert_eq!(*requested.lock().unwrap(), ["http://public.test/artwork"]);
}

#[test]
fn artwork_fetch_refuses_private_redirects_proxies_and_non_images() {
    let (direct, requested) = agent(vec![("http://public.test/artwork", redirect("http://private.test/artwork"))], None);
    for uri in ["file:///secret", "data:image/png;base64,AQ==", "http://private.test/artwork"] {
        assert!(fetch_thumbnail_with(&direct, uri).is_none());
    }
    assert!(requested.lock().unwrap().is_empty());
    assert!(fetch_thumbnail_with(&direct, "http://public.test/artwork").is_none());
    assert_eq!(*requested.lock().unwrap(), ["http://public.test/artwork"]);
    let (bad, _) = agent(vec![("http://public.test/artwork", page("not an image"))], None);
    assert!(fetch_thumbnail_with(&bad, "http://public.test/artwork").is_none());
    let (proxy, requested) = agent(vec![], Some(ureq::Proxy::new("http://proxy.test:8080").unwrap()));
    assert!(fetch_thumbnail_with(&proxy, "http://public.test/artwork").is_none());
    assert!(requested.lock().unwrap().is_empty());
}
