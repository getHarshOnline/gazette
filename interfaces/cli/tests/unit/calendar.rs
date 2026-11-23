use super::*;
#[test]
fn captures_utc_year_and_preserves_explicit_year() {
    let epoch = std::time::UNIX_EPOCH;
    assert_eq!(year_at(epoch).unwrap().get(), 1970);
    let boundary = epoch + std::time::Duration::from_secs(1_735_689_600);
    assert_eq!(
        year_at(boundary - std::time::Duration::from_secs(1))
            .unwrap()
            .get(),
        2024
    );
    assert_eq!(year_at(boundary).unwrap().get(), 2025);
    assert_eq!(
        resolve(Some(RenderYear::new(2040).unwrap())).unwrap().get(),
        2040
    );
}
