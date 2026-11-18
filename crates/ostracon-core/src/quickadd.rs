//! Todoist-style quick-add: `#Project`, natural-language dates and `!reminders` in a task's name.
//!
//! The composer's text is a [`Draft`]: plain text interleaved with blocks for the recognised project, due date
//! and reminders. A recognised phrase becomes a block once the whitespace after it is typed. Backspacing over a
//! block turns it back into its words as *literal* text, which is never recognised again, so a task can still be
//! called "Plan Monday's standup".

use std::ops::Range;

use crate::dates::{DateFragment, DateParser, PhraseValue, ReminderFragment};
use crate::model::{Due, Project, ProjectId, Reminder};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    /// Typed text. Literal text is never recognised.
    Text {
        text: String,
        literal: bool,
    },
    Block(Block),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    /// The words as typed, which a reverted block turns back into.
    pub text: String,
    pub value: BlockValue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockValue {
    Project(ProjectId),
    Due(DateFragment),
    Reminder(ReminderFragment),
}

/// What a draft describes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuickAddResult {
    /// The name with recognised blocks taken out and whitespace tidied.
    pub name: String,
    pub project_id: Option<ProjectId>,
    pub due: Option<Due>,
    pub reminders: Vec<Reminder>,
    /// The words that set the due date, as typed.
    pub due_text: Option<String>,
}

/// Reads a whole piece of text as if it had been typed into the composer and submitted.
pub fn parse(text: &str, projects: &[Project], dates: &DateParser) -> QuickAddResult {
    let mut draft = Draft::from_text(text, false);
    draft.convert(projects, Some(dates), true);
    draft.result(dates)
}

/// Projects matching a partly typed name, those starting with it first.
pub fn suggestions<'a>(query: &str, projects: &'a [Project]) -> Vec<&'a Project> {
    let query = query.to_lowercase();
    let (mut prefixed, mut rest) = (Vec::new(), Vec::new());
    for project in projects {
        let name = project.name.to_lowercase();
        if name.starts_with(&query) {
            prefixed.push(project);
        } else if name.contains(&query) {
            rest.push(project);
        }
    }
    prefixed.append(&mut rest);
    prefixed
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Draft {
    segments: Vec<Segment>,
}

impl Draft {
    pub fn new() -> Self {
        Draft::default()
    }

    /// A draft holding `text`. An existing task's name is loaded as literal text, so it is never re-read.
    pub fn from_text(text: &str, literal: bool) -> Self {
        Draft::from_segments(vec![Segment::Text {
            text: text.to_owned(),
            literal,
        }])
    }

    pub fn from_segments(segments: Vec<Segment>) -> Self {
        Draft {
            segments: normalised(segments),
        }
    }

    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    /// The whole name with blocks shown as their words.
    pub fn plain_text(&self) -> String {
        self.segments.iter().map(segment_text).collect()
    }

    pub fn len(&self) -> usize {
        self.segments.iter().map(|s| segment_text(s).len()).sum()
    }

    /// Each block with its byte range in [`Draft::plain_text`].
    pub fn blocks(&self) -> Vec<(Range<usize>, &Block)> {
        let mut offset = 0;
        let mut blocks = Vec::new();
        for segment in &self.segments {
            let len = segment_text(segment).len();
            if let Segment::Block(block) = segment {
                blocks.push((offset..offset + len, block));
            }
            offset += len;
        }
        blocks
    }

    // Recognition

    /// Turns recognised words in non-literal text into blocks.
    ///
    /// While typing, words convert only once whitespace follows them, so "fri" isn't taken before "friday" is
    /// finished; `including_trailing` converts everything, for submitting. A date or reminder block absorbs
    /// finished words typed straight after it that extend it ("tomorrow" then "at 5pm"). Only the latest project
    /// and date stay as blocks (plus a lone time or day that completes the date); older ones revert to literal
    /// text. Pass `dates: None` to recognise projects only.
    pub fn convert(
        &mut self,
        projects: &[Project],
        dates: Option<&DateParser>,
        including_trailing: bool,
    ) {
        if let Some(dates) = dates {
            self.extend_blocks(dates, including_trailing);
        }
        let segments = std::mem::take(&mut self.segments);
        self.segments = segments
            .into_iter()
            .flat_map(|segment| match segment {
                Segment::Text {
                    text,
                    literal: false,
                } => convert_text(&text, projects, dates, including_trailing),
                other => vec![other],
            })
            .collect();
        self.keep_latest_blocks();
        self.segments = normalised(std::mem::take(&mut self.segments));
    }

    /// Turns the block at segment `index` back into its words, as literal text.
    pub fn revert_block(&mut self, index: usize) {
        if let Some(Segment::Block(block)) = self.segments.get(index) {
            self.segments[index] = Segment::Text {
                text: block.text.clone(),
                literal: true,
            };
            self.segments = normalised(std::mem::take(&mut self.segments));
        }
    }

    /// Turns matching blocks back into literal text.
    pub fn revert_blocks(&mut self, predicate: impl Fn(&BlockValue) -> bool) {
        for segment in &mut self.segments {
            if let Segment::Block(block) = segment
                && predicate(&block.value)
            {
                *segment = Segment::Text {
                    text: block.text.clone(),
                    literal: true,
                };
            }
        }
        self.segments = normalised(std::mem::take(&mut self.segments));
    }

    /// Deletes matching blocks and their words.
    pub fn remove_blocks(&mut self, predicate: impl Fn(&BlockValue) -> bool) {
        self.segments
            .retain(|segment| !matches!(segment, Segment::Block(block) if predicate(&block.value)));
        self.segments = normalised(std::mem::take(&mut self.segments));
    }

    /// The task the blocks describe.
    pub fn result(&self, dates: &DateParser) -> QuickAddResult {
        let mut name = String::new();
        let mut project_id = None;
        let mut due: Option<DateFragment> = None;
        let mut due_texts = Vec::new();
        let mut reminders = Vec::new();
        for segment in &self.segments {
            match segment {
                Segment::Text { text, .. } => name.push_str(text),
                Segment::Block(block) => {
                    name.push(' ');
                    match block.value {
                        BlockValue::Project(id) => project_id = Some(id),
                        BlockValue::Due(fragment) => {
                            due = Some(match due {
                                Some(existing) if existing.complements(&fragment) => {
                                    existing.combined(&fragment)
                                }
                                _ => fragment,
                            });
                            due_texts.push(block.text.as_str());
                        }
                        BlockValue::Reminder(fragment) => reminders.push(fragment),
                    }
                }
            }
        }
        let due = due.map(|fragment| dates.resolve(fragment));
        let mut reminders: Vec<Reminder> = reminders
            .into_iter()
            .map(|fragment| dates.resolve_reminder(fragment, due))
            .collect();
        Reminder::sort(&mut reminders);
        QuickAddResult {
            name: tidy(&name),
            project_id,
            due,
            reminders,
            due_text: (!due_texts.is_empty()).then(|| due_texts.join(" … ")),
        }
    }

    // Project autocomplete

    /// The `#query` being typed just before `cursor`: its byte range (from the `#`) and the partial name.
    pub fn project_query(&self, cursor: usize) -> Option<(Range<usize>, String)> {
        let mut offset = 0;
        for segment in &self.segments {
            let len = segment_text(segment).len();
            if let Segment::Text {
                text,
                literal: false,
            } = segment
                && offset < cursor
                && cursor <= offset + len
            {
                let before = &text[..cursor - offset];
                let hash = hash_starts(before).last().copied()?;
                let query = &before[hash + 1..];
                if query.contains('\n') {
                    return None;
                }
                return Some((offset + hash..cursor, query.to_owned()));
            }
            offset += len;
        }
        None
    }

    /// Replaces the `#query` before `cursor` with a block for `project` and a space. Returns the new cursor.
    pub fn complete_project(&mut self, cursor: usize, project: &Project) -> usize {
        let Some((range, _)) = self.project_query(cursor) else {
            return cursor;
        };
        let block = Block {
            text: format!("#{}", project.name),
            value: BlockValue::Project(project.id),
        };
        let block_len = block.text.len();
        self.replace_with_segments(
            range.clone(),
            vec![
                Segment::Block(block),
                Segment::Text {
                    text: " ".to_owned(),
                    literal: false,
                },
            ],
        );
        self.keep_latest_blocks();
        self.segments = normalised(std::mem::take(&mut self.segments));
        range.start + block_len + 1
    }

    // Editing. Positions are byte offsets in `plain_text()`, on char boundaries.

    /// Replaces `range` with `text`, as typing or pasting does. A block the edit cuts into becomes literal text
    /// first. Text typed strictly inside literal text stays literal; anywhere else it can be recognised.
    pub fn edit(&mut self, range: Range<usize>, text: &str) {
        self.revert_blocks_touched_by(&range);
        let literal = self.is_inside_literal(&range);
        self.replace_with_segments(
            range,
            vec![Segment::Text {
                text: text.to_owned(),
                literal,
            }],
        );
        self.segments = normalised(std::mem::take(&mut self.segments));
    }

    /// Backspace with no selection: a block ending at `cursor` turns back into literal text and nothing is
    /// deleted; otherwise the character before `cursor` goes. Returns the new cursor.
    pub fn backspace(&mut self, cursor: usize) -> usize {
        if let Some(index) = self.block_index_where(|range| range.end == cursor) {
            self.revert_block(index);
            return cursor;
        }
        let text = self.plain_text();
        let Some((start, _)) = text[..cursor].char_indices().next_back() else {
            return cursor;
        };
        self.edit(start..cursor, "");
        start
    }

    /// Forward delete with no selection: a block starting at `cursor` turns back into literal text; otherwise
    /// the character after `cursor` goes. Returns the new cursor.
    pub fn delete_forward(&mut self, cursor: usize) -> usize {
        if let Some(index) = self.block_index_where(|range| range.start == cursor) {
            self.revert_block(index);
            return cursor;
        }
        let text = self.plain_text();
        let Some(c) = text[cursor..].chars().next() else {
            return cursor;
        };
        self.edit(cursor..cursor + c.len_utf8(), "");
        cursor
    }

    fn block_index_where(&self, predicate: impl Fn(&Range<usize>) -> bool) -> Option<usize> {
        let mut offset = 0;
        for (index, segment) in self.segments.iter().enumerate() {
            let len = segment_text(segment).len();
            if matches!(segment, Segment::Block(_)) && predicate(&(offset..offset + len)) {
                return Some(index);
            }
            offset += len;
        }
        None
    }

    fn revert_blocks_touched_by(&mut self, range: &Range<usize>) {
        let mut offset = 0;
        for segment in &mut self.segments {
            let len = segment_text(segment).len();
            let (start, end) = (offset, offset + len);
            offset = end;
            let Segment::Block(block) = segment else {
                continue;
            };
            let cuts = if range.is_empty() {
                start < range.start && range.start < end
            } else {
                range.start < end && start < range.end
            };
            if cuts {
                *segment = Segment::Text {
                    text: block.text.clone(),
                    literal: true,
                };
            }
        }
    }

    fn is_inside_literal(&self, range: &Range<usize>) -> bool {
        let mut offset = 0;
        for segment in &self.segments {
            let len = segment_text(segment).len();
            let (start, end) = (offset, offset + len);
            offset = end;
            if let Segment::Text { literal: true, .. } = segment {
                let interior = start < range.start && range.start < end;
                let replaces_its_start =
                    !range.is_empty() && start == range.start && range.end < end;
                if interior || replaces_its_start {
                    return true;
                }
            }
        }
        false
    }

    /// Cuts `range` out of the text segments and puts `insert` in its place. Blocks must lie wholly outside
    /// `range` or wholly inside it (those inside are dropped).
    fn replace_with_segments(&mut self, range: Range<usize>, insert: Vec<Segment>) {
        let mut before = Vec::new();
        let mut after = Vec::new();
        let mut offset = 0;
        for segment in std::mem::take(&mut self.segments) {
            let len = segment_text(&segment).len();
            let (start, end) = (offset, offset + len);
            offset = end;
            if end <= range.start && !(start == end && start == range.start) {
                before.push(segment);
            } else if start >= range.end {
                after.push(segment);
            } else if let Segment::Text { text, literal } = segment {
                let head = &text[..range.start.saturating_sub(start).min(len)];
                let tail = &text[range.end.saturating_sub(start).min(len)..];
                before.push(Segment::Text {
                    text: head.to_owned(),
                    literal,
                });
                after.push(Segment::Text {
                    text: tail.to_owned(),
                    literal,
                });
            }
        }
        before.extend(insert);
        before.extend(after);
        self.segments = before;
    }

    /// Lets a date or reminder block grow into finished words typed after it, e.g. "at 5" then "pm".
    fn extend_blocks(&mut self, dates: &DateParser, including_trailing: bool) {
        let mut index = 0;
        while index + 1 < self.segments.len() {
            let (
                Segment::Block(block),
                Segment::Text {
                    text,
                    literal: false,
                },
            ) = (&self.segments[index], &self.segments[index + 1])
            else {
                index += 1;
                continue;
            };
            let ends = finished_word_ends(text, including_trailing);
            for &end in ends.iter().take(3).rev() {
                let extended = format!("{}{}", block.text, &text[..end]);
                let value = match block.value {
                    BlockValue::Due(_) => dates.date_fragment(&extended).map(BlockValue::Due),
                    BlockValue::Reminder(_) => {
                        dates.reminder_fragment(&extended).map(BlockValue::Reminder)
                    }
                    BlockValue::Project(_) => None,
                };
                if let Some(value) = value {
                    let rest = text[end..].to_owned();
                    self.segments[index] = Segment::Block(Block {
                        text: extended,
                        value,
                    });
                    self.segments[index + 1] = Segment::Text {
                        text: rest,
                        literal: false,
                    };
                    break;
                }
            }
            index += 1;
        }
    }

    /// One project block and one date block (or a day and a time that complete each other); the rest revert.
    fn keep_latest_blocks(&mut self) {
        let mut latest_due: Option<DateFragment> = None;
        let mut kept_day_and_time = false;
        let mut has_project = false;
        for segment in self.segments.iter_mut().rev() {
            let Segment::Block(block) = segment else {
                continue;
            };
            let keep = match block.value {
                BlockValue::Reminder(_) => true,
                BlockValue::Project(_) => !std::mem::replace(&mut has_project, true),
                BlockValue::Due(fragment) => match latest_due {
                    None => {
                        latest_due = Some(fragment);
                        true
                    }
                    Some(latest) if !kept_day_and_time && latest.complements(&fragment) => {
                        kept_day_and_time = true;
                        true
                    }
                    Some(_) => false,
                },
            };
            if !keep {
                *segment = Segment::Text {
                    text: block.text.clone(),
                    literal: true,
                };
            }
        }
    }
}

fn segment_text(segment: &Segment) -> &str {
    match segment {
        Segment::Text { text, .. } => text,
        Segment::Block(block) => &block.text,
    }
}

/// The end of each word in `text` that has been finished: followed by whitespace, or the last one when
/// `including_trailing` is set.
fn finished_word_ends(text: &str, including_trailing: bool) -> Vec<usize> {
    let mut ends = Vec::new();
    let mut in_word = false;
    for (index, c) in text.char_indices() {
        if c.is_whitespace() {
            if in_word {
                ends.push(index);
            }
            in_word = false;
        } else {
            in_word = true;
        }
    }
    if in_word && including_trailing {
        ends.push(text.len());
    }
    ends
}

/// Byte offsets of `#` characters that begin a word.
fn hash_starts(text: &str) -> Vec<usize> {
    let mut starts = Vec::new();
    let mut previous_is_space = true;
    for (index, c) in text.char_indices() {
        if c == '#' && previous_is_space {
            starts.push(index);
        }
        previous_is_space = c.is_whitespace();
    }
    starts
}

/// `#Project` words naming an existing project, preferring the longest name, so "#Work Admin" beats "#Work".
fn project_matches<'a>(text: &str, projects: &'a [Project]) -> Vec<(Range<usize>, &'a Project)> {
    let mut candidates: Vec<&Project> = projects.iter().collect();
    candidates.sort_by_key(|project| std::cmp::Reverse(project.name.chars().count()));
    let mut matches = Vec::new();
    for hash in hash_starts(text) {
        let name_start = hash + 1;
        for project in &candidates {
            let count = project.name.chars().count();
            let Some(name_end) = text[name_start..]
                .char_indices()
                .map(|(i, _)| name_start + i)
                .chain(std::iter::once(text.len()))
                .nth(count)
            else {
                continue;
            };
            let whole_word = text[name_end..]
                .chars()
                .next()
                .is_none_or(char::is_whitespace);
            if whole_word
                && text[name_start..name_end].to_lowercase() == project.name.to_lowercase()
            {
                matches.push((hash..name_end, *project));
                break;
            }
        }
    }
    matches
}

/// Splits text into plain runs and blocks for every finished project, date and reminder in it.
fn convert_text(
    text: &str,
    projects: &[Project],
    dates: Option<&DateParser>,
    including_trailing: bool,
) -> Vec<Segment> {
    let is_finished = |range: &Range<usize>| match text[range.end..].chars().next() {
        Some(c) => c.is_whitespace(),
        None => including_trailing,
    };
    let project_matches = project_matches(text, projects);
    let mut found: Vec<(Range<usize>, BlockValue)> = Vec::new();
    for (range, project) in &project_matches {
        if !is_finished(range) {
            continue;
        }
        // "#Work " might yet become "#Work Admin"; wait until the name can't grow into a longer one.
        let typed = format!("{}{}", project.name, &text[range.end..]).to_lowercase();
        let could_grow = projects.iter().any(|other| {
            other.name.chars().count() > project.name.chars().count()
                && other.name.to_lowercase().starts_with(&typed)
        });
        if including_trailing || !could_grow {
            found.push((range.clone(), BlockValue::Project(project.id)));
        }
    }
    for phrase in dates.map(|dates| dates.phrases(text)).unwrap_or_default() {
        if !is_finished(&phrase.range) {
            continue;
        }
        // Words inside a project's name ("#Work Monday") aren't dates.
        if project_matches
            .iter()
            .any(|(range, _)| range.start < phrase.range.end && phrase.range.start < range.end)
        {
            continue;
        }
        found.push((
            phrase.range,
            match phrase.value {
                PhraseValue::Due(fragment) => BlockValue::Due(fragment),
                PhraseValue::Reminder(fragment) => BlockValue::Reminder(fragment),
            },
        ));
    }
    found.sort_by_key(|(range, _)| range.start);

    let mut segments = Vec::new();
    let mut position = 0;
    for (range, value) in found {
        if range.start < position {
            continue;
        }
        segments.push(Segment::Text {
            text: text[position..range.start].to_owned(),
            literal: false,
        });
        segments.push(Segment::Block(Block {
            text: text[range.clone()].to_owned(),
            value,
        }));
        position = range.end;
    }
    segments.push(Segment::Text {
        text: text[position..].to_owned(),
        literal: false,
    });
    segments
}

/// Joins neighbouring text of the same kind and drops empty text.
fn normalised(segments: Vec<Segment>) -> Vec<Segment> {
    let mut result: Vec<Segment> = Vec::new();
    for segment in segments {
        if let Segment::Text { text, literal } = &segment {
            if text.is_empty() {
                continue;
            }
            if let Some(Segment::Text {
                text: previous,
                literal: previous_literal,
            }) = result.last_mut()
                && previous_literal == literal
            {
                previous.push_str(text);
                continue;
            }
        }
        result.push(segment);
    }
    result
}

fn tidy(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ProjectColor;
    use jiff::civil::{date, time};

    fn dates() -> DateParser {
        let now = date(2026, 1, 14)
            .at(10, 30, 0, 0)
            .in_tz("Europe/London")
            .unwrap();
        DateParser::new(now).with_day_first(true)
    }

    fn projects() -> Vec<Project> {
        ["Inbox", "Work", "Work Admin", "Home"]
            .iter()
            .enumerate()
            .map(|(i, name)| Project {
                id: ProjectId(i as i64 + 1),
                name: name.to_string(),
                color: ProjectColor::Blue,
                position: i as i64,
                is_inbox: i == 0,
            })
            .collect()
    }

    /// Types `text` one character at a time, converting as the composer does.
    fn type_text(text: &str) -> Draft {
        let mut draft = Draft::new();
        for c in text.chars() {
            let end = draft.len();
            draft.edit(end..end, &c.to_string());
            draft.convert(&projects(), Some(&dates()), false);
        }
        draft
    }

    fn block_texts(draft: &Draft) -> Vec<String> {
        draft
            .blocks()
            .into_iter()
            .map(|(_, block)| block.text.clone())
            .collect()
    }

    #[test]
    fn parses_projects_dates_and_reminders_anywhere() {
        let result = parse("Pay rent tomorrow 9am #Home !1h", &projects(), &dates());
        assert_eq!(result.name, "Pay rent");
        assert_eq!(result.project_id, Some(ProjectId(4)));
        assert_eq!(
            result.due,
            Some(Due::at(date(2026, 1, 15), time(9, 0, 0, 0)))
        );
        assert_eq!(result.reminders, vec![Reminder::Before { minutes: 60 }]);
        assert_eq!(result.due_text.as_deref(), Some("tomorrow 9am"));
    }

    #[test]
    fn prefers_the_longest_project_name() {
        let result = parse("File expenses #work admin", &projects(), &dates());
        assert_eq!(result.project_id, Some(ProjectId(3)));
        assert_eq!(result.name, "File expenses");
        let result = parse("#Unknown thing", &projects(), &dates());
        assert_eq!(result.project_id, None);
        assert_eq!(result.name, "#Unknown thing");
    }

    #[test]
    fn blocks_form_once_the_following_space_is_typed() {
        assert!(
            type_text("Call fri").blocks().is_empty(),
            "fri might become friday"
        );
        assert_eq!(block_texts(&type_text("Call friday ")), vec!["friday"]);
    }

    #[test]
    fn a_project_waits_until_it_cannot_grow() {
        assert!(
            type_text("Do #Work ").blocks().is_empty(),
            "#Work might become #Work Admin"
        );
        assert_eq!(
            block_texts(&type_text("Do #Work Admin ")),
            vec!["#Work Admin"]
        );
        assert_eq!(block_texts(&type_text("Do #Work it ")), vec!["#Work"]);
    }

    #[test]
    fn date_blocks_absorb_words_that_extend_them() {
        let draft = type_text("Call tomorrow at 5pm ");
        assert_eq!(block_texts(&draft), vec!["tomorrow at 5pm"]);
        let result = draft.result(&dates());
        assert_eq!(
            result.due,
            Some(Due::at(date(2026, 1, 15), time(17, 0, 0, 0)))
        );
        assert_eq!(result.name, "Call");
    }

    #[test]
    fn only_the_latest_date_stays_a_block() {
        let result = parse("Move monday meeting to friday", &projects(), &dates());
        assert_eq!(result.due, Some(Due::day(date(2026, 1, 16))));
        assert_eq!(result.name, "Move monday meeting to");
        let result = parse("5pm call mum tomorrow", &projects(), &dates());
        assert_eq!(
            result.due,
            Some(Due::at(date(2026, 1, 15), time(17, 0, 0, 0))),
            "a time and a day combine"
        );
    }

    #[test]
    fn backspacing_over_a_block_makes_it_plain_text_for_good() {
        let mut draft = type_text("Plan monday ");
        assert_eq!(block_texts(&draft), vec!["monday"]);
        // Delete the space, then backspace over the block.
        let cursor = draft.backspace(draft.len());
        let cursor = draft.backspace(cursor);
        assert_eq!(cursor, "Plan monday".len(), "reverting deletes nothing");
        assert!(draft.blocks().is_empty());
        // Typing on doesn't bring it back.
        for c in "'s standup ".chars() {
            let end = draft.len();
            draft.edit(end..end, &c.to_string());
            draft.convert(&projects(), Some(&dates()), false);
        }
        draft.convert(&projects(), Some(&dates()), true);
        assert!(draft.blocks().is_empty());
        assert_eq!(draft.result(&dates()).name, "Plan monday's standup");
        assert_eq!(draft.result(&dates()).due, None);
    }

    #[test]
    fn an_existing_name_is_never_reread() {
        let mut draft = Draft::from_text("Review friday notes", true);
        draft.convert(&projects(), Some(&dates()), true);
        assert!(draft.blocks().is_empty());
        // Typing inside it stays literal; typing after it can still be recognised.
        draft.edit(7..7, "tomorrow ");
        let end = draft.len();
        draft.edit(end..end, " tomorrow");
        draft.convert(&projects(), Some(&dates()), true);
        assert_eq!(block_texts(&draft), vec!["tomorrow"]);
        assert_eq!(draft.result(&dates()).name, "Review tomorrow friday notes");
    }

    #[test]
    fn editing_inside_a_block_reverts_it() {
        let mut draft = type_text("Call friday ");
        draft.edit(7..7, "X");
        assert!(draft.blocks().is_empty());
        assert_eq!(draft.plain_text(), "Call frXiday ");
    }

    #[test]
    fn project_autocomplete() {
        let mut draft = type_text("Buy milk #ho");
        let (range, query) = draft.project_query(draft.len()).unwrap();
        assert_eq!((range, query.as_str()), (9..12, "ho"));
        let home = projects()[3].clone();
        let cursor = draft.complete_project(draft.len(), &home);
        assert_eq!(draft.plain_text(), "Buy milk #Home ");
        assert_eq!(cursor, draft.len());
        assert_eq!(draft.result(&dates()).project_id, Some(home.id));
        assert_eq!(
            suggestions("w", &projects())
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>(),
            vec!["Work", "Work Admin"]
        );
        assert_eq!(suggestions("admin", &projects()).len(), 1);
    }

    #[test]
    fn reminders_relative_and_absolute() {
        let result = parse(
            "Dentist friday 3pm !30min !friday 9am",
            &projects(),
            &dates(),
        );
        assert_eq!(result.reminders.len(), 2);
        assert!(result.reminders.contains(&Reminder::Before { minutes: 30 }));
        assert_eq!(result.name, "Dentist");
    }
}
