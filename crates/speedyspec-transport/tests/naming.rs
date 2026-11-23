use speedyspec_transport::naming::{pascal, plural};

#[test]
fn pascal_and_plural() {
    assert_eq!(pascal("post_type"), "PostType");
    assert_eq!(pascal("update-report"), "UpdateReport");
    assert_eq!(pascal("acme/thing"), "AcmeThing");
    assert_eq!(pascal("report"), "Report");
    assert_eq!(plural("category"), "categories");
    assert_eq!(plural("box"), "boxes");
    assert_eq!(plural("report"), "reports");
}
