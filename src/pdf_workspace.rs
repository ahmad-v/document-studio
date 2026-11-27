use std::collections::VecDeque;

pub(crate) const MAX_DOCUMENT_BYTES: u64 = 32 * 1024 * 1024;
pub(crate) const MAX_DOCUMENT_PAGES: usize = 1000;
pub(crate) const MAX_DOCUMENT_OBJECTS: usize = 100_000;
pub(crate) const PREVIEW_MAX_EDGE: f32 = 1400.0;
pub(crate) const PREVIEW_MAX_PIXELS: u32 = 1_500_000;
pub(crate) const VISIBLE_THUMBNAILS: usize = 9;
pub(crate) const THUMBNAIL_CACHE_CAPACITY: usize = 12;
pub(crate) const THUMBNAIL_MAX_EDGE: u16 = 160;

/// Move a selected 1-based position by one place without changing its page identity.
/// The order is bounded by `MAX_DOCUMENT_PAGES`, matching the PDF loading limit.
pub(crate) fn move_page(order: &mut [usize], position: usize, delta: isize) -> Option<usize> {
    if order.is_empty()
        || order.len() > MAX_DOCUMENT_PAGES
        || position == 0
        || position > order.len()
    {
        return None;
    }
    let target = position.checked_add_signed(delta)?;
    if target == 0 || target > order.len() || delta.unsigned_abs() != 1 {
        return None;
    }
    order.swap(position - 1, target - 1);
    Some(target)
}

pub(crate) fn serialize_page_order(order: &[usize]) -> Option<String> {
    if order.is_empty() || order.len() > MAX_DOCUMENT_PAGES {
        return None;
    }
    let mut seen = vec![false; order.len()];
    for page in order {
        let Some(slot) = page.checked_sub(1).and_then(|page| seen.get_mut(page)) else {
            return None;
        };
        if *slot {
            return None;
        }
        *slot = true;
    }
    Some(
        order
            .iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join(","),
    )
}

pub(crate) fn document_within_limits(byte_len: u64, pages: usize, objects: usize) -> bool {
    byte_len <= MAX_DOCUMENT_BYTES && pages <= MAX_DOCUMENT_PAGES && objects <= MAX_DOCUMENT_OBJECTS
}

pub(crate) fn page_window(current: usize, total: usize) -> Vec<usize> {
    if current == 0 || current > total || total == 0 {
        return Vec::new();
    }
    let max_count = VISIBLE_THUMBNAILS.min(total);
    let half = (max_count - 1) / 2;
    let mut start = current.saturating_sub(half).max(1);
    let mut end = (start + max_count - 1).min(total);
    start = end.saturating_sub(max_count - 1).max(1);
    if end < start {
        end = start;
    }
    (start..=end).collect()
}

pub(crate) fn parse_page_selection(value: &str, total: usize) -> Option<usize> {
    if value.is_empty() || value.len() > 4 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let page = value.parse::<usize>().ok()?;
    (page > 0 && page <= total).then_some(page)
}

/// A small least-recently-used cache keyed by 1-based page number.
#[derive(Debug)]
pub(crate) struct BoundedPageCache<T> {
    entries: VecDeque<(usize, T)>,
}

impl<T> Default for BoundedPageCache<T> {
    fn default() -> Self {
        Self {
            entries: VecDeque::with_capacity(THUMBNAIL_CACHE_CAPACITY),
        }
    }
}

impl<T> BoundedPageCache<T> {
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(crate) fn peek(&self, page: usize) -> Option<&T> {
        self.entries
            .iter()
            .find_map(|(cached_page, value)| (*cached_page == page).then_some(value))
    }

    pub(crate) fn touch(&mut self, page: usize) -> bool {
        let Some(index) = self
            .entries
            .iter()
            .position(|(cached_page, _)| *cached_page == page)
        else {
            return false;
        };
        if index + 1 != self.entries.len() {
            if let Some(entry) = self.entries.remove(index) {
                self.entries.push_back(entry);
            }
        }
        true
    }

    pub(crate) fn insert(&mut self, page: usize, value: T) {
        self.entries.retain(|(cached_page, _)| *cached_page != page);
        self.entries.push_back((page, value));
        while self.entries.len() > THUMBNAIL_CACHE_CAPACITY {
            self.entries.pop_front();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_window_is_small_and_clamped_to_document_edges() {
        assert!(page_window(0, 5).is_empty());
        assert!(page_window(6, 5).is_empty());
        assert_eq!(page_window(1, 20), (1..=9).collect::<Vec<_>>());
        assert_eq!(page_window(10, 20), (6..=14).collect::<Vec<_>>());
        assert_eq!(page_window(20, 20), (12..=20).collect::<Vec<_>>());
        assert!(page_window(500, 1000).len() <= VISIBLE_THUMBNAILS);
    }

    #[test]
    fn page_selection_accepts_only_existing_positive_pages() {
        assert_eq!(parse_page_selection("1", 10), Some(1));
        assert_eq!(parse_page_selection("10", 10), Some(10));
        assert_eq!(parse_page_selection("0", 10), None);
        assert_eq!(parse_page_selection("11", 10), None);
        assert_eq!(parse_page_selection("-1", 10), None);
        assert_eq!(parse_page_selection("99999", 1000), None);
        assert_eq!(parse_page_selection("1x", 10), None);
    }

    #[test]
    fn thumbnail_cache_is_bounded_and_evicts_the_least_recently_used() {
        let mut cache = BoundedPageCache::default();
        for page in 1..=THUMBNAIL_CACHE_CAPACITY {
            cache.insert(page, page * 10);
        }
        assert_eq!(cache.len(), THUMBNAIL_CACHE_CAPACITY);
        assert_eq!(cache.peek(1), Some(&10));
        assert!(cache.touch(1));
        cache.insert(THUMBNAIL_CACHE_CAPACITY + 1, 999);
        assert_eq!(cache.len(), THUMBNAIL_CACHE_CAPACITY);
        assert!(cache.peek(1).is_some());
        assert!(cache.peek(2).is_none());
        assert_eq!(cache.peek(13), Some(&999));
    }

    #[test]
    fn document_size_page_and_object_limits_are_enforced() {
        assert!(document_within_limits(
            MAX_DOCUMENT_BYTES,
            MAX_DOCUMENT_PAGES,
            MAX_DOCUMENT_OBJECTS
        ));
        assert!(!document_within_limits(MAX_DOCUMENT_BYTES + 1, 1, 1));
        assert!(!document_within_limits(1, MAX_DOCUMENT_PAGES + 1, 1));
        assert!(!document_within_limits(1, 1, MAX_DOCUMENT_OBJECTS + 1));
    }

    #[test]
    fn page_reordering_moves_one_selected_page_and_serializes_a_permutation() {
        let mut order = vec![1, 2, 3, 4];
        assert_eq!(move_page(&mut order, 3, -1), Some(2));
        assert_eq!(order, [1, 3, 2, 4]);
        assert_eq!(move_page(&mut order, 2, 1), Some(3));
        assert_eq!(order, [1, 2, 3, 4]);
        assert_eq!(serialize_page_order(&order).as_deref(), Some("1,2,3,4"));
        assert_eq!(serialize_page_order(&[3, 1, 2]).as_deref(), Some("3,1,2"));
        assert_eq!(serialize_page_order(&[1, 1]), None);
        assert_eq!(serialize_page_order(&[1, 3]), None);
    }

    #[test]
    fn page_reordering_rejects_invalid_positions_and_document_sizes() {
        let mut order = vec![1, 2];
        assert_eq!(move_page(&mut order, 1, -1), None);
        assert_eq!(move_page(&mut order, 2, 1), None);
        assert_eq!(move_page(&mut order, 0, 1), None);
        assert_eq!(move_page(&mut order, 1, 2), None);
        let mut oversized = vec![1; MAX_DOCUMENT_PAGES + 1];
        assert_eq!(move_page(&mut oversized, 1, 1), None);
    }
}
