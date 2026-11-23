//! Page invitations do not follow an authored page into its redirect surface.

use super::*;

#[test]
fn selected_invitation_is_absent_from_rendered_alias_context() {
    let fixture = Fixture::new(&[
        ("theme/templates/page.html", b"{% if invitations is defined %}selected invitation{% else %}plain page{% endif %}".as_slice()),
        ("theme/templates/alias.html", b"{% if invitations is defined %}unexpected invitation{% else %}redirect only{% endif %}".as_slice()),
    ]);
    let config = fixture.config().replace(
        "[[sites]]\nid='site:one'",
        "[[sites]]\ncomposition.invitations.'theme:services'={state='enabled',destination='content:other',articles=true}\nid='site:one'",
    );
    let inputs = acquire(&config);
    let rendered = render(&inputs).unwrap();
    let tree = rendered.site().output();
    assert_eq!(bytes(tree, "guide/index.html"), b"selected invitation");
    assert_eq!(bytes(tree, "old.html"), b"redirect only");
}
