//! The memory repository prints a string under `%d` as `vsprintf( '%d' )` does, the same integers the SQL
//! repository sends.

use speedyspec_wp_posts_contracts::{Post, PostId, PostRepository, PostRow, RowValue};
use speedyspec_wp_posts_memory::MemoryPostRepository;

#[test]
fn strings_under_d_follow_php_sprintf() {
    for (given, want) in [
        (&b"1e3"[..], 1000),
        (b"1.5e2", 150),
        (b" 12", 12),
        (b"0x1A", 0),
        (b"1e400", 0),
        (b"\n5", 0),
    ] {
        let repo = MemoryPostRepository::new();
        let id = repo
            .insert(
                &PostRow::from_post(&Post::default())
                    .with("menu_order", RowValue::Str(given.to_vec())),
            )
            .unwrap()
            .written()
            .unwrap();
        let stored = repo.find(id).unwrap().unwrap().menu_order;
        assert_eq!(stored, want, "{}", String::from_utf8_lossy(given));
        assert_eq!(id, PostId(1));
    }
}
