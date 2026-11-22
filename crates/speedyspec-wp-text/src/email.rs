//! `is_email()` and `sanitize_email()`'s cores: the verdict and the context string their filters get.

use crate::strings::php_trim;

/// What `is_email()` decided.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmailCheck<'a> {
    /// Valid: the address as given.
    Valid(&'a [u8]),
    /// Invalid, with the context WordPress passes to its `is_email` filter.
    Invalid(&'static str),
}

/// `[a-zA-Z0-9!#$%&'*+/=?^_`{|}~.-]`: the characters `is_email()` allows in the local part.
fn local_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"!#$%&'*+/=?^_`{|}~.-".contains(&b)
}

/// PHP's `trim()` characters plus `.` (domains) or `-` (labels).
const TRIM_DOT: &[u8] = b" \t\n\r\0\x0B.";
const TRIM_HYPHEN: &[u8] = b" \t\n\r\0\x0B-";

/// `explode( '@', $email, 2 )` once `strpos( $email, '@', 1 )` found one.
fn split_at_sign(email: &[u8]) -> Option<(&[u8], &[u8])> {
    if email.len() < 6 {
        return None;
    }
    email[1..].iter().position(|&b| b == b'@')?;
    let at = email.iter().position(|&b| b == b'@').expect("found above");
    Some((&email[..at], &email[at + 1..]))
}

/// WordPress's `is_email()` before its filter.
///
/// @param email
///     The address, already a string.
/// @return
///     Its verdict.
pub fn is_email_core(email: &[u8]) -> EmailCheck<'_> {
    if email.len() < 6 {
        return EmailCheck::Invalid("email_too_short");
    }
    let Some((local, domain)) = split_at_sign(email) else {
        return EmailCheck::Invalid("email_no_at");
    };
    // PCRE's `$` also matches before one final newline.
    let checked = local.strip_suffix(b"\n").unwrap_or(local);
    if checked.is_empty() || !checked.iter().all(|&b| local_char(b)) {
        return EmailCheck::Invalid("local_invalid_chars");
    }
    if domain.windows(2).any(|w| w == b"..") {
        return EmailCheck::Invalid("domain_period_sequence");
    }
    if php_trim(domain, TRIM_DOT) != domain {
        return EmailCheck::Invalid("domain_period_limits");
    }
    let subs: Vec<&[u8]> = domain.split(|&b| b == b'.').collect();
    if subs.len() < 2 {
        return EmailCheck::Invalid("domain_no_periods");
    }
    for sub in subs {
        if php_trim(sub, TRIM_HYPHEN) != sub {
            return EmailCheck::Invalid("sub_hyphen_limits");
        }
        let label = sub.strip_suffix(b"\n").unwrap_or(sub);
        if label.is_empty() || !label.iter().all(|&b| b.is_ascii_alphanumeric() || b == b'-') {
            return EmailCheck::Invalid("sub_invalid_chars");
        }
    }
    EmailCheck::Valid(email)
}

/// WordPress's `sanitize_email()` before its filter.
///
/// @param email
///     The address, already a string.
/// @return
///     The sanitized address (`""` when it can't be made valid) and, when it is `""`, the context WordPress passes
///     to its `sanitize_email` filter.
pub fn sanitize_email_core(email: &[u8]) -> (Vec<u8>, Option<&'static str>) {
    let invalid = |context| (Vec::new(), Some(context));
    if email.len() < 6 {
        return invalid("email_too_short");
    }
    let Some((local, domain)) = split_at_sign(email) else {
        return invalid("email_no_at");
    };
    let local: Vec<u8> = local.iter().copied().filter(|&b| local_char(b)).collect();
    if local.is_empty() {
        return invalid("local_invalid_chars");
    }
    // preg_replace( '/\.{2,}/', '', $domain ): runs of two or more periods go.
    let mut collapsed = Vec::with_capacity(domain.len());
    let mut i = 0;
    while i < domain.len() {
        let run = domain[i..].iter().take_while(|&&b| b == b'.').count();
        if run >= 2 {
            i += run;
        } else {
            collapsed.push(domain[i]);
            i += 1;
        }
    }
    if collapsed.is_empty() {
        return invalid("domain_period_sequence");
    }
    let domain = php_trim(&collapsed, TRIM_DOT);
    if domain.is_empty() {
        return invalid("domain_period_limits");
    }
    let subs: Vec<&[u8]> = domain.split(|&b| b == b'.').collect();
    if subs.len() < 2 {
        return invalid("domain_no_periods");
    }
    let new_subs: Vec<Vec<u8>> = subs
        .into_iter()
        .map(|sub| php_trim(sub, TRIM_HYPHEN).iter().copied().filter(|b| b.is_ascii_alphanumeric() || *b == b'-').collect::<Vec<u8>>())
        .filter(|sub| !sub.is_empty())
        .collect();
    if new_subs.len() < 2 {
        return invalid("domain_no_valid_subs");
    }
    ([local.as_slice(), b"@", &new_subs.join(&b'.')].concat(), None)
}
