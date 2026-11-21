//! The benchmark corpus: post snippets (WordPress's whole-posts test data and block fixtures over 200 bytes) and a
//! full page (a theme's head, navigation and scripts around 40 of those posts), as the design spike measured.

use crate::posts;

/// Every post over 200 bytes.
pub fn bench_snippets() -> Vec<Vec<u8>> {
    posts().into_iter().filter(|p| p.len() > 200).collect()
}

/// A rendered home page: 40 posts as theme articles, with a head, navigation, inline scripts and comments.
pub fn bench_page() -> Vec<u8> {
    let snippets = bench_snippets();
    let mut body = Vec::new();
    for i in 0..40 {
        let p = &snippets[i % snippets.len()];
        body.extend_from_slice(
            format!(
                "<article id=\"post-{i}\" class=\"post-{i} post type-post status-publish format-standard hentry \
                 category-uncategorized\"><header class=\"entry-header\"><h2 class=\"entry-title\"><a \
                 href=\"https://example.com/?p={i}\" rel=\"bookmark\">Post {i}</a></h2></header><div \
                 class=\"entry-content\">"
            )
            .as_bytes(),
        );
        body.extend_from_slice(p);
        body.extend_from_slice(
            b"</div><footer class=\"entry-footer\"><span class=\"posted-on\"><time class=\"entry-date published\" \
              datetime=\"2026-11-20T10:00:00+00:00\">November 20, 2026</time></span></footer></article>\n",
        );
    }
    let head = "<!DOCTYPE html><html lang=\"en-US\"><head><meta charset=\"UTF-8\"><meta name=\"viewport\" \
        content=\"width=device-width, initial-scale=1\"><title>Site</title><link rel=\"stylesheet\" \
        id=\"wp-block-library-css\" href=\"https://example.com/wp-includes/css/dist/block-library/style.min.css?ver=6.9\" \
        media=\"all\"><style id=\"global-styles-inline-css\">:root{--wp--preset--color--black:#000;\
        --wp--preset--color--white:#fff}body{margin:0}</style><script \
        src=\"https://example.com/wp-includes/js/jquery/jquery.min.js?ver=3.7.1\" id=\"jquery-core-js\"></script>\
        <script>var x = \"<div>\" + 1 < 2;</script></head><body class=\"home blog wp-embed-responsive\"><div \
        id=\"page\" class=\"site\"><a class=\"skip-link screen-reader-text\" href=\"#content\">Skip</a><nav \
        id=\"site-navigation\" class=\"main-navigation\"><ul id=\"primary-menu\" class=\"menu\"><li \
        class=\"menu-item\"><a href=\"/\">Home</a></li><li class=\"menu-item\"><a href=\"/about/\" \
        onclick=\"track()\">About</a></li></ul></nav><main id=\"main\" class=\"site-main\">";
    let tail = "</main><!-- #main --></div><script type=\"speculationrules\">{\"prefetch\":[{\"source\":\"document\"}]}\
        </script></body></html>";
    [head.as_bytes(), &body, tail.as_bytes()].concat()
}
