use crate::epub::{self, EpubBook};
use iced::{
    widget::{button, column, container, row, scrollable, text},
    Element, Length, Task,
};
use localwright_core::Job;
use std::{ops::Range, panic::AssertUnwindSafe, path::PathBuf};

const PAGE_BYTES: usize = 16 * 1024;
const VISIBLE_CHAPTERS: usize = 11;

#[derive(Debug, Clone)]
enum Message {
    Open,
    OpenChosen(Option<PathBuf>),
    Loaded(PathBuf, std::result::Result<EpubBook, String>),
    SelectChapter(usize),
    PreviousChapter,
    NextChapter,
    PreviousPage,
    NextPage,
    SmallerText,
    LargerText,
    Cancel,
}

struct Reader {
    path: Option<PathBuf>,
    book: Option<EpubBook>,
    chapter: usize,
    page: usize,
    page_starts: Vec<usize>,
    text_size: u16,
    recent: Vec<PathBuf>,
    dialog_opening: bool,
    loading: bool,
    job: Job,
    status: String,
}

impl Default for Reader {
    fn default() -> Self {
        Self {
            path: None,
            book: None,
            chapter: 0,
            page: 0,
            page_starts: Vec::new(),
            text_size: 18,
            recent: Vec::new(),
            dialog_opening: false,
            loading: false,
            job: Job::new(),
            status: "Open a local EPUB book to read its XHTML text in spine order.".into(),
        }
    }
}

fn remember_recent(recent: &mut Vec<PathBuf>, path: PathBuf) {
    recent.retain(|existing| existing != &path);
    recent.insert(0, path);
    recent.truncate(8);
}

fn page_starts(value: &str) -> Vec<usize> {
    let mut starts = vec![0];
    let mut last = 0;
    for (offset, _) in value.char_indices().skip(1) {
        if offset.saturating_sub(last) >= PAGE_BYTES {
            starts.push(offset);
            last = offset;
        }
    }
    starts
}

fn chapter_window(selected: usize, total: usize) -> Range<usize> {
    let before = VISIBLE_CHAPTERS / 2;
    let mut start = selected.saturating_sub(before);
    let end = (start + VISIBLE_CHAPTERS).min(total);
    if end.saturating_sub(start) < VISIBLE_CHAPTERS {
        start = end.saturating_sub(VISIBLE_CHAPTERS);
    }
    start..end
}

fn load_task(path: PathBuf, job: Job) -> Task<Message> {
    let result_path = path.clone();
    Task::perform(
        async move {
            let worker_path = path;
            std::thread::spawn(move || {
                std::panic::catch_unwind(AssertUnwindSafe(|| {
                    epub::read_book(&worker_path, &job).map_err(|error| error.message)
                }))
                .unwrap_or_else(|_| Err("The EPUB reader stopped unexpectedly".into()))
            })
            .join()
            .unwrap_or_else(|_| Err("The EPUB loading worker stopped unexpectedly".into()))
        },
        move |result| Message::Loaded(result_path, result),
    )
}

impl Reader {
    fn open_path(&mut self, path: PathBuf) -> Task<Message> {
        self.job.cancel();
        self.job = Job::new();
        self.loading = true;
        self.path = Some(path.clone());
        self.book = None;
        self.page_starts.clear();
        self.chapter = 0;
        self.page = 0;
        self.status = format!("Reading bounded EPUB contents from {}…", path.display());
        load_task(path, self.job.clone())
    }

    fn select_chapter(&mut self, chapter: usize) {
        let Some(book) = &self.book else {
            return;
        };
        let Some(selected) = book.chapters.get(chapter) else {
            return;
        };
        self.chapter = chapter;
        self.page = 0;
        self.page_starts = page_starts(&selected.text);
        self.status = format!(
            "Reading chapter {} of {}.",
            chapter + 1,
            book.chapters.len()
        );
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Open if !self.loading && !self.dialog_opening => {
                self.dialog_opening = true;
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .set_title("Open an EPUB book")
                            .add_filter("EPUB book", &["epub"])
                            .pick_file()
                            .await
                            .map(|file| file.path().to_path_buf())
                    },
                    Message::OpenChosen,
                );
            }
            Message::OpenChosen(path) => {
                self.dialog_opening = false;
                if let Some(path) = path.filter(|_| !self.loading) {
                    return self.open_path(path);
                }
            }
            Message::Loaded(path, result) if self.path.as_ref() == Some(&path) => {
                self.loading = false;
                self.job = Job::new();
                match result {
                    Ok(book) => {
                        remember_recent(&mut self.recent, path);
                        let count = book.chapters.len();
                        self.book = Some(book);
                        if count > 0 {
                            self.select_chapter(0);
                        } else {
                            self.status = "The EPUB spine contains no readable XHTML text.".into();
                        }
                    }
                    Err(error) => {
                        self.book = None;
                        self.page_starts.clear();
                        self.status = format!("Could not open this EPUB: {error}");
                    }
                }
            }
            Message::SelectChapter(chapter) if !self.loading => self.select_chapter(chapter),
            Message::PreviousChapter if !self.loading && self.chapter > 0 => {
                self.select_chapter(self.chapter - 1)
            }
            Message::NextChapter
                if !self.loading
                    && self
                        .book
                        .as_ref()
                        .is_some_and(|book| self.chapter + 1 < book.chapters.len()) =>
            {
                self.select_chapter(self.chapter + 1)
            }
            Message::PreviousPage if !self.loading && self.page > 0 => self.page -= 1,
            Message::NextPage if !self.loading && self.page + 1 < self.page_starts.len() => {
                self.page += 1
            }
            Message::SmallerText => self.text_size = self.text_size.saturating_sub(1).max(12),
            Message::LargerText => self.text_size = (self.text_size + 1).min(30),
            Message::Cancel if self.loading => {
                self.job.cancel();
                self.status = "Cancellation requested; stopping between EPUB members.".into();
            }
            _ => {}
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let open = button("Open EPUB…")
            .on_press_maybe((!self.loading && !self.dialog_opening).then_some(Message::Open));
        let cancel =
            button("Cancel loading").on_press_maybe(self.loading.then_some(Message::Cancel));
        let mut chapter_buttons = row![].spacing(6);
        if let Some(book) = &self.book {
            for chapter in chapter_window(self.chapter, book.chapters.len()) {
                let item = &book.chapters[chapter];
                let label = format!("{} · {}", chapter + 1, display_value(&item.label, 100));
                let chapter_button = button(text(label).size(13))
                    .style(if chapter == self.chapter {
                        button::primary
                    } else {
                        button::secondary
                    })
                    .on_press_maybe(
                        (chapter != self.chapter).then_some(Message::SelectChapter(chapter)),
                    );
                chapter_buttons = chapter_buttons.push(chapter_button);
            }
        }
        let chapter_strip = scrollable(chapter_buttons)
            .direction(scrollable::Direction::Horizontal(
                scrollable::Scrollbar::default(),
            ))
            .height(Length::Fixed(42.0));

        let previous_chapter = button("Previous chapter").on_press_maybe(
            (!self.loading && self.chapter > 0).then_some(Message::PreviousChapter),
        );
        let next_chapter = button("Next chapter").on_press_maybe(
            (!self.loading
                && self
                    .book
                    .as_ref()
                    .is_some_and(|book| self.chapter + 1 < book.chapters.len()))
            .then_some(Message::NextChapter),
        );
        let previous_page = button("Previous text page")
            .on_press_maybe((!self.loading && self.page > 0).then_some(Message::PreviousPage));
        let next_page = button("Next text page").on_press_maybe(
            (!self.loading && self.page + 1 < self.page_starts.len()).then_some(Message::NextPage),
        );
        let controls = row![
            previous_chapter,
            next_chapter,
            previous_page,
            text(format!(
                "Text page {} of {}",
                self.page + 1,
                self.page_starts.len().max(1)
            )),
            next_page,
            button("A−").on_press(Message::SmallerText),
            text(format!("{} pt", self.text_size)),
            button("A+").on_press(Message::LargerText),
            cancel,
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);

        let title = self
            .book
            .as_ref()
            .and_then(|book| book.title.as_deref())
            .or_else(|| {
                self.path
                    .as_ref()
                    .and_then(|path| path.file_name())
                    .and_then(|name| name.to_str())
            })
            .unwrap_or("EPUB Reader");
        let title = display_value(title, 240);
        let authors = self
            .book
            .as_ref()
            .map(|book| {
                book.creators
                    .iter()
                    .take(3)
                    .map(|author| display_value(author, 96))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        let language = self
            .book
            .as_ref()
            .and_then(|book| book.language.as_deref())
            .unwrap_or("language unspecified");
        let language = display_value(language, 48);
        let metadata = if authors.is_empty() {
            format!("{title} · {language}")
        } else {
            format!("{title} · {authors} · {language}")
        };

        let chapter_title = self
            .book
            .as_ref()
            .and_then(|book| book.chapters.get(self.chapter))
            .map(|chapter| display_value(&chapter.label, 120))
            .unwrap_or_else(|| "No chapter selected".into());
        let visible_text = self
            .book
            .as_ref()
            .and_then(|book| book.chapters.get(self.chapter))
            .and_then(|chapter| {
                let start = *self.page_starts.get(self.page)?;
                let end = self
                    .page_starts
                    .get(self.page + 1)
                    .copied()
                    .unwrap_or(chapter.text.len());
                chapter.text.get(start..end)
            })
            .unwrap_or("Open an EPUB file to begin reading.");
        let reading_area = scrollable(
            container(text(visible_text).size(self.text_size))
                .width(Length::Fill)
                .padding(28),
        )
        .height(Length::Fill);

        let recent: Vec<Element<'_, Message>> = self
            .recent
            .iter()
            .map(|path| {
                let name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("Recent EPUB");
                button(text(name).size(12))
                    .on_press_maybe(
                        (!self.loading && !self.dialog_opening)
                            .then_some(Message::OpenChosen(Some(path.clone()))),
                    )
                    .into()
            })
            .collect();

        column![
            row![open, text(&self.status)].spacing(12).align_y(iced::Alignment::Center),
            text(metadata).size(14),
            chapter_strip,
            row![text(chapter_title).size(20), controls]
                .spacing(10)
                .align_y(iced::Alignment::Center),
            reading_area,
            scrollable(row(recent).spacing(6))
                .direction(scrollable::Direction::Horizontal(iced::widget::scrollable::Scrollbar::default()))
                .height(Length::Fixed(34.0)),
            text("Plain-text reading from EPUB XHTML spine order. Layout, CSS, images, tables, navigation, and interactive content are not rendered.").size(12),
        ]
        .spacing(12)
        .padding(18)
        .into()
    }
}

fn display_value(value: &str, max_chars: usize) -> String {
    let mut result = value.chars().take(max_chars).collect::<String>();
    if value.chars().count() > max_chars {
        result.push('…');
    }
    result
}

pub(super) fn run(path: PathBuf) -> std::result::Result<(), String> {
    let mut reader = Reader::default();
    let initial_task = reader.open_path(path);
    iced::application(
        "Document Studio — EPUB Reader",
        Reader::update,
        Reader::view,
    )
    .window_size(iced::Size::new(1080.0, 820.0))
    .run_with(move || (reader, initial_task))
    .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{chapter_window, display_value, page_starts, remember_recent, PAGE_BYTES};
    use std::path::PathBuf;

    #[test]
    fn reader_chunks_are_bounded_and_keep_utf8_boundaries() {
        let value = format!("{}éabc", "a".repeat(PAGE_BYTES));
        let starts = page_starts(&value);
        assert_eq!(starts[0], 0);
        assert!(starts.iter().all(|start| value.is_char_boundary(*start)));
        assert!(starts
            .windows(2)
            .all(|pair| pair[1] - pair[0] <= PAGE_BYTES + 4));
        assert!(starts.len() >= 2);
    }

    #[test]
    fn chapter_window_is_bounded_and_contains_selection() {
        assert_eq!(chapter_window(0, 3), 0..3);
        let window = chapter_window(700, 2000);
        assert_eq!(window.len(), 11);
        assert!(window.contains(&700));
        assert_eq!(chapter_window(1999, 2000), 1989..2000);
    }

    #[test]
    fn recent_epub_list_is_deduplicated_and_bounded() {
        let mut recent = Vec::new();
        for index in 0..10 {
            remember_recent(&mut recent, PathBuf::from(format!("{index}.epub")));
        }
        remember_recent(&mut recent, PathBuf::from("5.epub"));
        assert_eq!(recent.len(), 8);
        assert_eq!(recent[0], PathBuf::from("5.epub"));
    }

    #[test]
    fn reader_truncates_untrusted_metadata_for_display() {
        assert_eq!(display_value("Résumé", 6), "Résumé");
        assert_eq!(display_value("Résumé book", 6), "Résumé…");
    }
}
