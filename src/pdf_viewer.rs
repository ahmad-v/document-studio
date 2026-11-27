use crate::{
    load,
    pdf_workspace::{
        page_window, parse_page_selection, BoundedPageCache, THUMBNAIL_CACHE_CAPACITY,
    },
    render_viewer_batch, ViewerBatch,
};
use iced::{
    widget::{button, column, container, image, row, scrollable, text, text_input},
    ContentFit, Element, Length, Task,
};
use localwright_core::Job;
use std::{panic::AssertUnwindSafe, path::PathBuf, sync::Arc};

#[derive(Debug, Clone)]
enum Message {
    Open,
    OpenDialogComplete(Option<PathBuf>),
    OpenRecent(PathBuf),
    Previous,
    Next,
    SelectPage(usize),
    PageInput(String),
    GoToPage,
    ZoomIn,
    ZoomOut,
    ActualSize,
    Fit,
    Cancel,
    Rendered(usize, std::result::Result<Arc<ViewerBatch>, String>),
}

struct Viewer {
    path: PathBuf,
    page: usize,
    pages: usize,
    preview: Option<image::Handle>,
    preview_size: Option<(u32, u32)>,
    thumbnails: BoundedPageCache<image::Handle>,
    page_input: String,
    zoom: Option<f32>,
    status: String,
    recent: Vec<PathBuf>,
    job: Job,
    loading: bool,
}

fn remember_recent(recent: &mut Vec<PathBuf>, path: PathBuf) {
    recent.retain(|existing| existing != &path);
    recent.insert(0, path);
    recent.truncate(8);
}

fn render_task(path: PathBuf, page: usize, thumbnail_pages: Vec<usize>, job: Job) -> Task<Message> {
    Task::perform(
        async move {
            std::thread::spawn(move || {
                std::panic::catch_unwind(AssertUnwindSafe(|| {
                    render_viewer_batch(&path, page, &thumbnail_pages, &job)
                        .map(Arc::new)
                        .map_err(|error| error.message)
                }))
                .unwrap_or_else(|_| Err("The PDF renderer stopped unexpectedly".into()))
            })
            .join()
            .unwrap_or_else(|_| Err("The PDF preview worker stopped unexpectedly".into()))
        },
        move |result| Message::Rendered(page, result),
    )
}

impl Viewer {
    fn open_path(&mut self, path: PathBuf) -> Task<Message> {
        self.path = path.clone();
        remember_recent(&mut self.recent, path);
        self.page = 1;
        self.pages = 1;
        self.page_input = "1".into();
        self.preview = None;
        self.preview_size = None;
        self.thumbnails = BoundedPageCache::default();
        self.zoom = None;
        self.request_page(1)
    }

    fn request_page(&mut self, page: usize) -> Task<Message> {
        if self.loading || page == 0 || page > self.pages {
            return Task::none();
        }
        self.page = page;
        self.page_input = page.to_string();
        self.preview = None;
        self.preview_size = None;
        let visible = page_window(page, self.pages);
        for visible_page in &visible {
            self.thumbnails.touch(*visible_page);
        }
        let missing = visible
            .into_iter()
            .filter(|thumbnail_page| self.thumbnails.peek(*thumbnail_page).is_none())
            .collect::<Vec<_>>();
        self.loading = true;
        self.job = Job::new();
        self.status = format!("Rendering page {page}…");
        render_task(self.path.clone(), page, missing, self.job.clone())
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Open if !self.loading => {
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .set_title("Open a PDF document")
                            .add_filter("PDF document", &["pdf"])
                            .pick_file()
                            .await
                            .map(|file| file.path().to_path_buf())
                    },
                    Message::OpenDialogComplete,
                );
            }
            Message::OpenDialogComplete(Some(path)) if !self.loading => {
                return self.open_path(path);
            }
            Message::OpenRecent(path) if !self.loading => return self.open_path(path),
            Message::Previous if !self.loading && self.page > 1 => {
                return self.request_page(self.page - 1);
            }
            Message::Next if !self.loading && self.page < self.pages => {
                return self.request_page(self.page + 1);
            }
            Message::SelectPage(page) if !self.loading => return self.request_page(page),
            Message::PageInput(value) => self.page_input = value,
            Message::GoToPage if !self.loading => {
                if let Some(page) = parse_page_selection(&self.page_input, self.pages) {
                    return self.request_page(page);
                }
                self.status = format!("Enter a page from 1 to {}", self.pages);
            }
            Message::ZoomIn => {
                self.zoom = Some((self.zoom.unwrap_or(1.0) * 1.2).min(4.0));
            }
            Message::ZoomOut => {
                self.zoom = Some((self.zoom.unwrap_or(1.0) / 1.2).max(0.5));
            }
            Message::ActualSize => self.zoom = Some(1.0),
            Message::Fit => self.zoom = None,
            Message::Cancel if self.loading => {
                self.job.cancel();
                self.status = "Cancellation requested; stopping between render stages".into();
            }
            Message::Rendered(page, result) => {
                if page == self.page {
                    match result {
                        Ok(batch) => {
                            self.preview = Some(image::Handle::from_rgba(
                                batch.preview.width,
                                batch.preview.height,
                                batch.preview.pixels.clone(),
                            ));
                            self.preview_size = Some((batch.preview.width, batch.preview.height));
                            self.pages = batch.page_count;
                            for thumbnail in &batch.thumbnails {
                                self.thumbnails.insert(
                                    thumbnail.page,
                                    image::Handle::from_rgba(
                                        thumbnail.width,
                                        thumbnail.height,
                                        thumbnail.pixels.clone(),
                                    ),
                                );
                            }
                            self.status = format!(
                                "Rendered at {} × {} pixels; thumbnail cache holds at most {} pages",
                                batch.preview.width,
                                batch.preview.height,
                                THUMBNAIL_CACHE_CAPACITY
                            );
                        }
                        Err(error) => {
                            self.preview = None;
                            self.preview_size = None;
                            self.status = format!("Could not preview this page: {error}");
                        }
                    }
                }
                self.loading = false;
                self.job = Job::new();
            }
            _ => {}
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let previous = button("Previous");
        let previous = if !self.loading && self.page > 1 {
            previous.on_press(Message::Previous)
        } else {
            previous
        };
        let next = button("Next");
        let next = if !self.loading && self.page < self.pages {
            next.on_press(Message::Next)
        } else {
            next
        };
        let cancel = button("Cancel rendering");
        let cancel = if self.loading {
            cancel.on_press(Message::Cancel)
        } else {
            cancel
        };
        let go = button("Go");
        let go = if !self.loading {
            go.on_press(Message::GoToPage)
        } else {
            go
        };
        let mut thumbnails = row![].spacing(8).align_y(iced::Alignment::Center);
        for page in page_window(self.page, self.pages) {
            let item: Element<'_, Message> = if let Some(handle) = self.thumbnails.peek(page) {
                let thumb = image(handle.clone())
                    .content_fit(ContentFit::Contain)
                    .width(Length::Fixed(96.0))
                    .height(Length::Fixed(112.0));
                let selected = button(
                    column![thumb, text(format!("Page {page}"))].align_x(iced::Alignment::Center),
                );
                if !self.loading && page != self.page {
                    selected.on_press(Message::SelectPage(page)).into()
                } else {
                    selected.into()
                }
            } else {
                button(
                    column![text("Preview"), text(format!("Page {page}"))]
                        .width(Length::Fixed(96.0))
                        .height(Length::Fixed(112.0))
                        .align_x(iced::Alignment::Center),
                )
                .on_press_maybe(
                    (!self.loading && page != self.page).then_some(Message::SelectPage(page)),
                )
                .into()
            };
            thumbnails = thumbnails.push(item);
        }
        let thumbnail_strip = scrollable(thumbnails)
            .direction(scrollable::Direction::Horizontal(
                scrollable::Scrollbar::default(),
            ))
            .height(Length::Fixed(132.0));
        let controls = row![
            button("Open PDF").on_press_maybe((!self.loading).then_some(Message::Open)),
            previous,
            text(format!("Page {} of {}", self.page, self.pages)),
            next,
            text_input("Page", &self.page_input)
                .on_input(Message::PageInput)
                .on_submit(Message::GoToPage)
                .width(Length::Fixed(72.0)),
            go,
            button("Zoom −").on_press(Message::ZoomOut),
            button("Zoom +").on_press(Message::ZoomIn),
            button("Actual").on_press(Message::ActualSize),
            button("Fit").on_press(Message::Fit),
            cancel,
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);
        let content: Element<'_, Message> = match &self.preview {
            Some(handle) => {
                let preview = image(handle.clone()).content_fit(ContentFit::Contain);
                let (width, height) = self.preview_size.unwrap_or((800, 1050));
                match self.zoom {
                    Some(scale) => preview
                        .width(Length::Fixed(width as f32 * scale))
                        .height(Length::Fixed(height as f32 * scale))
                        .into(),
                    None => preview.width(Length::Fill).height(Length::Fill).into(),
                }
            }
            None => text(&self.status).into(),
        };
        let recent: Vec<Element<'_, Message>> = self.recent.iter().map(|path| {
            let label = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("Recent PDF");
            button(text(label).size(12))
                .on_press_maybe((!self.loading).then_some(Message::OpenRecent(path.clone())))
                .into()
        });
        column![
            controls,
            scrollable(row(recent).spacing(6))
                .direction(scrollable::Direction::Horizontal(
                    scrollable::Scrollbar::default(),
                ))
                .height(Length::Fixed(36.0)),
            text(&self.status),
            thumbnail_strip,
            scrollable(
                container(content)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill)
                    .padding(8)
            )
            .direction(scrollable::Direction::Both {
                vertical: scrollable::Scrollbar::default(),
                horizontal: scrollable::Scrollbar::default(),
            })
            .height(Length::Fill),
            text("Experimental raster preview. Unsupported PDF features may be missing or differ."),
        ]
        .spacing(10)
        .padding(14)
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::remember_recent;
    use std::path::{Path, PathBuf};

    #[test]
    fn recent_documents_are_deduplicated_and_bounded() {
        let mut recent = Vec::new();
        for index in 0..10 {
            remember_recent(&mut recent, PathBuf::from(format!("{index}.pdf")));
        }
        remember_recent(&mut recent, PathBuf::from("5.pdf"));
        assert_eq!(recent.len(), 8);
        assert_eq!(recent[0], PathBuf::from("5.pdf"));
        assert_eq!(
            recent
                .iter()
                .filter(|path| path.as_path() == Path::new("5.pdf"))
                .count(),
            1
        );
        assert!(!recent.contains(&PathBuf::from("0.pdf")));
    }
}

pub(super) fn run(path: PathBuf, initial_page: usize) -> std::result::Result<(), String> {
    let pages = std::panic::catch_unwind(AssertUnwindSafe(|| load(&path)))
        .map_err(|_| "PDF parser stopped unexpectedly while opening the preview".to_owned())?
        .map_err(|error| error.message)?
        .get_pages()
        .len();
    if initial_page == 0 || initial_page > pages {
        return Err("Choose an existing 1-based PDF page number".into());
    }
    let mut viewer = Viewer {
        path: path.clone(),
        page: initial_page,
        pages,
        preview: None,
        preview_size: None,
        thumbnails: BoundedPageCache::default(),
        page_input: initial_page.to_string(),
        zoom: None,
        status: format!("Rendering page {initial_page}…"),
        recent: vec![path.clone()],
        job: Job::new(),
        loading: true,
    };
    let visible = page_window(initial_page, pages);
    for visible_page in &visible {
        viewer.thumbnails.touch(*visible_page);
    }
    let missing = visible;
    let initial_task = render_task(path, initial_page, missing, viewer.job.clone());
    iced::application(
        "Document Studio — PDF Preview",
        Viewer::update,
        Viewer::view,
    )
    .window_size(iced::Size::new(1120.0, 880.0))
    .run_with(move || (viewer, initial_task))
    .map_err(|error| error.to_string())
}
