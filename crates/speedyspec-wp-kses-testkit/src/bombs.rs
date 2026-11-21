//! Bombs aimed at kses itself: inputs whose size is a parameter, to check that kses's work and output grow at most
//! a little faster than the input.

/// A bomb family: its name and how to build one of about `n` bytes.
pub type BombGenerator = (&'static str, fn(usize) -> Vec<u8>);

fn repeat(unit: &[u8], n: usize) -> Vec<u8> {
    unit.repeat((n / unit.len()).max(1))
}

/// Every bomb family aimed at kses's own work.
pub fn bomb_generators() -> Vec<BombGenerator> {
    vec![
        ("nested-comments", |n| {
            [repeat(b"<!--", n / 2), b"x".to_vec(), repeat(b"-->", n / 2)].concat()
        }),
        ("unclosed-comments", |n| repeat(b"<!--", n)),
        ("nested-invalid-closers", |n| {
            [repeat(b"</1", n / 2), repeat(b">", n / 2)].concat()
        }),
        // PHP never finishes these: every pass of the comment's "until stable" loop adds a `&gt;`.
        ("never-settling-comment", |n| {
            [b"<!--".to_vec(), repeat(b"</<i>", n)].concat()
        }),
        ("never-settling-comments", |n| repeat(b"<!--</<i>-->", n)),
        ("nested-declarations", |n| {
            [repeat(b"<!a", n / 2), repeat(b">", n / 2)].concat()
        }),
        ("unterminated-tag", |n| {
            [b"<p ".to_vec(), repeat(b"a", n)].concat()
        }),
        ("long-attribute-value", |n| {
            [
                b"<p title=\"".to_vec(),
                repeat(b"a", n),
                b"\">x</p>".to_vec(),
            ]
            .concat()
        }),
        ("many-attributes", |n| {
            [b"<p".to_vec(), repeat(b" a=1", n), b">x</p>".to_vec()].concat()
        }),
        ("many-distinct-attributes", |n| {
            let mut out = b"<p".to_vec();
            let mut i = 0usize;
            while out.len() < n {
                out.extend_from_slice(format!(" a{i}=1").as_bytes());
                i += 1;
            }
            out.extend_from_slice(b">x</p>");
            out
        }),
        ("many-duplicate-styles", |n| {
            [
                b"<p".to_vec(),
                repeat(b" style=\"color:red\"", n),
                b">x</p>".to_vec(),
            ]
            .concat()
        }),
        ("many-entities", |n| repeat(b"&#65;&amp;&nbsp;&bogus;", n)),
        ("ampersands", |n| repeat(b"&", n)),
        ("deep-tags", |n| {
            [repeat(b"<div>", n / 2), repeat(b"</div>", n / 2)].concat()
        }),
        ("stray-gt", |n| repeat(b">", n)),
        ("lone-lt", |n| repeat(b"<", n)),
        ("long-style", |n| {
            [
                b"<p style=\"".to_vec(),
                repeat(b"color:red;", n),
                b"\">x</p>".to_vec(),
            ]
            .concat()
        }),
        ("nested-css-functions", |n| {
            [
                b"<p style=\"width:".to_vec(),
                repeat(b"calc(", n / 2),
                b"1px".to_vec(),
                repeat(b")", n / 2),
                b"\">x</p>".to_vec(),
            ]
            .concat()
        }),
        ("many-url-functions", |n| {
            [
                b"<p style=\"background:".to_vec(),
                repeat(b"url(a.png) ", n),
                b"\">x</p>".to_vec(),
            ]
            .concat()
        }),
        ("many-distinct-url-functions", |n| {
            let urls: Vec<u8> = (0..n / 12)
                .flat_map(|i| format!("url(a{i}.png) ").into_bytes())
                .collect();
            [
                b"<p style=\"background:".to_vec(),
                urls,
                b"\">x</p>".to_vec(),
            ]
            .concat()
        }),
        ("protocol-chain", |n| {
            [
                b"<a href=\"".to_vec(),
                repeat(b"javascript:", n),
                b"x\">y</a>".to_vec(),
            ]
            .concat()
        }),
        ("feed-chain", |n| {
            [
                b"<a href=\"".to_vec(),
                repeat(b"feed:", n),
                b"http://a\">y</a>".to_vec(),
            ]
            .concat()
        }),
    ]
}
