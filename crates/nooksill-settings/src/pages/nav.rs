//! Two-level navigation: primary entries and the content tabs they expand to.
//!
//! A group temporarily turns the navigation column into its own tab strip; the
//! page enum still names the content shown on the right, so a page keeps one
//! renderer no matter how it was reached.

use super::Page;

/// A primary entry that owns a set of content pages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavGroup {
    Dock,
    Content,
    Update,
}

impl NavGroup {
    pub const ALL: [NavGroup; 3] = [NavGroup::Dock, NavGroup::Content, NavGroup::Update];

    /// Primary navigation label; group labels are not page titles.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            NavGroup::Dock => nooksill_core::i18n::t("Dock bar"),
            NavGroup::Content => nooksill_core::i18n::t("Content & library"),
            NavGroup::Update => nooksill_core::i18n::t("Updates & network"),
        }
    }

    /// Tabs shown while the group is open, in navigation order.
    #[must_use]
    pub fn tabs(self) -> &'static [Page] {
        match self {
            NavGroup::Dock => &[
                Page::Appearance,
                Page::Position,
                Page::Icons,
                Page::Interaction,
                Page::Extras,
                Page::Effect,
            ],
            NavGroup::Content => &[Page::Items, Page::Library, Page::Add],
            NavGroup::Update => &[Page::UpdatePolicy, Page::UpdateNetwork],
        }
    }

    /// Groups whose tabs render the live Dock preview above the cards.
    #[must_use]
    pub fn shows_preview(self) -> bool {
        matches!(self, NavGroup::Dock)
    }

    /// The group owning `page`, if the page is never a primary entry.
    #[must_use]
    pub fn containing(page: Page) -> Option<Self> {
        Self::ALL.into_iter().find(|g| g.tabs().contains(&page))
    }
}

/// One row of the primary navigation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavItem {
    Page(Page),
    Group(NavGroup),
}

impl NavItem {
    /// Order of the primary column. Group entries replace the pages they own.
    pub const PRIMARY: [NavItem; 6] = [
        NavItem::Page(Page::General),
        NavItem::Group(NavGroup::Dock),
        NavItem::Page(Page::Performance),
        NavItem::Group(NavGroup::Content),
        NavItem::Group(NavGroup::Update),
        NavItem::Page(Page::About),
    ];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            NavItem::Page(p) => p.title(),
            NavItem::Group(g) => g.label(),
        }
    }

    /// A group matches a search when any of its tabs does, so searching a
    /// buried control still surfaces the group that owns it.
    #[must_use]
    pub fn matches(self, filter: &str) -> bool {
        if filter.is_empty() {
            return true;
        }
        let own = self.label().to_lowercase().contains(filter);
        match self {
            NavItem::Page(_) => own,
            NavItem::Group(g) => {
                own || g
                    .tabs()
                    .iter()
                    .any(|p| p.title().to_lowercase().contains(filter))
            }
        }
    }

    /// Page shown first when this entry is opened.
    #[must_use]
    pub fn entry_page(self) -> Page {
        match self {
            NavItem::Page(p) => p,
            NavItem::Group(g) => g.tabs()[0],
        }
    }
}
