//! Choosing a content coding from `Accept-Encoding`.

use http::HeaderValue;

/// The body variant to send.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    Br,
    Gzip,
    Identity,
}

/// The best acceptable variant: highest weight wins, `br` before `gzip` before identity on a tie, `q=0` excludes, `*`
/// covers codings not listed, and an unlisted identity never beats a listed compressed coding. With no header, or nothing acceptable, the identity body is sent.
pub fn choose(accept: Option<&HeaderValue>, has_br: bool, has_gzip: bool) -> Encoding {
    let Some(value) = accept.and_then(|v| v.to_str().ok()) else {
        return Encoding::Identity;
    };
    let mut star = None;
    let mut weights: Vec<(String, f32)> = Vec::new();
    for item in value.split(',') {
        let mut parts = item.split(';');
        let coding = parts.next().unwrap_or("").trim().to_ascii_lowercase();
        if coding.is_empty() {
            continue;
        }
        let mut q = 1.0f32;
        for param in parts {
            let param = param.trim();
            if let Some(raw) = param
                .strip_prefix("q=")
                .or_else(|| param.strip_prefix("Q="))
            {
                q = raw.trim().parse::<f32>().unwrap_or(1.0).clamp(0.0, 1.0);
            }
        }
        if coding == "*" {
            star = Some(q)
        } else {
            weights.push((coding, q))
        }
    }
    let weight = |name: &str, default: f32| {
        weights
            .iter()
            .find(|(coding, _)| coding == name)
            .map(|(_, q)| *q)
            .or(star)
            .unwrap_or(default)
    };

    // Identity is the fallback whatever its weight; it only beats a compressed variant when listed (or covered by `*`)
    // with a higher weight.
    let mut best = (Encoding::Identity, weight("identity", 0.0));
    for (available, encoding, name) in [
        (has_gzip, Encoding::Gzip, "gzip"),
        (has_br, Encoding::Br, "br"),
    ] {
        let q = weight(name, 0.0);
        if available && q > 0.0 && q >= best.1 {
            best = (encoding, q);
        }
    }

    best.0
}
