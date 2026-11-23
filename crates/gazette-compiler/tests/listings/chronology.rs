//! Reading-index chronology preserves authored precision and compares known instants.

use super::*;

#[test]
fn mixed_precision_uses_day_order_without_changing_original_dates() {
    use workspace::ListingVariant;

    let entries = [
        ("original", "Original", Some("2023-12-28")),
        ("later", "Later", Some("2025-08-31T12:27:11.670Z")),
        ("previous", "Previous day", Some("2026-10-04")),
        ("early", "Z first instant", Some("2026-10-05T00:00:00Z")),
        (
            "offset-a",
            "Same instant",
            Some("2026-10-06T00:30:00+14:00"),
        ),
        (
            "offset-z",
            "Same instant",
            Some("2026-10-04T20:30:00-14:00"),
        ),
        (
            "late",
            "Z last instant",
            Some("2026-10-05T23:59:59.999999999Z"),
        ),
        ("date-b", "A unknown time", Some("2026-10-05")),
        ("date-a", "Same unknown time", Some("2026-10-05")),
        ("date-z", "Same unknown time", Some("2026-10-05")),
        ("next", "Next day", Some("2026-10-06")),
        ("undated", "AAA undated", None),
    ];
    let mut files = vec![("one/index.md".into(), index("content:index", "/blog/"))];
    files.extend(entries.iter().map(|(id, title, date)| {
        (
            format!("one/{id}.md"),
            article(
                &format!("content:{id}"),
                title,
                &date.map_or_else(String::new, |date| format!("published='{date}'")),
                "",
            ),
        )
    }));
    let compiled = configuration(&[alternating(3)], "");
    let pages = project(&compiled, &parse(&files)).expect("mixed precision reading indexes");
    for (variant, expected) in [
        (
            ListingVariant::Primary,
            [
                "next", "late", "offset-a", "offset-z", "early", "date-b", "date-a", "date-z",
                "previous", "later", "original", "undated",
            ],
        ),
        (
            ListingVariant::Alternate,
            [
                "original", "later", "previous", "early", "offset-a", "offset-z", "late", "date-b",
                "date-a", "date-z", "next", "undated",
            ],
        ),
    ] {
        let mut ordered: Vec<_> = pages
            .iter()
            .filter(|page| page.variant == variant)
            .collect();
        ordered.sort_by_key(|page| page.page_number);
        assert_eq!(ordered.len(), 4);
        assert_eq!(
            ordered
                .iter()
                .flat_map(|page| ids(page))
                .collect::<Vec<_>>(),
            expected.map(|id| format!("content:{id}")),
        );
        for page in ordered {
            assert_eq!(
                page.latest.as_ref().unwrap().id,
                key("content:next"),
                "older mixed precision cannot obscure the sole newest editorial day"
            );
            for item in &page.items {
                let original = entries
                    .iter()
                    .find(|(id, _, _)| item.id.as_str() == format!("content:{id}"))
                    .unwrap()
                    .2;
                assert_eq!(json!(item.published), json!(original));
            }
        }
    }
}

#[test]
fn timestamp_order_compares_instants_and_uses_title_then_id_for_equal_instants() {
    let compiled = configuration(
        &[listing("content:index", "standalone-articles", DATES, 20)],
        "",
    );
    let files = vec![
        ("one/index.md".into(), index("content:index", "/blog/")),
        (
            "one/z.md".into(),
            article("content:z", "Same", "published='2026-10-05T10:00:00Z'", ""),
        ),
        (
            "one/a.md".into(),
            article(
                "content:a",
                "Same",
                "published='2026-10-05T15:30:00+05:30'",
                "",
            ),
        ),
        (
            "one/latest.md".into(),
            article(
                "content:latest",
                "Later instant",
                "published='2026-10-05T10:00:00.001Z'",
                "",
            ),
        ),
        (
            "one/undated.md".into(),
            article("content:undated", "AAA missing date", "", ""),
        ),
    ];
    let pages = project(&compiled, &parse(&files)).expect("known-offset comparison");
    assert_eq!(
        ids(&pages[0]),
        [
            "content:latest",
            "content:a",
            "content:z",
            "content:undated"
        ]
    );
    assert_eq!(
        json!(pages[0].items[1].published),
        "2026-10-05T15:30:00+05:30"
    );
}

#[test]
fn filtered_timestamp_does_not_poison_a_date_only_listing() {
    let compiled = configuration(
        &[listing("content:index", "standalone-articles", DATES, 20)],
        "",
    );
    let files = vec![
        ("one/index.md".into(), index("content:index", "/blog/")),
        (
            "one/date.md".into(),
            article("content:date", "Date", "published='2026-10-05'", ""),
        ),
        (
            "hidden/instant.md".into(),
            article(
                "content:instant",
                "Hidden",
                "published='2026-10-05T00:00:00Z'",
                "",
            ),
        ),
    ];
    let pages = project(&compiled, &parse(&files)).expect("filter precedes chronology");
    assert_eq!(ids(&pages[0]), ["content:date"]);
    assert_eq!(pages[0].latest.as_ref().unwrap().id, key("content:date"));
}

#[test]
fn latest_uses_only_the_newest_editorial_day_across_slices_and_orderings() {
    for (entries, expected) in [
        (
            vec![("old", "2023-12-28"), ("new", "2026-11-19T12:00:00+05:30")],
            Some("new"),
        ),
        (
            vec![("old", "2023-12-28T12:00:00Z"), ("new", "2026-11-19")],
            Some("new"),
        ),
        (
            vec![
                ("old-date", "2026-11-18"),
                ("old-time", "2026-11-18T12:00:00Z"),
                ("new", "2026-11-19T12:00:00Z"),
            ],
            Some("new"),
        ),
        (
            vec![
                ("old-date", "2026-11-18"),
                ("old-time", "2026-11-18T12:00:00Z"),
                ("new", "2026-11-19"),
            ],
            Some("new"),
        ),
        (
            vec![
                ("date", "2026-11-19"),
                ("time", "2026-11-19T23:59:59.999999999Z"),
            ],
            None,
        ),
        (
            vec![
                ("date", "2026-11-19"),
                ("time", "2026-11-20T00:30:00+14:00"),
            ],
            None,
        ),
        (
            vec![
                ("old", "2023-12-28"),
                ("z", "2026-11-19T10:00:00Z"),
                ("a", "2026-11-19T15:30:00+05:30"),
            ],
            Some("a"),
        ),
        (
            vec![
                ("old", "2023-12-28T12:00:00Z"),
                ("z", "2026-11-19"),
                ("a", "2026-11-19"),
            ],
            Some("a"),
        ),
    ] {
        let mut files = vec![("one/index.md".into(), index("content:index", "/blog/"))];
        files.extend(entries.iter().map(|(id, date)| {
            (
                format!("one/{id}.md"),
                article(
                    &format!("content:{id}"),
                    "Same title",
                    &format!("published='{date}'"),
                    "",
                ),
            )
        }));
        let corpus = parse(&files);
        for declaration in [
            alternating(1),
            listing(
                "content:index",
                "standalone-articles",
                "first=[],remaining='title-then-id'",
                1,
            ),
        ] {
            let compiled = configuration(&[declaration], "");
            let pages = project(&compiled, &corpus).expect("complete editorial-day listings");
            for page in pages {
                assert_eq!(
                    page.latest.as_ref().map(|item| item.id.to_string()),
                    expected.map(|id| format!("content:{id}")),
                    "one latest identity for every slice/order: {entries:?}"
                );
                for item in &page.items {
                    let original = entries
                        .iter()
                        .find(|(id, _)| item.id.as_str() == format!("content:{id}"))
                        .unwrap()
                        .1;
                    assert_eq!(json!(item.published), json!(original));
                }
            }
        }
    }
}

#[test]
fn ascending_preserves_equal_date_title_identity_ties_and_puts_undated_last() {
    let compiled = configuration(&[alternating(2)], "");
    let files = vec![
        ("one/index.md".into(), index("content:index", "/blog/")),
        (
            "one/z.md".into(),
            article("content:z", "Same", "published='2026-10-05'", ""),
        ),
        (
            "one/a.md".into(),
            article("content:a", "Same", "published='2026-10-05'", ""),
        ),
        (
            "one/b.md".into(),
            article("content:b", "A tie title", "published='2026-10-05'", ""),
        ),
        (
            "one/older.md".into(),
            article("content:older", "Older", "published='2026-10-01'", ""),
        ),
        (
            "one/none.md".into(),
            article("content:none", "AAA undated", "modified='1900-01-01'", ""),
        ),
    ];
    let pages = project(&compiled, &parse(&files)).unwrap();
    let mut older: Vec<_> = pages
        .iter()
        .filter(|p| p.variant == workspace::ListingVariant::Alternate)
        .collect();
    older.sort_by_key(|p| p.page_number);
    assert_eq!(
        older.iter().flat_map(|p| ids(p)).collect::<Vec<_>>(),
        [
            "content:older",
            "content:b",
            "content:a",
            "content:z",
            "content:none"
        ]
    );
}
