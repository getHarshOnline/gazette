//! Page and service identities stay distinct; provider facts come from the catalog.

use super::{Described, PrincipalNode, identity};
use crate::authored::SubjectView;
use serde::{Serialize, Serializer, ser::SerializeMap};

pub(super) struct MainEntity<'a, 'b>(
    pub(super) &'a SubjectView<'b>,
    pub(super) Option<&'a Described>,
);

impl Serialize for MainEntity<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            SubjectView::Principal { principal, website } => {
                PrincipalNode(principal, website.as_ref())
                    .within(self.1)
                    .serialize(serializer)
            }
            SubjectView::Service {
                service,
                provider,
                website,
            } => {
                let mut map = serializer.serialize_map(None)?;
                identity(&mut map, "service", &service.id)?;
                map.serialize_entry("@type", "Service")?;
                map.serialize_entry("name", &service.name)?;
                if let Some(description) = &service.description {
                    map.serialize_entry("description", description)?;
                }
                map.serialize_entry(
                    "provider",
                    &PrincipalNode(provider, website.as_ref()).within(self.1),
                )?;
                map.end()
            }
        }
    }
}
