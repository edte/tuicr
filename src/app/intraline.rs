use super::*;

impl App {
    pub fn configure_word_diff(
        &mut self,
        enabled: Option<bool>,
        tokenization_regex: Option<&str>,
        max_line_distance: Option<f64>,
        max_line_distance_for_naively_paired_lines: Option<f64>,
        max_line_length: Option<usize>,
    ) {
        if let Some(enabled) = enabled {
            self.word_diff.enabled = enabled;
        }
        if let Some(pattern) = tokenization_regex
            && let Ok(regex) = regex::Regex::new(pattern)
        {
            self.word_diff.tokenization_regex = regex;
        }
        if let Some(distance) = max_line_distance {
            self.word_diff.max_line_distance = distance;
        }
        if let Some(distance) = max_line_distance_for_naively_paired_lines {
            self.word_diff.max_line_distance_for_naively_paired_lines = distance;
        }
        if let Some(length) = max_line_length {
            self.word_diff.max_line_length = length;
        }
        self.intraline_cache.get_mut().clear();
        self.rebuild_annotations();
    }

    pub(crate) fn intraline_diff(
        &self,
        file_idx: usize,
        hunk_idx: usize,
    ) -> Option<Arc<crate::intraline::HunkDiff>> {
        if !self.word_diff.enabled {
            return None;
        }
        let file = self.diff_files.get(file_idx)?;
        let hunk = file.hunks.get(hunk_idx)?;
        let key = (file.content_hash, hunk_idx);
        if let Some(diff) = self.intraline_cache.borrow().get(&key) {
            return Some(Arc::clone(diff));
        }

        let diff = Arc::new(crate::intraline::HunkDiff::infer(
            &hunk.lines,
            &self.word_diff,
        ));
        self.intraline_cache
            .borrow_mut()
            .insert(key, Arc::clone(&diff));
        Some(diff)
    }
}
