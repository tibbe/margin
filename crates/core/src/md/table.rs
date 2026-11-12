//! How a table shows as a grid: the widths of its columns, by the rule
//! every editor follows (the spec's Tables, in rendering).

/// What a column's text needs, as an editor measured it: the width of its
/// longest word, and of its widest cell on one line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColumnText {
    pub longest_word: f64,
    pub one_line: f64,
}

/// The width of each column's text, for a grid whose columns' text has
/// `available` width in all, padding left out.
pub fn column_widths(columns: &[ColumnText], available: f64) -> Vec<f64> {
    let one_line: f64 = columns.iter().map(|c| c.one_line).sum();
    if one_line <= available {
        return columns.iter().map(|c| c.one_line).collect();
    }
    let words: f64 = columns.iter().map(|c| c.longest_word).sum();
    if words <= available {
        // `one_line > available >= words`, so some column needs more.
        let more = one_line - words;
        return columns
            .iter()
            .map(|c| c.longest_word + (available - words) * (c.one_line - c.longest_word) / more)
            .collect();
    }
    columns
        .iter()
        .map(|c| available * c.one_line / one_line)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(longest_word: f64, one_line: f64) -> ColumnText {
        ColumnText {
            longest_word,
            one_line,
        }
    }

    #[test]
    fn columns_that_fit_take_their_widest_cell() {
        assert_eq!(
            column_widths(&[col(10.0, 40.0), col(20.0, 60.0)], 200.0),
            [40.0, 60.0]
        );
    }

    /// Each column gets its longest word, and the rest is shared by what
    /// each needs beyond that: 40 and 120 more, of 80 left.
    #[test]
    fn columns_that_dont_fit_share_the_width_by_what_they_need() {
        assert_eq!(
            column_widths(&[col(10.0, 50.0), col(30.0, 150.0)], 120.0),
            [30.0, 90.0]
        );
    }

    /// Words that don't fit together: widths by the one-line width.
    #[test]
    fn longest_words_that_dont_fit_share_by_the_one_line_width() {
        assert_eq!(
            column_widths(&[col(80.0, 100.0), col(60.0, 300.0)], 100.0),
            [25.0, 75.0]
        );
    }

    #[test]
    fn no_columns_or_empty_ones_take_no_width() {
        assert_eq!(
            column_widths(&[col(0.0, 0.0), col(0.0, 0.0)], 100.0),
            [0.0, 0.0]
        );
        assert_eq!(column_widths(&[], 100.0), Vec::<f64>::new());
    }
}
