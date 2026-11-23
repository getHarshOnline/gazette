//! Account selections use their authored locations, including inherited defaults.

use serde_json::Value;

use super::{DeclarationKind, Error, Key, PathSegment, References, invalid, key};

impl References<'_, '_> {
    pub(super) fn accounts(&mut self) -> Result<(), Error> {
        let source: Value = self.syntax.deserialize().map_err(Error::Toml)?;
        for (index, account) in array(&source, "accounts").iter().enumerate() {
            if let Some(owner) = account.get("owner") {
                let owner: Key = serde_json::from_value(owner.clone()).map_err(|_| invalid())?;
                self.add(
                    DeclarationKind::Principal,
                    &owner,
                    &[key("accounts"), PathSegment::Index(index), key("owner")],
                )?;
            }
        }
        self.account_list(
            source.get("defaults").and_then(|v| v.get("publication")),
            vec![key("defaults"), key("publication")],
        )?;
        for (index, publisher) in array(&source, "publishers").iter().enumerate() {
            self.account_list(
                publisher.get("publication"),
                vec![
                    key("publishers"),
                    PathSegment::Index(index),
                    key("publication"),
                ],
            )?;
        }
        for (index, site) in array(&source, "sites").iter().enumerate() {
            self.account_list(Some(site), vec![key("sites"), PathSegment::Index(index)])?;
        }
        for (brand_index, brand) in array(&source, "brands").iter().enumerate() {
            for (index, publication) in array(brand, "publications").iter().enumerate() {
                self.account_list(
                    Some(publication),
                    vec![
                        key("brands"),
                        PathSegment::Index(brand_index),
                        key("publications"),
                        PathSegment::Index(index),
                    ],
                )?;
            }
        }
        Ok(())
    }

    fn account_list(
        &mut self,
        publication: Option<&Value>,
        mut path: Vec<PathSegment>,
    ) -> Result<(), Error> {
        let Some(accounts) = publication
            .and_then(|v| v.get("accounts"))
            .and_then(Value::as_array)
        else {
            return Ok(());
        };
        path.push(key("accounts"));
        for (index, account) in accounts.iter().enumerate() {
            let id: Key = serde_json::from_value(account.clone()).map_err(|_| invalid())?;
            path.push(PathSegment::Index(index));
            self.add(DeclarationKind::Account, &id, &path)?;
            path.pop();
        }
        Ok(())
    }
}

fn array<'a>(value: &'a Value, field: &str) -> &'a [Value] {
    value
        .get(field)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}
