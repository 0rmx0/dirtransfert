use std::io::{Read, Write};
use std::net::{TcpStream, UdpSocket};
use std::time::Duration;

const SSDP_ADDR: (&str, u16) = ("239.255.255.250", 1900);
const DESCRIPTION_TIMEOUT: Duration = Duration::from_secs(5);
const RULE_DESC: &str = "DirTransfert";

fn split_url(url: &str) -> Result<(String, u16, String), String> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| format!("URL non supportee : {}", url))?;
    let (hostport, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (host, port) = match hostport.rfind(':') {
        Some(i) => (
            hostport[..i].to_string(),
            hostport[i + 1..].parse::<u16>().unwrap_or(80),
        ),
        None => (hostport.to_string(), 80),
    };
    if host.is_empty() {
        return Err("hote vide".into());
    }
    Ok((host, port, path.to_string()))
}

fn http_raw(host: &str, port: u16, request: &str) -> Result<String, String> {
    let mut s = TcpStream::connect((host, port))
        .map_err(|e| format!("connexion a {}:{} : {}", host, port, e))?;
    s.set_read_timeout(Some(DESCRIPTION_TIMEOUT)).ok();
    s.set_write_timeout(Some(DESCRIPTION_TIMEOUT)).ok();
    s.write_all(request.as_bytes())
        .map_err(|e| format!("envoi HTTP : {}", e))?;
    let mut out = String::new();
    s.read_to_string(&mut out)
        .map_err(|e| format!("lecture reponse HTTP : {}", e))?;
    let status_ok = out
        .lines()
        .next()
        .map(|l| l.contains("200"))
        .unwrap_or(false);
    if !status_ok {
        let snippet: String = out.chars().take(200).collect();
        return Err(format!("reponse HTTP non-200 : {}", snippet));
    }
    Ok(out)
}

fn http_get(url: &str) -> Result<String, String> {
    let (host, port, path) = split_url(url)?;
    let req = format!(
        "GET {} HTTP/1.1\r\nHost: {}:{}\r\nConnection: close\r\n\r\n",
        path, host, port
    );
    http_raw(&host, port, &req)
}

fn find_all(hay: &str, open: &str, close: &str) -> Vec<String> {
    let mut v = Vec::new();
    let mut rest = hay;
    while let Some(i) = rest.find(open) {
        rest = &rest[i + open.len()..];
        if let Some(j) = rest.find(close) {
            v.push(rest[..j].to_string());
            rest = &rest[j + close.len()..];
        } else {
            break;
        }
    }
    v
}

fn find_tag(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let i = xml.find(&open)?;
    let after = &xml[i + open.len()..];
    let j = after.find(&close)?;
    Some(after[..j].trim().to_string())
}


fn parse_services(desc: &str, location: &str) -> Vec<(String, u16, String, String)> {
    let mut out = Vec::new();
    let url_base = find_tag(desc, "URLBase").unwrap_or_default();
    let default_base = match split_url(location) {
        Ok((h, p, _)) => format!("http://{}:{}", h, p),
        Err(_) => String::new(),
    };
    for block in find_all(desc, "<service>", "</service>") {
        let st = match find_tag(&block, "serviceType") {
            Some(s) => s,
            None => continue,
        };
        let cu = match find_tag(&block, "controlURL") {
            Some(s) => s,
            None => continue,
        };
        if !st.contains("WANIPConnection") && !st.contains("WANPPPConnection") {
            continue;
        }
        let full = if cu.starts_with("http://") {
            cu
        } else if cu.starts_with('/') {
            if !url_base.is_empty() {
                format!("{}{}", url_base.trim_end_matches('/'), cu)
            } else {
                format!("{}{}", default_base, cu)
            }
        } else if !url_base.is_empty() {
            format!("{}/{}", url_base.trim_end_matches('/'), cu)
        } else {
            format!("{}/{}", default_base, cu)
        };
        if let Ok((h, p, path)) = split_url(&full) {
            out.push((h, p, path, st));
        }
    }
    out
}

fn ssdp_discover(timeout: Duration) -> Vec<String> {
    let sock = match UdpSocket::bind("0.0.0.0:0") {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    sock.set_read_timeout(Some(timeout)).ok();
    let msg = format!(
        "M-SEARCH * HTTP/1.1\r\nHOST: {}:{}\r\nMAN: \"ssdp:discover\"\r\nMX: 2\r\nST: upnp:rootdevice\r\n\r\n",
        SSDP_ADDR.0, SSDP_ADDR.1
    );
    if sock.send_to(msg.as_bytes(), SSDP_ADDR).is_err() {
        return Vec::new();
    }
    let mut locations = Vec::new();
    let deadline = std::time::Instant::now() + timeout;
    let mut buf = [0u8; 4096];
    while std::time::Instant::now() < deadline {
        match sock.recv_from(&mut buf) {
            Ok((n, _)) => {
                let resp = String::from_utf8_lossy(&buf[..n]).to_string();
                for line in resp.lines() {
                    let lower = line.to_ascii_lowercase();
                    if lower.starts_with("location:") {
                        let loc = line["location:".len()..].trim().to_string();
                        if !loc.is_empty() && !locations.contains(&loc) {
                            locations.push(loc);
                        }
                    }
                }
            }
            Err(_) => break,
        }
    }
    locations
}

fn local_ip_towards(host: &str) -> Result<String, String> {
    let s = UdpSocket::bind("0.0.0.0:0").map_err(|e| format!("bind : {}", e))?;
    s.connect((host, 80)).map_err(|e| format!("connect : {}", e))?;
    let addr = s.local_addr().map_err(|e| format!("adresse locale : {}", e))?;
    Ok(addr.ip().to_string())
}

fn soap_call(
    host: &str,
    port: u16,
    path: &str,
    service_type: &str,
    action: &str,
    body: &str,
) -> Result<String, String> {
    let envelope = format!(
        "<?xml version=\"1.0\"?><s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\"><s:Body><u:{} xmlns:u=\"{}\">{}</u:{}></s:Body></s:Envelope>",
        action, service_type, body, action
    );
    let req = format!(
        "POST {} HTTP/1.1\r\nHost: {}:{}\r\nContent-Type: text/xml; charset=\"utf-8\"\r\nSOAPAction: \"{}#{}\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        path, host, port, service_type, action, envelope.len(), envelope
    );
    http_raw(host, port, &req)
}


pub fn local_ip() -> String {
    let s = match UdpSocket::bind("0.0.0.0:0") {
        Ok(s) => s,
        Err(_) => return String::new(),
    };
    if s.connect("8.8.8.8:80").is_err() {
        return String::new();
    }
    s.local_addr().map(|a| a.ip().to_string()).unwrap_or_default()
}

pub struct Mapping {
    pub external_ip: String,
    pub external_port: u16,
}

fn try_add_on_device(location: &str, port: u16) -> Result<Mapping, String> {
    let desc = http_get(location)?;
    let services = parse_services(&desc, location);
    if services.is_empty() {
        return Err("aucun service WANIPConnection trouve".into());
    }
    for (host, cport, path, st) in services {
        let internal_ip = local_ip_towards(&host)?;
        let add_body = format!(
            "<NewRemoteHost></NewRemoteHost><NewExternalPort>{}</NewExternalPort><NewProtocol>TCP</NewProtocol><NewInternalPort>{}</NewInternalPort><NewInternalClient>{}</NewInternalClient><NewEnabled>1</NewEnabled><NewPortMappingDescription>{}</NewPortMappingDescription><NewLeaseDuration>0</NewLeaseDuration>",
            port, port, internal_ip, RULE_DESC
        );
        match soap_call(&host, cport, &path, &st, "AddPortMapping", &add_body) {
            Ok(_) => {
                let ip_body = "<NewExternalIPAddress></NewExternalIPAddress>";
                let ext_ip = soap_call(&host, cport, &path, &st, "GetExternalIPAddress", ip_body)
                    .ok()
                    .and_then(|resp| find_tag(&resp, "NewExternalIPAddress"))
                    .unwrap_or_default();
                return Ok(Mapping {
                    external_ip: ext_ip,
                    external_port: port,
                });
            }
            Err(e) => {
                let _ = e;
            }
        }
    }
    Err("la box a refuse l'ouverture du port".into())
}


pub fn get_external_ip() -> Result<String, String> {
    let locations = ssdp_discover(Duration::from_secs(2));
    if locations.is_empty() {
        return Err("aucune box UPnP detectee".into());
    }
    for loc in &locations {
        let desc = match http_get(loc) {
            Ok(d) => d,
            Err(_) => continue,
        };
        for (host, cport, path, st) in parse_services(&desc, loc) {
            let body = "<NewExternalIPAddress></NewExternalIPAddress>";
            if let Ok(resp) = soap_call(&host, cport, &path, &st, "GetExternalIPAddress", body) {
                if let Some(ip) = find_tag(&resp, "NewExternalIPAddress") {
                    if !ip.is_empty() {
                        return Ok(ip);
                    }
                }
            }
        }
    }
    Err("IP publique introuvable via la box".into())
}

pub fn add_port_mapping(port: u16) -> Result<Mapping, String> {
    let locations = ssdp_discover(Duration::from_secs(3));
    if locations.is_empty() {
        return Err("aucune box UPnP detectee sur le reseau".into());
    }
    let mut last = String::new();
    for loc in &locations {
        match try_add_on_device(loc, port) {
            Ok(m) => return Ok(m),
            Err(e) => last = e,
        }
    }
    Err(format!(
        "ouverture UPnP impossible ({}) sur {} equipement(s) teste(s)",
        last,
        locations.len()
    ))
}

pub fn delete_port_mapping(port: u16) -> Result<(), String> {
    let locations = ssdp_discover(Duration::from_secs(3));
    for loc in &locations {
        let desc = match http_get(loc) {
            Ok(d) => d,
            Err(_) => continue,
        };
        for (host, cport, path, st) in parse_services(&desc, loc) {
            let body = format!(
                "<NewRemoteHost></NewRemoteHost><NewExternalPort>{}</NewExternalPort><NewProtocol>TCP</NewProtocol>",
                port
            );
            let _ = soap_call(&host, cport, &path, &st, "DeletePortMapping", &body);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DESC: &str = "<?xml version=\"1.0\"?>
<root xmlns=\"urn:schemas-upnp-org:device-1-0\">
<URLBase>http://192.168.1.1:5000</URLBase>
<device>
<serviceList>
<service>
<serviceType>urn:schemas-upnp-org:service:WANIPConnection:1</serviceType>
<controlURL>/upnp/control/WANIPConn1</controlURL>
</service>
<service>
<serviceType>urn:schemas-upnp-org:service:Other:1</serviceType>
<controlURL>/ignore</controlURL>
</service>
</serviceList>
</device>
</root>";

    #[test]
    fn parses_wan_service() {
        let loc = "http://192.168.1.1:5000/root.xml";
        let svcs = parse_services(DESC, loc);
        assert_eq!(svcs.len(), 1);
        assert_eq!(svcs[0].0, "192.168.1.1");
        assert_eq!(svcs[0].1, 5000);
        assert_eq!(svcs[0].2, "/upnp/control/WANIPConn1");
        assert!(svcs[0].3.contains("WANIPConnection"));
    }

    #[test]
    fn splits_urls() {
        let (h, p, path) = split_url("http://10.0.0.138:49152/igd.xml").unwrap();
        assert_eq!(h, "10.0.0.138");
        assert_eq!(p, 49152);
        assert_eq!(path, "/igd.xml");
        let (h, p, _) = split_url("http://example.fr/desc.xml").unwrap();
        assert_eq!(h, "example.fr");
        assert_eq!(p, 80);
    }

    #[test]
    fn finds_tags() {
        assert_eq!(find_tag("<a><b>7</b></a>", "b").as_deref(), Some("7"));
        assert_eq!(find_tag("<a></a>", "b"), None);
    }
}

#[cfg(test)]
mod e2e_tests {
    use super::*;

    #[test]
    fn full_add_get_delete_against_fake_igd() {
        if std::env::var("DTX_IGD_URL").ok().is_none() {
            return;
        }
        let loc = std::env::var("DTX_IGD_URL").unwrap();
        let m = try_add_on_device(&loc, 47500).expect("AddPortMapping doit reussir");
        assert_eq!(m.external_port, 47500);
        assert_eq!(m.external_ip, "82.65.12.34");
        delete_port_mapping(47500).expect("DeletePortMapping doit reussir");
    }
}
