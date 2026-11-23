//! Bind generated continuation endpoints to the same canonical discovery selection.

use std::collections::{BTreeMap, BTreeSet};

use super::{SitePages, SitePagesError};
use crate::{authored::ListingPage, workspace::ListingOrder};

impl SitePages<'_> {
    /// Join complete compiler-generated reading pages without inventing content identities.
    /// Call after discovery and taxonomy selection; undiscoverable indexes remain absent.
    ///
    /// # Errors
    /// Refuses incomplete/foreign owners, changed pagination, URLs or output routes.
    pub fn with_listing_pages(&self, pages: &[ListingPage]) -> Result<Self, SitePagesError> {
        let mut groups: BTreeMap<_, Vec<_>> = BTreeMap::new();
        for page in pages {
            groups
                .entry((&page.content, page.variant))
                .or_default()
                .push(page);
        }
        if groups.len()
            != self
                .publication
                .listings
                .iter()
                .map(|declaration| declaration.orders().count())
                .sum::<usize>()
        {
            return Err(SitePagesError::Listing);
        }
        let base = self
            .owner
            .base_url(&self.publication.site)
            .ok_or(SitePagesError::BrandRequired)?;
        let mut urls: BTreeSet<_> = self
            .pages
            .iter()
            .map(|page| page.route.canonical_url.clone())
            .chain(self.taxonomies.iter().map(|page| page.url.clone()))
            .collect();
        let mut continuation_urls = Vec::new();
        for declaration in &self.publication.listings {
            let owner = self
                .owner
                .graph()
                .node(&declaration.content)
                .ok_or(SitePagesError::Listing)?;
            let route = owner.route.as_ref().ok_or(SitePagesError::Listing)?;
            declaration
                .validate_alternate()
                .map_err(|_| SitePagesError::Listing)?;
            let mut shared_total = None;
            for (variant, order, relative) in declaration.orders() {
                let root = route
                    .path
                    .join_directory(relative)
                    .map_err(|_| SitePagesError::Listing)?;
                let mut views = groups
                    .remove(&(&declaration.content, variant))
                    .ok_or(SitePagesError::Listing)?;
                views.sort_unstable_by_key(|page| page.page_number);
                let total = views.first().ok_or(SitePagesError::Listing)?.total_items;
                if shared_total
                    .replace(total)
                    .is_some_and(|previous| previous != total)
                {
                    return Err(SitePagesError::Listing);
                }
                let size = declaration.pagination.page_size;
                if size == 0 || views.len() != total.div_ceil(size).max(1) {
                    return Err(SitePagesError::Listing);
                }
                for (index, page) in views.iter().enumerate() {
                    if page.page_size != size {
                        return Err(SitePagesError::Listing);
                    }
                    let expected = if index == 0 {
                        root.clone()
                    } else {
                        let entry = declaration
                            .pagination
                            .continuation(index + 1)
                            .map_err(|_| SitePagesError::Listing)?;
                        root.join_directory(Some(&entry))
                            .map_err(|_| SitePagesError::Listing)?
                    };
                    match (&declaration.select, &page.subject) {
                        (
                            crate::workspace::ListingSelection::ProjectArticles {
                                project,
                                explore,
                                residence,
                                ..
                            },
                            Some(subject),
                        ) => {
                            let landing = declaration
                                .select
                                .project_landing(self.owner)
                                .map_err(|_| SitePagesError::Listing)?
                                .ok_or(SitePagesError::Listing)?;
                            let selected =
                                self.owner.project(project).ok_or(SitePagesError::Listing)?;
                            let node = self
                                .owner
                                .graph()
                                .node(landing)
                                .ok_or(SitePagesError::Listing)?;
                            let route = self.owner.route(landing).ok_or(SitePagesError::Listing)?;
                            if subject.residence != *residence
                                || &subject.project != project
                                || subject.name != selected.name
                                || &subject.landing.id != landing
                                || subject.landing.title != node.title
                                || subject.landing.url != route.url
                                || subject.landing.destination.content.as_ref() != Some(landing)
                                || subject.landing.destination.site.as_ref() != Some(&node.site)
                            {
                                return Err(SitePagesError::Listing);
                            }
                            match (explore, &subject.explore) {
                                (None, None) => {}
                                (Some(id), Some(target)) => {
                                    let node = self
                                        .owner
                                        .graph()
                                        .node(id)
                                        .ok_or(SitePagesError::Listing)?;
                                    let route =
                                        self.owner.route(id).ok_or(SitePagesError::Listing)?;
                                    if &target.id != id
                                        || target.title != node.title
                                        || target.url != route.url
                                        || target.destination.content.as_ref() != Some(id)
                                        || target.destination.site.as_ref() != Some(&node.site)
                                    {
                                        return Err(SitePagesError::Listing);
                                    }
                                }
                                _ => return Err(SitePagesError::Listing),
                            }
                        }
                        (crate::workspace::ListingSelection::ProjectArticles { .. }, None) => {
                            return Err(SitePagesError::Listing);
                        }
                        (_, Some(_)) => return Err(SitePagesError::Listing),
                        (_, None) => {}
                    }
                    let start = index.checked_mul(size).ok_or(SitePagesError::Listing)?;
                    if page.variant != variant
                        || page.direction
                            != match order {
                                ListingOrder::Published { direction, .. } => Some(*direction),
                                ListingOrder::Priority { .. } => None,
                            }
                        || page.page_number != index + 1
                        || page.page_count != views.len()
                        || page.total_items != total
                        || page.items.len() != total.saturating_sub(start).min(size)
                        || page.title != owner.title
                        || page.route != expected
                        || page.output != *expected.output()
                        || page.url != base.resolve(&expected)
                        || page.previous.as_deref()
                            != index
                                .checked_sub(1)
                                .map(|previous| views[previous].url.as_str())
                        || page.next.as_deref()
                            != views.get(index + 1).map(|next| next.url.as_str())
                    {
                        return Err(SitePagesError::Listing);
                    }
                    let ordinals = crate::authored::listings::page_numbers(index + 1, views.len());
                    if page.pages.len() != ordinals.len() {
                        return Err(SitePagesError::Listing);
                    }
                    let mut previous = 0;
                    for (link, number) in page.pages.iter().zip(ordinals) {
                        if link.number != number
                            || link.url != views[number - 1].url
                            || link.current != (number == index + 1)
                            || link.gap_before != (previous > 0 && number > previous + 1)
                        {
                            return Err(SitePagesError::Listing);
                        }
                        previous = number;
                    }
                    let orders: Vec<_> = declaration.orders().collect();
                    let sort_count = if orders.len() > 1 { orders.len() } else { 0 };
                    if page.sorts.len() != sort_count {
                        return Err(SitePagesError::Listing);
                    }
                    for (link, (sort_variant, sort_order, relative)) in
                        page.sorts.iter().zip(orders)
                    {
                        let ListingOrder::Published { direction, .. } = sort_order else {
                            return Err(SitePagesError::Listing);
                        };
                        let target = route
                            .path
                            .join_directory(relative)
                            .map_err(|_| SitePagesError::Listing)?;
                        if link.direction != *direction
                            || link.current != (sort_variant == variant)
                            || link.url != base.resolve(&target)
                        {
                            return Err(SitePagesError::Listing);
                        }
                    }
                    if !page.is_primary()
                        && self
                            .pages
                            .iter()
                            .any(|selected| selected.node.id == owner.id)
                    {
                        if !urls.insert(page.url.clone()) {
                            return Err(SitePagesError::Collision);
                        }
                        continuation_urls.push(page.url.clone());
                    }
                }
            }
        }
        continuation_urls.sort_unstable();
        Ok(Self {
            owner: self.owner,
            name: self.name,
            publication: self.publication,
            pages: self.pages.clone(),
            mirrors: self.mirrors.clone(),
            taxonomies: self
                .taxonomies
                .iter()
                .map(|page| super::TaxonomyDiscovery {
                    url: page.url.clone(),
                })
                .collect(),
            listing_pages: continuation_urls,
        })
    }
}
