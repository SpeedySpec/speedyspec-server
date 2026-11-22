use std::ptr;

use crate::bindings::*;

#[test]
fn itworks() {
    let mut error_code = 0;
    let mut error_offset = 0;
    let pattern = r"\w+";
    let code = unsafe {
        pcre2_compile_8(
            pattern.as_ptr(),
            pattern.len(),
            PCRE2_UCP | PCRE2_UTF,
            &mut error_code,
            &mut error_offset,
            ptr::null_mut(),
        )
    };
    if code.is_null() {
        panic!(
            "compilation failed; error code: {:?}, offset: {:?}",
            error_code, error_offset
        );
    }

    let match_data = unsafe { pcre2_match_data_create_from_pattern_8(code, ptr::null_mut()) };
    if match_data.is_null() {
        unsafe {
            pcre2_code_free_8(code);
        }
        panic!("could not allocate match_data");
    }

    let ovector = unsafe { pcre2_get_ovector_pointer_8(match_data) };
    if ovector.is_null() {
        unsafe {
            pcre2_match_data_free_8(match_data);
            pcre2_code_free_8(code);
        }
        panic!("could not get ovector");
    }

    let subject = "  βabcβ foo";
    let rc = unsafe {
        pcre2_match_8(
            code,
            subject.as_ptr(),
            subject.len(),
            0,
            0,
            match_data,
            ptr::null_mut(),
        )
    };
    if rc <= 0 {
        unsafe {
            pcre2_match_data_free_8(match_data);
            pcre2_code_free_8(code);
        }
        panic!("error executing match");
    }

    let (s, e) = unsafe { (*ovector.offset(0), *ovector.offset(1)) };
    unsafe {
        pcre2_match_data_free_8(match_data);
        pcre2_code_free_8(code);
    }

    assert_eq!("βabcβ", &subject[s..e]);
}

/// The library is the static PHP's PCRE2 version, so both sides share one PCRE2's semantics.
#[test]
fn is_phps_pcre2_version() {
    let mut buf = [0u8; 64];
    let n = unsafe { pcre2_config_8(PCRE2_CONFIG_VERSION, buf.as_mut_ptr().cast()) };
    assert!(n > 0, "pcre2_config failed: {n}");
    let version = std::ffi::CStr::from_bytes_until_nul(&buf)
        .unwrap()
        .to_str()
        .unwrap();
    assert!(version.starts_with("10.44 "), "{version}");
}

/// Every symbol the library defines carries the speedyspec_ prefix, so none collides with the static PHP's
/// PCRE2 (whose internals keep their stock names). Fails when the vendored PCRE2 gains a symbol: run
/// `vendor/pcre2-sys/update-prefix`.
#[test]
fn every_symbol_is_prefixed() {
    let lib = env!("SPEEDYSPEC_PCRE2_LIB");
    let output = match std::process::Command::new("nm").args(["-g", lib]).output() {
        Ok(output) if output.status.success() => output,
        _ => {
            eprintln!("nm is not available; skipping the symbol check");
            return;
        }
    };
    let listing = String::from_utf8_lossy(&output.stdout);
    let mut defined = 0;
    let mut stock = Vec::new();
    for line in listing.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        // "<address> <type> <name>"; undefined symbols have no address. Lowercase types are local.
        let [_, kind, name] = fields[..] else {
            continue;
        };
        if kind == "U" || !kind.chars().all(|c| c.is_ascii_uppercase()) {
            continue;
        }
        defined += 1;
        let name = name
            .strip_prefix('_')
            .filter(|_| cfg!(target_vendor = "apple"))
            .unwrap_or(name);
        if !name.starts_with("speedyspec_") {
            stock.push(name.to_string());
        }
    }
    assert!(
        defined > 100,
        "expected PCRE2's symbols in {lib}, found {defined}"
    );
    assert!(
        stock.is_empty(),
        "symbols without the speedyspec_ prefix: {stock:?}"
    );
}
